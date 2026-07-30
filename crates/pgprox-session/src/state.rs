//! The connection handshake, as a pure function of state and message.
//!
//! Everything before the first `ReadyForQuery`: the TLS decision, the GSSAPI
//! refusal, protocol version negotiation, and which credential the client is
//! asked for. The next stage, checking that credential, is a separate machine
//! in this crate's `auth` module.
//!
//! # Why this is a state machine rather than a sequence of awaits
//!
//! Written as straight-line async code, the handshake is a chain of reads and
//! writes whose error cases can only be reached by making a real client
//! misbehave in a real socket. Written this way, every case is a function call:
//! a client that sends `SSLRequest` twice, one that skips it under
//! `require_tls`, one that asks for protocol 4.0, one that sends a startup
//! packet with no `user`. Those are the cases that decide whether the proxy is
//! safe to expose, and they are exactly the ones an integration test reaches
//! last, if at all.
//!
//! # What it deliberately does not do
//!
//! It does not read, write, or decode. The caller hands it a decoded
//! [`Startup`] and gets back one [`Reply`] describing what to send. That is
//! what keeps the crate's I/O shell thin enough to cover.

use pgprox_core::error::ClientError;
use pgprox_core::ids::ConnId;
use pgprox_proto::startup::{Startup, VersionResponse, negotiate_version};

/// Whether this listener offers, requires, or refuses TLS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsPosture {
    /// The client must send `SSLRequest` before its startup packet.
    ///
    /// This is the only posture that may be used with token authentication:
    /// the JWT travels in the password field, so without TLS it travels in the
    /// clear. See `standards/security.md`.
    Required,
    /// TLS is accepted if asked for and not required.
    Optional,
    /// TLS is refused. Only sane for a listener bound to a loopback address.
    Disabled,
}

/// Which credential the client is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Credential {
    /// `AuthenticationCleartextPassword`, which is how a JWT arrives.
    Jwt,
    /// `AuthenticationSASL`, for a user matching a static-credential rule.
    Scram,
}

/// What the caller should send, and what state to move to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Answer `S` and start the TLS handshake.
    AcceptTls,
    /// Answer `N`. The client decides whether to continue in the clear.
    RefuseTls,
    /// Answer `N` to `GSSENCRequest`. Never supported. See ADR 0016.
    RefuseGss,
    /// Ask for this credential.
    Ask(Credential),
    /// Forward a cancellation for a connection this proxy issued a key for.
    Cancel(ConnId),
    /// Send an `ErrorResponse` and close.
    Fail(ClientError),
}

/// One step's output.
///
/// `negotiate` is separate from `action` rather than being an action of its
/// own because it never occurs alone: a client offered a lower minor version
/// is still asked for a credential in the same breath. Modelling it as a
/// second action would allow a state where the proxy negotiates and then says
/// nothing, which is a hang rather than an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// Send `NegotiateProtocolVersion` offering this minor version first.
    pub negotiate: Option<i32>,
    /// Then this.
    pub action: Action,
}

impl Reply {
    /// A reply with nothing to negotiate.
    const fn just(action: Action) -> Self {
        Self {
            negotiate: None,
            action,
        }
    }
}

/// How far the handshake has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Nothing has arrived yet.
    Opening,
    /// TLS has been settled, one way or the other.
    Settled,
    /// A credential has been asked for.
    Asked,
    /// Nothing more may arrive.
    Closed,
}

/// What the client said about itself in its startup packet.
///
/// Owned rather than borrowed: the startup packet's buffer is reused for the
/// next read, and this outlives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupInfo {
    /// The `user` parameter. Postgres requires it, so its absence is an error.
    pub user: String,
    /// The `database` parameter, defaulting to the user name as Postgres does.
    pub database: String,
    /// Runtime settings packed into `options`, notably `search_path`.
    pub options: Vec<(String, String)>,
}

/// How the handshake behaves.
#[derive(Debug, Clone)]
pub struct HandshakeConfig {
    /// Whether TLS is required, offered, or refused.
    pub tls: TlsPosture,
    /// Startup users that authenticate with SCRAM against a local credential
    /// rather than with a token.
    ///
    /// This is how an admin client reaches the `SHOW` pseudo-database without
    /// a sidecar being involved.
    pub static_users: Vec<String>,
}

impl Default for HandshakeConfig {
    fn default() -> Self {
        Self {
            // Required rather than Optional, because the default posture of a
            // listener that carries tokens is the safe one. An operator who
            // wants otherwise says so.
            tls: TlsPosture::Required,
            static_users: Vec::new(),
        }
    }
}

/// The handshake state machine.
#[derive(Debug, Clone)]
pub struct Handshake {
    config: HandshakeConfig,
    stage: Stage,
    tls: bool,
    startup: Option<StartupInfo>,
}

impl Handshake {
    /// Starts a handshake for a newly accepted connection.
    #[must_use]
    pub const fn new(config: HandshakeConfig) -> Self {
        Self {
            config,
            stage: Stage::Opening,
            tls: false,
            startup: None,
        }
    }

    /// Whether the connection is encrypted.
    #[must_use]
    pub const fn is_encrypted(&self) -> bool {
        self.tls
    }

    /// What the client said about itself, once its startup packet has arrived.
    #[must_use]
    pub const fn startup(&self) -> Option<&StartupInfo> {
        self.startup.as_ref()
    }

    /// Whether the handshake has finished, successfully or not.
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        matches!(self.stage, Stage::Closed)
    }

    /// Whether a credential has been asked for and not yet supplied.
    #[must_use]
    pub const fn is_awaiting_credential(&self) -> bool {
        matches!(self.stage, Stage::Asked)
    }

    /// Feeds one decoded first-phase message in.
    pub fn on_startup(&mut self, message: &Startup<'_>) -> Reply {
        match message {
            Startup::SslRequest => self.on_ssl_request(),
            Startup::GssEncRequest => self.on_gss_request(),
            Startup::CancelRequest { conn } => self.on_cancel(*conn),
            Startup::StartupMessage { version, .. } => self.on_message(*version, message),
            // Startup is non_exhaustive: a variant added upstream must not
            // silently take a default path through the handshake.
            _ => self.violation("unrecognized first message"),
        }
    }

    fn on_ssl_request(&mut self) -> Reply {
        if !matches!(self.stage, Stage::Opening) {
            return self.violation("SSLRequest arrived after the handshake had moved on");
        }
        self.stage = Stage::Settled;
        if matches!(self.config.tls, TlsPosture::Disabled) {
            return Reply::just(Action::RefuseTls);
        }
        self.tls = true;
        Reply::just(Action::AcceptTls)
    }

    fn on_gss_request(&mut self) -> Reply {
        // Refused without changing stage. A client that asked for GSSAPI and
        // was told no is entitled to try SSLRequest next, and treating the
        // refusal as having settled TLS would then reject a client doing
        // exactly what the protocol tells it to.
        if matches!(self.stage, Stage::Asked | Stage::Closed) {
            return self.violation("GSSENCRequest arrived after the handshake had moved on");
        }
        Reply::just(Action::RefuseGss)
    }

    fn on_cancel(&mut self, conn: ConnId) -> Reply {
        // A CancelRequest is a whole connection's worth of conversation: it
        // arrives on a fresh socket, carries only the key, and nothing else
        // follows it.
        if matches!(self.stage, Stage::Asked | Stage::Closed) {
            return self.violation("CancelRequest arrived mid-handshake");
        }
        self.stage = Stage::Closed;
        Reply::just(Action::Cancel(conn))
    }

    fn on_message(&mut self, version: i32, message: &Startup<'_>) -> Reply {
        if matches!(self.stage, Stage::Asked | Stage::Closed) {
            return self.violation("a second startup packet arrived");
        }

        // Checked before the version, because a client that reached here in
        // the clear has already sent its startup parameters unencrypted, and
        // the answer is the same whatever version it asked for.
        if matches!(self.config.tls, TlsPosture::Required) && !self.tls {
            return self.fail(ClientError::TlsRequired);
        }

        let negotiate = match negotiate_version(version) {
            VersionResponse::Accept => None,
            VersionResponse::Negotiate { minor } => Some(minor),
            // Unsupported, and anything a later version of the codec adds.
            // Refusing an unknown answer is the safe default: proceeding would
            // mean speaking a framing this crate does not implement.
            _ => return self.violation("unsupported protocol major version"),
        };

        let Some(user) = message.user() else {
            return self.violation("startup packet has no user parameter");
        };
        let Some(database) = message.database() else {
            return self.violation("startup packet has no database parameter");
        };

        let credential = if self.config.static_users.iter().any(|u| u == user) {
            Credential::Scram
        } else {
            Credential::Jwt
        };

        self.startup = Some(StartupInfo {
            user: user.to_owned(),
            database: database.to_owned(),
            options: message.options(),
        });
        self.stage = Stage::Asked;

        Reply {
            negotiate,
            action: Action::Ask(credential),
        }
    }

    fn violation(&mut self, what: &'static str) -> Reply {
        self.fail(ClientError::ProtocolViolation(what))
    }

    fn fail(&mut self, error: ClientError) -> Reply {
        self.stage = Stage::Closed;
        Reply::just(Action::Fail(error))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use pgprox_core::ids::NodeId;
    use pgprox_proto::startup::StartupParam;
    use proptest::prelude::*;

    fn message<'a>(version: i32, params: &'a [StartupParam<'a>]) -> Startup<'a> {
        Startup::StartupMessage {
            version,
            params: params.to_vec(),
        }
    }

    fn login<'a>() -> Vec<StartupParam<'a>> {
        vec![
            StartupParam {
                name: "user",
                value: "acme_app",
            },
            StartupParam {
                name: "database",
                value: "acme",
            },
        ]
    }

    fn encrypted(config: HandshakeConfig) -> Handshake {
        let mut hs = Handshake::new(config);
        assert_eq!(
            hs.on_startup(&Startup::SslRequest).action,
            Action::AcceptTls
        );
        hs
    }

    #[test]
    fn a_client_that_asks_for_tls_gets_it() {
        let mut hs = Handshake::new(HandshakeConfig::default());
        assert_eq!(
            hs.on_startup(&Startup::SslRequest).action,
            Action::AcceptTls
        );
        assert!(hs.is_encrypted());
    }

    #[test]
    fn a_disabled_listener_refuses_tls_rather_than_pretending() {
        let mut hs = Handshake::new(HandshakeConfig {
            tls: TlsPosture::Disabled,
            ..HandshakeConfig::default()
        });
        assert_eq!(
            hs.on_startup(&Startup::SslRequest).action,
            Action::RefuseTls
        );
        assert!(
            !hs.is_encrypted(),
            "a refused SSLRequest left the session believing it was encrypted"
        );
    }

    #[test]
    fn skipping_tls_where_it_is_required_is_explained_rather_than_dropped() {
        // The plan calls this out by name. A dropped socket looks like a
        // network fault and sends the operator to the wrong place; a driver
        // shown 28000 with this message reports it verbatim.
        let mut hs = Handshake::new(HandshakeConfig::default());
        assert!(!hs.is_closed(), "a fresh handshake is not finished");
        let params = login();
        let reply = hs.on_startup(&message(196_608, &params));

        assert_eq!(reply.action, Action::Fail(ClientError::TlsRequired));
        assert!(hs.is_closed());
        // And not waiting for anything. A refused handshake that still says it
        // wants a credential would have the shell read a password message on a
        // connection it has already decided to close.
        assert!(!hs.is_awaiting_credential());
    }

    #[test]
    fn an_optional_listener_accepts_a_client_that_skipped_tls() {
        let mut hs = Handshake::new(HandshakeConfig {
            tls: TlsPosture::Optional,
            ..HandshakeConfig::default()
        });
        let params = login();
        assert_eq!(
            hs.on_startup(&message(196_608, &params)).action,
            Action::Ask(Credential::Jwt)
        );
    }

    #[test]
    fn gssapi_is_refused_and_the_client_may_still_ask_for_tls() {
        // A client told no to GSSAPI tries SSLRequest next. Treating the
        // refusal as having settled TLS would reject a client doing exactly
        // what the protocol tells it to.
        let mut hs = Handshake::new(HandshakeConfig::default());
        assert_eq!(
            hs.on_startup(&Startup::GssEncRequest).action,
            Action::RefuseGss
        );
        assert_eq!(
            hs.on_startup(&Startup::SslRequest).action,
            Action::AcceptTls
        );
        assert!(hs.is_encrypted());
    }

    #[test]
    fn a_second_ssl_request_is_a_protocol_violation() {
        let mut hs = encrypted(HandshakeConfig::default());
        let reply = hs.on_startup(&Startup::SslRequest);
        assert!(matches!(
            reply.action,
            Action::Fail(ClientError::ProtocolViolation(_))
        ));
    }

    #[test]
    fn a_token_client_is_asked_for_a_password() {
        let mut hs = encrypted(HandshakeConfig::default());
        assert!(!hs.is_awaiting_credential(), "nothing has been asked yet");
        let params = login();
        let reply = hs.on_startup(&message(196_608, &params));

        assert_eq!(reply.negotiate, None);
        assert_eq!(reply.action, Action::Ask(Credential::Jwt));
        assert!(hs.is_awaiting_credential());
        // Asked, not finished. Both flags read the same field, and a `Closed`
        // that answered true here would end the connection before the password
        // it just asked for could arrive.
        assert!(!hs.is_closed());
        assert_eq!(hs.startup().unwrap().user, "acme_app");
        assert_eq!(hs.startup().unwrap().database, "acme");
    }

    #[test]
    fn a_static_user_is_asked_for_scram_instead() {
        let mut hs = encrypted(HandshakeConfig {
            static_users: vec!["pgprox_admin".to_owned()],
            ..HandshakeConfig::default()
        });
        let params = vec![
            StartupParam {
                name: "user",
                value: "pgprox_admin",
            },
            StartupParam {
                name: "database",
                value: "pgprox",
            },
        ];
        assert_eq!(
            hs.on_startup(&message(196_608, &params)).action,
            Action::Ask(Credential::Scram)
        );
    }

    #[test]
    fn a_32_client_is_negotiated_down_and_asked_in_the_same_breath() {
        // Negotiating and then saying nothing is a hang, not an error, which
        // is why the two travel together in one reply.
        let mut hs = encrypted(HandshakeConfig::default());
        let params = login();
        let reply = hs.on_startup(&message(196_610, &params));

        assert_eq!(reply.negotiate, Some(0));
        assert_eq!(reply.action, Action::Ask(Credential::Jwt));
    }

    #[test]
    fn a_version_outside_major_3_is_refused() {
        let mut hs = encrypted(HandshakeConfig::default());
        let params = login();
        let reply = hs.on_startup(&message(4 << 16, &params));
        assert!(matches!(
            reply.action,
            Action::Fail(ClientError::ProtocolViolation(_))
        ));
    }

    #[test]
    fn a_startup_packet_with_no_user_is_refused() {
        let mut hs = encrypted(HandshakeConfig::default());
        let params = vec![StartupParam {
            name: "database",
            value: "acme",
        }];
        assert!(matches!(
            hs.on_startup(&message(196_608, &params)).action,
            Action::Fail(ClientError::ProtocolViolation(_))
        ));
    }

    #[test]
    fn database_defaults_to_the_user_as_postgres_does() {
        let mut hs = encrypted(HandshakeConfig::default());
        let params = vec![StartupParam {
            name: "user",
            value: "acme_app",
        }];
        hs.on_startup(&message(196_608, &params));
        assert_eq!(hs.startup().unwrap().database, "acme_app");
    }

    #[test]
    fn options_are_carried_through_because_search_path_is_a_cache_key() {
        let mut hs = encrypted(HandshakeConfig::default());
        let params = vec![
            StartupParam {
                name: "user",
                value: "acme_app",
            },
            StartupParam {
                name: "options",
                value: "-c search_path=tenant",
            },
        ];
        hs.on_startup(&message(196_608, &params));
        assert_eq!(
            hs.startup().unwrap().options,
            vec![("search_path".to_owned(), "tenant".to_owned())]
        );
    }

    #[test]
    fn a_cancel_request_is_forwarded_and_ends_the_connection() {
        let conn = ConnId::new(NodeId::new(3), 99);
        let mut hs = Handshake::new(HandshakeConfig::default());
        let reply = hs.on_startup(&Startup::CancelRequest { conn });

        assert_eq!(reply.action, Action::Cancel(conn));
        assert!(
            hs.is_closed(),
            "a cancel connection stayed open, so the next message would be honoured on it"
        );
    }

    #[test]
    fn a_cancel_request_needs_no_tls_even_where_tls_is_required() {
        // It carries no credential, and refusing it would make cancellation
        // depend on a handshake the client has no reason to perform.
        let conn = ConnId::new(NodeId::new(1), 1);
        let mut hs = Handshake::new(HandshakeConfig::default());
        assert_eq!(
            hs.on_startup(&Startup::CancelRequest { conn }).action,
            Action::Cancel(conn)
        );
    }

    #[test]
    fn a_second_startup_packet_is_a_protocol_violation() {
        let mut hs = encrypted(HandshakeConfig::default());
        let params = login();
        hs.on_startup(&message(196_608, &params));
        assert!(matches!(
            hs.on_startup(&message(196_608, &params)).action,
            Action::Fail(ClientError::ProtocolViolation(_))
        ));
    }

    #[test]
    fn everything_after_a_failure_is_refused() {
        let mut hs = Handshake::new(HandshakeConfig::default());
        let params = login();
        hs.on_startup(&message(196_608, &params));

        for message in [
            Startup::SslRequest,
            Startup::GssEncRequest,
            Startup::CancelRequest {
                conn: ConnId::new(NodeId::new(1), 1),
            },
        ] {
            assert!(
                matches!(hs.on_startup(&message).action, Action::Fail(_)),
                "{message:?} was honoured after the handshake had failed"
            );
        }
    }

    proptest! {
        /// Whatever a client sends, the handshake never asks for a credential
        /// over an unencrypted connection when TLS is required.
        ///
        /// This is the property the whole token design rests on: the JWT
        /// travels in the password field, so asking for it in the clear hands
        /// a bearer token to anyone on the path.
        #[test]
        fn a_credential_is_never_asked_for_in_the_clear(
            steps in prop::collection::vec(0_u8..4, 1..8),
            version in prop::sample::select(vec![196_608_i32, 196_610, 4 << 16]),
        ) {
            let mut hs = Handshake::new(HandshakeConfig::default());
            let params = login();

            for step in steps {
                let message = match step {
                    0 => Startup::SslRequest,
                    1 => Startup::GssEncRequest,
                    2 => Startup::CancelRequest { conn: ConnId::new(NodeId::new(1), 1) },
                    _ => message(version, &params),
                };
                let reply = hs.on_startup(&message);
                if matches!(reply.action, Action::Ask(_)) {
                    prop_assert!(
                        hs.is_encrypted(),
                        "asked for a credential over a connection in the clear"
                    );
                }
            }
        }
    }
}
