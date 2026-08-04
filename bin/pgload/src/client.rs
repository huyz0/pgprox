//! One client connection, from startup to the transactions it runs.
//!
//! Generic over the stream so the whole conversation can be tested against a
//! fake server in memory. The socket only appears in [`crate::run`].
//!
//! # Both protocols
//!
//! The workload declares what share of statements go through the extended
//! protocol, and this sends them that way: `Parse` under a name the connection
//! reuses, then `Bind`, `Execute` and `Sync`. Every mainstream driver works
//! like that, and it is the path whose statement mapping deadlocked twice
//! during M6, so a load client that only sent simple queries would measure a
//! proxy nobody deploys.
//!
//! A name is parsed once per connection and reused after that, which is what a
//! driver's statement cache does and what makes the proxy's mapping work for
//! its living rather than on the first statement only.

use pgprox_load::sampler::Transaction;
use std::collections::HashSet;

use pgprox_auth::scram::{ClientExchange, SCRAM_SHA_256};
use pgprox_core::secret::SecretString;
use pgprox_proto::backend::{AuthRequest, BackendMessage};
use pgprox_proto::frame::{Decoded, Frame, Tag, decode};
use pgprox_proto::{backend, encode_frontend};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// The largest frame this client will accept, matching the proxy's own limit.
const MAX_FRAME: usize = 64 * 1024 * 1024;

/// The comment the proxy reads as "this one may go to a replica".
///
/// A statement-level override rather than a session-level `SET`, because the
/// workload mixes eligible and ineligible reads inside one session and a
/// session-level setting could not express that.
const REPLICA_HINT: &str = "/* pgprox:replica */ ";

/// What went wrong on one connection.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SessionError {
    /// The other end went away.
    #[error("disconnected")]
    Disconnected,
    /// The socket failed.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// The bytes were not a frame.
    #[error("frame: {0}")]
    Frame(#[from] pgprox_proto::frame::DecodeError),
    /// The server refused, or a statement failed.
    ///
    /// The SQLSTATE travels with the message because one code changes what the
    /// caller does with it: `57P01` is the node asking this client to go
    /// somewhere else, which is a reconnect rather than a failure. Everything
    /// else here is a failure.
    #[error("server: {message}")]
    Server {
        /// The five-character SQLSTATE, empty when the server sent none.
        code: String,
        /// What it said.
        message: String,
    },
    /// The server asked for a credential this client cannot produce.
    #[error("authentication: {0}")]
    Auth(String),
}

/// The SQLSTATE a node sends a client it wants somewhere else.
///
/// `admin_shutdown`. The drain sends it to idle clients, the shed sends it to
/// clients it is moving toward their home node, and both expect the client to
/// reconnect: it is the code every mainstream driver already reconnects from,
/// which is why it was chosen.
pub const ADMIN_SHUTDOWN: &str = "57P01";

/// A transaction that did not finish.
#[derive(Debug)]
pub struct Failed {
    /// Why.
    pub error: SessionError,
    /// Whether any statement in the transaction had already succeeded.
    ///
    /// This is what separates a relocation from a loss. A node draining
    /// gracefully closes a connection between transactions, so nothing had
    /// started and the client reconnects having lost nothing. The same code
    /// arriving after a statement has succeeded is the force-close at the end
    /// of `drain_grace`, and that lost work.
    pub work_lost: bool,
}

impl Failed {
    /// Whether this is a node relocating a client rather than a failure.
    #[must_use]
    pub fn is_relocation(&self) -> bool {
        !self.work_lost
            && matches!(&self.error, SessionError::Server { code, .. } if code == ADMIN_SHUTDOWN)
    }
}

/// A connection that has finished starting up.
#[derive(Debug)]
pub struct Session<S> {
    io: S,
    read: Vec<u8>,
    write: Vec<u8>,
    /// Statement names this connection has already parsed.
    ///
    /// A driver's statement cache, in the one form that matters here: the
    /// second use of a name sends `Bind` alone, which is where a proxy that
    /// mapped the name wrongly stops working.
    prepared: HashSet<String>,
}

/// The mechanisms an `AuthenticationSASL` body offers.
///
/// The body is the four-byte subtype and then a run of null-terminated names
/// ending in an empty one. Parsed rather than assumed, so a server offering
/// only `SCRAM-SHA-256-PLUS` is refused by name instead of being answered with
/// a mechanism it did not list.
fn sasl_mechanisms(body: &[u8]) -> Vec<String> {
    body.get(4..)
        .unwrap_or_default()
        .split(|byte| *byte == 0)
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .filter(|name| !name.is_empty())
        .collect()
}

/// The answer to `AuthenticationMD5Password`.
///
/// `"md5" + hex(md5(hex(md5(password + username)) + salt))`, which is Postgres's
/// own construction: the inner digest is what the server stores, so the salt is
/// what stops the stored value being a password equivalent on the wire.
///
/// The inner hex is lowercase and that is load-bearing rather than cosmetic. It
/// is the string the server hashes on its side, so a client that upper-cased it
/// would compute a different digest and fail with a correct implementation of
/// everything else.
fn md5_password(user: &str, password: &str, salt: &[u8]) -> String {
    use md5::{Digest as _, Md5};

    let mut inner = Md5::new();
    inner.update(password.as_bytes());
    inner.update(user.as_bytes());
    let inner = format!("{:x}", inner.finalize());

    let mut outer = Md5::new();
    outer.update(inner.as_bytes());
    outer.update(salt);
    format!("md5{:x}", outer.finalize())
}

/// The payload of a SASL challenge or result, after the four-byte subtype.
///
/// `from_utf8` rather than `from_utf8_lossy`: every field of a SCRAM message is
/// base64 or a bare token, so bytes that are not UTF-8 mean this is not a SCRAM
/// message, and replacing them would hand the exchange something that fails
/// later with a worse error.
fn sasl_payload(body: &[u8]) -> Result<String, SessionError> {
    let bytes = body.get(4..).unwrap_or_default();
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| SessionError::Auth("the SASL payload is not UTF-8".to_owned()))
}

impl<S: AsyncRead + AsyncWrite + Unpin> Session<S> {
    /// Sends the startup packet and answers whatever the server asks for.
    ///
    /// # Errors
    ///
    /// Fails on a socket error, on a refusal, and on any authentication method
    /// other than "none", "send it in the clear", and SCRAM-SHA-256. MD5 is
    /// refused by name rather than silently: a load client that could not
    /// authenticate has to say why, since the alternative is a run reporting
    /// zero transactions and no reason.
    ///
    /// SCRAM was on that list until `M32.1`. It is here because `pgbouncer` and
    /// `pgcat` both authenticate clients that way, so without it there was no
    /// comparison to run against either.
    pub async fn start(
        io: S,
        user: &str,
        database: &str,
        password: &str,
    ) -> Result<Self, SessionError> {
        let mut session = Self {
            io,
            read: Vec::new(),
            write: Vec::new(),
            prepared: HashSet::new(),
        };

        let mut packet = Vec::new();
        encode_frontend::startup_message(
            &mut packet,
            pgprox_proto::encode::PROTOCOL_3_0,
            &[
                ("user", user),
                ("database", database),
                ("application_name", "pgload"),
            ],
        );
        session.io.write_all(&packet).await?;
        session.io.flush().await?;

        session.authenticate(user, password).await?;
        Ok(session)
    }

    /// Runs one transaction, and returns when the server is ready again.
    ///
    /// A transaction of more than one statement is wrapped in `BEGIN` and
    /// `COMMIT`, because that is what makes the proxy hold a connection across
    /// statements and holding is the behaviour worth measuring.
    ///
    /// # Errors
    ///
    /// Fails on a socket error and on any `ErrorResponse`. The caller counts
    /// it and opens a new connection: a session that has seen an error may be
    /// in a transaction the client did not open.
    pub async fn transaction(&mut self, transaction: &Transaction) -> Result<(), Failed> {
        // Whether anything in this transaction has succeeded yet, which is the
        // only thing that separates a node relocating this client from a node
        // taking work away from it.
        let mut started = false;
        let step = |result: Result<(), SessionError>, started: bool| {
            result.map_err(|error| Failed {
                error,
                work_lost: started,
            })
        };

        let wrapped = transaction.statements.len() > 1;
        if wrapped {
            step(self.simple_query("BEGIN").await, started)?;
            started = true;
        }
        for statement in &transaction.statements {
            let sql = if statement.replica_eligible {
                format!("{REPLICA_HINT}{}", statement.sql)
            } else {
                statement.sql.clone()
            };
            let sent = if statement.prepared {
                self.extended_query(&statement.name, &sql).await
            } else {
                self.simple_query(&sql).await
            };
            step(sent, started)?;
            started = true;
        }
        if wrapped {
            step(self.simple_query("COMMIT").await, started)?;
        }
        Ok(())
    }

    /// Says goodbye, so the server sees a close rather than a dropped socket.
    ///
    /// Churn is part of the workload, and a proxy that saw every churned
    /// connection as an abrupt disconnect would be measured on its error path
    /// rather than on its close path.
    ///
    /// # Errors
    ///
    /// Fails when the socket does, which for a connection being closed anyway
    /// is worth reporting but not worth retrying.
    pub async fn terminate(&mut self) -> Result<(), SessionError> {
        self.write.clear();
        encode_frontend::terminate(&mut self.write);
        self.io.write_all(&self.write).await?;
        self.io.flush().await?;
        Ok(())
    }

    async fn simple_query(&mut self, sql: &str) -> Result<(), SessionError> {
        self.write.clear();
        encode_frontend::query(&mut self.write, sql);
        self.io.write_all(&self.write).await?;
        self.io.flush().await?;
        self.until_ready().await
    }

    /// Reads until the server says it is ready again.
    async fn until_ready(&mut self) -> Result<(), SessionError> {
        // Read to `ReadyForQuery`, keeping the first error rather than the
        // last: the first says what went wrong, and the ones after it are
        // usually "current transaction is aborted".
        let mut failure = None;
        loop {
            let (tag, body) = match self.frame().await {
                Ok(frame) => frame,
                // A fatal error is an `ErrorResponse` and then a closed socket,
                // with no `ReadyForQuery` after it, which is what Postgres and
                // this proxy both do. Reporting the disconnect instead of the
                // message loses the only part that says why: a run full of
                // "disconnected" sent this milestone looking for a proxy bug
                // when the proxy was answering 53300 correctly.
                Err(SessionError::Disconnected | SessionError::Io(_)) if failure.is_some() => {
                    let (code, message) =
                        failure.unwrap_or_else(|| (String::new(), "the server closed".to_owned()));
                    return Err(SessionError::Server { code, message });
                }
                Err(other) => return Err(other),
            };
            let frame = Frame::new(tag, &body);
            match backend::decode(&frame) {
                Ok(BackendMessage::ReadyForQuery(_)) => {
                    return match failure {
                        Some((code, message)) => Err(SessionError::Server { code, message }),
                        None => Ok(()),
                    };
                }
                Ok(BackendMessage::ErrorResponse(fields)) => {
                    failure
                        .get_or_insert_with(|| (fields.code.to_owned(), fields.message.to_owned()));
                }
                _ => {}
            }
        }
    }

    /// Runs one statement through the extended protocol.
    ///
    /// `Parse` only the first time this connection sees the name, which is
    /// what a driver's statement cache does. After that it is `Bind`,
    /// `Execute` and `Sync`, and the proxy has to know which upstream
    /// connection holds that name.
    async fn extended_query(&mut self, name: &str, sql: &str) -> Result<(), SessionError> {
        // Named per statement shape and per connection. Two connections using
        // the same name for the same SQL is exactly the case the proxy's
        // mapping exists for.
        let statement = format!("pgload_{name}");
        self.write.clear();
        if !self.prepared.contains(&statement) {
            encode_frontend::parse(&mut self.write, &statement, sql);
        }
        encode_frontend::bind(&mut self.write, "", &statement);
        encode_frontend::execute(&mut self.write, "");
        encode_frontend::sync(&mut self.write);
        self.io.write_all(&self.write).await?;
        self.io.flush().await?;

        let outcome = self.until_ready().await;
        if outcome.is_ok() {
            // Only on success: a `Parse` that failed left nothing behind, and
            // remembering it would send a `Bind` for a statement that does not
            // exist for the rest of this connection's life.
            self.prepared.insert(statement);
        }
        outcome
    }

    async fn authenticate(&mut self, user: &str, password: &str) -> Result<(), SessionError> {
        // Built on the first `Sasl` and kept for the two messages after it. The
        // exchange is `pgprox-auth`'s, which is where the proxy's own client
        // half lives too: one SCRAM client in the workspace, because two would
        // be two chances to get an authentication exchange wrong. `M32.1`.
        let mut exchange = ClientExchange::default();
        let secret = SecretString::new(password.to_owned());

        loop {
            let (tag, body) = self.frame().await?;
            let frame = Frame::new(tag, &body);
            match backend::decode(&frame) {
                Ok(BackendMessage::ReadyForQuery(_)) => return Ok(()),
                Ok(BackendMessage::ErrorResponse(fields)) => {
                    return Err(SessionError::Server {
                        code: fields.code.to_owned(),
                        message: fields.message.to_owned(),
                    });
                }
                Ok(BackendMessage::Authentication(request)) => match request {
                    AuthRequest::Ok => {}
                    AuthRequest::CleartextPassword => {
                        self.write.clear();
                        encode_frontend::password_message(&mut self.write, password);
                        self.send().await?;
                    }
                    AuthRequest::Md5Password => {
                        // `pgcat` offers clients nothing else, and an arm of
                        // the comparison is worth more than the point that
                        // would be made by refusing. `M32.6`.
                        //
                        // The proxy still refuses MD5, for the reason on its
                        // own dial path: this is a measurement tool, and it has
                        // to speak what the thing it measures asks for.
                        let salt = body.get(4..8).ok_or_else(|| {
                            SessionError::Auth("the md5 request carried no salt".to_owned())
                        })?;
                        let answer = md5_password(user, password, salt);
                        self.write.clear();
                        encode_frontend::password_message(&mut self.write, &answer);
                        self.send().await?;
                    }
                    AuthRequest::Sasl => {
                        // The body lists what the server offers and this client
                        // has one mechanism, so a server offering only
                        // `SCRAM-SHA-256-PLUS` is refused by name below rather
                        // than answered with something it did not ask for.
                        let offered = sasl_mechanisms(&body);
                        if !offered.iter().any(|m| m == SCRAM_SHA_256) {
                            return Err(SessionError::Auth(format!(
                                "the server offers {offered:?} and this client has {SCRAM_SHA_256}"
                            )));
                        }
                        let first = exchange.client_first(user);
                        self.write.clear();
                        encode_frontend::sasl_initial_response(
                            &mut self.write,
                            SCRAM_SHA_256,
                            &first,
                        );
                        self.send().await?;
                    }
                    AuthRequest::SaslContinue => {
                        let server_first = sasl_payload(&body)?;
                        let final_message = exchange
                            .client_final(&secret, &server_first)
                            .map_err(|err| SessionError::Auth(err.to_string()))?;
                        self.write.clear();
                        encode_frontend::sasl_response(&mut self.write, &final_message);
                        self.send().await?;
                    }
                    AuthRequest::SaslFinal => {
                        // Checked rather than skipped. SCRAM is mutual, and a
                        // client that does not verify the server's signature
                        // completes a handshake with whatever answered the
                        // socket. A load client has no secrets to lose and it
                        // is still the wrong thing to demonstrate.
                        let server_final = sasl_payload(&body)?;
                        exchange
                            .verify(&server_final)
                            .map_err(|err| SessionError::Auth(err.to_string()))?;
                    }
                    other => {
                        return Err(SessionError::Auth(format!(
                            "{other:?} is not a method this client can answer"
                        )));
                    }
                },
                _ => {}
            }
        }
    }

    /// Writes what is queued and flushes it.
    async fn send(&mut self) -> Result<(), SessionError> {
        self.io.write_all(&self.write).await?;
        self.io.flush().await?;
        Ok(())
    }

    /// Reads one tagged frame, filling from the socket as needed.
    async fn frame(&mut self) -> Result<(Tag, Vec<u8>), SessionError> {
        loop {
            match decode(&self.read, MAX_FRAME)? {
                Decoded::Frame(frame, consumed) => {
                    let tag = frame.tag();
                    let body = frame.body().to_vec();
                    self.read.drain(..consumed);
                    return Ok((tag, body));
                }
                Decoded::Incomplete { .. } => {
                    let mut chunk = [0_u8; 8192];
                    let read = self.io.read(&mut chunk).await?;
                    if read == 0 {
                        return Err(SessionError::Disconnected);
                    }
                    self.read.extend_from_slice(&chunk[..read]);
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {

    #[test]
    fn the_frame_cap_is_the_size_it_says() {
        // `M17.5` found `64 * 1024 * 1024` able to become `64 + 1024 + 1024`
        // unnoticed, which is 65 KiB and would make this client refuse results
        // the proxy relays fine. It matches the proxy's own relay limit by
        // intent, so it is asserted rather than described.
        assert_eq!(MAX_FRAME, 64 * 1024 * 1024);
    }
    use super::*;
    use pgprox_core::error::ClientError;
    use pgprox_load::sampler::Planned;
    use pgprox_load::workload::Kind;
    use pgprox_proto::backend::TxStatus;
    use pgprox_proto::encode;
    use tokio::io::DuplexStream;

    /// A server that reads what the client sends and answers by script.
    struct FakeServer {
        io: DuplexStream,
        seen: Vec<String>,
    }

    impl FakeServer {
        /// Reads one frame, recording a `Query`'s SQL.
        async fn take(&mut self) -> Tag {
            let mut header = [0_u8; 5];
            self.io.read_exact(&mut header).await.unwrap();
            let len = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
            let mut body = vec![0; len - 4];
            self.io.read_exact(&mut body).await.unwrap();

            let tag = Tag(header[0]);
            if tag == Tag::QUERY {
                let text = String::from_utf8_lossy(&body);
                self.seen.push(text.trim_end_matches('\0').to_owned());
            }
            tag
        }

        /// Reads one frame and hands back its tag and body.
        ///
        /// `take` records a `Query`'s SQL and throws the rest away, which is
        /// what every test before `M32.1` wanted. A SASL exchange needs the
        /// bytes.
        async fn take_body(&mut self) -> (Tag, Vec<u8>) {
            let mut header = [0_u8; 5];
            self.io.read_exact(&mut header).await.unwrap();
            let len = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
            let mut body = vec![0; len - 4];
            self.io.read_exact(&mut body).await.unwrap();
            (Tag(header[0]), body)
        }

        async fn take_startup(&mut self) {
            let mut length = [0_u8; 4];
            self.io.read_exact(&mut length).await.unwrap();
            let len = u32::from_be_bytes(length) as usize;
            let mut rest = vec![0; len - 4];
            self.io.read_exact(&mut rest).await.unwrap();
        }

        async fn send(&mut self, bytes: &[u8]) {
            self.io.write_all(bytes).await.unwrap();
            self.io.flush().await.unwrap();
        }

        async fn ready(&mut self) {
            let mut out = Vec::new();
            encode::ready_for_query(&mut out, TxStatus::Idle);
            self.send(&out).await;
        }

        async fn complete(&mut self) {
            let mut out = Vec::new();
            encode::command_complete(&mut out, "SELECT 1");
            encode::ready_for_query(&mut out, TxStatus::Idle);
            self.send(&out).await;
        }
    }

    fn pair() -> (DuplexStream, FakeServer) {
        let (client, server) = tokio::io::duplex(64 * 1024);
        (
            client,
            FakeServer {
                io: server,
                seen: Vec::new(),
            },
        )
    }

    fn statement(sql: &str, eligible: bool) -> Planned {
        Planned {
            name: "s".into(),
            sql: sql.into(),
            kind: Kind::Read,
            replica_eligible: eligible,
            prepared: false,
        }
    }

    fn prepared_statement(sql: &str) -> Planned {
        Planned {
            prepared: true,
            ..statement(sql, false)
        }
    }

    #[tokio::test]
    async fn a_startup_that_is_answered_ok_leaves_a_usable_session() {
        let (client, mut server) = pair();
        let serving = tokio::spawn(async move {
            server.take_startup().await;
            let mut out = Vec::new();
            encode::authentication_ok(&mut out);
            server.send(&out).await;
            server.ready().await;

            server.take().await;
            server.complete().await;
            server
        });

        let mut session = Session::start(client, "u", "d", "").await.unwrap();
        session
            .transaction(&Transaction {
                think_ms: 0,
                tenant: "hot-0".into(),
                statements: vec![statement("SELECT 1", false)],
            })
            .await
            .unwrap();

        let server = serving.await.unwrap();
        assert_eq!(server.seen, vec!["SELECT 1".to_owned()]);
    }

    #[tokio::test]
    async fn a_cleartext_request_is_answered_with_the_password() {
        // The proxy asks for a JWT this way. A client that could not answer
        // would report a run of zero transactions against a working proxy.
        let (client, mut server) = pair();
        let serving = tokio::spawn(async move {
            server.take_startup().await;
            let mut out = Vec::new();
            encode::authentication_cleartext_password(&mut out);
            server.send(&out).await;

            // The password message, kept so the test can assert what arrived.
            let mut header = [0_u8; 5];
            server.io.read_exact(&mut header).await.unwrap();
            let len = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
            let mut body = vec![0; len - 4];
            server.io.read_exact(&mut body).await.unwrap();

            let mut out = Vec::new();
            encode::authentication_ok(&mut out);
            server.send(&out).await;
            server.ready().await;
            (Tag(header[0]), body)
        });

        let session = Session::start(client, "u", "d", "a-token").await.unwrap();
        drop(session);

        let (tag, body) = serving.await.unwrap();
        assert_eq!(tag, Tag::PASSWORD);
        assert_eq!(String::from_utf8_lossy(&body), "a-token\0");
    }

    #[tokio::test]
    async fn a_method_this_client_cannot_answer_is_reported_by_name() {
        // Rather than hanging, which is what a client that ignored the request
        // would do, and which reads as a proxy that stopped answering.
        //
        // GSSAPI, subtype 7. This asked for MD5 until `M32.6`, which taught
        // this client to answer it because `pgcat` offers nothing else. The
        // test is about the refusal rather than about MD5, so it now names a
        // method that is still refused instead of being deleted.
        let (client, mut server) = pair();
        tokio::spawn(async move {
            server.take_startup().await;
            // Built by hand: this crate encodes only what the proxy sends, and
            // the proxy never asks a client for GSSAPI.
            let mut out = vec![Tag::AUTHENTICATION.get()];
            out.extend_from_slice(&8_u32.to_be_bytes());
            out.extend_from_slice(&7_i32.to_be_bytes());
            server.send(&out).await;
            server
        });

        let error = Session::start(client, "u", "d", "pw").await.unwrap_err();
        assert!(
            matches!(error, SessionError::Auth(ref detail) if detail.contains('7')),
            "{error}"
        );
    }

    #[tokio::test]
    async fn an_md5_request_is_answered_with_the_salted_digest() {
        // The hermetic half of `M32.6`. The run against `pgcat` is the other
        // half and it needs a container; this needs nothing and fails if the
        // answer stops being sent at all.
        let (client, mut server) = pair();
        let serving = tokio::spawn(async move {
            server.take_startup().await;
            let mut out = vec![Tag::AUTHENTICATION.get()];
            out.extend_from_slice(&12_u32.to_be_bytes());
            out.extend_from_slice(&5_i32.to_be_bytes());
            out.extend_from_slice(&[1, 2, 3, 4]);
            server.send(&out).await;

            let (tag, body) = server.take_body().await;
            let mut out = Vec::new();
            encode::authentication_ok(&mut out);
            server.send(&out).await;
            server.ready().await;
            (tag, body)
        });

        Session::start(client, "acme_app", "tenant_acme", "acme-password")
            .await
            .unwrap();

        let (tag, body) = serving.await.unwrap();
        assert_eq!(tag, Tag::PASSWORD);
        assert_eq!(
            String::from_utf8(body).unwrap().trim_end_matches('\0'),
            md5_password("acme_app", "acme-password", &[1, 2, 3, 4])
        );
    }

    #[tokio::test]
    async fn a_refused_startup_is_reported_with_what_the_server_said() {
        let (client, mut server) = pair();
        tokio::spawn(async move {
            server.take_startup().await;
            let mut out = Vec::new();
            encode::error_response(&mut out, &ClientError::Draining);
            server.send(&out).await;
            server
        });

        let error = Session::start(client, "u", "d", "").await.unwrap_err();
        assert!(matches!(error, SessionError::Server { .. }), "{error}");
    }

    #[tokio::test]
    async fn a_prepared_statement_is_parsed_once_and_bound_after_that() {
        // What a driver's statement cache does, and the case the proxy's
        // mapping exists for: the second use sends `Bind` naming a statement
        // the client never re-parsed.
        let (client, mut server) = pair();
        let serving = tokio::spawn(async move {
            server.take_startup().await;
            let mut out = Vec::new();
            encode::authentication_ok(&mut out);
            server.send(&out).await;
            server.ready().await;

            let mut tags = Vec::new();
            // Two rounds: Parse, Bind, Execute, Sync, then Bind, Execute, Sync.
            for _ in 0..7 {
                let tag = server.take().await;
                tags.push(tag);
                if tag == Tag::SYNC {
                    server.complete().await;
                }
            }
            tags
        });

        let mut session = Session::start(client, "u", "d", "").await.unwrap();
        for _ in 0..2 {
            session
                .transaction(&Transaction {
                    think_ms: 0,
                    tenant: "hot-0".into(),
                    statements: vec![prepared_statement("SELECT 1")],
                })
                .await
                .unwrap();
        }

        let tags = serving.await.unwrap();
        assert_eq!(
            tags,
            vec![
                Tag::PARSE,
                Tag::BIND,
                Tag::EXECUTE,
                Tag::SYNC,
                Tag::BIND,
                Tag::EXECUTE,
                Tag::SYNC,
            ],
            "the statement was parsed again rather than reused"
        );
    }

    #[tokio::test]
    async fn a_multi_statement_transaction_is_wrapped_in_begin_and_commit() {
        // Wrapping is what makes the proxy hold a connection across
        // statements, and holding is the behaviour a scale run measures.
        let (client, mut server) = pair();
        let serving = tokio::spawn(async move {
            server.take_startup().await;
            let mut out = Vec::new();
            encode::authentication_ok(&mut out);
            server.send(&out).await;
            server.ready().await;

            for _ in 0..4 {
                server.take().await;
                server.complete().await;
            }
            server
        });

        let mut session = Session::start(client, "u", "d", "").await.unwrap();
        session
            .transaction(&Transaction {
                think_ms: 0,
                tenant: "hot-0".into(),
                statements: vec![statement("SELECT 1", false), statement("SELECT 2", true)],
            })
            .await
            .unwrap();

        let server = serving.await.unwrap();
        assert_eq!(
            server.seen,
            vec![
                "BEGIN".to_owned(),
                "SELECT 1".to_owned(),
                format!("{REPLICA_HINT}SELECT 2"),
                "COMMIT".to_owned(),
            ]
        );
    }

    #[tokio::test]
    async fn an_eligible_read_carries_the_hint_and_an_ineligible_one_does_not() {
        let (client, mut server) = pair();
        let serving = tokio::spawn(async move {
            server.take_startup().await;
            let mut out = Vec::new();
            encode::authentication_ok(&mut out);
            server.send(&out).await;
            server.ready().await;
            server.take().await;
            server.complete().await;
            server
        });

        let mut session = Session::start(client, "u", "d", "").await.unwrap();
        session
            .transaction(&Transaction {
                think_ms: 0,
                tenant: "hot-0".into(),
                statements: vec![statement("SELECT 1", true)],
            })
            .await
            .unwrap();

        let server = serving.await.unwrap();
        assert!(
            server.seen[0].starts_with(REPLICA_HINT),
            "{:?}",
            server.seen
        );
    }

    #[tokio::test]
    async fn a_statement_error_is_reported_rather_than_counted_as_a_success() {
        // The whole point of counting errors: a run whose statements all
        // failed must not report a wonderful p99.
        let (client, mut server) = pair();
        tokio::spawn(async move {
            server.take_startup().await;
            let mut out = Vec::new();
            encode::authentication_ok(&mut out);
            server.send(&out).await;
            server.ready().await;

            server.take().await;
            let mut out = Vec::new();
            encode::error_response(&mut out, &ClientError::Draining);
            encode::ready_for_query(&mut out, TxStatus::Failed);
            server.send(&out).await;
            server
        });

        let mut session = Session::start(client, "u", "d", "").await.unwrap();
        let error = session
            .transaction(&Transaction {
                think_ms: 0,
                tenant: "hot-0".into(),
                statements: vec![statement("SELECT 1", false)],
            })
            .await
            .unwrap_err();
        assert!(
            matches!(error.error, SessionError::Server { .. }),
            "{}",
            error.error
        );
    }

    #[tokio::test]
    async fn a_fatal_error_is_reported_by_its_message_rather_than_the_close() {
        // A fatal error is an `ErrorResponse` and then a closed socket, with
        // no `ReadyForQuery`. Reporting the disconnect loses the only part
        // that says why, and a run full of "disconnected" reads as a proxy
        // dropping sockets when the proxy answered correctly.
        let (client, mut server) = pair();
        tokio::spawn(async move {
            server.take_startup().await;
            let mut out = Vec::new();
            encode::authentication_ok(&mut out);
            server.send(&out).await;
            server.ready().await;

            server.take().await;
            let mut out = Vec::new();
            encode::error_response(&mut out, &ClientError::Draining);
            server.send(&out).await;
            // And gone, with no ReadyForQuery, as a fatal error is.
            server
        });

        let mut session = Session::start(client, "u", "d", "").await.unwrap();
        let error = session
            .transaction(&Transaction {
                think_ms: 0,
                tenant: "hot-0".into(),
                statements: vec![statement("SELECT 1", false)],
            })
            .await
            .unwrap_err();

        match &error.error {
            SessionError::Server { code, message } => {
                assert!(message.contains("administrator"), "{message}");
                assert_eq!(code, ADMIN_SHUTDOWN, "the SQLSTATE was lost");
            }
            other => panic!("reported as {other} rather than by its message"),
        }

        // And it is a relocation rather than a failure, because nothing in the
        // transaction had succeeded when the node asked this client to leave.
        // A drain that counted as a lost transaction would make "zero failed
        // transactions" a target a working drain cannot hit.
        assert!(!error.work_lost, "nothing had run yet");
        assert!(error.is_relocation(), "a drain read as a failure");
    }

    #[tokio::test]
    async fn a_shutdown_after_a_statement_has_run_is_a_loss_rather_than_a_relocation() {
        // The other side of the same code. Between transactions, 57P01 is a
        // node relocating a client. Once a statement has succeeded it is the
        // force-close at the end of drain_grace, and that took work away.
        let (client, mut server) = pair();
        tokio::spawn(async move {
            server.take_startup().await;
            let mut out = Vec::new();
            encode::authentication_ok(&mut out);
            server.send(&out).await;
            server.ready().await;

            // BEGIN, then the first statement, both answered.
            for _ in 0..2 {
                server.take().await;
                server.ready().await;
            }

            // And then the node gives up on it mid-transaction.
            server.take().await;
            let mut out = Vec::new();
            encode::error_response(&mut out, &ClientError::Draining);
            server.send(&out).await;
            server
        });

        let mut session = Session::start(client, "u", "d", "").await.unwrap();
        let error = session
            .transaction(&Transaction {
                think_ms: 0,
                tenant: "hot-0".into(),
                statements: vec![statement("SELECT 1", false), statement("SELECT 2", false)],
            })
            .await
            .unwrap_err();

        assert!(error.work_lost, "a statement had already succeeded");
        assert!(!error.is_relocation(), "a lost transaction read as a move");
    }

    #[tokio::test]
    async fn a_server_that_goes_away_is_reported_as_a_disconnect() {
        let (client, mut server) = pair();
        tokio::spawn(async move {
            // Takes the startup packet and then goes, which is what a node
            // being killed mid-handshake looks like.
            server.take_startup().await;
        });
        let error = Session::start(client, "u", "d", "").await.unwrap_err();
        assert!(matches!(error, SessionError::Disconnected), "{error}");
    }

    #[tokio::test]
    async fn terminating_sends_a_goodbye() {
        // Churn is part of the workload. A dropped socket would measure the
        // proxy's error path instead of its close path.
        let (client, mut server) = pair();
        let serving = tokio::spawn(async move {
            server.take_startup().await;
            let mut out = Vec::new();
            encode::authentication_ok(&mut out);
            server.send(&out).await;
            server.ready().await;
            server.take().await
        });

        let mut session = Session::start(client, "u", "d", "").await.unwrap();
        session.terminate().await.unwrap();
        assert_eq!(serving.await.unwrap(), Tag::TERMINATE);
    }

    /// A server that runs the other half of SCRAM, with real arithmetic.
    ///
    /// `pgprox-auth`'s server side, so the two halves of the exchange are the
    /// two halves this workspace ships rather than a fake agreeing with itself.
    /// A client that computed the proof wrongly fails `verify_client_proof`
    /// here, and a client that skipped verification would still pass, which is
    /// why the test below also drives a server whose signature is wrong.
    async fn serve_scram(server: &mut FakeServer, password: &str, sign_correctly: bool) {
        use base64::Engine as _;
        use base64::engine::general_purpose::STANDARD as BASE64;

        server.take_startup().await;
        let mut out = Vec::new();
        encode::authentication_sasl(&mut out, &[SCRAM_SHA_256]);
        server.send(&out).await;

        // The client-first message, as the client sent it.
        let (_, body) = server.take_body().await;
        let initial = String::from_utf8(body).unwrap();
        let client_first = initial
            .split_once('\0')
            .map_or(initial.clone(), |(_, rest)| {
                // Mechanism, then a four-byte length, then the payload.
                rest.get(4..).unwrap_or_default().to_owned()
            });
        let client_first_bare = client_first.trim_start_matches("n,,").to_owned();
        let client_nonce = client_first_bare
            .rsplit_once("r=")
            .map(|(_, nonce)| nonce.to_owned())
            .unwrap();

        let salt = b"pgload-salt";
        let iterations = 4_096;
        let nonce = format!("{client_nonce}SERVERPART");
        let server_first = format!("r={nonce},s={},i={iterations}", BASE64.encode(salt));

        let mut out = Vec::new();
        encode::authentication_sasl_continue(&mut out, &server_first);
        server.send(&out).await;

        // The client-final message, which is the one carrying the proof.
        //
        // No length prefix on this one. `SASLInitialResponse` names a mechanism
        // and declares its payload length; `SASLResponse` is the payload and
        // nothing else, and slicing four bytes off it here truncated `c=biws`
        // to `iws` and failed verification with the arithmetic working
        // perfectly.
        let (_, body) = server.take_body().await;
        let client_final = String::from_utf8(body).unwrap();
        let (without_proof, proof) = client_final.rsplit_once(",p=").unwrap();

        let keys =
            pgprox_auth::scram::ScramKeys::derive(password.as_bytes(), salt, iterations).unwrap();
        let auth_message =
            pgprox_auth::scram::auth_message(&client_first_bare, &server_first, without_proof);

        let proof = BASE64.decode(proof).unwrap();
        pgprox_auth::scram::verify_client_proof(&proof, &keys.stored_key, &auth_message).unwrap();

        let signature = if sign_correctly {
            pgprox_auth::scram::server_signature(&keys, &auth_message).to_vec()
        } else {
            vec![0_u8; 32]
        };

        let mut out = Vec::new();
        encode::authentication_sasl_final(&mut out, &format!("v={}", BASE64.encode(signature)));
        encode::authentication_ok(&mut out);
        server.send(&out).await;
        server.ready().await;
    }

    #[test]
    fn the_md5_answer_is_postgres_own_construction() {
        // Against a value Postgres computed, not one this code did. Taken from
        // a running postgres:17-alpine:
        //
        //   SELECT 'md5' || md5(
        //     convert_to(md5('acme-password' || 'acme_app'), 'UTF8')
        //     || '\x01020304'::bytea);
        //
        // `convert_to` is what makes that the right query, and leaving it out
        // is how this expectation was wrong twice before it was right.
        // `text || bytea` coerces the salt to its text form, so the server
        // hashes the eleven characters `\x01020304` rather than the four
        // bytes, and its answer then disagrees with a correct client. The
        // implementation below never changed.
        //
        // The value comes from the server rather than from here because a
        // test that recomputed the same formula in the same order would pass
        // for a wrong formula. `M32.6`.
        assert_eq!(
            md5_password("acme_app", "acme-password", &[1, 2, 3, 4]),
            "md5f61dfd93e36618e09a57836829fd2073"
        );

        // The salt is what makes two connections differ. Without it the answer
        // would be a password equivalent anybody on the wire could replay.
        assert_ne!(
            md5_password("acme_app", "acme-password", &[1, 2, 3, 4]),
            md5_password("acme_app", "acme-password", &[4, 3, 2, 1])
        );

        // And the username is in the inner digest, so the same password under
        // two roles is two different stored values.
        assert_ne!(
            md5_password("acme_app", "pw", &[0; 4]),
            md5_password("other", "pw", &[0; 4])
        );
    }

    #[tokio::test]
    async fn a_scram_handshake_completes_and_leaves_a_usable_session() {
        // `M32.1`. Without this there is no comparison to run: pgbouncer and
        // pgcat both authenticate clients with SCRAM, and this client spoke
        // trust and cleartext only.
        let (client, mut server) = pair();
        let serving = tokio::spawn(async move {
            serve_scram(&mut server, "s3cret", true).await;
        });

        let session = Session::start(client, "acme_app", "tenant_acme", "s3cret").await;
        assert!(session.is_ok(), "{:?}", session.err());
        serving.await.unwrap();
    }

    #[tokio::test]
    async fn a_server_that_cannot_prove_it_knew_the_password_is_refused() {
        // SCRAM is mutual and the client half of that is easy to leave out,
        // because a handshake that skips it still succeeds against a real
        // server. This is the arm where the server signature is wrong.
        let (client, mut server) = pair();
        let serving = tokio::spawn(async move {
            serve_scram(&mut server, "s3cret", false).await;
        });

        let Err(error) = Session::start(client, "acme_app", "tenant_acme", "s3cret").await else {
            panic!("a server that signed wrongly was accepted");
        };
        assert!(
            matches!(&error, SessionError::Auth(reason) if reason.contains("verification")),
            "{error:?}"
        );
        serving.await.unwrap();
    }

    #[tokio::test]
    async fn a_mechanism_this_client_does_not_have_is_refused_by_name() {
        // The refusal has to name what was offered. A load run that reports
        // zero transactions and no reason is a run nobody can act on, which is
        // what the doc comment on `start` has said since this client existed.
        let (client, mut server) = pair();
        let serving = tokio::spawn(async move {
            server.take_startup().await;
            let mut out = Vec::new();
            encode::authentication_sasl(&mut out, &["SCRAM-SHA-256-PLUS"]);
            server.send(&out).await;
        });

        let Err(error) = Session::start(client, "u", "d", "pw").await else {
            panic!("a mechanism this client cannot answer was accepted");
        };
        assert!(
            matches!(&error, SessionError::Auth(reason)
                if reason.contains("SCRAM-SHA-256-PLUS") && reason.contains("SCRAM-SHA-256")),
            "{error:?}"
        );
        serving.await.unwrap();
    }
}
