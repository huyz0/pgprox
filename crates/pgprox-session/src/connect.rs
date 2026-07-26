//! Opening an upstream connection, as a state machine.
//!
//! The other side of [`crate::state`]: there the proxy is a server deciding
//! what to ask a client for, here it is a client answering what Postgres asks
//! it. The two are separate machines because they refuse different things. A
//! client offering an unexpected authentication method is a client to be
//! refused; a *server* offering one is a deployment this proxy cannot serve,
//! and saying which method it was is the difference between a five-minute fix
//! and an afternoon.
//!
//! # What it harvests
//!
//! The `ParameterStatus` set the server sends during startup: `server_version`,
//! `client_encoding`, `DateStyle` and the rest. A client that connects through
//! this proxy has to be told the same values, or a driver that reads
//! `server_version` to decide which syntax to use gets it wrong. This is the
//! one place they can be obtained, which is why they are collected here rather
//! than guessed at anywhere else.
//!
//! # Where the arithmetic is not
//!
//! SCRAM needs HMAC and PBKDF2, which live in `pgprox-auth`, which this crate
//! may not depend on. So the machine routes payloads and a
//! [`UpstreamScram`] implementation computes them, exactly as the client-side
//! exchange does in [`crate::auth`]. Neither direction has crypto in this
//! crate.

use std::fmt;

use pgprox_core::ids::ServerId;
use pgprox_core::pool::PoolError;
use pgprox_core::secret::SecretString;
use pgprox_proto::backend::{self, AuthRequest, BackendMessage};
use pgprox_proto::frame::{Frame, Tag};

/// The client half of a SCRAM exchange, for talking to Postgres.
///
/// Stateful across three messages, so an implementation is created per
/// connection attempt rather than shared.
pub trait UpstreamScram: Send + fmt::Debug {
    /// The client-first message, including a fresh nonce.
    fn client_first(&mut self, user: &str) -> String;

    /// The client-final message, given what the server sent back.
    ///
    /// # Errors
    ///
    /// Fails when the server's message is malformed or its nonce does not
    /// extend the one this exchange sent.
    fn client_final(
        &mut self,
        password: &SecretString,
        server_first: &str,
    ) -> Result<String, String>;

    /// Checks the server proved it knew the password too.
    ///
    /// # Errors
    ///
    /// Fails when the signature does not match, which means something is
    /// answering on the database's behalf.
    fn verify(&mut self, server_final: &str) -> Result<(), String>;
}

/// What the driver should do next.
#[derive(Debug, PartialEq, Eq)]
pub enum Need {
    /// Send the startup packet.
    Startup,
    /// Send the password in the clear.
    Password,
    /// Send `SASLInitialResponse` with a client-first message.
    SaslStart,
    /// Answer this server-first message with `SASLResponse`.
    SaslContinue(String),
    /// Check this server-final message, then keep reading.
    SaslVerify(String),
    /// Read another message.
    Read,
    /// The connection is usable.
    Ready,
    /// Give up, and say why.
    Fail(PoolError),
}

/// How far the upstream handshake has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Nothing sent yet.
    Opening,
    /// The startup packet is out; the server is deciding.
    Authenticating,
    /// Authenticated; collecting the parameters that follow.
    Settling,
    /// Usable.
    Ready,
    /// Failed.
    Failed,
}

/// The upstream handshake.
#[derive(Debug)]
pub struct UpstreamHandshake {
    server: ServerId,
    stage: Stage,
    parameters: Vec<(String, String)>,
    backend_key: Option<(i32, i32)>,
}

impl UpstreamHandshake {
    /// Starts a handshake against `server`.
    #[must_use]
    pub fn new(server: ServerId) -> Self {
        Self {
            server,
            stage: Stage::Opening,
            parameters: Vec::new(),
            backend_key: None,
        }
    }

    /// The first thing to do.
    pub const fn begin(&mut self) -> Need {
        self.stage = Stage::Authenticating;
        Need::Startup
    }

    /// The `ParameterStatus` set the server reported.
    #[must_use]
    pub fn parameters(&self) -> &[(String, String)] {
        &self.parameters
    }

    /// The server's own cancel key for this connection.
    ///
    /// Kept because cancelling a query means sending a `CancelRequest` with
    /// *this* key, not the one the proxy handed its client. The proxy issues
    /// its own downstream and has to remember the real one to use it.
    #[must_use]
    pub const fn backend_key(&self) -> Option<(i32, i32)> {
        self.backend_key
    }

    /// Whether the connection is usable.
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self.stage, Stage::Ready)
    }

    /// Feeds in one frame from the server.
    pub fn on_frame(&mut self, tag: Tag, body: &[u8]) -> Need {
        if matches!(self.stage, Stage::Ready | Stage::Failed) {
            return self.fail("a message arrived after the handshake had finished");
        }

        let frame = Frame::new(tag, body);
        let Ok(message) = backend::decode(&frame) else {
            return self.fail("the server sent a message this proxy could not decode");
        };

        match message {
            BackendMessage::Authentication(request) => self.on_auth(request, body),
            BackendMessage::ParameterStatus { name, value } => {
                self.parameters.push((name.to_owned(), value.to_owned()));
                Need::Read
            }
            BackendMessage::BackendKeyData { process_id, secret } => {
                self.backend_key = Some((process_id, secret));
                Need::Read
            }
            BackendMessage::ReadyForQuery(_) => {
                if matches!(self.stage, Stage::Settling) {
                    self.stage = Stage::Ready;
                    Need::Ready
                } else {
                    // Before AuthenticationOk. A server that says it is ready
                    // without having authenticated us is not one to trust with
                    // a tenant's data.
                    self.fail("the server was ready before it authenticated the connection")
                }
            }
            // The server's own words, which is the most useful thing an
            // operator can be given here: "password authentication failed" and
            // "database does not exist" need different fixes.
            BackendMessage::ErrorResponse(error) => self.fail_owned(format!(
                "the server refused the connection: {} ({})",
                error.message, error.code
            )),
            // A notice during startup is informational. Dropping it silently
            // would be worse than forwarding it, but there is nobody to
            // forward it to yet.
            BackendMessage::NoticeResponse(_) => Need::Read,
            _ => self.fail("the server sent something unexpected during startup"),
        }
    }

    /// What to do about an authentication request.
    ///
    /// `body` is the whole message, so the SASL payload after the four-byte
    /// subtype is reachable. The decoder deliberately does not expose it: a
    /// payload is a credential everywhere else it appears.
    fn on_auth(&mut self, request: AuthRequest, body: &[u8]) -> Need {
        let payload = || String::from_utf8_lossy(body.get(4..).unwrap_or_default()).into_owned();

        match request {
            AuthRequest::Ok => {
                self.stage = Stage::Settling;
                Need::Read
            }
            AuthRequest::CleartextPassword => Need::Password,
            AuthRequest::Sasl => Need::SaslStart,
            AuthRequest::SaslContinue => Need::SaslContinue(payload()),
            AuthRequest::SaslFinal => Need::SaslVerify(payload()),
            // Supportable, and deliberately not supported: md5 was deprecated
            // in Postgres 14 and removed as a default, and adding it would put
            // a second hash implementation in this project for the benefit of
            // a server configuration nobody should be running.
            AuthRequest::Md5Password => {
                self.fail("the server asked for md5, which this proxy does not implement")
            }
            // Named rather than described, because the number is what an
            // operator will search for.
            AuthRequest::Other(subtype) => self.fail_owned(format!(
                "the server asked for authentication method {subtype}, \
                 which this proxy does not implement"
            )),
            _ => self.fail("the server asked for an authentication method this proxy cannot name"),
        }
    }

    fn fail(&mut self, reason: &str) -> Need {
        self.fail_owned(reason.to_owned())
    }

    fn fail_owned(&mut self, reason: String) -> Need {
        self.stage = Stage::Failed;
        Need::Fail(PoolError::ConnectFailed {
            server: self.server.clone(),
            reason,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use pgprox_proto::encode;

    fn server() -> ServerId {
        ServerId::new("db-1", 5432)
    }

    /// Splits an encoded backend message into the tag and body the machine
    /// takes, which is what a driver reading frames would hand it.
    fn framed(bytes: &[u8]) -> (Tag, Vec<u8>) {
        use pgprox_proto::frame::{DEFAULT_MAX_FRAME, Decoded, decode};
        let Decoded::Frame(frame, consumed) = decode(bytes, DEFAULT_MAX_FRAME).unwrap() else {
            panic!("the test built a message that does not decode");
        };
        assert_eq!(consumed, bytes.len());
        (frame.tag(), frame.body().to_vec())
    }

    fn message(build: impl FnOnce(&mut Vec<u8>)) -> (Tag, Vec<u8>) {
        let mut out = Vec::new();
        build(&mut out);
        framed(&out)
    }

    fn feed(handshake: &mut UpstreamHandshake, built: &(Tag, Vec<u8>)) -> Need {
        handshake.on_frame(built.0, &built.1)
    }

    fn auth_ok() -> (Tag, Vec<u8>) {
        message(encode::authentication_ok)
    }

    fn ready() -> (Tag, Vec<u8>) {
        message(|out| encode::ready_for_query(out, pgprox_proto::backend::TxStatus::Idle))
    }

    #[test]
    fn a_trust_connection_reaches_ready_and_harvests_its_parameters() {
        let mut handshake = UpstreamHandshake::new(server());
        assert_eq!(handshake.begin(), Need::Startup);
        assert_eq!(feed(&mut handshake, &auth_ok()), Need::Read);

        for (name, value) in [("server_version", "17.2"), ("client_encoding", "UTF8")] {
            assert_eq!(
                feed(
                    &mut handshake,
                    &message(|out| encode::parameter_status(out, name, value))
                ),
                Need::Read
            );
        }
        assert_eq!(feed(&mut handshake, &ready()), Need::Ready);

        assert!(handshake.is_ready());
        assert_eq!(
            handshake.parameters(),
            [
                ("server_version".to_owned(), "17.2".to_owned()),
                ("client_encoding".to_owned(), "UTF8".to_owned()),
            ]
        );
    }

    #[test]
    fn a_cleartext_password_is_asked_for_and_then_the_connection_settles() {
        let mut handshake = UpstreamHandshake::new(server());
        handshake.begin();

        assert_eq!(
            feed(
                &mut handshake,
                &message(encode::authentication_cleartext_password)
            ),
            Need::Password
        );
        assert_eq!(feed(&mut handshake, &auth_ok()), Need::Read);
        assert_eq!(feed(&mut handshake, &ready()), Need::Ready);
    }

    #[test]
    fn a_scram_exchange_routes_each_payload_without_computing_any_of_it() {
        let mut handshake = UpstreamHandshake::new(server());
        handshake.begin();

        assert_eq!(
            feed(
                &mut handshake,
                &message(|out| encode::authentication_sasl(out, &["SCRAM-SHA-256"]))
            ),
            Need::SaslStart
        );
        assert_eq!(
            feed(
                &mut handshake,
                &message(|out| encode::authentication_sasl_continue(
                    out,
                    "r=NONCE,s=U0FMVA==,i=4096"
                ))
            ),
            Need::SaslContinue("r=NONCE,s=U0FMVA==,i=4096".to_owned())
        );
        assert_eq!(
            feed(
                &mut handshake,
                &message(|out| encode::authentication_sasl_final(out, "v=U0lHTg=="))
            ),
            Need::SaslVerify("v=U0lHTg==".to_owned())
        );
        assert_eq!(feed(&mut handshake, &auth_ok()), Need::Read);
        assert_eq!(feed(&mut handshake, &ready()), Need::Ready);
    }

    #[test]
    fn the_servers_own_cancel_key_is_kept() {
        // Cancelling a query means sending this key, not the one the proxy
        // handed its client. Losing it means cancellation silently stops
        // working against the one thing it has to work against.
        let mut handshake = UpstreamHandshake::new(server());
        handshake.begin();
        feed(&mut handshake, &auth_ok());

        let conn = pgprox_core::ids::ConnId::new(pgprox_core::ids::NodeId::new(1), 9);
        let (process_id, secret) = pgprox_proto::backend::key_from_conn_id(conn);
        feed(
            &mut handshake,
            &message(|out| encode::backend_key_data(out, conn)),
        );

        assert_eq!(handshake.backend_key(), Some((process_id, secret)));
    }

    #[test]
    fn md5_is_refused_by_name_rather_than_as_a_generic_failure() {
        // Supportable and deliberately not supported. An operator who sees
        // this knows to change password_encryption; one who sees "connection
        // failed" goes looking at the network.
        let mut handshake = UpstreamHandshake::new(server());
        handshake.begin();

        let mut out = Vec::new();
        // Subtype 5 with a four-byte salt, which is what a real server sends.
        out.push(Tag::AUTHENTICATION.get());
        out.extend_from_slice(&12_u32.to_be_bytes());
        out.extend_from_slice(&5_i32.to_be_bytes());
        out.extend_from_slice(&[1, 2, 3, 4]);

        let Need::Fail(PoolError::ConnectFailed { reason, .. }) =
            feed(&mut handshake, &framed(&out))
        else {
            panic!("an md5 request did not fail the connection");
        };
        assert!(reason.contains("md5"), "{reason}");
    }

    #[test]
    fn an_unknown_authentication_method_names_its_number() {
        // The number is what an operator will search for, and GSSAPI, SSPI and
        // the rest all arrive this way.
        let mut handshake = UpstreamHandshake::new(server());
        handshake.begin();

        let mut out = Vec::new();
        out.push(Tag::AUTHENTICATION.get());
        out.extend_from_slice(&8_u32.to_be_bytes());
        out.extend_from_slice(&7_i32.to_be_bytes());

        let Need::Fail(PoolError::ConnectFailed { reason, .. }) =
            feed(&mut handshake, &framed(&out))
        else {
            panic!("an unknown method did not fail the connection");
        };
        assert!(reason.contains('7'), "{reason}");
    }

    #[test]
    fn a_refusal_carries_the_servers_own_words() {
        // "password authentication failed" and "database does not exist" need
        // different fixes, and this is the only place either is visible.
        let mut handshake = UpstreamHandshake::new(server());
        handshake.begin();

        let mut out = Vec::new();
        out.push(Tag::ERROR_RESPONSE.get());
        let body = b"SFATAL\0C3D000\0Mdatabase \"nope\" does not exist\0\0";
        out.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_be_bytes());
        out.extend_from_slice(body);

        let Need::Fail(PoolError::ConnectFailed { reason, .. }) =
            feed(&mut handshake, &framed(&out))
        else {
            panic!("an ErrorResponse did not fail the connection");
        };
        assert!(reason.contains("does not exist"), "{reason}");
        assert!(reason.contains("3D000"), "{reason}");
    }

    #[test]
    fn a_notice_during_startup_does_not_fail_the_connection() {
        let mut handshake = UpstreamHandshake::new(server());
        handshake.begin();

        let mut out = Vec::new();
        out.push(Tag::NOTICE_RESPONSE.get());
        let body = b"SNOTICE\0C00000\0Msomething informative\0\0";
        out.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_be_bytes());
        out.extend_from_slice(body);

        assert_eq!(feed(&mut handshake, &framed(&out)), Need::Read);
    }

    #[test]
    fn a_server_that_is_ready_before_it_authenticates_is_refused() {
        // Not a real Postgres, then. Trusting it would mean handing a tenant's
        // queries to whatever answered the socket.
        let mut handshake = UpstreamHandshake::new(server());
        handshake.begin();

        assert!(matches!(feed(&mut handshake, &ready()), Need::Fail(_)));
        assert!(!handshake.is_ready());
    }

    #[test]
    fn a_message_after_the_handshake_finished_is_refused() {
        let mut handshake = UpstreamHandshake::new(server());
        handshake.begin();
        feed(&mut handshake, &auth_ok());
        feed(&mut handshake, &ready());

        assert!(matches!(feed(&mut handshake, &auth_ok()), Need::Fail(_)));
    }

    #[test]
    fn an_undecodable_message_fails_the_connection_rather_than_panicking() {
        // These bytes come from the network. A malformed one must not take
        // down a node serving a hundred thousand other connections.
        let mut handshake = UpstreamHandshake::new(server());
        handshake.begin();

        assert!(matches!(
            handshake.on_frame(Tag::READY_FOR_QUERY, &[]),
            Need::Fail(_)
        ));
    }

    #[test]
    fn a_data_row_during_startup_is_refused_rather_than_ignored() {
        let mut handshake = UpstreamHandshake::new(server());
        handshake.begin();

        let mut out = Vec::new();
        out.push(Tag::DATA_ROW.get());
        out.extend_from_slice(&6_u32.to_be_bytes());
        out.extend_from_slice(&0_i16.to_be_bytes());

        assert!(matches!(feed(&mut handshake, &framed(&out)), Need::Fail(_)));
    }
}
