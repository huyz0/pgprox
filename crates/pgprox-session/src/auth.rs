//! Checking the credential the handshake asked for.
//!
//! Two paths. [`TokenAuth`] is the ordinary one: the client answered
//! `AuthenticationCleartextPassword` with a JWT. [`ScramAuth`] is how an admin
//! client reaches the `SHOW` pseudo-database with a static credential and no
//! sidecar involved.
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

use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;
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

/// A SCRAM challenge for one user: what `server-first-message` carries.
///
/// Both fields are already base64 where SCRAM says they are, so nothing here
/// encodes or decodes. That is deliberate. See [`StaticCredentials`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScramChallenge {
    /// The user's salt, base64 as it appears on the wire and in Postgres's own
    /// `SCRAM-SHA-256$...` verifier string.
    pub salt: String,
    /// The iteration count.
    pub iterations: u32,
}

/// Where a static user's credential lives, and who does the arithmetic.
///
/// The trait exists because `pgprox-session` may compose `pgprox-proto`,
/// `pgprox-pool` and `pgprox-route`, and not `pgprox-auth`, which is where
/// this project's HMAC and PBKDF2 live. Rather than widen that rule, the
/// exchange is split: this crate owns the message sequence, and whoever
/// implements this trait owns the crypto. The composition root, which may
/// depend on everything, joins them.
///
/// A consequence worth stating: no base64 and no hashing appear in this crate
/// at all, so there is no second implementation of either to drift from the
/// first.
pub trait StaticCredentials: Send + Sync + fmt::Debug {
    /// The challenge for a user, or `None` if there is no such user.
    ///
    /// Returning `None` is safe: the caller substitutes a mock challenge so an
    /// unknown user is indistinguishable from a wrong password. See
    /// [`ScramConfig::mock_salt`].
    fn challenge(&self, user: &str) -> Option<ScramChallenge>;

    /// Verifies a proof, returning the base64 server signature if it is right.
    ///
    /// `auth_message` is the SCRAM `AuthMessage` this crate assembled. Both it
    /// and `proof` are passed as they appear on the wire, so an implementation
    /// needs no knowledge of the message format.
    fn verify(&self, user: &str, auth_message: &str, proof: &str) -> Option<String>;
}

/// So a composition root can share one set of credentials between every
/// session without cloning the keys into each.
///
/// Here rather than in the binary, because the orphan rule puts it here: the
/// trait is this crate's and `Arc` is the standard library's.
impl<T: StaticCredentials + ?Sized> StaticCredentials for Arc<T> {
    fn challenge(&self, user: &str) -> Option<ScramChallenge> {
        self.as_ref().challenge(user)
    }

    fn verify(&self, user: &str, auth_message: &str, proof: &str) -> Option<String> {
        self.as_ref().verify(user, auth_message, proof)
    }
}

/// How the SCRAM exchange behaves.
#[derive(Debug, Clone)]
pub struct ScramConfig {
    /// The salt offered for a user who does not exist.
    ///
    /// Postgres calls this a mock authentication and it is the reason
    /// `SHOW USERS` is not available over an unauthenticated connection: an
    /// unknown user that failed early, or failed with a different salt each
    /// attempt, would answer "does this account exist" to anyone who asked.
    /// It must be fixed for the process's lifetime and it is not a secret.
    pub mock_salt: String,
    /// The iteration count offered with the mock salt.
    pub mock_iterations: u32,
}

/// How far the SCRAM exchange has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScramStage {
    /// Waiting for `SASLInitialResponse`.
    AwaitingInitial,
    /// Waiting for `SASLResponse` carrying the proof.
    AwaitingProof,
    /// Finished, one way or the other.
    Done,
}

/// What the shell should send next in a SCRAM exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaslProgress {
    /// `AuthenticationSASLContinue` with this payload.
    Continue(String),
    /// `AuthenticationSASLFinal` with this payload, then `AuthenticationOk`.
    Final(String),
    /// Send an `ErrorResponse` and close.
    Fail(ClientError),
}

/// The mechanism this proxy offers. Exactly one.
///
/// Not `SCRAM-SHA-256-PLUS`: channel binding ties the exchange to the TLS
/// session, and the proxy terminates TLS itself, so the binding a client would
/// verify is to the proxy rather than to the database. Offering it would state
/// a guarantee that is not being made.
pub const SCRAM_SHA_256: &str = "SCRAM-SHA-256";

/// The base64 of a `n,,` gs2 header, which is what the client echoes in `c=`.
///
/// Hardcoded rather than computed, because computing it would mean a base64
/// implementation in a crate that has deliberately avoided having one, to
/// encode one of two constants.
const GS2_NO_BINDING: &str = "biws";
/// The same for a `y,,` header: a client that supports binding, talking to a
/// server that did not offer it.
const GS2_BINDING_NOT_OFFERED: &str = "eSws";

/// The server side of a SCRAM-SHA-256 exchange.
#[derive(Debug)]
pub struct ScramAuth<C> {
    credentials: C,
    config: ScramConfig,
    user: String,
    server_nonce: String,
    stage: ScramStage,
    known: bool,
    combined_nonce: String,
    expected_channel_binding: &'static str,
    client_first_bare: String,
    server_first: String,
}

impl<C: StaticCredentials> ScramAuth<C> {
    /// Starts an exchange.
    ///
    /// `server_nonce` is supplied rather than generated, because generating it
    /// needs entropy and this crate's logic holds no I/O. The obligation to
    /// make it unpredictable belongs to the caller, and this crate's
    /// `AGENTS.md` says where.
    #[must_use]
    pub fn new(
        credentials: C,
        config: ScramConfig,
        user: impl Into<String>,
        server_nonce: impl Into<String>,
    ) -> Self {
        Self {
            credentials,
            config,
            user: user.into(),
            server_nonce: server_nonce.into(),
            stage: ScramStage::AwaitingInitial,
            known: false,
            combined_nonce: String::new(),
            expected_channel_binding: GS2_NO_BINDING,
            client_first_bare: String::new(),
            server_first: String::new(),
        }
    }

    /// Whether the exchange has finished.
    #[must_use]
    pub const fn is_done(&self) -> bool {
        matches!(self.stage, ScramStage::Done)
    }

    /// Feeds in the body of a `SASLInitialResponse`.
    ///
    /// The body is a mechanism name, a length, and the client's first message.
    pub fn on_initial(&mut self, body: &[u8]) -> SaslProgress {
        if !matches!(self.stage, ScramStage::AwaitingInitial) {
            return self.violation("a second SASLInitialResponse arrived");
        }

        let Some((mechanism, payload)) = split_sasl_initial(body) else {
            return self.violation("malformed SASLInitialResponse");
        };
        if mechanism != SCRAM_SHA_256 {
            // Including SCRAM-SHA-256-PLUS, which was never offered. A client
            // that picks a mechanism the server did not advertise is either
            // broken or probing.
            return self.violation("unsupported SASL mechanism");
        }

        let Some((header, bare)) = split_gs2(payload) else {
            return self.violation("malformed SCRAM client-first-message");
        };
        self.expected_channel_binding = match header {
            "n" => GS2_NO_BINDING,
            "y" => GS2_BINDING_NOT_OFFERED,
            // "p=..." asks for channel binding, which is not offered.
            _ => return self.violation("channel binding was requested and is not offered"),
        };

        let Some(client_nonce) = field(bare, 'r') else {
            return self.violation("SCRAM client-first-message has no nonce");
        };
        if client_nonce.is_empty() {
            return self.violation("SCRAM client-first-message has an empty nonce");
        }

        // The user comes from the startup packet, not from the n= field.
        // Postgres does the same, and libpq sends n= empty.
        let challenge = self.credentials.challenge(&self.user);
        self.known = challenge.is_some();
        let challenge = challenge.unwrap_or_else(|| ScramChallenge {
            salt: self.config.mock_salt.clone(),
            iterations: self.config.mock_iterations,
        });

        bare.clone_into(&mut self.client_first_bare);
        self.combined_nonce = format!("{client_nonce}{}", self.server_nonce);
        self.server_first = format!(
            "r={},s={},i={}",
            self.combined_nonce, challenge.salt, challenge.iterations
        );
        self.stage = ScramStage::AwaitingProof;

        SaslProgress::Continue(self.server_first.clone())
    }

    /// Feeds in the body of a `SASLResponse`, which carries the proof.
    pub fn on_response(&mut self, body: &[u8]) -> SaslProgress {
        if !matches!(self.stage, ScramStage::AwaitingProof) {
            return self.violation("a SASLResponse arrived out of sequence");
        }
        self.stage = ScramStage::Done;

        let Ok(message) = std::str::from_utf8(body) else {
            return self.violation("SCRAM client-final-message is not UTF-8");
        };

        let Some(binding) = field(message, 'c') else {
            return self.violation("SCRAM client-final-message has no channel binding");
        };
        if binding != self.expected_channel_binding {
            // A client that echoes a different gs2 header than it sent is the
            // shape of a downgrade attempt, so this is a violation rather than
            // a failed password.
            return self.violation("SCRAM channel binding does not match the client-first-message");
        }

        let Some(nonce) = field(message, 'r') else {
            return self.violation("SCRAM client-final-message has no nonce");
        };
        if nonce != self.combined_nonce {
            return self.violation("SCRAM nonce does not match");
        }

        let Some(proof) = field(message, 'p') else {
            return self.violation("SCRAM client-final-message has no proof");
        };

        let without_proof = match message.rfind(",p=") {
            Some(at) => &message[..at],
            None => return self.violation("SCRAM client-final-message has no proof"),
        };
        let auth_message = format!(
            "{},{},{}",
            self.client_first_bare, self.server_first, without_proof
        );

        // The lookup result is consulted here rather than at the challenge, so
        // an unknown user spends the same two round trips and fails with the
        // same message as a wrong password.
        let signature = if self.known {
            self.credentials.verify(&self.user, &auth_message, proof)
        } else {
            None
        };

        match signature {
            Some(signature) => SaslProgress::Final(format!("v={signature}")),
            None => SaslProgress::Fail(ClientError::AuthRefused(AuthRejection::TokenRejected)),
        }
    }

    fn violation(&mut self, what: &'static str) -> SaslProgress {
        self.stage = ScramStage::Done;
        SaslProgress::Fail(ClientError::ProtocolViolation(what))
    }
}

/// Splits a `SASLInitialResponse` body into its mechanism and payload.
///
/// The body is a null-terminated mechanism name, a big-endian `i32` length,
/// and that many bytes. A length of `-1` means no initial response, which
/// SCRAM does not permit.
fn split_sasl_initial(body: &[u8]) -> Option<(&str, &str)> {
    let end = body.iter().position(|b| *b == 0)?;
    let mechanism = std::str::from_utf8(&body[..end]).ok()?;
    let rest = body.get(end + 1..)?;
    let (len, payload) = rest.split_at_checked(4)?;
    let len = usize::try_from(i32::from_be_bytes(len.try_into().ok()?)).ok()?;
    std::str::from_utf8(payload.get(..len)?)
        .ok()
        .map(|payload| (mechanism, payload))
}

/// Splits a client-first-message into its gs2 flag and its bare part.
///
/// The header is `<flag>,<authzid>,` and the flag is what decides which
/// channel binding the client will echo back.
fn split_gs2(message: &str) -> Option<(&str, &str)> {
    let (flag, rest) = message.split_once(',')?;
    let (_authzid, bare) = rest.split_once(',')?;
    Some((flag, bare))
}

/// Reads one `key=value` attribute out of a comma-separated SCRAM message.
fn field(message: &str, key: char) -> Option<&str> {
    message
        .split(',')
        .find_map(|part| part.strip_prefix(key)?.strip_prefix('='))
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
        // Both ends of the flag. Only the second was ever asserted, so a
        // machine that reported itself finished before it had a grant went
        // unnoticed, and that is a session admitted without one.
        assert!(!auth.is_done());
        auth.on_password(b"a.token.here\0");
        assert!(!auth.is_done());
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

    /// A credential store that knows one user and one proof.
    ///
    /// Behaves like the real thing rather than recording calls: it answers a
    /// challenge, and it accepts exactly one proof for exactly one auth
    /// message, so a machine that assembled the auth message wrongly fails.
    #[derive(Debug)]
    struct OneUser {
        user: &'static str,
        proof: &'static str,
    }

    impl StaticCredentials for OneUser {
        fn challenge(&self, user: &str) -> Option<ScramChallenge> {
            (user == self.user).then(|| ScramChallenge {
                salt: "QSXCR+Q6sek8bf92".to_owned(),
                iterations: 4096,
            })
        }

        fn verify(&self, user: &str, auth_message: &str, proof: &str) -> Option<String> {
            // The auth message is checked for shape rather than value: this
            // fake has no crypto, and asserting the three parts are present is
            // what catches a machine that assembled it out of order.
            let parts: Vec<&str> = auth_message.split(',').collect();
            (user == self.user
                && proof == self.proof
                && parts.iter().any(|p| p.starts_with("r="))
                && auth_message.contains(",i=4096,")
                && !auth_message.contains(",p="))
            .then(|| "c2lnbmF0dXJl".to_owned())
        }
    }

    fn scram() -> ScramAuth<OneUser> {
        ScramAuth::new(
            OneUser {
                user: "pgprox_admin",
                proof: "cHJvb2Y=",
            },
            ScramConfig {
                mock_salt: "bW9ja3NhbHQ=".to_owned(),
                mock_iterations: 4096,
            },
            "pgprox_admin",
            "SERVERNONCE",
        )
    }

    #[test]
    fn credentials_behind_an_arc_answer_the_same_as_the_credentials() {
        // The blanket impl exists so a composition root can share one set of
        // keys between every session, which means the shared path is the one
        // production runs and the bare one is the one tests ran. Forwarding
        // that returned `None` would refuse every login on a real node while
        // every test here passed.
        let one = OneUser {
            user: "pgprox_admin",
            proof: "cHJvb2Y=",
        };
        let shared = Arc::new(OneUser {
            user: "pgprox_admin",
            proof: "cHJvb2Y=",
        });

        assert_eq!(
            shared.challenge("pgprox_admin"),
            one.challenge("pgprox_admin")
        );
        assert!(shared.challenge("pgprox_admin").is_some());
        assert!(shared.challenge("somebody_else").is_none());

        let auth_message = "n=,r=CLIENTNONCE,r=CLIENTNONCESERVERNONCE,s=QSXCR+Q6sek8bf92,i=4096,c=biws,r=CLIENTNONCESERVERNONCE";
        assert_eq!(
            shared.verify("pgprox_admin", auth_message, "cHJvb2Y="),
            one.verify("pgprox_admin", auth_message, "cHJvb2Y="),
        );
        assert_eq!(
            shared.verify("pgprox_admin", auth_message, "cHJvb2Y="),
            Some("c2lnbmF0dXJl".to_owned()),
        );
        assert!(
            shared
                .verify("pgprox_admin", auth_message, "d3Jvbmc=")
                .is_none()
        );
    }

    fn initial(mechanism: &str, payload: &str) -> Vec<u8> {
        let mut body = mechanism.as_bytes().to_vec();
        body.push(0);
        body.extend_from_slice(&i32::try_from(payload.len()).unwrap().to_be_bytes());
        body.extend_from_slice(payload.as_bytes());
        body
    }

    #[test]
    fn a_correct_proof_completes_the_exchange() {
        let mut auth = scram();
        assert!(!auth.is_done());
        let SaslProgress::Continue(server_first) =
            auth.on_initial(&initial(SCRAM_SHA_256, "n,,n=,r=CLIENTNONCE"))
        else {
            panic!("a well-formed client-first-message was refused");
        };
        // Not after the challenge either: the proof has not arrived yet, and a
        // machine that called itself finished here would let one through.
        assert!(!auth.is_done());
        assert_eq!(
            server_first,
            "r=CLIENTNONCESERVERNONCE,s=QSXCR+Q6sek8bf92,i=4096"
        );

        let final_message = auth.on_response(b"c=biws,r=CLIENTNONCESERVERNONCE,p=cHJvb2Y=");
        assert_eq!(
            final_message,
            SaslProgress::Final("v=c2lnbmF0dXJl".to_owned())
        );
        assert!(auth.is_done());
    }

    #[test]
    fn a_wrong_proof_is_refused_as_an_authentication_failure() {
        let mut auth = scram();
        auth.on_initial(&initial(SCRAM_SHA_256, "n,,n=,r=CLIENTNONCE"));
        assert_eq!(
            auth.on_response(b"c=biws,r=CLIENTNONCESERVERNONCE,p=d3Jvbmc="),
            SaslProgress::Fail(ClientError::AuthRefused(AuthRejection::TokenRejected))
        );
    }

    #[test]
    fn an_unknown_user_is_indistinguishable_from_a_wrong_password() {
        // Postgres calls this mock authentication. A user that failed early,
        // or that got a different salt each attempt, would answer "does this
        // account exist" to anyone who asked.
        let mut known = scram();
        let mut unknown = ScramAuth::new(
            OneUser {
                user: "pgprox_admin",
                proof: "cHJvb2Y=",
            },
            ScramConfig {
                mock_salt: "bW9ja3NhbHQ=".to_owned(),
                mock_iterations: 4096,
            },
            "nobody",
            "SERVERNONCE",
        );

        let first = known.on_initial(&initial(SCRAM_SHA_256, "n,,n=,r=CLIENTNONCE"));
        let other = unknown.on_initial(&initial(SCRAM_SHA_256, "n,,n=,r=CLIENTNONCE"));
        assert!(
            matches!(first, SaslProgress::Continue(_))
                && matches!(other, SaslProgress::Continue(_)),
            "an unknown user was told so before the proof: {other:?}"
        );

        let response = b"c=biws,r=CLIENTNONCESERVERNONCE,p=cHJvb2Y=";
        assert_eq!(
            unknown.on_response(response),
            SaslProgress::Fail(ClientError::AuthRefused(AuthRejection::TokenRejected)),
        );
        assert!(
            matches!(known.on_response(response), SaslProgress::Final(_)),
            "the known user stopped working, so this proves nothing"
        );
    }

    #[test]
    fn the_mock_challenge_is_stable_across_attempts() {
        // A salt that varied per attempt is the same oracle by another route.
        let payload = initial(SCRAM_SHA_256, "n,,n=,r=CLIENTNONCE");
        let mut first = ScramAuth::new(
            OneUser {
                user: "pgprox_admin",
                proof: "cHJvb2Y=",
            },
            ScramConfig {
                mock_salt: "bW9ja3NhbHQ=".to_owned(),
                mock_iterations: 4096,
            },
            "nobody",
            "SERVERNONCE",
        );
        let mut again = ScramAuth::new(
            OneUser {
                user: "pgprox_admin",
                proof: "cHJvb2Y=",
            },
            ScramConfig {
                mock_salt: "bW9ja3NhbHQ=".to_owned(),
                mock_iterations: 4096,
            },
            "nobody",
            "SERVERNONCE",
        );
        assert_eq!(first.on_initial(&payload), again.on_initial(&payload));
    }

    #[test]
    fn channel_binding_is_refused_because_it_was_never_offered() {
        // The proxy terminates TLS, so a binding the client verified would be
        // to the proxy rather than to the database. Offering it would state a
        // guarantee that is not being made.
        let mut auth = scram();
        assert!(matches!(
            auth.on_initial(&initial(
                SCRAM_SHA_256,
                "p=tls-server-end-point,,n=,r=CLIENTNONCE"
            )),
            SaslProgress::Fail(ClientError::ProtocolViolation(_))
        ));
    }

    #[test]
    fn a_mechanism_that_was_not_offered_is_refused() {
        let mut auth = scram();
        assert!(matches!(
            auth.on_initial(&initial("SCRAM-SHA-256-PLUS", "n,,n=,r=CLIENTNONCE")),
            SaslProgress::Fail(ClientError::ProtocolViolation(_))
        ));
    }

    #[test]
    fn a_client_that_did_not_offer_binding_may_still_say_it_supports_it() {
        // The y,, header: a client that knows about channel binding, talking
        // to a server that did not advertise it. Its c= echo differs, and
        // rejecting it would break every modern driver against a proxy that
        // deliberately does not offer PLUS.
        let mut auth = scram();
        assert!(matches!(
            auth.on_initial(&initial(SCRAM_SHA_256, "y,,n=,r=CLIENTNONCE")),
            SaslProgress::Continue(_)
        ));
        assert!(matches!(
            auth.on_response(b"c=eSws,r=CLIENTNONCESERVERNONCE,p=cHJvb2Y="),
            SaslProgress::Final(_)
        ));
    }

    #[test]
    fn echoing_a_different_gs2_header_is_a_downgrade_attempt() {
        let mut auth = scram();
        auth.on_initial(&initial(SCRAM_SHA_256, "y,,n=,r=CLIENTNONCE"));
        assert!(
            matches!(
                auth.on_response(b"c=biws,r=CLIENTNONCESERVERNONCE,p=cHJvb2Y="),
                SaslProgress::Fail(ClientError::ProtocolViolation(_))
            ),
            "a client that changed its gs2 header between messages was accepted"
        );
    }

    #[test]
    fn a_nonce_that_does_not_carry_the_server_part_is_refused() {
        // Without this the exchange is replayable: the server's nonce is the
        // only thing making each one unique.
        let mut auth = scram();
        auth.on_initial(&initial(SCRAM_SHA_256, "n,,n=,r=CLIENTNONCE"));
        assert!(matches!(
            auth.on_response(b"c=biws,r=CLIENTNONCE,p=cHJvb2Y="),
            SaslProgress::Fail(ClientError::ProtocolViolation(_))
        ));
    }

    #[test]
    fn malformed_sasl_messages_are_refused_rather_than_guessed_at() {
        for body in [
            b"".as_slice(),
            b"SCRAM-SHA-256".as_slice(),
            b"SCRAM-SHA-256\0\0\0".as_slice(),
        ] {
            let mut auth = scram();
            assert!(
                matches!(
                    auth.on_initial(body),
                    SaslProgress::Fail(ClientError::ProtocolViolation(_))
                ),
                "a truncated SASLInitialResponse was accepted: {body:?}"
            );
        }

        let mut truncated = scram();
        let mut body = initial(SCRAM_SHA_256, "n,,n=,r=CLIENTNONCE");
        body.truncate(body.len() - 4);
        assert!(matches!(
            truncated.on_initial(&body),
            SaslProgress::Fail(ClientError::ProtocolViolation(_))
        ));
    }

    #[test]
    fn a_client_first_message_without_a_nonce_is_refused() {
        for payload in ["n,,n=", "n,,n=,r=", "no-commas-here"] {
            let mut auth = scram();
            assert!(
                matches!(
                    auth.on_initial(&initial(SCRAM_SHA_256, payload)),
                    SaslProgress::Fail(ClientError::ProtocolViolation(_))
                ),
                "{payload} was accepted as a client-first-message"
            );
        }
    }

    #[test]
    fn a_client_final_message_missing_a_field_is_refused() {
        for payload in [
            "r=CLIENTNONCESERVERNONCE,p=cHJvb2Y=",
            "c=biws,p=cHJvb2Y=",
            "c=biws,r=CLIENTNONCESERVERNONCE",
        ] {
            let mut auth = scram();
            auth.on_initial(&initial(SCRAM_SHA_256, "n,,n=,r=CLIENTNONCE"));
            assert!(
                matches!(
                    auth.on_response(payload.as_bytes()),
                    SaslProgress::Fail(ClientError::ProtocolViolation(_))
                ),
                "{payload} was accepted as a client-final-message"
            );
        }
    }

    #[test]
    fn a_non_utf8_client_final_message_is_refused() {
        let mut auth = scram();
        auth.on_initial(&initial(SCRAM_SHA_256, "n,,n=,r=CLIENTNONCE"));
        assert!(matches!(
            auth.on_response(&[0xff, 0xfe]),
            SaslProgress::Fail(ClientError::ProtocolViolation(_))
        ));
    }

    #[test]
    fn sasl_messages_out_of_sequence_are_refused() {
        let mut early = scram();
        assert!(matches!(
            early.on_response(b"c=biws,r=x,p=y"),
            SaslProgress::Fail(ClientError::ProtocolViolation(_))
        ));

        let mut twice = scram();
        let payload = initial(SCRAM_SHA_256, "n,,n=,r=CLIENTNONCE");
        twice.on_initial(&payload);
        assert!(matches!(
            twice.on_initial(&payload),
            SaslProgress::Fail(ClientError::ProtocolViolation(_))
        ));
    }

    #[test]
    fn a_scram_failure_tells_the_client_exactly_what_a_bad_token_does() {
        // Two authentication paths that answer differently are two oracles.
        let mut auth = scram();
        auth.on_initial(&initial(SCRAM_SHA_256, "n,,n=,r=CLIENTNONCE"));
        let SaslProgress::Fail(err) =
            auth.on_response(b"c=biws,r=CLIENTNONCESERVERNONCE,p=d3Jvbmc=")
        else {
            panic!("a wrong proof authenticated");
        };
        assert_eq!(err.client_message(), "authentication failed");
    }
}
