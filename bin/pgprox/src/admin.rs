//! The static-user path: SCRAM in, `SHOW` out.
//!
//! # Why this exists at all
//!
//! ADR 0002 chose JWT for tenants and SCRAM passthrough for everything else,
//! because migrations, monitoring and a human with psql do not have tokens. M6.4
//! built the exchange, M1F.6 to M1F.10 built the crypto, and the listener
//! answered every SCRAM client with a refusal, so the surface those decisions
//! were for had no door.
//!
//! # A static user reaches the proxy, never a database
//!
//! This session never opens an upstream connection. It answers `SHOW` from the
//! same [`Observatory`] the HTTP API reads, which is what ADR 0018 means by the
//! two surfaces being unable to disagree, and it refuses everything else.
//!
//! That is also the security position: a static credential is an operator
//! credential, and an operator credential that could reach a tenant's data
//! would be a way around the whole token path.
//!
//! # The password lives in the environment, not the command line
//!
//! `ps` is readable by every process on the host and a command line is in it.
//! The user's name is an argument because a name is not a secret.

use std::sync::Arc;

use pgprox_admin::Handled;
use pgprox_core::admin::Observatory;
use pgprox_core::error::{AuthRejection, ClientError};
use pgprox_proto::encode;
use pgprox_proto::frame::Tag;
use pgprox_proto::frontend::{self, FrontendMessage};
use pgprox_session::auth::{ScramChallenge, StaticCredentials};
use pgprox_session::shell::{ShellError, Wire};
use tokio::io::{AsyncRead, AsyncWrite};

/// How expensive a stored password is to attack.
///
/// The number Postgres 17 uses. It is not a secret and it travels to the
/// client in the server-first message.
pub const ITERATIONS: u32 = 4096;

/// The environment variable the static user's password comes from.
pub const PASSWORD_VAR: &str = "PGPROX_ADMIN_PASSWORD";

/// The salt offered to a user that does not exist.
///
/// Fixed for the process's lifetime and not a secret. A salt that varied per
/// attempt, or an early failure, would answer "does this account exist" to
/// anyone who asked. See `ScramConfig::mock_salt`.
pub const MOCK_SALT: &str = "cGdwcm94LW1vY2stc2FsdA==";

/// The static users this node accepts, and their keys.
///
/// One implementation of the trait `pgprox-session` declares, and the only
/// place in the binary that touches a password. The keys are derived once at
/// startup, so the password itself is not kept: what is kept is what SCRAM
/// verifies against, which is the same thing Postgres stores.
pub struct StaticAdmin {
    user: String,
    salt: Vec<u8>,
    keys: pgprox_auth::scram::ScramKeys,
}

impl std::fmt::Debug for StaticAdmin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The user, never the keys. `StoredKey` verifies a proof, so a log line
        // carrying one is a log line carrying half a credential.
        f.debug_struct("StaticAdmin")
            .field("user", &self.user)
            .finish_non_exhaustive()
    }
}

impl StaticAdmin {
    /// Derives the keys for one static user.
    ///
    /// # Errors
    ///
    /// Fails when the derivation does, which means the crypto provider is
    /// unavailable, and a node that cannot derive keys cannot verify anyone.
    pub fn new(user: impl Into<String>, password: &str, salt: Vec<u8>) -> Result<Self, String> {
        let keys = pgprox_auth::scram::ScramKeys::derive(password.as_bytes(), &salt, ITERATIONS)
            .map_err(|err| err.to_string())?;
        Ok(Self {
            user: user.into(),
            salt,
            keys,
        })
    }

    /// Which user this accepts.
    #[must_use]
    pub fn user(&self) -> &str {
        &self.user
    }
}

impl StaticCredentials for StaticAdmin {
    fn challenge(&self, user: &str) -> Option<ScramChallenge> {
        // `None` for an unknown user, and the caller answers with a mock
        // challenge instead: an early failure would tell anyone who asked
        // which accounts exist.
        (user == self.user).then(|| ScramChallenge {
            salt: base64_encode(&self.salt),
            iterations: ITERATIONS,
        })
    }

    fn verify(&self, user: &str, auth_message: &str, proof: &str) -> Option<String> {
        if user != self.user {
            return None;
        }
        let proof = base64_decode(proof)?;
        let proof: [u8; 32] = proof.try_into().ok()?;

        // Constant time, inside `pgprox-auth`: this crate does no comparing of
        // its own, so there is no second implementation to get wrong.
        pgprox_auth::scram::verify_client_proof(&proof, &self.keys.stored_key, auth_message)
            .ok()?;
        Some(base64_encode(&pgprox_auth::scram::server_signature(
            &self.keys,
            auth_message,
        )))
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.decode(text).ok()
}

/// Serves an authenticated static user until it disconnects.
///
/// # Errors
///
/// Fails when the socket does. A statement this surface does not answer is
/// reported to the client and the session continues, because an operator
/// mistyping `SHOW POOLS` should not be disconnected for it.
pub async fn serve<S>(
    wire: &mut Wire<S>,
    observatory: &Arc<dyn Observatory>,
) -> Result<(), ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut body = Vec::new();
    loop {
        let tag = wire
            .read_tagged(&mut body, pgprox_proto::frame::DEFAULT_MAX_FRAME)
            .await?;
        if tag == Tag::TERMINATE {
            return Ok(());
        }

        let frame = pgprox_proto::frame::Frame::new(tag, &body);
        // Extended query, COPY, anything else: this surface is a `SHOW`
        // console, and pretending otherwise would leave a driver waiting for a
        // message shape that is never coming.
        let Ok(FrontendMessage::Query { sql }) = frontend::decode(&frame) else {
            answer_error(
                wire,
                &ClientError::ProtocolViolation("this connection answers SHOW only"),
            );
            wire.flush().await?;
            continue;
        };
        let sql = sql.to_owned();

        match pgprox_admin::handle(observatory.as_ref(), &sql).await {
            Ok(Handled::Answered(rows)) => {
                let columns = rows.columns.clone();
                wire.queue(|out| {
                    encode::row_description(out, &columns);
                    for row in &rows.rows {
                        encode::data_row(out, row);
                    }
                    encode::command_complete(out, "SHOW");
                    encode::ready_for_query(out, pgprox_proto::backend::TxStatus::Idle);
                });
            }
            // Not a `SHOW`. There is no upstream to relay it to on this
            // connection, and saying so beats a hang.
            Ok(Handled::Relay) => answer_error(
                wire,
                &ClientError::ProtocolViolation("this connection answers SHOW only"),
            ),
            Ok(Handled::Rejected(_)) => answer_error(
                wire,
                &ClientError::ProtocolViolation("no such SHOW on this proxy"),
            ),
            // `Handled` is non_exhaustive, so a variant added later lands
            // here rather than failing to compile in a binary that cannot be
            // taught about it from this side. Reported, not ignored.
            _ => answer_error(
                wire,
                &ClientError::Internal("the admin surface could not answer"),
            ),
        }
        wire.flush().await?;
    }
}

/// Reports an error and leaves the session open.
fn answer_error<S>(wire: &mut Wire<S>, error: &ClientError)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    wire.queue(|out| {
        encode::error_response(out, error);
        encode::ready_for_query(out, pgprox_proto::backend::TxStatus::Idle);
    });
}

/// The refusal a static user gets when the node has none configured.
#[must_use]
pub const fn not_configured() -> ClientError {
    // The same message a bad token gets: which of the two failed is an
    // operator's business, and telling a caller that static users exist here
    // but they are not one is an oracle.
    ClientError::AuthRefused(AuthRejection::NotPermitted)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {

    /// A slab for a test wire.
    ///
    /// Sized for one connection's worth of borrowing, which is what a test
    /// has. The bound is what makes an exhausted slab reachable in a test at
    /// all, so it is small on purpose.
    fn test_slab() -> std::sync::Arc<pgprox_core::buf::BufferSlab> {
        pgprox_core::buf::BufferSlab::new(pgprox_core::buf::DEFAULT_BUFFER_SIZE, 8)
    }
    use super::*;
    use pgprox_core::admin::FakeObservatory;
    use pgprox_core::ids::NodeId;

    fn admin() -> StaticAdmin {
        StaticAdmin::new("pgprox_admin", "hunter2", b"a-fixed-salt".to_vec()).unwrap()
    }

    #[test]
    fn an_unknown_user_gets_no_challenge() {
        // Which is what stops this answering "does this account exist" to
        // anyone who asks: the caller substitutes a mock challenge.
        assert!(admin().challenge("somebody_else").is_none());
        assert!(admin().challenge("pgprox_admin").is_some());
    }

    #[test]
    fn the_challenge_carries_the_salt_and_iterations_the_keys_were_derived_with() {
        // A challenge that disagreed with the derivation would fail every
        // correct password, which is the bug that looks like a wrong password.
        let challenge = admin().challenge("pgprox_admin").unwrap();
        assert_eq!(challenge.iterations, ITERATIONS);
        assert_eq!(base64_decode(&challenge.salt).unwrap(), b"a-fixed-salt");
    }

    #[test]
    fn a_wrong_proof_is_refused() {
        let admin = admin();
        assert!(
            admin
                .verify("pgprox_admin", "auth-message", "not base64!")
                .is_none()
        );
        assert!(
            admin
                .verify("pgprox_admin", "auth-message", &base64_encode(&[0_u8; 32]))
                .is_none()
        );
    }

    #[test]
    fn the_right_proof_is_accepted_and_answered_with_a_signature() {
        // The round trip SCRAM is: the client proves it knows the password,
        // and the server proves it too, which is what stops a client talking
        // to something that only recorded the exchange.
        let admin = admin();
        let keys =
            pgprox_auth::scram::ScramKeys::derive(b"hunter2", b"a-fixed-salt", ITERATIONS).unwrap();
        let message = "n=pgprox_admin,r=nonce,s=salt,i=4096,c=biws,r=nonce";
        let proof = pgprox_auth::scram::client_proof(&keys, message);

        let signature = admin
            .verify("pgprox_admin", message, &base64_encode(&proof))
            .expect("a correct proof was refused");
        assert_eq!(
            base64_decode(&signature).unwrap(),
            pgprox_auth::scram::server_signature(&keys, message).to_vec()
        );
    }

    #[test]
    fn debug_prints_no_key() {
        // `StoredKey` verifies a proof, so a log line carrying one carries
        // half a credential.
        let rendered = format!("{:?}", admin());
        assert!(rendered.contains("pgprox_admin"));
        assert!(!rendered.contains("stored_key"), "{rendered}");
    }

    /// Runs one statement against the surface and returns the frames it wrote.
    async fn ask(sql: &str) -> Vec<(Tag, Vec<u8>)> {
        use tokio::io::AsyncWriteExt as _;

        let (ours, mut theirs) = tokio::io::duplex(64 * 1024);
        let observatory: Arc<dyn Observatory> = FakeObservatory::new(NodeId::new(1));

        let served = tokio::spawn(async move {
            let mut wire = Wire::new(ours, test_slab());
            serve(&mut wire, &observatory).await
        });

        let mut out = Vec::new();
        pgprox_proto::encode_frontend::query(&mut out, sql);
        theirs.write_all(&out).await.unwrap();

        let frames = read_until_ready(&mut theirs).await;

        let mut bye = Vec::new();
        pgprox_proto::encode_frontend::terminate(&mut bye);
        theirs.write_all(&bye).await.unwrap();
        served.await.unwrap().unwrap();
        frames
    }

    async fn read_until_ready<S: AsyncRead + Unpin>(io: &mut S) -> Vec<(Tag, Vec<u8>)> {
        use tokio::io::AsyncReadExt as _;

        let mut frames = Vec::new();
        loop {
            let mut header = [0_u8; 5];
            io.read_exact(&mut header).await.unwrap();
            let len = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
            let mut body = vec![0; len - 4];
            io.read_exact(&mut body).await.unwrap();

            let tag = Tag(header[0]);
            frames.push((tag, body));
            if tag == Tag::READY_FOR_QUERY {
                return frames;
            }
        }
    }

    #[tokio::test]
    async fn a_show_is_answered_with_rows() {
        let frames = ask("SHOW POOLS").await;
        let tags: Vec<Tag> = frames.iter().map(|(tag, _)| *tag).collect();

        assert!(
            tags.contains(&Tag::ROW_DESCRIPTION),
            "a SHOW answered with no columns: {tags:?}"
        );
        assert_eq!(tags.last(), Some(&Tag::READY_FOR_QUERY));
    }

    #[tokio::test]
    async fn show_cache_reaches_a_psql_session() {
        // The surface an operator is actually in when they want to know
        // whether the cache is on. The rows come from `pgprox-admin`, which has
        // its own tests; what this checks is that the console reaches them,
        // because a command that parses everywhere except here is a command
        // nobody can run.
        let frames = ask("SHOW CACHE").await;
        let tags: Vec<Tag> = frames.iter().map(|(tag, _)| *tag).collect();

        assert!(
            tags.contains(&Tag::ROW_DESCRIPTION),
            "SHOW CACHE answered with no columns: {tags:?}"
        );
        let described = frames
            .iter()
            .find(|(tag, _)| *tag == Tag::ROW_DESCRIPTION)
            .map(|(_, body)| String::from_utf8_lossy(body).into_owned())
            .unwrap_or_default();
        assert!(described.contains("tenants"), "got {described:?}");
        assert!(described.contains("promise"), "got {described:?}");
    }

    #[tokio::test]
    async fn a_statement_that_is_not_a_show_is_refused_and_the_session_continues() {
        // An operator mistyping a command should not be disconnected for it,
        // and there is no upstream on this connection to relay it to.
        let frames = ask("SELECT 1").await;

        assert_eq!(
            frames.first().map(|(tag, _)| *tag),
            Some(Tag::ERROR_RESPONSE)
        );
        assert_eq!(
            frames.last().map(|(tag, _)| *tag),
            Some(Tag::READY_FOR_QUERY)
        );
    }

    #[tokio::test]
    async fn a_show_this_proxy_does_not_have_says_so() {
        let frames = ask("SHOW NOTHING").await;
        let (_, body) = frames.first().unwrap();

        assert_eq!(
            frames.first().map(|(tag, _)| *tag),
            Some(Tag::ERROR_RESPONSE)
        );
        // The code, not the words: `ClientError::client_message` is
        // deliberately vague to clients, and this surface does not get to opt
        // out of that because the caller authenticated.
        assert!(String::from_utf8_lossy(body).contains("08P01"));
    }
}
