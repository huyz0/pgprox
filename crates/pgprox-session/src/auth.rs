//! Checking the credential the handshake asked for.
//!
//! This is the token path: the client sent `AuthenticationCleartextPassword`'s
//! answer, and that answer is a JWT. The static-user SCRAM path is separate.
//!
//! # Why resolution is an output rather than an await
//!
//! Resolving a token means an RPC to the sidecar, which is I/O, and this crate
//! keeps its logic out of the I/O shell. So the machine emits
//! [`Progress::Resolve`] carrying the request, the shell awaits the resolver,
//! and the answer comes back in through [`TokenAuth::on_resolved`].
//!
//! That split is not ceremony. It makes the cases that matter reachable
//! without a sidecar: a token that arrives empty, one that is not UTF-8, a
//! grant that expired between being issued and being used, a sidecar that is
//! down, and a client that sends its password twice.
//!
//! # The token is a bearer credential
//!
//! It is held in a [`SecretString`] from the moment it is parsed, so no
//! `Debug` of any type here can print it. `a_token_cannot_reach_a_log` asserts
//! that rather than trusting it.

use std::net::IpAddr;
use std::time::SystemTime;

use pgprox_core::auth::{AuthError, AuthRequest, Grant};
use pgprox_core::error::{AuthRejection, ClientError};
use pgprox_core::secret::SecretString;

use crate::state::StartupInfo;

/// What the shell should do next.
#[derive(Debug)]
pub enum Progress {
    /// Ask the resolver, then feed the answer to [`TokenAuth::on_resolved`].
    Resolve(Box<AuthRequest>),
    /// Authentication succeeded. Send `AuthenticationOk` and carry on.
    Ready(Box<Grant>),
    /// Send an `ErrorResponse` and close.
    Fail(ClientError),
}

/// How far the token exchange has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Waiting for the client's `PasswordMessage`.
    AwaitingPassword,
    /// The request is out with the resolver.
    Resolving,
    /// Finished, one way or the other.
    Done,
}

/// The token exchange.
///
/// No manual `Debug`: every field is either plain or a [`SecretString`], which
/// redacts itself.
#[derive(Debug, Clone)]
pub struct TokenAuth {
    user: String,
    database: String,
    client_addr: IpAddr,
    stage: Stage,
}

impl TokenAuth {
    /// Starts the exchange for a client whose startup packet has been read.
    #[must_use]
    pub fn new(startup: &StartupInfo, client_addr: IpAddr) -> Self {
        Self {
            user: startup.user.clone(),
            database: startup.database.clone(),
            client_addr,
            stage: Stage::AwaitingPassword,
        }
    }

    /// Whether the exchange has finished.
    #[must_use]
    pub const fn is_done(&self) -> bool {
        matches!(self.stage, Stage::Done)
    }

    /// Feeds in the body of a `PasswordMessage`.
    ///
    /// `body` is the raw frame payload, which Postgres defines as a
    /// null-terminated string. The trailing null is stripped here rather than
    /// by the codec, because the codec deliberately does not look inside this
    /// message at all.
    pub fn on_password(&mut self, body: &[u8]) -> Progress {
        if !matches!(self.stage, Stage::AwaitingPassword) {
            self.stage = Stage::Done;
            return Progress::Fail(ClientError::ProtocolViolation(
                "a second password message arrived",
            ));
        }

        let text = body.strip_suffix(&[0]).unwrap_or(body);

        // Refused here rather than at the sidecar. An empty or non-UTF-8
        // password cannot be a token under any signature, so forwarding it
        // would spend an RPC to be told what is already known, and a flood of
        // empty passwords is the cheapest way to make that matter.
        let Ok(token) = std::str::from_utf8(text) else {
            return self.refuse(AuthRejection::Malformed);
        };
        if token.is_empty() {
            return self.refuse(AuthRejection::Malformed);
        }

        self.stage = Stage::Resolving;
        Progress::Resolve(Box::new(AuthRequest {
            token: SecretString::new(token),
            startup_database: self.database.clone(),
            startup_user: self.user.clone(),
            client_addr: self.client_addr,
        }))
    }

    /// Feeds in what the resolver said.
    ///
    /// `now` is passed rather than read, because this crate's logic holds no
    /// clock. It is what the expiry check below is measured against.
    pub fn on_resolved(&mut self, result: Result<Grant, AuthError>, now: SystemTime) -> Progress {
        if !matches!(self.stage, Stage::Resolving) {
            self.stage = Stage::Done;
            return Progress::Fail(ClientError::ProtocolViolation(
                "a resolver answer arrived with no request outstanding",
            ));
        }
        self.stage = Stage::Done;

        match result {
            Err(err) => Progress::Fail(err.into()),
            // Checked here even though the cache clamps its TTL to the token's
            // expiry. The clamp stops an expired grant being served from cache;
            // it says nothing about one that arrives already expired, and the
            // session is the last place that can refuse it.
            Ok(grant) if grant.is_expired(now) => {
                Progress::Fail(ClientError::AuthRefused(AuthRejection::TokenExpired))
            }
            Ok(grant) => Progress::Ready(Box::new(grant)),
        }
    }

    fn refuse(&mut self, reason: AuthRejection) -> Progress {
        self.stage = Stage::Done;
        Progress::Fail(ClientError::AuthRefused(reason))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use std::time::Duration;

    use pgprox_core::auth::{Backend, ClaimSet, PoolHints, TlsMode};
    use pgprox_core::ids::{ServerId, TenantId};

    fn startup() -> StartupInfo {
        StartupInfo {
            user: "acme_app".to_owned(),
            database: "acme".to_owned(),
            options: Vec::new(),
        }
    }

    fn machine() -> TokenAuth {
        TokenAuth::new(&startup(), IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7)))
    }

    fn grant() -> Grant {
        Grant {
            tenant: TenantId::new("acme"),
            primary: Backend {
                server: ServerId::new("db-1", 5432),
                database: "acme".into(),
                user: "acme_app".into(),
                password: SecretString::new("hunter2"),
                tls: TlsMode::Verified,
            },
            replicas: Vec::new(),
            pool: PoolHints::default(),
            ttl: Duration::from_secs(60),
            claims: ClaimSet {
                subject: Some("acme".to_owned()),
                expires_at: Some(SystemTime::now() + Duration::from_secs(300)),
                issued_at: None,
            },
        }
    }

    #[test]
    fn a_password_becomes_a_resolver_request_carrying_the_startup_fields() {
        let mut auth = machine();
        let Progress::Resolve(request) = auth.on_password(b"a.token.here\0") else {
            panic!("a well-formed token did not reach the resolver");
        };

        assert_eq!(request.token.expose(), "a.token.here");
        assert_eq!(request.startup_user, "acme_app");
        assert_eq!(request.startup_database, "acme");
        assert_eq!(request.client_addr, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7)));
    }

    #[test]
    fn a_password_message_without_its_trailing_null_still_works() {
        // Postgres defines the body as null-terminated, and every driver sends
        // it that way. Being strict here would turn one driver's off-by-one
        // into an authentication failure nobody could diagnose.
        let mut auth = machine();
        let Progress::Resolve(request) = auth.on_password(b"a.token.here") else {
            panic!("a token without its terminator was refused");
        };
        assert_eq!(request.token.expose(), "a.token.here");
    }

    #[test]
    fn an_empty_password_is_refused_without_asking_the_sidecar() {
        // A flood of empty passwords is the cheapest way to turn a proxy into
        // a load generator pointed at the sidecar.
        for body in [b"".as_slice(), b"\0".as_slice()] {
            let mut auth = machine();
            assert!(
                matches!(
                    auth.on_password(body),
                    Progress::Fail(ClientError::AuthRefused(_))
                ),
                "an empty password reached the resolver"
            );
        }
    }

    #[test]
    fn a_non_utf8_password_is_refused_without_asking_the_sidecar() {
        let mut auth = machine();
        assert!(matches!(
            auth.on_password(&[0xff, 0xfe, 0x00]),
            Progress::Fail(ClientError::AuthRefused(_))
        ));
    }

    #[test]
    fn a_resolved_grant_finishes_the_exchange() {
        let mut auth = machine();
        auth.on_password(b"a.token.here\0");
        let Progress::Ready(resolved) = auth.on_resolved(Ok(grant()), SystemTime::now()) else {
            panic!("a valid grant did not authenticate");
        };
        assert_eq!(resolved.tenant, TenantId::new("acme"));
        assert!(auth.is_done());
    }

    #[test]
    fn a_refusal_reaches_the_client_as_one_message_whatever_the_reason() {
        // Telling a caller which part of their credential was wrong is an
        // oracle. The reason survives for the operator, not for the wire.
        for reason in [
            AuthRejection::TokenRejected,
            AuthRejection::TokenExpired,
            AuthRejection::NotPermitted,
        ] {
            let mut auth = machine();
            auth.on_password(b"a.token.here\0");
            let Progress::Fail(err) =
                auth.on_resolved(Err(AuthError::Refused(reason)), SystemTime::now())
            else {
                panic!("a refused token authenticated");
            };
            assert_eq!(err.client_message(), "authentication failed");
        }
    }

    #[test]
    fn a_sidecar_that_is_down_is_not_reported_as_a_bad_password() {
        // The distinction matters operationally: one is the client's problem
        // and the other is ours, and a client told "authentication failed"
        // will keep retrying with a token that was fine all along.
        let mut auth = machine();
        auth.on_password(b"a.token.here\0");
        let Progress::Fail(err) = auth.on_resolved(
            Err(AuthError::Unavailable {
                reason: "connection refused".to_owned(),
            }),
            SystemTime::now(),
        ) else {
            panic!("an unavailable sidecar authenticated the client");
        };
        assert_eq!(err, ClientError::SidecarUnavailable);
    }

    #[test]
    fn a_grant_that_arrives_already_expired_is_refused() {
        // The cache clamps its TTL to the token's expiry, which stops an
        // expired grant being served from cache. It says nothing about one
        // that arrives expired, and this is the last place that can refuse it.
        let mut auth = machine();
        auth.on_password(b"a.token.here\0");
        let expired = SystemTime::now() + Duration::from_secs(3600);
        assert_eq!(
            match auth.on_resolved(Ok(grant()), expired) {
                Progress::Fail(err) => err,
                other => panic!("an expired grant authenticated: {other:?}"),
            },
            ClientError::AuthRefused(AuthRejection::TokenExpired)
        );
    }

    #[test]
    fn a_second_password_message_is_a_protocol_violation() {
        let mut auth = machine();
        auth.on_password(b"a.token.here\0");
        assert!(matches!(
            auth.on_password(b"another.token\0"),
            Progress::Fail(ClientError::ProtocolViolation(_))
        ));
    }

    #[test]
    fn an_answer_with_no_request_outstanding_is_a_protocol_violation() {
        // Not reachable from the wire, but reachable from a shell that lost
        // track of its own state, which is the bug worth catching.
        let mut auth = machine();
        assert!(matches!(
            auth.on_resolved(Ok(grant()), SystemTime::now()),
            Progress::Fail(ClientError::ProtocolViolation(_))
        ));
    }

    #[test]
    fn nothing_is_honoured_after_the_exchange_ends() {
        let mut auth = machine();
        auth.on_password(b"a.token.here\0");
        auth.on_resolved(Ok(grant()), SystemTime::now());

        assert!(matches!(auth.on_password(b"x\0"), Progress::Fail(_)));
        assert!(matches!(
            auth.on_resolved(Ok(grant()), SystemTime::now()),
            Progress::Fail(_)
        ));
    }

    #[test]
    fn a_token_cannot_reach_a_log() {
        // The machine is Debug, and something will eventually log it. The
        // token is in a SecretString from the moment it is parsed, so there is
        // no state in which formatting the machine or its output prints one.
        let mut auth = machine();
        let progress = auth.on_password(b"super.secret.token\0");

        for rendered in [format!("{auth:?}"), format!("{progress:?}")] {
            assert!(
                !rendered.contains("super.secret.token"),
                "the token appeared in a Debug rendering: {rendered}"
            );
        }
    }
}
