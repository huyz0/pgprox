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

impl<S: AsyncRead + AsyncWrite + Unpin> Session<S> {
    /// Sends the startup packet and answers whatever the server asks for.
    ///
    /// # Errors
    ///
    /// Fails on a socket error, on a refusal, and on any authentication
    /// method other than "none" or "send it in the clear". MD5 and SCRAM are
    /// refused by name rather than silently: a load client that could not
    /// authenticate has to say why, since the alternative is a run reporting
    /// zero transactions and no reason.
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

        session.authenticate(password).await?;
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

    async fn authenticate(&mut self, password: &str) -> Result<(), SessionError> {
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
                        self.io.write_all(&self.write).await?;
                        self.io.flush().await?;
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
        let (client, mut server) = pair();
        tokio::spawn(async move {
            server.take_startup().await;
            // Built by hand: this crate encodes only what the proxy sends,
            // and nothing in the proxy asks a client for MD5.
            let mut out = vec![Tag::AUTHENTICATION.get()];
            out.extend_from_slice(&12_u32.to_be_bytes());
            out.extend_from_slice(&5_i32.to_be_bytes());
            out.extend_from_slice(&[1, 2, 3, 4]);
            server.send(&out).await;
            server
        });

        let error = Session::start(client, "u", "d", "pw").await.unwrap_err();
        assert!(
            matches!(error, SessionError::Auth(ref detail) if detail.contains("Md5")),
            "{error}"
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
}
