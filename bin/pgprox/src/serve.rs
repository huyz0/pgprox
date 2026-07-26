//! Serving one client, and accepting the next.
//!
//! Everything above this file decides things. This one does them: it accepts a
//! socket, runs the handshake, holds the session's place in the registry, and
//! moves frames between the client and whichever upstream connection the pool
//! lends it.
//!
//! # The connection ceiling is refused politely
//!
//! A node at its limit answers with an `ErrorResponse` carrying SQLSTATE
//! 53300 rather than dropping the socket. A dropped socket looks like a
//! network fault to every driver, and sends the operator to the wrong place.
//!
//! # Where the upstream connection lives
//!
//! With the session, for as long as it holds the guard. It is taken out of the
//! pool on acquire and given back on release, because the relay awaits on it
//! and the pool's lock cannot be held across an await. See
//! `LivePool::take_connection`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use pgprox_core::admin::ClientState;
use pgprox_core::auth::{CredentialResolver, Grant};
use pgprox_core::clock::Clock;
use pgprox_core::error::ClientError;
use pgprox_core::ids::{ConnId, NodeId};
use pgprox_core::pool::{UpstreamGuard, UpstreamPool};
use pgprox_proto::backend::{self, BackendMessage, TxStatus};
use pgprox_proto::encode;
use pgprox_proto::frame::Frame;
use pgprox_route::replica::Replicas;
use pgprox_session::cancel::Registry;
use pgprox_session::connect::Upstreamed;
use pgprox_session::probe::ParameterCache;
use pgprox_session::relay::{ClientAction, Relay};
use pgprox_session::shell::{
    Handoff, ShellError, Wire, accept, authenticate_scram, authenticate_token, negotiate,
};
use pgprox_session::state::{Credential, Handshake, HandshakeConfig};
use pgprox_session::{TokenAuth, connect::PgConnector};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::dial::TcpUpstream;
use crate::sessions::Sessions;
use crate::wiring::NodePool;

/// How many clients a node will serve at once.
///
/// A counter with a guard rather than a semaphore, because refusing has to
/// happen after the handshake has told the client why, and a permit held
/// across that would count a connection that is being turned away.
#[derive(Debug)]
pub struct Gate {
    live: AtomicU32,
    ceiling: u32,
}

/// A client's place under the ceiling, released on drop.
///
/// Holds an `Arc` rather than a borrow, because a session runs on its own task
/// and a borrowed place could not travel there.
#[derive(Debug)]
pub struct Admitted(Arc<Gate>);

impl Drop for Admitted {
    fn drop(&mut self) {
        self.0.live.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Gate {
    /// A gate admitting `ceiling` clients at once.
    #[must_use]
    pub const fn new(ceiling: u32) -> Self {
        Self {
            live: AtomicU32::new(0),
            ceiling,
        }
    }

    /// How many clients are being served.
    #[must_use]
    pub fn live(&self) -> u32 {
        self.live.load(Ordering::SeqCst)
    }

    /// Admits a client, or refuses because the node is full.
    pub fn admit(self: &Arc<Self>) -> Option<Admitted> {
        self.live
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |live| {
                (live < self.ceiling).then_some(live + 1)
            })
            .ok()
            .map(|_| Admitted(Arc::clone(self)))
    }
}

/// Everything one session needs that outlives it.
pub struct Context {
    /// This node.
    pub node: NodeId,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// How the handshake behaves.
    pub handshake: HandshakeConfig,
    /// Who resolves tokens.
    pub resolver: Arc<dyn CredentialResolver>,
    /// Where upstream connections come from.
    pub connector: Arc<PgConnector<TcpUpstream>>,
    /// The pool they are held in.
    pub pool: Arc<NodePool>,
    /// What a client is told about the server before it has one.
    pub parameters: Arc<ParameterCache>,
    /// Who is being served.
    pub sessions: Arc<Sessions>,
    /// Which queries can be cancelled.
    pub cancels: Arc<Registry>,
    /// How long a client waits for an upstream connection.
    pub acquire_timeout: Duration,
    /// Where the other nodes are, for a cancel this node does not own.
    pub peers: std::collections::BTreeMap<NodeId, String>,
    /// The replica sets this node is watching, one per primary.
    pub replicas: Arc<crate::replicas::ReplicaSets>,
    /// Fired when the node has begun draining.
    ///
    /// A session between transactions closes on it. One inside a transaction
    /// does not: finishing what it holds is the whole point of a drain, and
    /// the grace timer behind [`Context::closing`] is what bounds it.
    pub draining: crate::run::Shutdown,
    /// Fired when the drain's grace has run out.
    ///
    /// Whatever is still connected is closed, mid-transaction or not.
    pub closing: crate::run::Shutdown,
    /// The static users this node accepts, if any.
    ///
    /// `None` is a node with none, which refuses a SCRAM client with the same
    /// message a bad token gets: telling a caller that static users exist here
    /// but they are not one is an oracle.
    pub statics: Option<Arc<crate::admin::StaticAdmin>>,
    /// What the static-user surface reads.
    pub observatory: Arc<dyn pgprox_core::admin::Observatory>,
    /// How a client's connection is upgraded, when the node has certificates.
    ///
    /// `None` is a node with none, which answers `N` to an `SSLRequest` and
    /// serves in the clear. The handshake config is what decides whether that
    /// is allowed; this is only the means.
    pub tls: Option<tokio_rustls::TlsAcceptor>,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("node", &self.node)
            .finish_non_exhaustive()
    }
}

/// Runs one client to completion.
///
/// # Errors
///
/// Fails when the client disconnects, misbehaves, or is refused. Every case
/// has already been reported to the client where the protocol allows it.
pub async fn session<S>(stream: S, context: &Context, admitted: Admitted) -> Result<(), ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut wire = Wire::new(stream);
    let mut handshake = Handshake::new(context.handshake.clone());

    match negotiate(&mut wire, &mut handshake).await? {
        Handoff::Upgrade => {
            let Some(acceptor) = context.tls.clone() else {
                // The state machine only answers `S` when it has been told TLS
                // is available, so reaching this means the listener said yes
                // and the node has no certificate. A refusal rather than a
                // silent downgrade: the client asked for TLS.
                return Err(wire
                    .refuse(ClientError::ProtocolViolation("TLS is not available"))
                    .await);
            };

            // The leftover bytes travel with the stream, and there must not be
            // any: a client that sent its ClientHello before reading our `S`
            // would have them buffered here, and handing rustls a stream
            // missing its first bytes fails in a way that reads as a cipher
            // mismatch. See the hazard note in this crate's AGENTS.md.
            let (io, pending) = wire.into_parts();
            if !pending.is_empty() {
                return Err(ShellError::Disconnected);
            }

            let upgraded = acceptor
                .accept(io)
                .await
                .map_err(|_| ShellError::Disconnected)?;
            let mut wire = Wire::new(upgraded);

            // The same handshake, which is what makes "TLS was accepted"
            // survive the change of stream type.
            match negotiate(&mut wire, &mut handshake).await? {
                Handoff::Cancel(conn) => cancel(conn, context).await,
                Handoff::Upgrade => Err(wire
                    .refuse(ClientError::ProtocolViolation("TLS was already negotiated"))
                    .await),
                Handoff::Ask(credential) => {
                    serve_client(&mut wire, &mut handshake, credential, context, admitted).await
                }
            }
        }
        Handoff::Cancel(conn) => cancel(conn, context).await,
        Handoff::Ask(credential) => {
            serve_client(&mut wire, &mut handshake, credential, context, admitted).await
        }
    }
}

/// What a static user is told about the server it reached.
///
/// The proxy itself, rather than a database: this connection has no upstream,
/// and a `server_version` copied from one would name a server this session
/// cannot reach. The version is what a driver checks before deciding which
/// syntax to use, and every statement here is `SHOW`.
fn proxy_parameters() -> Vec<(String, String)> {
    vec![
        ("server_version".to_owned(), "17.0 (pgprox)".to_owned()),
        ("server_encoding".to_owned(), "UTF8".to_owned()),
        ("client_encoding".to_owned(), "UTF8".to_owned()),
        ("DateStyle".to_owned(), "ISO, MDY".to_owned()),
    ]
}

/// Everything after the handshake has settled, whatever the stream turned out
/// to be.
///
/// Split from [`session`] so the plaintext and TLS paths are the same code
/// rather than the same code twice. Generic, so both instantiations are
/// compiled and neither can rot.
async fn serve_client<S>(
    wire: &mut Wire<S>,
    handshake: &mut Handshake,
    credential: Credential,
    context: &Context,
    admitted: Admitted,
) -> Result<(), ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let grant = match credential {
        Credential::Scram => {
            // A static user never reaches a database: it authenticates against
            // this node and gets the `SHOW` surface. An operator credential
            // that could reach a tenant's data would be a way around the whole
            // token path.
            let Some(statics) = context.statics.clone() else {
                return Err(wire.refuse(crate::admin::not_configured()).await);
            };
            let startup = handshake.startup().ok_or(ShellError::Disconnected)?.clone();
            let mut scram = pgprox_session::auth::ScramAuth::new(
                statics,
                pgprox_session::auth::ScramConfig {
                    mock_salt: crate::admin::MOCK_SALT.to_owned(),
                    mock_iterations: crate::admin::ITERATIONS,
                },
                startup.user.clone(),
                // Generated here because entropy is I/O, and the crate that
                // owns the exchange holds none. See its AGENTS.md.
                pgprox_auth::scram::generate_nonce(),
            );
            authenticate_scram(wire, &mut scram).await?;

            // The exchange ends at SASLFinal, and a client is still waiting to
            // be told it is in: `accept` is what sends AuthenticationOk, the
            // parameters and the first ReadyForQuery. Without it psql hangs
            // after a successful authentication, which is the worst possible
            // shape for a bug in an admin path.
            let Some(conn) = context.cancels.issue() else {
                return Err(wire
                    .refuse(ClientError::Internal("no entropy for a cancel key"))
                    .await);
            };
            accept(wire, conn, &proxy_parameters()).await?;

            drop(admitted);
            return crate::admin::serve(wire, &context.observatory).await;
        }
        Credential::Jwt => {
            let startup = handshake.startup().ok_or(ShellError::Disconnected)?.clone();
            let mut auth = TokenAuth::new(&startup, std::net::IpAddr::from([0, 0, 0, 0]));
            authenticate_token(
                wire,
                &mut auth,
                context.resolver.as_ref(),
                std::time::SystemTime::now(),
            )
            .await?
        }
    };

    // The grant is what says where this tenant's database is, so the connector
    // learns it here: it is the only moment the credentials exist.
    context.connector.learn(&grant.primary);
    for replica in &grant.replicas {
        context.connector.learn(replica);
    }

    let parameters = context
        .parameters
        .ensure(context.connector.as_ref(), &grant.primary)
        .await
        .map_err(|err| ShellError::Refused(err.into()))?;

    // Refused rather than issued from a fallback: a cancel key is a bearer
    // token, and one drawn from anything predictable lets a tenant cancel its
    // neighbour's queries.
    let Some(conn) = context.cancels.issue() else {
        return Err(wire
            .refuse(ClientError::Internal("no entropy for a cancel key"))
            .await);
    };
    accept(wire, conn, &parameters).await?;

    // The signal a shed decision fires. Registered with the session rather
    // than held by it, because the decision is the node's and the session is a
    // task on a socket.
    let shed = crate::run::Shutdown::new();
    let _registered = context.sessions.register(
        conn,
        grant.tenant.clone(),
        context.node,
        context.clock.now(),
        // The tenant's own allowance where the grant states one. A grant that
        // does not is a tenant with no per-tenant cap, and the shed decision
        // reads a zero budget as "no headroom anywhere", which refuses rather
        // than moves. Refusing is the direction that costs nobody a reconnect.
        grant.pool.max_upstream.unwrap_or(0),
        shed.clone(),
    );
    let outcome = relay(wire, context, &grant, conn, &shed).await;
    drop(admitted);
    outcome
}

/// Moves frames between a client and the upstream connections it borrows.
async fn relay<S>(
    wire: &mut Wire<S>,
    context: &Context,
    grant: &Grant,
    conn: ConnId,
    shed: &crate::run::Shutdown,
) -> Result<(), ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut relay = Relay::new();
    // What this session expects any connection it borrows to look like, and
    // what each borrowed connection currently carries. A transaction-pooling
    // proxy hands a session a different connection per transaction, so without
    // these a `SET` is silently lost at the next boundary.
    let mut session_state = pgprox_session::resume::SessionMemory::default();
    let mut held: Option<(UpstreamGuard, Upstreamed<crate::dial::Stream>)> = None;
    let mut body = Vec::new();
    // How many `ParseComplete`s belong to statements this proxy replayed
    // rather than to anything the client sent.
    let mut swallow_parse_complete = 0_usize;
    // The watch this grant's replicas are polled into, shared with every other
    // session on the same primary. A grant with no replicas gets none, and
    // every route decision for it lands on the primary by the same rule that
    // sends a read to the primary when no replica is eligible.
    let watch = context.replicas.watch_for(grant);
    let none = Replicas::new(0, pgprox_route::replica::ReplicaConfig::default());

    loop {
        // A session between transactions leaves as soon as the node says it is
        // draining. One holding a connection stays until it gives it back, or
        // until the grace timer says otherwise: finishing in-flight work is
        // what a drain is for.
        let idle = held.is_none();
        let tag = tokio::select! {
            result = wire.read_tagged(&mut body) => result?,
            () = context.draining.waited(), if idle => {
                return Err(wire.refuse(ClientError::Draining).await);
            }
            // A shed is the same shape as a drain and for the same reason:
            // 57P01 is what every mainstream driver reconnects from, and only
            // between transactions, because relocating a client must not cost
            // it work it had already done.
            () = shed.waited(), if idle => {
                return Err(wire
                    .refuse(ClientError::Shed {
                        tenant: grant.tenant.clone(),
                    })
                    .await);
            }
            () = context.closing.waited() => return Ok(()),
        };
        let frame = Frame::new(tag, &body);
        let Ok(message) = pgprox_proto::frontend::decode(&frame) else {
            return Err(wire
                .refuse(ClientError::ProtocolViolation("undecodable message"))
                .await);
        };

        let now = context.clock.now();
        let rewritten = observe(&message, &body, &mut session_state);
        let Some(outgoing) = rewritten else {
            return Err(wire
                .refuse(ClientError::ProtocolViolation(
                    "a statement name this session never parsed",
                ))
                .await);
        };

        let outcome = match watch.as_ref() {
            Some(watch) => watch.with_replicas(|replicas| relay.on_client(&message, replicas, now)),
            None => relay.on_client(&message, &none, now),
        };
        if let Some(reason) = outcome.pinned {
            context.sessions.set_pinned(conn, reason.as_str());
        }

        match outcome.action {
            ClientAction::Close => return Ok(()),
            ClientAction::Answer(_) => {
                // A SET pgprox.route the server never sees. The client still
                // needs a ReadyForQuery, or it waits forever for one.
                wire.queue(|out| encode::ready_for_query(out, TxStatus::Idle));
                wire.flush().await?;
                continue;
            }
            ClientAction::Send {
                acquire: true,
                target,
            } => {
                let mut borrowed = borrow(context, grant, target, conn).await?;
                // Before the client's own frame reaches the server, and only
                // where the connection does not already match: a warm pool
                // serving one tenant replays nothing.
                resume(&mut borrowed.1, &session_state).await?;
                held = Some(borrowed);
                relay.acquired();
            }
            ClientAction::Send { acquire: false, .. } => {}
        }

        // A `Bind` names a statement the target connection may never have
        // seen: this session prepared it, and the pool has since handed the
        // session a different connection. The `Parse` goes first, and its
        // `ParseComplete` is swallowed below, because the client did not ask
        // for it and a driver counting replies would be one ahead.
        let injected = matches!(
            message,
            pgprox_proto::frontend::FrontendMessage::Bind { .. }
        );

        let Some((_guard, upstream)) = held.as_mut() else {
            return Err(wire
                .refuse(ClientError::ProtocolViolation(
                    "a message arrived with no connection to send it on",
                ))
                .await);
        };

        if injected {
            swallow_parse_complete += prepare_for_bind(upstream, &message, &session_state);
        }

        // Forwarded with the statement name mapped and nothing else touched:
        // the relay never rewrites what it does not have to.
        forward(&mut upstream.wire, tag, &outgoing);
        upstream.wire.flush().await?;

        if pump(
            wire,
            upstream,
            &mut relay,
            context,
            conn,
            &mut swallow_parse_complete,
        )
        .await?
        {
            release(&mut held, &mut relay, context, conn).await?;
        }
    }
}

/// Takes an upstream connection for this session to use.
async fn borrow(
    context: &Context,
    grant: &Grant,
    target: pgprox_core::route::RouteTarget,
    conn: ConnId,
) -> Result<(UpstreamGuard, Upstreamed<crate::dial::Stream>), ShellError> {
    // The route decision names a replica by its index in the grant. This is
    // where that becomes a backend, and therefore a pool.
    let backend = crate::replicas::backend_for(grant, target);
    let deadline = context.clock.now() + context.acquire_timeout;
    context
        .sessions
        .set_state(conn, ClientState::Waiting, context.clock.now());

    let guard = context
        .pool
        .acquire(&backend.pool_key(), deadline)
        .await
        .map_err(|err| ShellError::Refused(err.into()))?;
    let taken = context
        .pool
        .take_connection(guard.key(), guard.id())
        .ok_or(ShellError::Disconnected)?;

    // Registered while it is held and only while it is held: cancelling a
    // connection that has gone back to the pool cancels a stranger's query.
    context.cancels.hold(
        conn,
        pgprox_session::cancel::Cancellation {
            server: backend.server.clone(),
            key: guard.key().clone(),
            backend_key: taken.backend_key.unwrap_or((0, 0)),
        },
    );
    context
        .sessions
        .set_state(conn, ClientState::Active, context.clock.now());

    Ok((guard, taken))
}

/// Sends the `Parse` a `Bind` needs, when the connection may not hold it.
///
/// Returns how many `ParseComplete`s the client must not be shown. The client
/// did not send this `Parse`, and a driver counting replies would be one
/// ahead of the server for the rest of the session.
///
/// Always, rather than only when the connection is known not to hold it: the
/// pool does not yet remember what each connection carries, so preparing again
/// is the answer that cannot be wrong. `Parse` of a statement a connection
/// already holds is accepted by Postgres.
fn prepare_for_bind(
    upstream: &mut Upstreamed<crate::dial::Stream>,
    message: &pgprox_proto::frontend::FrontendMessage<'_>,
    session: &pgprox_session::resume::SessionMemory,
) -> usize {
    let pgprox_proto::frontend::FrontendMessage::Bind { statement, .. } = message else {
        return 0;
    };
    let Some(prepared) = session.statements.get(statement) else {
        return 0;
    };

    let global = prepared.global.as_str().to_owned();
    let sql = prepared.sql.clone();
    upstream.wire.queue(|out| {
        pgprox_proto::encode_frontend::parse(out, &global, &sql);
    });
    1
}

/// Records what this statement does to the session, and maps its name.
///
/// Both halves of "what does this session look like to the next connection it
/// borrows": a `SET` it has to be told about, and a prepared statement it has
/// to hold. Together rather than apart because they are read from the same
/// frame and a caller that did one and forgot the other is the bug.
fn observe(
    message: &pgprox_proto::frontend::FrontendMessage<'_>,
    body: &[u8],
    session: &mut pgprox_session::resume::SessionMemory,
) -> Option<Vec<u8>> {
    use pgprox_proto::frontend::FrontendMessage as Message;

    if let Message::Query { sql } | Message::Parse { sql, .. } = message {
        session
            .params
            .observe_statement(sql, pgprox_pool::pin::REPLAYABLE_PARAMETERS);
    }
    map_statement_name(message, body, session)
}

/// Gives the borrowed connection back at a transaction boundary.
///
/// Everything that has to happen exactly once per transaction, in the order it
/// has to happen in: the write position is read while a connection to the
/// primary is still held, the connection goes back, and only then is the guard
/// marked clean.
async fn release(
    held: &mut Option<(UpstreamGuard, Upstreamed<crate::dial::Stream>)>,
    relay: &mut Relay,
    context: &Context,
    conn: ConnId,
) -> Result<(), ShellError> {
    // Before the connection goes back, and only when the transaction wrote:
    // the position has to come from the primary, and this is the last moment a
    // connection to it is held. A failure leaves the watermark where it was,
    // so the session keeps reading from the primary rather than from a replica
    // that may not have the write.
    if relay.wrote()
        && let Some((_, upstream)) = held.as_mut()
        && let Ok(lsn) = pgprox_session::probe::primary_lsn(&mut upstream.wire).await
    {
        relay.record_write(lsn);
    }

    let (mut guard, upstream) = held.take().ok_or(ShellError::Disconnected)?;
    context
        .pool
        .return_connection(guard.key(), guard.id(), upstream);
    // Marked clean only here, at the boundary the relay named: a guard dropped
    // without this discards its connection, which is right for every other way
    // out of the relay loop and wrong for this one.
    guard.release_clean();
    context.cancels.release(conn);
    context.sessions.count_transaction();
    context
        .sessions
        .set_state(conn, ClientState::Idle, context.clock.now());
    relay.released();
    Ok(())
}

/// Puts this proxy's name for a prepared statement into the frame.
///
/// The client's own name is private to its connection here: two sessions may
/// both call one `s1`, and a session's `s1` is bound on whichever connection
/// the pool lends it next. What goes on the wire is derived from the SQL, so
/// two sessions preparing the same statement share one server-side name.
///
/// `None` means the frame cannot be forwarded: a `Bind` for something this
/// session never parsed, or a body whose name field could not be found. Both
/// are refusals rather than pass-throughs, because forwarding either would
/// send a private name to a server that has never seen it.
fn map_statement_name(
    message: &pgprox_proto::frontend::FrontendMessage<'_>,
    body: &[u8],
    session: &mut pgprox_session::resume::SessionMemory,
) -> Option<Vec<u8>> {
    use pgprox_proto::frontend::FrontendMessage as Message;
    use pgprox_proto::rewrite;

    match message {
        Message::Parse { statement, sql } => {
            let global = session.statements.parse(statement, sql);
            rewrite::parse_statement(body, global.as_str())
        }
        Message::Bind { statement, .. } => {
            let prepared = session.statements.get(statement)?;
            rewrite::bind_statement(body, prepared.global.as_str())
        }
        Message::Describe { name, .. } | Message::Close { name, .. }
            if rewrite::describes_statement(body) =>
        {
            let prepared = session.statements.get(name)?;
            rewrite::described_statement(body, prepared.global.as_str())
        }
        // Everything else travels as it arrived. A portal is the client's own
        // name for a result set and this proxy does not rename it.
        _ => Some(body.to_vec()),
    }
}

/// Brings a freshly borrowed connection up to this session's parameters.
///
/// Runs before the client's own frame, and sends nothing at all when the
/// connection already matches, which is the common case for a warm pool
/// serving one tenant. Each replayed statement's answer is read and discarded:
/// the client asked for none of them and must not see them.
async fn resume(
    upstream: &mut Upstreamed<crate::dial::Stream>,
    session: &pgprox_session::resume::SessionMemory,
) -> Result<(), ShellError> {
    use pgprox_session::resume::Step;

    // The connection's own memory is not tracked across borrows yet, so this
    // replays onto a connection assumed to carry nothing. Correct and
    // occasionally wasteful: a `SET` applied twice is the same as once, where
    // a `SET` skipped is a session that forgot something. `M6.49` gives the
    // pool the memory that makes the skip safe.
    let connection = pgprox_session::resume::ConnectionMemory::default();

    for step in pgprox_session::resume::on_acquire(session, &connection) {
        let Step::Run(sql) = step else {
            continue;
        };
        upstream
            .wire
            .queue(|out| pgprox_proto::encode_frontend::query(out, &sql));
        upstream.wire.flush().await?;

        let mut body = Vec::new();
        loop {
            let tag = upstream.wire.read_tagged(&mut body).await?;
            if tag == pgprox_proto::frame::Tag::READY_FOR_QUERY {
                break;
            }
            if tag == pgprox_proto::frame::Tag::ERROR_RESPONSE {
                // A parameter the server refuses is the session's problem
                // rather than this connection's, and the client is about to
                // send a statement that will fail in a way it can read. The
                // replay stops here rather than pretending it worked.
                return Err(ShellError::Refused(ClientError::ProtocolViolation(
                    "a replayed session parameter was refused",
                )));
            }
        }
    }
    Ok(())
}

/// Copies the server's answer back, returning whether the connection is free.
async fn pump<S>(
    wire: &mut Wire<S>,
    upstream: &mut Upstreamed<crate::dial::Stream>,
    relay: &mut Relay,
    context: &Context,
    conn: ConnId,
    swallow_parse_complete: &mut usize,
) -> Result<bool, ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut body = Vec::new();
    loop {
        let tag = upstream.wire.read_tagged(&mut body).await?;
        let frame = Frame::new(tag, &body);
        let decoded = backend::decode(&frame).unwrap_or(BackendMessage::Opaque(tag));
        let server = relay.on_server(&decoded);
        if let Some(reason) = server.pinned {
            context.sessions.set_pinned(conn, reason.as_str());
        }

        // The reply to a `Parse` this proxy sent on the client's behalf. The
        // client never sent that `Parse` and must not see its completion, or
        // every reply after it is one out of step.
        if tag == pgprox_proto::frame::Tag::PARSE_COMPLETE && *swallow_parse_complete > 0 {
            *swallow_parse_complete -= 1;
            continue;
        }

        forward(wire, tag, &body);

        // A copy reverses the direction the conversation is going in, and this
        // loop is one-way. Everything else the proxy relays is a request the
        // client made and an answer the server gives, so reading until
        // `ReadyForQuery` is exactly right; a copy-in is the server asking the
        // client for data, and waiting for a `ReadyForQuery` that cannot
        // arrive until the client has sent it wedges both sides. `pgprox-proto`
        // has tracked COPY since M1.8 and nothing here used it.
        if matches!(
            decoded,
            BackendMessage::CopyInResponse | BackendMessage::CopyBothResponse
        ) {
            wire.flush().await?;
            copying(wire, upstream, &mut body).await?;
            continue;
        }

        if matches!(decoded, BackendMessage::ReadyForQuery(_)) {
            wire.flush().await?;
            return Ok(server.release);
        }
    }
}

/// Moves the client's copy data upstream until the copy ends.
///
/// Returns when the client has said it is finished, with `CopyDone` or
/// `CopyFail`, or when it has sent something else: `Terminate` and a stray
/// `Query` both end the copy from the protocol's point of view, and the
/// server's answer to either comes back through the caller's loop.
///
/// The direction is one-way on purpose. During a copy-in the server sends
/// nothing until the stream ends, apart from an `ErrorResponse` it is entitled
/// to send at any point; that error arrives on the caller's next read, which is
/// where every other server message is handled. Racing both directions here
/// would put two readers on the same connection for no case that needs one.
async fn copying<S>(
    wire: &mut Wire<S>,
    upstream: &mut Upstreamed<crate::dial::Stream>,
    body: &mut Vec<u8>,
) -> Result<(), ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    use pgprox_proto::frame::Tag;

    loop {
        let tag = wire.read_tagged(body).await?;
        forward(&mut upstream.wire, tag, body);
        upstream.wire.flush().await?;

        if tag != Tag::COPY_DATA {
            return Ok(());
        }
    }
}

/// Queues one frame verbatim.
fn forward<S>(wire: &mut Wire<S>, tag: pgprox_proto::frame::Tag, body: &[u8])
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    wire.queue(|out| {
        out.push(tag.get());
        out.extend_from_slice(
            &u32::try_from(body.len() + 4)
                .unwrap_or(u32::MAX)
                .to_be_bytes(),
        );
        out.extend_from_slice(body);
    });
}

/// Forwards a cancellation, or refuses it.
///
/// Whatever happens, nothing is sent back: a `CancelRequest` gets no answer by
/// design, so a client cannot use one to learn whether a key is real.
async fn cancel(conn: ConnId, context: &Context) -> Result<(), ShellError> {
    context.deliver(conn).await;
    Ok(())
}

impl Context {
    /// Cancels a query this node is running, or passes the request on.
    async fn deliver(&self, conn: ConnId) {
        use pgprox_session::cancel::Routing;

        match self.cancels.route(conn) {
            Routing::Local(cancellation) => {
                let Some(backend) = self.connector.backend(&cancellation.key) else {
                    return;
                };
                let Ok(stream) = self.connector.dial(&backend).await else {
                    return;
                };
                let _ = pgprox_session::cancel::send(stream, cancellation.backend_key).await;
            }
            // The node in the key is which pod issued it, and a client's
            // cancel arrives on whichever pod its second connection reached.
            // A peer this node has no address for is a cancel that cannot be
            // delivered, which is the same outcome as an unknown key: nothing.
            Routing::Peer(node) => {
                if let Some(peer) = self.peers.get(&node) {
                    crate::gossip::forward(peer, conn).await;
                }
            }
            Routing::Unknown => {}
        }
    }
}

#[async_trait::async_trait]
impl crate::gossip::CancelSink for Context {
    fn clients(&self) -> Vec<pgprox_core::admin::ClientView> {
        self.sessions.views(self.clock.now())
    }

    async fn cancel(&self, conn: ConnId) {
        // A forwarded cancel is delivered locally or dropped. Forwarding it
        // again would let two nodes with a stale peer table bounce one between
        // them forever.
        use pgprox_session::cancel::Routing;

        if matches!(self.cancels.route(conn), Routing::Local(_)) {
            self.deliver(conn).await;
        }
    }
}

/// Tells a client the node is full, then closes.
///
/// # Errors
///
/// Fails when the socket does. The refusal itself is not an error here: it is
/// the point.
pub async fn refuse_full<S>(stream: S, cap: u32) -> Result<(), ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut wire = Wire::new(stream);
    // Read whatever the client opened with, so the error lands where a driver
    // expects one rather than in the middle of its startup.
    let mut body = Vec::new();
    let _ = wire.read_untagged(&mut body).await;

    wire.queue(|out| {
        encode::error_response(
            out,
            &ClientError::UpstreamAtCap {
                server: pgprox_core::ids::ServerId::new("this node", 0),
                cap,
            },
        );
    });
    wire.flush().await
}

/// Accepts clients until the listener stops.
///
/// Each one is served on its own task. A session that fails takes nothing with
/// it: the error is the client's, and a node serving a hundred thousand of
/// them cannot let one end the loop.
///
/// # Errors
///
/// Fails only when accepting itself does, which means the listening socket has
/// gone and there is nothing left to serve.
pub async fn accept_loop(
    listener: tokio::net::TcpListener,
    context: Arc<Context>,
    gate: Arc<Gate>,
    ceiling: u32,
) -> std::io::Result<()> {
    loop {
        let (socket, _) = listener.accept().await?;
        let _ = socket.set_nodelay(true);
        let context = Arc::clone(&context);
        let gate = Arc::clone(&gate);

        tokio::spawn(async move {
            // A draining node refuses rather than stopping its listener. The
            // socket is accepted so the client is told 57P01, which every
            // mainstream driver treats as a clean server-initiated close and
            // reconnects from; a closed listener would leave it retrying
            // against a refused connection instead. It is also what makes a
            // drain reversible: nothing had to be torn down to start it.
            if context.draining.fired() {
                tracing::debug!("refused a client: this node is draining");
                let _ = refuse_draining(socket).await;
                return;
            }
            // Then admission, and that refusal is a message too. A driver told
            // 53300 reports it; a driver whose socket vanished reports a
            // network error.
            let Some(admitted) = gate.admit() else {
                // Warn rather than debug: this is the node at its ceiling,
                // which is a capacity decision somebody has to take.
                tracing::warn!(ceiling, "refused a client: at the connection ceiling");
                let _ = refuse_full(socket, ceiling).await;
                return;
            };
            let _ = session(socket, context.as_ref(), admitted).await;
        });
    }
}

/// Tells a client the node is going away, then closes.
///
/// # Errors
///
/// Fails when the socket does.
pub async fn refuse_draining<S>(stream: S) -> Result<(), ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut wire = Wire::new(stream);
    let mut body = Vec::new();
    let _ = wire.read_untagged(&mut body).await;

    wire.queue(|out| encode::error_response(out, &ClientError::Draining));
    wire.flush().await
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use pgprox_proto::frame::Tag;

    #[test]
    fn a_gate_admits_up_to_its_ceiling() {
        let gate = Arc::new(Gate::new(2));
        let first = gate.admit().expect("room for one");
        let second = gate.admit().expect("room for two");

        assert_eq!(gate.live(), 2);
        assert!(
            gate.admit().is_none(),
            "a full node admitted a third client"
        );

        drop(first);
        assert!(
            gate.admit().is_some(),
            "a departed client did not free its place"
        );
        drop(second);
    }

    #[test]
    fn a_gate_of_zero_admits_nobody() {
        // Configuration validation refuses this, and the gate must not be the
        // thing that decides otherwise.
        assert!(Arc::new(Gate::new(0)).admit().is_none());
    }

    #[tokio::test]
    async fn a_full_node_says_so_rather_than_dropping_the_socket() {
        // A dropped socket reads as a network fault to every driver and sends
        // the operator to the wrong place.
        use tokio::io::AsyncReadExt;

        let (ours, mut theirs) = tokio::io::duplex(4096);
        let client = tokio::spawn(async move {
            // A startup packet, so the refusal lands where a driver expects.
            let mut packet = Vec::new();
            pgprox_proto::encode_frontend::startup_message(
                &mut packet,
                pgprox_proto::encode::PROTOCOL_3_0,
                &[("user", "acme_app")],
            );
            tokio::io::AsyncWriteExt::write_all(&mut theirs, &packet)
                .await
                .unwrap();

            let mut header = [0_u8; 5];
            theirs.read_exact(&mut header).await.unwrap();
            let len = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
            let mut body = vec![0; len - 4];
            theirs.read_exact(&mut body).await.unwrap();
            (Tag(header[0]), body)
        });

        refuse_full(ours, 10).await.unwrap();
        let (tag, body) = client.await.unwrap();

        assert_eq!(tag, Tag::ERROR_RESPONSE);
        let rendered = String::from_utf8_lossy(&body);
        assert!(rendered.contains("53300"), "{rendered}");
        assert!(
            rendered.contains("too many connections"),
            "the client was not told why: {rendered}"
        );
    }

    use pgprox_core::auth::{Backend, ClaimSet, FakeCredentialResolver, PoolHints, TlsMode};
    use pgprox_core::clock::FakeClock;
    use pgprox_core::ids::{PoolKey, ServerId};
    use pgprox_core::secret::SecretString;
    use pgprox_pool::live::LivePool;
    use pgprox_pool::pool::PoolConfig;
    use pgprox_session::state::TlsPosture;
    use std::net::SocketAddr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Entropy that is not, so a test's cancel keys are predictable.
    #[derive(Debug, Default)]
    struct Fixed;

    impl pgprox_session::cancel::Entropy for Fixed {
        fn next(&self) -> Option<u64> {
            Some(42)
        }
    }

    /// A Postgres that authenticates anyone and answers every query the same
    /// way.
    ///
    /// A real socket speaking the real protocol: everything the proxy sends it
    /// is decoded by this project's own decoder, and everything it sends back
    /// goes through the proxy's relay untouched.
    /// Every statement each fake server was sent, by port.
    ///
    /// A test asserting that something was replayed has to see what reached
    /// the server, and the alternative is a fake per test that reads the same
    /// bytes a different way.
    fn seen() -> &'static std::sync::Mutex<std::collections::HashMap<u16, Vec<String>>> {
        static SEEN: std::sync::OnceLock<
            std::sync::Mutex<std::collections::HashMap<u16, Vec<String>>>,
        > = std::sync::OnceLock::new();
        SEEN.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
    }

    /// What one fake server was sent.
    fn statements_seen(addr: SocketAddr) -> Vec<String> {
        seen()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&addr.port())
            .cloned()
            .unwrap_or_default()
    }

    async fn fake_postgres() -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    // The startup packet.
                    let mut len = [0_u8; 4];
                    if socket.read_exact(&mut len).await.is_err() {
                        return;
                    }
                    let mut body = vec![0; u32::from_be_bytes(len) as usize - 4];
                    let _ = socket.read_exact(&mut body).await;

                    let mut out = Vec::new();
                    encode::authentication_ok(&mut out);
                    encode::parameter_status(&mut out, "server_version", "17.2");
                    encode::backend_key_data(&mut out, ConnId::new(NodeId::new(9), 0x00AB_CDEF));
                    encode::ready_for_query(&mut out, TxStatus::Idle);
                    let _ = socket.write_all(&out).await;

                    // Then one canned answer per query.
                    let mut in_transaction = false;
                    loop {
                        let mut header = [0_u8; 5];
                        if socket.read_exact(&mut header).await.is_err() {
                            return;
                        }
                        let len = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
                        let mut body = vec![0; len - 4];
                        if socket.read_exact(&mut body).await.is_err() {
                            return;
                        }

                        let mut out = Vec::new();
                        // The replica poller's own question, answered as a
                        // replica answers it: a replay position and t for
                        // pg_is_in_recovery. Every other query gets the canned
                        // completion below.
                        let sql = String::from_utf8_lossy(&body)
                            .trim_end_matches('\0')
                            .to_owned();
                        seen()
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .entry(addr.port())
                            .or_default()
                            .push(sql.clone());

                        if sql.contains("pg_last_wal_replay_lsn") {
                            out.extend_from_slice(&text_row(&[Some(REPLICA_REPLAYED), Some("t")]));
                            encode::ready_for_query(&mut out, TxStatus::Idle);
                            if socket.write_all(&out).await.is_err() {
                                return;
                            }
                            continue;
                        }
                        // The position the proxy asks for after a write, ahead
                        // of anything the replica reports having replayed.
                        if sql.contains("pg_current_wal_insert_lsn") {
                            out.extend_from_slice(&text_row(&[Some(PRIMARY_WRITTEN)]));
                            encode::ready_for_query(&mut out, TxStatus::Idle);
                            if socket.write_all(&out).await.is_err() {
                                return;
                            }
                            continue;
                        }
                        // A copy-in, answered as the server answers one: an
                        // invitation, then nothing until the client says it is
                        // done.
                        if sql.contains("COPY") && sql.contains("FROM STDIN") {
                            if serve_copy_in(&mut socket).await.is_err() {
                                return;
                            }
                            continue;
                        }

                        // The transaction status is what the relay releases on,
                        // so the fake has to track it: answering Idle to a
                        // BEGIN would make every session look releasable while
                        // it was mid-transaction.
                        if sql.contains("BEGIN") {
                            in_transaction = true;
                        } else if sql.contains("COMMIT") || sql.contains("ROLLBACK") {
                            in_transaction = false;
                        }

                        out.push(Tag::COMMAND_COMPLETE.get());
                        let text = b"SELECT 1\0";
                        out.extend_from_slice(
                            &u32::try_from(text.len() + 4).unwrap().to_be_bytes(),
                        );
                        out.extend_from_slice(text);
                        encode::ready_for_query(
                            &mut out,
                            if in_transaction {
                                TxStatus::InTransaction
                            } else {
                                TxStatus::Idle
                            },
                        );
                        if socket.write_all(&out).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });

        addr
    }

    /// How far the fake replica says it has replayed.
    const REPLICA_REPLAYED: &str = "16/B374D848";

    /// Where the fake primary says the last write landed, which is ahead of it.
    const PRIMARY_WRITTEN: &str = "16/C0000000";

    /// Answers a `COPY ... FROM STDIN` the way a server does: an invitation,
    /// silence until the client finishes, then a completion.
    ///
    /// Silence is the point. A server that answered a copy-in immediately
    /// would not reproduce the deadlock this fake exists to catch.
    async fn serve_copy_in(socket: &mut tokio::net::TcpStream) -> std::io::Result<()> {
        let mut invitation = vec![Tag::COPY_IN_RESPONSE.get()];
        // Length, the overall format (text), and no per-column formats.
        invitation.extend_from_slice(&7_u32.to_be_bytes());
        invitation.extend_from_slice(&[0, 0, 0]);
        socket.write_all(&invitation).await?;

        loop {
            let mut header = [0_u8; 5];
            socket.read_exact(&mut header).await?;
            let len = u32::from_be_bytes(header[1..].try_into().unwrap_or([0; 4])) as usize;
            let mut chunk = vec![0; len.saturating_sub(4)];
            socket.read_exact(&mut chunk).await?;
            if header[0] != Tag::COPY_DATA.get() {
                break;
            }
        }

        let mut done = vec![Tag::COMMAND_COMPLETE.get()];
        let text = b"COPY 2\0";
        done.extend_from_slice(&u32::try_from(text.len() + 4).unwrap().to_be_bytes());
        done.extend_from_slice(text);
        encode::ready_for_query(&mut done, TxStatus::Idle);
        socket.write_all(&done).await
    }

    /// One `DataRow` carrying text values.
    fn text_row(values: &[Option<&str>]) -> Vec<u8> {
        let mut body = i16::try_from(values.len()).unwrap().to_be_bytes().to_vec();
        for value in values {
            match *value {
                None => body.extend_from_slice(&(-1_i32).to_be_bytes()),
                Some(text) => {
                    body.extend_from_slice(&i32::try_from(text.len()).unwrap().to_be_bytes());
                    body.extend_from_slice(text.as_bytes());
                }
            }
        }
        let mut out = vec![Tag::DATA_ROW.get()];
        out.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_be_bytes());
        out.extend_from_slice(&body);
        out
    }

    fn grant_for(addr: SocketAddr) -> Grant {
        Grant {
            tenant: pgprox_core::ids::TenantId::new("acme"),
            primary: Backend {
                server: ServerId::new("127.0.0.1", addr.port()),
                database: "acme".into(),
                user: "acme_app".into(),
                password: SecretString::new("hunter2"),
                tls: TlsMode::Disabled,
            },
            replicas: Vec::new(),
            pool: PoolHints::default(),
            ttl: Duration::from_secs(60),
            claims: ClaimSet::default(),
        }
    }

    fn context_for(addr: SocketAddr) -> Context {
        let clock: Arc<dyn Clock> = Arc::new(FakeClock::new());
        let tls = pgprox_tls::client_config(tokio_rustls::rustls::RootCertStore::empty()).unwrap();
        let connector = Arc::new(PgConnector::new(TcpUpstream::new(tls)));

        Context {
            node: NodeId::new(1),
            clock: Arc::clone(&clock),
            handshake: HandshakeConfig {
                // No certificates are configured in a test, and a listener
                // that required TLS would refuse every client before the part
                // under test.
                tls: TlsPosture::Optional,
                static_users: Vec::new(),
            },
            resolver: Arc::new(
                FakeCredentialResolver::new().with_grant("good.token", grant_for(addr)),
            ),
            pool: LivePool::new(Arc::clone(&connector), clock, PoolConfig::default()),
            connector,
            parameters: Arc::new(ParameterCache::new()),
            sessions: Sessions::new(),
            cancels: Arc::new(Registry::new(NodeId::new(1), Box::new(Fixed))),
            acquire_timeout: Duration::from_secs(5),
            statics: None,
            observatory: pgprox_core::admin::FakeObservatory::new(NodeId::new(1)),
            tls: None,
            draining: crate::run::Shutdown::new(),
            closing: crate::run::Shutdown::new(),
            peers: std::collections::BTreeMap::new(),
            replicas: Arc::new(crate::replicas::ReplicaSets::new(
                TcpUpstream::new(
                    pgprox_tls::client_config(tokio_rustls::rustls::RootCertStore::empty())
                        .unwrap(),
                ),
                Arc::new(FakeClock::new()),
                crate::run::Shutdown::new(),
            )),
        }
    }

    /// Reads one tagged message from a client's end.
    async fn expect<S: AsyncRead + Unpin>(io: &mut S) -> (Tag, Vec<u8>) {
        let mut header = [0_u8; 5];
        io.read_exact(&mut header).await.unwrap();
        let len = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
        let mut body = vec![0; len - 4];
        io.read_exact(&mut body).await.unwrap();
        (Tag(header[0]), body)
    }

    fn startup_and_password(token: &str) -> Vec<u8> {
        let mut out = Vec::new();
        pgprox_proto::encode_frontend::startup_message(
            &mut out,
            pgprox_proto::encode::PROTOCOL_3_0,
            &[("user", "acme_app"), ("database", "acme")],
        );
        pgprox_proto::encode_frontend::password_message(&mut out, token);
        out
    }

    #[tokio::test]
    async fn a_client_connects_queries_and_the_connection_goes_back_to_the_pool() {
        // The milestone in one test. Every crate in this workspace is on this
        // path: the handshake, the token exchange, the parameter probe, the
        // pool, the relay's release rule, and the session registry.
        let addr = fake_postgres().await;
        let context = context_for(addr);
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(8192);
        let served = tokio::spawn({
            // The context has to outlive the task, and everything in it is
            // already shared.
            let context = Arc::new(context);
            let held = Arc::clone(&context);
            async move {
                let _ = session(ours, held.as_ref(), admitted).await;
                context
            }
        });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();

        // The credential request, then the acceptance: two authentication
        // messages, and reading one would leave the next assertion off by one.
        assert_eq!(expect(&mut client).await.0, Tag::AUTHENTICATION);
        assert_eq!(expect(&mut client).await.0, Tag::AUTHENTICATION);
        assert_eq!(
            expect(&mut client).await.0,
            Tag::PARAMETER_STATUS,
            "the client was not told what server it reached"
        );
        assert_eq!(expect(&mut client).await.0, Tag::BACKEND_KEY_DATA);
        let (tag, body) = expect(&mut client).await;
        assert_eq!(tag, Tag::READY_FOR_QUERY);
        assert_eq!(body, b"I");

        let mut query = Vec::new();
        pgprox_proto::encode_frontend::query(&mut query, "SELECT 1");
        client.write_all(&query).await.unwrap();

        assert_eq!(expect(&mut client).await.0, Tag::COMMAND_COMPLETE);
        assert_eq!(expect(&mut client).await.0, Tag::READY_FOR_QUERY);

        // Terminate, so the session ends and the task returns the context.
        let mut bye = Vec::new();
        pgprox_proto::encode_frontend::terminate(&mut bye);
        client.write_all(&bye).await.unwrap();

        let context = served.await.unwrap();
        let key = PoolKey::new(ServerId::new("127.0.0.1", addr.port()), "acme", "acme_app");
        let stats = pgprox_core::pool::UpstreamPool::stats(context.pool.as_ref(), &key);
        assert_eq!(
            stats.idle, 1,
            "the connection did not go back to the pool: {stats:?}"
        );
        assert_eq!(stats.active, 0);
        assert_eq!(context.sessions.transactions(), 1);
    }

    /// A context whose grant names one replica, both fakes on real sockets.
    ///
    /// A real clock, because the poller's freshness window is measured on one
    /// and a fake clock would leave every reading eternally new.
    fn with_a_replica(primary: SocketAddr, replica: SocketAddr) -> Context {
        let mut context = context_for(primary);
        let clock: Arc<dyn Clock> = Arc::new(pgprox_core::clock::SystemClock);
        context.clock = Arc::clone(&clock);
        context.replicas = Arc::new(crate::replicas::ReplicaSets::new(
            TcpUpstream::new(
                pgprox_tls::client_config(tokio_rustls::rustls::RootCertStore::empty()).unwrap(),
            ),
            clock,
            crate::run::Shutdown::new(),
        ));

        let mut grant = grant_for(primary);
        grant.replicas = vec![Backend {
            server: ServerId::new("127.0.0.1", replica.port()),
            ..grant_for(replica).primary
        }];
        context.resolver = Arc::new(FakeCredentialResolver::new().with_grant("good.token", grant));
        context
    }

    /// How many connections a pool holds for a server.
    fn held_for(context: &Context, addr: SocketAddr) -> u32 {
        let key = PoolKey::new(ServerId::new("127.0.0.1", addr.port()), "acme", "acme_app");
        let stats = pgprox_core::pool::UpstreamPool::stats(context.pool.as_ref(), &key);
        stats.active + stats.idle
    }

    #[tokio::test]
    async fn a_session_that_wrote_does_not_read_from_a_replica_behind_its_write() {
        // Read-your-writes. The replica reports a position behind where the
        // primary says the write landed, so it is ineligible to this session
        // and eligible to everyone else.
        let primary = fake_postgres().await;
        let replica = fake_postgres().await;
        let context = Arc::new(with_a_replica(primary, replica));

        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();
        let (ours, mut client) = tokio::io::duplex(8192);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut client).await;
        }

        // Long enough for the poll loop, which the first statement starts, to
        // have made the replica eligible.
        let mut write = Vec::new();
        pgprox_proto::encode_frontend::query(&mut write, "INSERT INTO t VALUES (1)");
        client.write_all(&write).await.unwrap();
        expect(&mut client).await;
        expect(&mut client).await;
        tokio::time::sleep(Duration::from_millis(600)).await;

        let mut read = Vec::new();
        pgprox_proto::encode_frontend::query(&mut read, "SELECT 1");
        client.write_all(&read).await.unwrap();
        expect(&mut client).await;
        expect(&mut client).await;

        assert_eq!(
            held_for(&context, replica),
            0,
            "a session read from a replica that had not replayed its own write"
        );
        assert!(held_for(&context, primary) > 0);

        drop(client);
        let _ = served.await;
    }

    #[tokio::test]
    async fn a_prepared_statement_is_replayed_onto_the_connection_that_binds_it() {
        // The client's name for a statement is private to its connection to
        // this proxy: two sessions may both call one `s1`, and a session's
        // `s1` is bound on whichever connection the pool lends it next. What
        // goes on the wire is a name derived from the SQL, and the `Parse`
        // goes with it.
        let addr = fake_postgres().await;
        let context = Arc::new(context_for(addr));
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(8192);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut client).await;
        }

        let mut extended = Vec::new();
        pgprox_proto::encode_frontend::parse(&mut extended, "s1", "SELECT $1");
        pgprox_proto::encode_frontend::bind(&mut extended, "", "s1");
        pgprox_proto::encode_frontend::sync(&mut extended);
        client.write_all(&extended).await.unwrap();

        // The fake answers everything with a completion, so what matters is
        // what it was sent.
        expect(&mut client).await;
        expect(&mut client).await;

        let seen = statements_seen(addr);
        assert!(
            !seen.iter().any(|sql| sql.contains("s1")),
            "the client's private statement name reached the server: {seen:?}"
        );

        drop(client);
        let _ = served.await;
    }

    #[tokio::test]
    async fn a_bind_for_a_statement_never_parsed_is_refused() {
        // Postgres would refuse it too, but not for the same reason: here the
        // name cannot even be translated, so forwarding it would send a
        // private name to a server that has never seen it.
        let addr = fake_postgres().await;
        let context = Arc::new(context_for(addr));
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(8192);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut client).await;
        }

        let mut orphan = Vec::new();
        pgprox_proto::encode_frontend::bind(&mut orphan, "", "never_parsed");
        client.write_all(&orphan).await.unwrap();

        let (tag, body) = expect(&mut client).await;
        assert_eq!(tag, Tag::ERROR_RESPONSE);
        assert!(String::from_utf8_lossy(&body).contains("08P01"));
        assert!(served.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn a_replayable_parameter_survives_a_change_of_connection() {
        // The point of transaction pooling is that a session gets a different
        // upstream connection per transaction, so a `SET` that was not
        // replayed is a session that forgets things between statements.
        let addr = fake_postgres().await;
        let context = Arc::new(context_for(addr));
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(8192);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut client).await;
        }

        // A replayable parameter, then a statement on what may be another
        // connection.
        let mut set = Vec::new();
        pgprox_proto::encode_frontend::query(&mut set, "SET application_name = 'reporting'");
        client.write_all(&set).await.unwrap();
        expect(&mut client).await;
        expect(&mut client).await;

        let mut query = Vec::new();
        pgprox_proto::encode_frontend::query(&mut query, "SELECT 1");
        client.write_all(&query).await.unwrap();
        expect(&mut client).await;
        expect(&mut client).await;

        // Twice: once because the client sent it, and once replayed onto the
        // connection borrowed for the statement after it. Asserting it was
        // merely present would pass with no replay at all, since the client's
        // own `SET` reached the server.
        let sets = statements_seen(addr)
            .iter()
            .filter(|sql| sql.contains("application_name"))
            .count();
        assert!(
            sets >= 2,
            "the parameter was not replayed onto the next connection: {:?}",
            statements_seen(addr)
        );

        drop(client);
        let _ = served.await;
    }

    #[tokio::test]
    async fn a_read_lands_on_a_replica_once_one_has_been_polled() {
        // M5 built the routing rule, M5.18 built the poller and M6.14 built
        // the prober, and the session path used none of them: it made a fresh
        // empty `Replicas` per session, so every replica was permanently
        // ineligible and every read went to the primary.
        let primary = fake_postgres().await;
        let replica = fake_postgres().await;
        let context = Arc::new(with_a_replica(primary, replica));

        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();
        let (ours, mut client) = tokio::io::duplex(8192);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut client).await;
        }

        // The first statement starts the poll loop and routes before it has an
        // answer, so it goes to the primary. That is the safe direction and it
        // is what the second statement is here to move past.
        let mut query = Vec::new();
        pgprox_proto::encode_frontend::query(&mut query, "SELECT 1");
        client.write_all(&query).await.unwrap();
        expect(&mut client).await;
        expect(&mut client).await;

        tokio::time::sleep(Duration::from_millis(600)).await;
        client.write_all(&query).await.unwrap();
        expect(&mut client).await;
        expect(&mut client).await;

        assert!(
            held_for(&context, replica) > 0,
            "a read-only statement never reached the replica"
        );

        drop(client);
        let _ = served.await;
    }

    #[tokio::test]
    async fn a_copy_from_stdin_completes_rather_than_wedging() {
        // The first pgbench run the e2e stack ever did found this: the server
        // answers a COPY with CopyInResponse and then waits for the client,
        // while a one-way pump waits for a ReadyForQuery that cannot arrive
        // until the client has been let through. Both sides waited forever and
        // the session held an upstream connection while they did.
        let addr = fake_postgres().await;
        let context = Arc::new(context_for(addr));
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(8192);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut client).await;
        }

        let mut copy = Vec::new();
        pgprox_proto::encode_frontend::query(&mut copy, "COPY t FROM STDIN");
        client.write_all(&copy).await.unwrap();

        // The server's invitation reaches the client.
        let (tag, _) = expect(&mut client).await;
        assert_eq!(tag, Tag::COPY_IN_RESPONSE);

        // Which the client answers with data and an end, and only then does
        // the exchange finish.
        let mut rows = Vec::new();
        for row in ["1\tone\n", "2\ttwo\n"] {
            rows.push(Tag::COPY_DATA.get());
            rows.extend_from_slice(&u32::try_from(row.len() + 4).unwrap().to_be_bytes());
            rows.extend_from_slice(row.as_bytes());
        }
        rows.push(Tag::COPY_DONE.get());
        rows.extend_from_slice(&4_u32.to_be_bytes());

        client.write_all(&rows).await.unwrap();

        let finished = tokio::time::timeout(Duration::from_secs(5), async {
            let mut seen = Vec::new();
            while seen.len() < 2 {
                seen.push(expect(&mut client).await.0);
            }
            seen
        })
        .await
        .expect("the copy never finished: the relay is wedged");

        assert_eq!(finished.last(), Some(&Tag::READY_FOR_QUERY));
        drop(client);
        let _ = served.await;
    }

    #[tokio::test]
    async fn a_rejected_token_never_reaches_the_database() {
        // The proxy is the authentication boundary. A bad token must cost the
        // upstream nothing at all.
        let addr = fake_postgres().await;
        let context = context_for(addr);
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(8192);
        let served = tokio::spawn({
            let context = Arc::new(context);
            let held = Arc::clone(&context);
            async move {
                let result = session(ours, held.as_ref(), admitted).await;
                (result, context)
            }
        });

        client
            .write_all(&startup_and_password("wrong.token"))
            .await
            .unwrap();
        assert_eq!(expect(&mut client).await.0, Tag::AUTHENTICATION);
        let (tag, body) = expect(&mut client).await;

        assert_eq!(tag, Tag::ERROR_RESPONSE);
        assert!(String::from_utf8_lossy(&body).contains("28000"));

        let (result, context) = served.await.unwrap();
        assert!(result.is_err());
        assert_eq!(
            context.connector.known(),
            0,
            "a refused client taught the connector where a database lives"
        );
        assert!(context.sessions.is_empty());
    }

    #[tokio::test]
    async fn a_client_is_refused_when_there_is_no_entropy_for_its_cancel_key() {
        // The alternative is a key drawn from something predictable, which
        // lets one tenant cancel another's queries, and the client would have
        // no way to know it had been handed one.
        #[derive(Debug)]
        struct Dry;

        impl pgprox_session::cancel::Entropy for Dry {
            fn next(&self) -> Option<u64> {
                None
            }
        }

        let addr = fake_postgres().await;
        let mut context = context_for(addr);
        context.cancels = Arc::new(Registry::new(NodeId::new(1), Box::new(Dry)));
        let context = Arc::new(context);
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(8192);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        // The credential request, then the refusal: the client never reaches
        // AuthenticationOk, because the key it would be given cannot be made.
        assert_eq!(expect(&mut client).await.0, Tag::AUTHENTICATION);

        let (tag, body) = expect(&mut client).await;
        assert_eq!(tag, Tag::ERROR_RESPONSE);
        let rendered = String::from_utf8_lossy(&body);
        assert!(rendered.contains("XX000"), "{rendered}");
        assert!(
            !rendered.contains("entropy"),
            "the client was told which internal condition failed: {rendered}"
        );
        assert!(served.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn an_idle_client_is_told_why_when_the_node_drains() {
        // Between transactions, so it leaves at once. 57P01 is the code every
        // mainstream driver treats as a clean server-initiated close and
        // reconnects from, which is what makes a drain invisible to the app.
        let addr = fake_postgres().await;
        let context = Arc::new(context_for(addr));
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(8192);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut client).await;
        }

        context.draining.fire();

        let (tag, body) = expect(&mut client).await;
        assert_eq!(tag, Tag::ERROR_RESPONSE);
        assert!(String::from_utf8_lossy(&body).contains("57P01"));
        assert!(served.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn a_shed_client_is_told_to_reconnect_rather_than_cut_off() {
        // 57P01 is the code every mainstream driver treats as a clean
        // server-initiated close and reconnects from, which is the entire
        // mechanism: the client comes back and lands on its home node.
        let addr = fake_postgres().await;
        let context = Arc::new(context_for(addr));
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(8192);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut client).await;
        }

        let conn = context.sessions.views(context.clock.now())[0].conn;
        assert!(context.sessions.shed(conn, context.clock.now()));

        let (tag, body) = expect(&mut client).await;
        assert_eq!(tag, Tag::ERROR_RESPONSE);
        assert!(
            String::from_utf8_lossy(&body).contains("57P01"),
            "{}",
            String::from_utf8_lossy(&body)
        );
        assert!(served.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn a_client_holding_a_connection_is_not_closed_by_the_drain_alone() {
        // Finishing in-flight work is what a drain is for. The grace timer
        // behind `closing` is what bounds it, and this asserts the two are
        // different signals rather than one.
        let addr = fake_postgres().await;
        let context = Arc::new(context_for(addr));
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(8192);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut client).await;
        }

        // BEGIN, which the fake answers as it answers everything. What matters
        // is that the session now holds an upstream connection.
        let mut begin = Vec::new();
        pgprox_proto::encode_frontend::query(&mut begin, "BEGIN");
        client.write_all(&begin).await.unwrap();
        expect(&mut client).await;
        expect(&mut client).await;

        context.draining.fire();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !served.is_finished(),
            "a session mid-transaction was closed by the drain rather than by the grace timer"
        );

        context.closing.fire();
        let ended = tokio::time::timeout(Duration::from_secs(5), served)
            .await
            .expect("the grace timer did not close a session that would not leave");
        assert!(ended.is_ok());
    }

    #[tokio::test]
    async fn a_session_that_ends_leaves_no_client_behind() {
        // The registry is what SHOW CLIENTS reads. A row for a client that has
        // gone is worse than no row: an operator chasing it finds nothing.
        let addr = fake_postgres().await;
        let context = Arc::new(context_for(addr));
        let gate = Arc::new(Gate::new(10));

        let (ours, mut client) = tokio::io::duplex(8192);
        let held = Arc::clone(&context);
        let admitted = gate.admit().unwrap();
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut client).await;
        }
        assert_eq!(context.sessions.len(), 1);

        drop(client);
        let _ = served.await.unwrap();

        assert!(
            context.sessions.is_empty(),
            "a disconnected client was still listed"
        );
        assert_eq!(gate.live(), 0, "its place under the ceiling was not freed");
    }

    #[tokio::test]
    async fn a_route_hint_is_answered_without_touching_the_database() {
        // SET pgprox.route is about the session, and Postgres would reject it
        // as an unknown parameter. Answering it here is what makes it a
        // feature rather than an error, and it must cost no connection.
        let addr = fake_postgres().await;
        let context = Arc::new(context_for(addr));
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(8192);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut client).await;
        }

        let mut hint = Vec::new();
        pgprox_proto::encode_frontend::query(&mut hint, "SET pgprox.route = 'replica'");
        client.write_all(&hint).await.unwrap();

        let (tag, body) = expect(&mut client).await;
        assert_eq!(tag, Tag::READY_FOR_QUERY);
        assert_eq!(body, b"I");

        let key = PoolKey::new(ServerId::new("127.0.0.1", addr.port()), "acme", "acme_app");
        let stats = pgprox_core::pool::UpstreamPool::stats(context.pool.as_ref(), &key);
        assert_eq!(
            stats.active + stats.idle,
            0,
            "a statement the server never sees opened a connection"
        );

        drop(client);
        let _ = served.await.unwrap();
    }

    /// A self-signed certificate and the client configuration that trusts it.
    fn certificate() -> (
        Arc<tokio_rustls::rustls::ServerConfig>,
        Arc<tokio_rustls::rustls::ClientConfig>,
    ) {
        use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};

        let issued = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
        let cert = CertificateDer::from(issued.cert.der().to_vec());
        let key = PrivateKeyDer::try_from(issued.signing_key.serialize_der()).unwrap();

        let server = pgprox_tls::server_config(vec![cert.clone()], key).unwrap();

        let mut roots = tokio_rustls::rustls::RootCertStore::empty();
        roots.add(cert).unwrap();
        (server, pgprox_tls::client_config(roots).unwrap())
    }

    #[tokio::test]
    async fn a_client_that_asks_for_tls_gets_it() {
        // Until this, every JWT crossed the network in cleartext: the listener
        // answered `N` and the session refused a client that insisted.
        use tokio_rustls::rustls::pki_types::ServerName;

        let addr = fake_postgres().await;
        let (server_tls, client_tls) = certificate();
        let mut context = context_for(addr);
        context.tls = Some(tokio_rustls::TlsAcceptor::from(server_tls));
        context.handshake.tls = TlsPosture::Required;
        let context = Arc::new(context);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = listener.local_addr().unwrap();
        let gate = Arc::new(Gate::new(10));
        tokio::spawn(accept_loop(listener, Arc::clone(&context), gate, 10));

        let mut socket = tokio::net::TcpStream::connect(proxy).await.unwrap();
        let mut request = Vec::new();
        pgprox_proto::encode_frontend::ssl_request(&mut request);
        socket.write_all(&request).await.unwrap();

        let mut answer = [0_u8; 1];
        socket.read_exact(&mut answer).await.unwrap();
        assert_eq!(answer[0], b'S', "the listener refused to upgrade");

        let connector = tokio_rustls::TlsConnector::from(client_tls);
        let mut tls = connector
            .connect(ServerName::try_from("localhost").unwrap(), socket)
            .await
            .expect("the TLS handshake failed");

        // And the session continues on the encrypted stream: same startup,
        // same token, same answer.
        tls.write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        assert_eq!(expect(&mut tls).await.0, Tag::AUTHENTICATION);
        assert_eq!(expect(&mut tls).await.0, Tag::AUTHENTICATION);
        for _ in 0..3 {
            expect(&mut tls).await;
        }

        let mut query = Vec::new();
        pgprox_proto::encode_frontend::query(&mut query, "SELECT 1");
        tls.write_all(&query).await.unwrap();
        assert_eq!(expect(&mut tls).await.0, Tag::COMMAND_COMPLETE);
    }

    #[tokio::test]
    async fn a_client_that_skips_tls_is_refused_when_it_is_required() {
        // The posture that matters for a deployment carrying JWTs: a token
        // must not be readable on the wire because a client chose not to ask.
        let addr = fake_postgres().await;
        let (server_tls, _) = certificate();
        let mut context = context_for(addr);
        context.tls = Some(tokio_rustls::TlsAcceptor::from(server_tls));
        context.handshake.tls = TlsPosture::Required;
        let context = Arc::new(context);

        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();
        let (ours, mut client) = tokio::io::duplex(8192);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();

        let (tag, body) = expect(&mut client).await;
        assert_eq!(tag, Tag::ERROR_RESPONSE);
        assert!(
            String::from_utf8_lossy(&body).contains("28000"),
            "{}",
            String::from_utf8_lossy(&body)
        );
        assert!(served.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn a_static_user_reaches_the_show_surface_over_scram() {
        // ADR 0002 chose SCRAM for the clients that have no token, M6.4 built
        // the exchange, and the listener refused every one of them until now.
        let addr = fake_postgres().await;
        let mut context = context_for(addr);
        let admin = Arc::new(
            crate::admin::StaticAdmin::new("pgprox_admin", "hunter2", b"salted".to_vec()).unwrap(),
        );
        context.handshake.static_users = vec!["pgprox_admin".to_owned()];
        context.statics = Some(Arc::clone(&admin));
        let context = Arc::new(context);

        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();
        let (ours, mut client) = tokio::io::duplex(8192);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        let mut packet = Vec::new();
        pgprox_proto::encode_frontend::startup_message(
            &mut packet,
            pgprox_proto::encode::PROTOCOL_3_0,
            &[("user", "pgprox_admin"), ("database", "pgprox")],
        );
        client.write_all(&packet).await.unwrap();

        // AuthenticationSASL, then the exchange, driven with this project's
        // own client-side SCRAM so the two halves are held together.
        assert_eq!(expect(&mut client).await.0, Tag::AUTHENTICATION);

        let nonce = pgprox_auth::scram::generate_nonce();
        let first = pgprox_auth::scram::client_first("", &nonce);
        let mut out = Vec::new();
        pgprox_proto::encode_frontend::sasl_initial_response(&mut out, "SCRAM-SHA-256", &first);
        client.write_all(&out).await.unwrap();

        let (tag, body) = expect(&mut client).await;
        assert_eq!(tag, Tag::AUTHENTICATION);
        // The payload follows the four-byte authentication type.
        let server_first = String::from_utf8_lossy(&body[4..]).into_owned();
        let parsed = pgprox_auth::scram::parse_server_first(&server_first, &nonce).unwrap();
        let keys =
            pgprox_auth::scram::ScramKeys::derive(b"hunter2", &parsed.salt, parsed.iterations)
                .unwrap();
        let without_proof = pgprox_auth::scram::client_final_without_proof(&parsed.nonce);
        let auth_message = pgprox_auth::scram::auth_message(
            &pgprox_auth::scram::client_first_bare("", &nonce),
            &server_first,
            &without_proof,
        );
        let proof = pgprox_auth::scram::client_proof(&keys, &auth_message);

        let final_message = format!(
            "{without_proof},p={}",
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, proof)
        );
        let mut out = Vec::new();
        pgprox_proto::encode_frontend::sasl_response(&mut out, &final_message);
        client.write_all(&out).await.unwrap();

        // SASLFinal, then AuthenticationOk, the parameters this proxy reports
        // about itself, the key, and the first ReadyForQuery.
        assert_eq!(expect(&mut client).await.0, Tag::AUTHENTICATION);
        assert_eq!(expect(&mut client).await.0, Tag::AUTHENTICATION);
        let mut seen = Vec::new();
        loop {
            let (tag, _) = expect(&mut client).await;
            seen.push(tag);
            if tag == Tag::READY_FOR_QUERY {
                break;
            }
        }
        assert!(
            seen.contains(&Tag::PARAMETER_STATUS),
            "an authenticated admin was told nothing about the server it reached: {seen:?}"
        );

        // And the surface answers.
        let mut query = Vec::new();
        pgprox_proto::encode_frontend::query(&mut query, "SHOW STATS");
        client.write_all(&query).await.unwrap();
        assert_eq!(expect(&mut client).await.0, Tag::ROW_DESCRIPTION);

        drop(client);
        let _ = served.await;
    }

    #[tokio::test]
    async fn a_static_user_is_refused_when_the_node_has_none_configured() {
        // The same message a bad token gets: telling a caller that static
        // users exist here but they are not one is an oracle.
        let addr = fake_postgres().await;
        let mut context = context_for(addr);
        context.handshake.static_users = vec!["pgprox_admin".to_owned()];
        let context = Arc::new(context);
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(8192);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        let mut packet = Vec::new();
        pgprox_proto::encode_frontend::startup_message(
            &mut packet,
            pgprox_proto::encode::PROTOCOL_3_0,
            &[("user", "pgprox_admin"), ("database", "pgprox")],
        );
        client.write_all(&packet).await.unwrap();

        assert_eq!(expect(&mut client).await.0, Tag::AUTHENTICATION);
        assert_eq!(expect(&mut client).await.0, Tag::ERROR_RESPONSE);
        assert!(served.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn a_cancel_for_a_query_this_node_is_not_running_is_dropped_silently() {
        // A CancelRequest gets no answer by design, so a client cannot use one
        // to learn whether a key is real.
        let addr = fake_postgres().await;
        let context = Arc::new(context_for(addr));
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(4096);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        let mut packet = Vec::new();
        let (process_id, secret) =
            pgprox_proto::backend::key_from_conn_id(ConnId::new(NodeId::new(1), 7));
        pgprox_proto::encode_frontend::cancel_request(&mut packet, process_id, secret);
        client.write_all(&packet).await.unwrap();

        served.await.unwrap().unwrap();
        let mut anything = [0_u8; 1];
        assert_eq!(
            client.read(&mut anything).await.unwrap(),
            0,
            "a cancel request was answered, which tells a prober the key was real"
        );
    }

    #[tokio::test]
    async fn a_cancel_for_a_held_query_reaches_the_server() {
        // The whole point: the key the proxy issued resolves to the key the
        // server issued, on a fresh connection carrying nothing else.
        let addr = fake_postgres().await;
        let context = Arc::new(context_for(addr));
        let backend = grant_for(addr).primary;
        context.connector.learn(&backend);

        let conn = context.cancels.issue().unwrap();
        context.cancels.hold(
            conn,
            pgprox_session::cancel::Cancellation {
                server: backend.server.clone(),
                key: backend.pool_key(),
                backend_key: (4242, 99),
            },
        );

        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();
        let (ours, mut client) = tokio::io::duplex(4096);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        let mut packet = Vec::new();
        let (process_id, secret) = pgprox_proto::backend::key_from_conn_id(conn);
        pgprox_proto::encode_frontend::cancel_request(&mut packet, process_id, secret);
        client.write_all(&packet).await.unwrap();

        served.await.unwrap().unwrap();
    }

    /// A server that captures the one `CancelRequest` sent to it.
    ///
    /// Not the fake Postgres above: a cancel arrives on its own connection
    /// carrying no startup packet, so what proves it arrived is the bytes
    /// themselves rather than a session that behaved.
    async fn cancel_catcher() -> (SocketAddr, tokio::sync::oneshot::Receiver<(i32, i32)>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (caught, catch) = tokio::sync::oneshot::channel();

        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut packet = [0_u8; 16];
            if socket.read_exact(&mut packet).await.is_ok() {
                let key = (
                    i32::from_be_bytes(packet[8..12].try_into().unwrap_or([0; 4])),
                    i32::from_be_bytes(packet[12..16].try_into().unwrap_or([0; 4])),
                );
                let _ = caught.send(key);
            }
        });

        (addr, catch)
    }

    #[tokio::test]
    async fn a_cancel_for_another_node_s_connection_is_forwarded_to_it() {
        // A client's cancel arrives on whichever pod its second connection
        // reached, which with three pods is usually the wrong one. Until this
        // it was dropped, so cancelling a query worked one time in three.
        let (upstream, caught) = cancel_catcher().await;
        let owner = Arc::new(context_for(upstream));
        let backend = grant_for(upstream).primary;
        owner.connector.learn(&backend);

        let conn = owner.cancels.issue().unwrap();
        owner.cancels.hold(
            conn,
            pgprox_session::cancel::Cancellation {
                server: backend.server.clone(),
                key: backend.pool_key(),
                backend_key: (4242, 99),
            },
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let gossip_at = listener.local_addr().unwrap();
        let cluster = pgprox_cluster::service::GossipCoordinator::new(
            NodeId::new(1),
            pgprox_cluster::coordinator::CoordinatorConfig::default(),
            Arc::new(FakeClock::new()),
        );
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let serving = tokio::spawn(crate::gossip::serve(
            listener,
            cluster,
            Arc::clone(&owner) as Arc<dyn crate::gossip::CancelSink>,
            async {
                let _ = stopped.await;
            },
        ));

        // The node the cancel lands on owns no such connection and knows only
        // where node 1 is.
        let mut elsewhere = context_for(upstream);
        elsewhere.node = NodeId::new(2);
        elsewhere.cancels = Arc::new(Registry::new(NodeId::new(2), Box::new(Fixed)));
        elsewhere.peers =
            std::collections::BTreeMap::from([(NodeId::new(1), gossip_at.to_string())]);
        elsewhere.deliver(conn).await;

        let key = tokio::time::timeout(Duration::from_secs(5), caught)
            .await
            .expect("the cancel never reached the node that owned the connection")
            .unwrap();
        assert_eq!(
            key,
            (4242, 99),
            "the forwarded cancel carried the wrong server key"
        );

        stop.send(()).unwrap();
        let _ = serving.await;
    }

    #[tokio::test]
    async fn a_forwarded_cancel_is_not_forwarded_again() {
        // Two nodes with stale peer tables would otherwise bounce one between
        // them for as long as both were up.
        let (upstream, _caught) = cancel_catcher().await;
        let mut context = context_for(upstream);
        context.node = NodeId::new(2);
        context.cancels = Arc::new(Registry::new(NodeId::new(2), Box::new(Fixed)));
        // A peer address that would answer, so the assertion is about the rule
        // rather than about the address being missing.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        context.peers = std::collections::BTreeMap::from([(
            NodeId::new(1),
            listener.local_addr().unwrap().to_string(),
        )]);

        crate::gossip::CancelSink::cancel(&context, ConnId::new(NodeId::new(1), 7)).await;

        assert!(
            tokio::time::timeout(Duration::from_millis(200), listener.accept())
                .await
                .is_err(),
            "a forwarded cancel was forwarded on"
        );
    }

    #[tokio::test]
    async fn a_full_node_refuses_the_next_client_and_keeps_serving_the_others() {
        // Refusal must not be contagious: the clients already connected are
        // mid-transaction and have done nothing wrong.
        let addr = fake_postgres().await;
        let context = Arc::new(context_for(addr));
        let gate = Arc::new(Gate::new(1));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = listener.local_addr().unwrap();

        tokio::spawn(accept_loop(
            listener,
            Arc::clone(&context),
            Arc::clone(&gate),
            1,
        ));

        let mut first = tokio::net::TcpStream::connect(proxy).await.unwrap();
        first
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut first).await;
        }

        let mut second = tokio::net::TcpStream::connect(proxy).await.unwrap();
        second
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        let (tag, body) = expect(&mut second).await;

        assert_eq!(tag, Tag::ERROR_RESPONSE);
        assert!(String::from_utf8_lossy(&body).contains("53300"));

        // The first client is still being served.
        let mut query = Vec::new();
        pgprox_proto::encode_frontend::query(&mut query, "SELECT 1");
        first.write_all(&query).await.unwrap();
        assert_eq!(expect(&mut first).await.0, Tag::COMMAND_COMPLETE);
    }
}
