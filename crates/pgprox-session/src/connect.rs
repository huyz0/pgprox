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

use std::collections::HashMap;
use std::sync::Arc;

use pgprox_core::buf::BufferSlab;
use std::fmt;
use std::sync::{Mutex, MutexGuard, PoisonError};

use pgprox_core::auth::Backend;
use pgprox_core::ids::{PoolKey, ServerId};
use pgprox_core::pool::PoolError;
use pgprox_core::secret::SecretString;
use pgprox_pool::live::Connector;
use pgprox_proto::backend::{self, AuthRequest, BackendMessage};
use pgprox_proto::frame::{DEFAULT_MAX_FRAME, Frame, Tag};
use pgprox_proto::{encode, encode_frontend};

use crate::auth::SCRAM_SHA_256;
use crate::shell::Wire;

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

/// How a socket to a backend is obtained.
///
/// Behind a trait because opening one means TLS, and TLS means `pgprox-tls`,
/// which this crate may not depend on. The composition root supplies it, the
/// same way it supplies the SCRAM arithmetic.
#[async_trait::async_trait]
pub trait Upstream: Send + Sync + fmt::Debug {
    /// A connected stream. A TCP socket, or a TLS one over it.
    type Stream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send;

    /// Opens a socket to this backend, with TLS if the backend asks for it.
    ///
    /// # Errors
    ///
    /// Fails when the host is unreachable or the TLS handshake fails.
    async fn dial(&self, backend: &Backend) -> Result<Self::Stream, PoolError>;

    /// A fresh SCRAM exchange, for a server that asks for one.
    fn scram(&self) -> Box<dyn UpstreamScram>;
}

/// An open, authenticated upstream connection.
pub struct Upstreamed<S> {
    /// The wire, with its buffers.
    pub wire: Wire<S>,
    /// What the server said about itself during startup.
    pub parameters: Vec<(String, String)>,
    /// The server's own cancel key, for cancelling queries on this connection.
    pub backend_key: Option<(i32, i32)>,
    /// The prepared statements this connection holds.
    ///
    /// Here rather than in a map beside the pool, because it is a property of
    /// the connection and this is the thing the pool lends: a map keyed by
    /// connection id would have to be kept in step with connections opening
    /// and closing, and the first missed entry is a `Bind` for a statement the
    /// server does not have.
    pub statements: pgprox_pool::statements::ConnectionStatements,
}

impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin> Upstreamed<S> {
    /// Whether this connection is unfit to hand to another session.
    ///
    /// `M20.5`. Nothing reads a pooled connection while it is idle: `Pool::idle`
    /// is a `VecDeque` and no task polls it. So whatever the server sent
    /// between borrowers is still in the socket, and the next session to be
    /// handed the connection reads it as the answer to its own frame.
    ///
    /// Two things arrive that way. An asynchronous message: `NoticeResponse`,
    /// `ParameterStatus`, `NotificationResponse`. And the end of the
    /// connection: `pg_terminate_backend`, `idle_session_timeout`, a failover,
    /// a restart. The second is the common one, and what a client saw was its
    /// query failing on a connection that was already dead when it arrived.
    ///
    /// # Readable means unfit, whatever it turned out to be
    ///
    /// A healthy idle connection has nothing to say, so nothing is readable and
    /// this answers false without a syscall. Anything readable is either the
    /// close or a message this session did not ask for, and neither is
    /// something to hand on: a connection with bytes in it is one whose state
    /// the proxy does not know. Deciding which of the two it is would mean
    /// reading, and the answer would be "discard it" either way.
    ///
    /// This is why the check is not "is the socket closed". Distinguishing
    /// costs a parse and buys nothing.
    ///
    /// pgbouncer instead runs its packet loop on servers in `SV_IDLE`, which
    /// keeps the connection by consuming what arrived. That is the better
    /// answer for a proxy with an event loop over every server; this one holds
    /// idle connections in a map with nothing watching them, and a check on
    /// borrow is the cheap half of the same guarantee.
    ///
    /// Destructive when it says yes: the poll may consume a byte. That is
    /// deliberate and safe, because a caller that gets `true` closes the
    /// connection rather than using it.
    pub async fn unfit(&mut self) -> bool {
        std::future::poll_fn(|cx| {
            let mut byte = [0_u8; 1];
            let mut buf = tokio::io::ReadBuf::new(&mut byte);
            // Ready either way: this future never suspends. `Pending` from the
            // socket is the answer rather than a reason to wait, because it
            // means the server has said nothing, which is what an idle
            // connection should look like.
            std::task::Poll::Ready(!matches!(
                std::pin::Pin::new(self.wire.io_mut()).poll_read(cx, &mut buf),
                std::task::Poll::Pending
            ))
        })
        .await
    }

    /// Says goodbye before the socket goes.
    ///
    /// `M20.4`. Postgres logs a client that vanishes without a `Terminate`, and
    /// this proxy reaps idle connections after thirty seconds by design with
    /// `min_pool` at zero, so reaping is the steady state rather than an
    /// exception. Without this, every one of them is a line on the database
    /// that looks like a client crashed, and a real fault is indistinguishable
    /// from routine housekeeping.
    ///
    /// Best effort. The connection is being closed either way, so a write that
    /// fails changes nothing about what happens next, and waiting for a flush
    /// on a server that has already gone would be the reaper blocking on a dead
    /// socket.
    ///
    /// # Only on a clean close
    ///
    /// Callers use this for a connection being retired from the pool, not for
    /// one discarded mid-transaction. A discarded connection is in a state
    /// nobody knows: if the server is in COPY-in it reads `CopyData`,
    /// `CopyDone` or `CopyFail` and nothing else, so a `Terminate` there is a
    /// protocol error rather than a courtesy. pgbouncer draws the same line
    /// with `disconnect_server`'s `send_term` argument. Closing the socket is
    /// what "this connection is in an unknown state" should look like.
    pub async fn goodbye(&mut self) {
        self.wire.queue(pgprox_proto::encode_frontend::terminate);
        let _ = self.wire.flush().await;
    }
}

impl<S> fmt::Debug for Upstreamed<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // No stream and no key: one is a socket and the other is a bearer
        // token for cancelling somebody's query.
        f.debug_struct("Upstreamed")
            .field("parameters", &self.parameters.len())
            .finish_non_exhaustive()
    }
}

/// Opens upstream connections for real.
///
/// The [`Connector`] contract takes a [`PoolKey`] and nothing else, so this
/// holds the directory of which backend each key means. Grants fill it as they
/// resolve, which is the only moment the credentials exist.
#[derive(Debug)]
pub struct PgConnector<U> {
    upstream: U,
    backends: Mutex<HashMap<PoolKey, Backend>>,
    /// Where an upstream connection's buffers come from.
    ///
    /// The same slab the client side borrows from, on purpose. Upstream
    /// connections are capped and client connections are not, so a shared
    /// bound means a burst of clients cannot starve the connections that
    /// serve them without the node noticing.
    slab: Arc<BufferSlab>,
}

impl<U: Upstream> PgConnector<U> {
    /// A connector over `upstream`, knowing no backends yet.
    #[must_use]
    pub fn new(upstream: U, slab: Arc<BufferSlab>) -> Self {
        Self {
            upstream,
            backends: Mutex::new(HashMap::new()),
            slab,
        }
    }

    /// Records where a pool key's connections should go.
    ///
    /// Called when a grant resolves. Overwrites, because a tenant's password
    /// can be rotated and the newest grant is the one to believe.
    pub fn learn(&self, backend: &Backend) {
        self.lock().insert(backend.pool_key(), backend.clone());
    }

    /// The backend a key names, if one is known.
    ///
    /// For callers that need to reach a server without going through the pool.
    /// Cancelling a query is the one: it needs a fresh connection carrying
    /// nothing but the cancel key, and taking one from the pool would count it
    /// against a cap it is not going to use.
    #[must_use]
    pub fn backend(&self, key: &PoolKey) -> Option<Backend> {
        self.lock().get(key).cloned()
    }

    /// Opens a socket to a backend without authenticating on it.
    ///
    /// Only a `CancelRequest` wants this: it carries no startup packet and
    /// gets no answer.
    ///
    /// # Errors
    ///
    /// Fails when the socket cannot be opened.
    pub async fn dial(&self, backend: &Backend) -> Result<U::Stream, PoolError> {
        self.upstream.dial(backend).await
    }

    /// How many backends are known.
    #[must_use]
    pub fn known(&self) -> usize {
        self.lock().len()
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<PoolKey, Backend>> {
        // A poisoned lock here means another thread panicked while holding a
        // map of backends. The map is still readable and the alternative is
        // taking the node down, so it is recovered rather than propagated.
        self.backends.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Opens and authenticates one connection.
    ///
    /// # Errors
    ///
    /// Fails when the key names no known backend, when the socket cannot be
    /// opened, or when the server refuses the credentials.
    pub async fn open(&self, key: &PoolKey) -> Result<Upstreamed<U::Stream>, PoolError> {
        let Some(backend) = self.lock().get(key).cloned() else {
            return Err(PoolError::ConnectFailed {
                server: key.server.clone(),
                reason: format!(
                    "no credentials are known for database {} as {}",
                    key.database, key.user
                ),
            });
        };

        let stream = self.upstream.dial(&backend).await?;
        drive(Wire::new(stream, Arc::clone(&self.slab)), &backend, || {
            self.upstream.scram()
        })
        .await
    }
}

#[async_trait::async_trait]
impl<U: Upstream + 'static> Connector for PgConnector<U>
where
    U::Stream: 'static,
{
    type Connection = Upstreamed<U::Stream>;

    async fn connect(&self, key: &PoolKey) -> Result<Self::Connection, PoolError> {
        self.open(key).await
    }
}

/// Runs the handshake to completion over an open stream.
///
/// Split out from [`PgConnector::open`] so the sequence can be tested against
/// a duplex pair without a dialer in the picture.
///
/// # Errors
///
/// Fails when the socket does, or when the server refuses.
pub async fn drive<S>(
    mut wire: Wire<S>,
    backend: &Backend,
    scram: impl FnOnce() -> Box<dyn UpstreamScram>,
) -> Result<Upstreamed<S>, PoolError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let server = backend.server.clone();
    let refused = |reason: String| PoolError::ConnectFailed {
        server: server.clone(),
        reason,
    };

    let mut handshake = UpstreamHandshake::new(backend.server.clone());
    let mut exchange: Option<Box<dyn UpstreamScram>> = None;
    let mut scram = Some(scram);
    let mut body = Vec::new();
    let mut need = handshake.begin();

    loop {
        match need {
            Need::Startup => {
                wire.queue(|out| {
                    encode_frontend::startup_message(
                        out,
                        encode::PROTOCOL_3_0,
                        &[
                            ("user", &backend.user),
                            ("database", &backend.database),
                            // Named so a DBA reading pg_stat_activity sees
                            // which process is holding the connection rather
                            // than a row that looks like the tenant's own app.
                            ("application_name", "pgprox"),
                        ],
                    );
                });
                need = Need::Read;
            }
            Need::Password => {
                wire.queue(|out| {
                    encode_frontend::password_message(out, backend.password.expose());
                });
                need = Need::Read;
            }
            Need::SaslStart => {
                let mut fresh = scram
                    .take()
                    .map(|make| make())
                    .ok_or_else(|| refused("the server asked for SASL twice".to_owned()))?;
                let first = fresh.client_first(&backend.user);
                wire.queue(|out| {
                    encode_frontend::sasl_initial_response(out, SCRAM_SHA_256, &first);
                });
                exchange = Some(fresh);
                need = Need::Read;
            }
            Need::SaslContinue(server_first) => {
                let exchange = exchange
                    .as_mut()
                    .ok_or_else(|| refused("a SASL challenge arrived first".to_owned()))?;
                let final_message = exchange
                    .client_final(&backend.password, &server_first)
                    .map_err(refused)?;
                wire.queue(|out| encode_frontend::sasl_response(out, &final_message));
                need = Need::Read;
            }
            Need::SaslVerify(server_final) => {
                let exchange = exchange
                    .as_mut()
                    .ok_or_else(|| refused("a SASL result arrived first".to_owned()))?;
                // Checked rather than assumed: a server that cannot prove it
                // knew the password is not the server this connection meant to
                // reach, whatever answered the socket.
                exchange.verify(&server_final).map_err(refused)?;
                need = Need::Read;
            }
            Need::Read => {
                // Flushed here rather than after each queue, so a handshake
                // step that writes and then reads costs one syscall pair.
                wire.flush()
                    .await
                    .map_err(|err| refused(format!("sending to the server failed: {err}")))?;
                let tag = wire
                    .read_tagged(&mut body, DEFAULT_MAX_FRAME)
                    .await
                    .map_err(|err| refused(format!("reading from the server failed: {err}")))?;
                need = handshake.on_frame(tag, &body);
            }
            Need::Ready => {
                return Ok(Upstreamed {
                    statements: pgprox_pool::statements::ConnectionStatements::new(
                        pgprox_pool::statements::StatementConfig::default(),
                    ),
                    wire,
                    parameters: handshake.parameters().to_vec(),
                    backend_key: handshake.backend_key(),
                });
            }
            Need::Fail(error) => return Err(error),
        }
    }
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

    use pgprox_core::auth::TlsMode;
    use pgprox_core::secret::SecretString;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, duplex};

    fn backend() -> Backend {
        Backend {
            server: server(),
            database: "acme".into(),
            user: "acme_app".into(),
            password: SecretString::new("hunter2"),
            tls: TlsMode::Disabled,
        }
    }

    /// A scripted server: writes canned bytes and records what it was sent.
    ///
    /// Not a mock. It speaks the real protocol, and every byte the connector
    /// sends it is decoded by this crate's own frontend decoder in the
    /// assertions below.
    struct Scripted {
        io: DuplexStream,
    }

    impl Scripted {
        async fn send(&mut self, build: impl FnOnce(&mut Vec<u8>)) {
            let mut out = Vec::new();
            build(&mut out);
            self.io.write_all(&out).await.unwrap();
        }

        /// Reads one untagged message body, which is how a startup packet
        /// arrives.
        async fn read_startup(&mut self) -> Vec<u8> {
            let mut len = [0_u8; 4];
            self.io.read_exact(&mut len).await.unwrap();
            let total = u32::from_be_bytes(len) as usize;
            let mut body = vec![0; total - 4];
            self.io.read_exact(&mut body).await.unwrap();
            body
        }

        /// Reads one tagged message, returning its tag and body.
        async fn read_tagged(&mut self) -> (Tag, Vec<u8>) {
            let mut header = [0_u8; 5];
            self.io.read_exact(&mut header).await.unwrap();
            let len = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
            let mut body = vec![0; len - 4];
            self.io.read_exact(&mut body).await.unwrap();
            (Tag(header[0]), body)
        }
    }

    fn duplex_pair() -> (Wire<DuplexStream>, Scripted) {
        let (ours, theirs) = duplex(4096);
        (Wire::new(ours, test_slab()), Scripted { io: theirs })
    }

    /// A SCRAM exchange with no arithmetic in it.
    #[derive(Debug, Default)]
    struct FakeScram {
        verified: bool,
    }

    impl UpstreamScram for FakeScram {
        fn client_first(&mut self, user: &str) -> String {
            format!("n,,n={user},r=CLIENTNONCE")
        }

        fn client_final(
            &mut self,
            password: &SecretString,
            server_first: &str,
        ) -> Result<String, String> {
            if password.expose() != "hunter2" {
                return Err("the connector sent the wrong password".to_owned());
            }
            Ok(format!("c=biws,{server_first},p=UFJPT0Y="))
        }

        fn verify(&mut self, server_final: &str) -> Result<(), String> {
            self.verified = true;
            (server_final == "v=U0lHTg==")
                .then_some(())
                .ok_or_else(|| "the server's signature did not match".to_owned())
        }
    }

    #[tokio::test]
    async fn a_trust_connection_is_opened_and_its_parameters_come_back() {
        let (wire, mut server) = duplex_pair();
        let task = tokio::spawn(async move {
            drive(wire, &backend(), || Box::new(FakeScram::default())).await
        });

        let startup = server.read_startup().await;
        assert!(
            String::from_utf8_lossy(&startup).contains("acme_app"),
            "the startup packet did not name the backend user"
        );
        assert!(
            String::from_utf8_lossy(&startup).contains("pgprox"),
            "the connection did not name itself in pg_stat_activity"
        );

        server.send(encode::authentication_ok).await;
        server
            .send(|out| encode::parameter_status(out, "server_version", "17.2"))
            .await;
        server
            .send(|out| encode::ready_for_query(out, pgprox_proto::backend::TxStatus::Idle))
            .await;

        let opened = task.await.unwrap().unwrap();
        assert_eq!(
            opened.parameters,
            [("server_version".to_owned(), "17.2".to_owned())]
        );
    }

    #[tokio::test]
    async fn a_cleartext_password_reaches_the_server() {
        let (wire, mut server) = duplex_pair();
        let task = tokio::spawn(async move {
            drive(wire, &backend(), || Box::new(FakeScram::default())).await
        });

        server.read_startup().await;
        server.send(encode::authentication_cleartext_password).await;

        let (tag, body) = server.read_tagged().await;
        assert_eq!(tag, Tag::PASSWORD);
        assert_eq!(
            body, b"hunter2\0",
            "the password was not sent as a null-terminated string"
        );

        server.send(encode::authentication_ok).await;
        server
            .send(|out| encode::ready_for_query(out, pgprox_proto::backend::TxStatus::Idle))
            .await;
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn a_scram_exchange_runs_all_three_messages() {
        let (wire, mut server) = duplex_pair();
        let task = tokio::spawn(async move {
            drive(wire, &backend(), || Box::new(FakeScram::default())).await
        });

        server.read_startup().await;
        server
            .send(|out| encode::authentication_sasl(out, &[SCRAM_SHA_256]))
            .await;

        let (tag, initial) = server.read_tagged().await;
        assert_eq!(tag, Tag::PASSWORD);
        assert!(
            initial.starts_with(b"SCRAM-SHA-256\0"),
            "the SASLInitialResponse did not name its mechanism"
        );

        server
            .send(|out| encode::authentication_sasl_continue(out, "r=NONCE,s=U0FMVA==,i=4096"))
            .await;
        let (_, final_message) = server.read_tagged().await;
        assert!(
            String::from_utf8_lossy(&final_message).contains("p=UFJPT0Y="),
            "the client-final message carried no proof"
        );

        server
            .send(|out| encode::authentication_sasl_final(out, "v=U0lHTg=="))
            .await;
        server.send(encode::authentication_ok).await;
        server
            .send(|out| encode::ready_for_query(out, pgprox_proto::backend::TxStatus::Idle))
            .await;

        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn a_server_whose_signature_does_not_match_is_refused() {
        // The half of SCRAM that exists to authenticate the server. Skipping
        // it means connecting to whatever answered the socket.
        let (wire, mut server) = duplex_pair();
        let task = tokio::spawn(async move {
            drive(wire, &backend(), || Box::new(FakeScram::default())).await
        });

        server.read_startup().await;
        server
            .send(|out| encode::authentication_sasl(out, &[SCRAM_SHA_256]))
            .await;
        server.read_tagged().await;
        server
            .send(|out| encode::authentication_sasl_continue(out, "r=NONCE,s=U0FMVA==,i=4096"))
            .await;
        server.read_tagged().await;
        server
            .send(|out| encode::authentication_sasl_final(out, "v=V1JPTkc="))
            .await;

        let Err(PoolError::ConnectFailed { reason, .. }) = task.await.unwrap() else {
            panic!("a server that proved nothing was accepted");
        };
        assert!(reason.contains("signature"), "{reason}");
    }

    #[tokio::test]
    async fn a_refusal_from_the_server_carries_its_own_words() {
        let (wire, mut server) = duplex_pair();
        let task = tokio::spawn(async move {
            drive(wire, &backend(), || Box::new(FakeScram::default())).await
        });

        server.read_startup().await;
        server
            .send(|out| {
                out.push(Tag::ERROR_RESPONSE.get());
                let body = b"SFATAL\0C28P01\0Mpassword authentication failed\0\0";
                out.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_be_bytes());
                out.extend_from_slice(body);
            })
            .await;

        let Err(PoolError::ConnectFailed { reason, .. }) = task.await.unwrap() else {
            panic!("a refused connection was reported as open");
        };
        assert!(reason.contains("28P01"), "{reason}");
    }

    #[tokio::test]
    async fn a_server_that_hangs_up_mid_handshake_is_reported() {
        let (wire, server) = duplex_pair();
        drop(server);

        assert!(matches!(
            drive(wire, &backend(), || Box::new(FakeScram::default())).await,
            Err(PoolError::ConnectFailed { .. })
        ));
    }

    /// A dialer that hands out one end of a duplex pair.
    #[derive(Debug)]
    struct Duplexer {
        server: Mutex<Option<DuplexStream>>,
    }

    #[async_trait::async_trait]
    impl Upstream for Duplexer {
        type Stream = DuplexStream;

        async fn dial(&self, _backend: &Backend) -> Result<Self::Stream, PoolError> {
            let (ours, theirs) = duplex(4096);
            *self.server.lock().unwrap() = Some(theirs);
            Ok(ours)
        }

        fn scram(&self) -> Box<dyn UpstreamScram> {
            Box::new(FakeScram::default())
        }
    }

    /// A dialer that never connects.
    #[derive(Debug)]
    struct Unreachable;

    #[async_trait::async_trait]
    impl Upstream for Unreachable {
        type Stream = DuplexStream;

        async fn dial(&self, backend: &Backend) -> Result<Self::Stream, PoolError> {
            Err(PoolError::ConnectFailed {
                server: backend.server.clone(),
                reason: "connection refused".to_owned(),
            })
        }

        fn scram(&self) -> Box<dyn UpstreamScram> {
            Box::new(FakeScram::default())
        }
    }

    /// What any Connector must do, whatever it is connecting to.
    ///
    /// Run against the real one and against a fake, so a behaviour the fake
    /// invents shows up as the two disagreeing rather than as a surprise at
    /// integration time.
    async fn a_connector_refuses_a_key_it_knows_nothing_about<C: Connector>(connector: &C) {
        let unknown = PoolKey::new(ServerId::new("db-9", 5432), "nope", "nobody");
        let Err(PoolError::ConnectFailed { server, .. }) = connector.connect(&unknown).await else {
            panic!("a connector opened a connection to a database it knows nothing about");
        };
        assert_eq!(server, ServerId::new("db-9", 5432));
    }

    /// A fake connector, of the shape pgprox-pool's own tests use.
    #[derive(Debug)]
    struct FakeConnector;

    #[async_trait::async_trait]
    impl Connector for FakeConnector {
        type Connection = u32;

        async fn connect(&self, key: &PoolKey) -> Result<u32, PoolError> {
            Err(PoolError::ConnectFailed {
                server: key.server.clone(),
                reason: "the fake knows no backends".to_owned(),
            })
        }
    }

    #[tokio::test]
    async fn the_fake_and_the_real_connector_agree_about_an_unknown_key() {
        a_connector_refuses_a_key_it_knows_nothing_about(&FakeConnector).await;
        a_connector_refuses_a_key_it_knows_nothing_about(&PgConnector::new(
            Unreachable,
            test_slab(),
        ))
        .await;
    }

    #[tokio::test]
    async fn a_dial_failure_reaches_the_caller_unchanged() {
        let connector = PgConnector::new(Unreachable, test_slab());
        connector.learn(&backend());

        let Err(PoolError::ConnectFailed { reason, .. }) =
            connector.connect(&backend().pool_key()).await
        else {
            panic!("an unreachable server was reported as connected");
        };
        assert_eq!(reason, "connection refused");
    }

    #[tokio::test]
    async fn a_learned_backend_is_opened_through_the_dialer() {
        let connector = std::sync::Arc::new(PgConnector::new(
            Duplexer {
                server: Mutex::new(None),
            },
            test_slab(),
        ));
        connector.learn(&backend());
        assert_eq!(connector.known(), 1);

        let opening = {
            let connector = std::sync::Arc::clone(&connector);
            let key = backend().pool_key();
            tokio::spawn(async move { connector.connect(&key).await })
        };

        // Wait for the dialer to have produced its end of the pair.
        let mut server = loop {
            if let Some(io) = connector.upstream.server.lock().unwrap().take() {
                break Scripted { io };
            }
            tokio::task::yield_now().await;
        };

        server.read_startup().await;
        server.send(encode::authentication_ok).await;
        server
            .send(|out| encode::ready_for_query(out, pgprox_proto::backend::TxStatus::Idle))
            .await;

        opening.await.unwrap().unwrap();
    }

    #[test]
    fn a_relearned_backend_replaces_the_old_one() {
        // A tenant's password can be rotated, and the newest grant is the one
        // to believe. Keeping the first would keep authenticating with a
        // password that has been withdrawn.
        let connector = PgConnector::new(Unreachable, test_slab());
        connector.learn(&backend());

        let mut rotated = backend();
        rotated.password = SecretString::new("hunter3");
        connector.learn(&rotated);

        assert_eq!(connector.known(), 1);
    }

    #[test]
    fn an_open_connection_prints_neither_its_socket_nor_its_cancel_key() {
        // The key is a bearer token for cancelling somebody's query, and this
        // type will end up in a log line eventually.
        let rendered = format!(
            "{:?}",
            Upstreamed::<DuplexStream> {
                statements: pgprox_pool::statements::ConnectionStatements::new(
                    pgprox_pool::statements::StatementConfig::default(),
                ),
                wire: Wire::new(duplex(8).0, test_slab()),
                parameters: vec![("server_version".to_owned(), "17.2".to_owned())],
                backend_key: Some((4242, 0x0bad_beef)),
            }
        );
        assert!(!rendered.contains("4242"), "{rendered}");
        assert!(!rendered.contains("beef"), "{rendered}");
        // And it still says what it is. A Debug that prints nothing passes
        // every assertion above and takes the field count out of the log line
        // with it, which is the half of this that is worth reading.
        assert!(rendered.contains("Upstreamed"), "{rendered}");
        assert!(rendered.contains("parameters: 1"), "{rendered}");
    }

    #[test]
    fn a_connector_counts_the_backends_it_has_learned() {
        // What `known` is for: an operator asking whether grants have reached
        // this node at all. Nothing read it, so a count that never moved off
        // one went unnoticed.
        let connector = PgConnector::new(Unreachable, test_slab());
        assert_eq!(connector.known(), 0);

        connector.learn(&backend());
        assert_eq!(connector.known(), 1);

        // Overwriting rather than adding, which is what a rotated password
        // does: same pool key, new secret.
        let rotated = Backend {
            password: SecretString::new("hunter3"),
            ..backend()
        };
        connector.learn(&rotated);
        assert_eq!(connector.known(), 1);

        let other = Backend {
            database: "globex".into(),
            ..backend()
        };
        connector.learn(&other);
        assert_eq!(connector.known(), 2);
    }

    #[tokio::test]
    async fn a_known_backend_can_be_dialled_without_authenticating() {
        // What a cancel request needs: a fresh socket carrying nothing but the
        // key. Taking one from the pool would count it against a cap it is
        // never going to use.
        let connector = PgConnector::new(
            Duplexer {
                server: Mutex::new(None),
            },
            test_slab(),
        );
        connector.learn(&backend());

        let known = connector.backend(&backend().pool_key()).expect("known");
        assert_eq!(known.server, server());
        assert!(connector.dial(&known).await.is_ok());
    }

    #[test]
    fn a_key_the_connector_never_learned_names_no_backend() {
        let connector = PgConnector::new(Unreachable, test_slab());
        assert!(connector.backend(&backend().pool_key()).is_none());
    }
}
