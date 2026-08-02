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
use pgprox_session::sequence::{Facts, Held};
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
    /// The ceiling, which a configuration reload can move.
    ///
    /// Atomic rather than fixed, because an operator raising it in the
    /// `ConfigMap` is usually doing so while the node is refusing
    /// connections, and a value read once at startup would need the restart
    /// they are trying to avoid.
    ceiling: AtomicU32,
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
            ceiling: AtomicU32::new(ceiling),
        }
    }

    /// The ceiling in force.
    #[must_use]
    pub fn ceiling(&self) -> u32 {
        self.ceiling.load(Ordering::SeqCst)
    }

    /// Moves the ceiling.
    ///
    /// Lowering it refuses the next client rather than closing an established
    /// one: a limit is about what a node takes on, and taking connections away
    /// from clients that already have them is a drain, which is a different
    /// thing with its own sequence.
    pub fn set_ceiling(&self, ceiling: u32) {
        self.ceiling.store(ceiling, Ordering::SeqCst);
    }

    /// How many clients are being served.
    #[must_use]
    pub fn live(&self) -> u32 {
        self.live.load(Ordering::SeqCst)
    }

    /// Admits a client, or refuses because the node is full.
    pub fn admit(self: &Arc<Self>) -> Option<Admitted> {
        let ceiling = self.ceiling();
        self.live
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |live| {
                (live < ceiling).then_some(live + 1)
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
    /// Where statements went, for the metric that answers what share of them
    /// a replica served.
    pub routes: Arc<crate::routes::RouteCounts>,
    /// Where every connection's read and write buffers come from.
    ///
    /// Shared with the connector, so client and upstream connections draw on
    /// one bound. A connection borrows when its socket has something to say
    /// and gives back when it is quiet, which is what makes an idle connection
    /// cost a socket rather than 32 KiB.
    pub slab: Arc<pgprox_core::buf::BufferSlab>,
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
    /// How long a client has to finish authenticating.
    ///
    /// From the moment its socket is accepted to the moment it is told it is
    /// in. A connection that has said nothing costs a slot under the ceiling,
    /// so without this a node is taken out of service by opening sockets and
    /// sending nothing: no credentials, no traffic, no way to tell it from a
    /// slow network.
    pub login_timeout: Duration,
    /// Where to find out where the other nodes are, for a cancel this node
    /// does not own.
    ///
    /// The source rather than a table taken at startup. A cancel arrives on
    /// whichever pod the client's second connection reached, so a node that
    /// held a stale table would drop cancels for every node that joined after
    /// it did, and cancelling a query would work one time in N. `M19.3`.
    pub peers: Arc<dyn pgprox_core::cluster::PeerSource>,
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
    /// The query cache.
    ///
    /// A running node always has one and it serves nobody until a document
    /// names a tenant, which is what ADR 0021 makes the default: the cache
    /// promises bounded staleness and a tenant has to ask for it. Being off is
    /// a property of the store rather than of this field, so an operator can
    /// turn it on without a restart.
    ///
    /// Still an `Option`, because a test that is not about the cache should
    /// not have to build one, and because the type here is the trait rather
    /// than the store: nothing on this path knows which implementation it has.
    pub cache: Option<Arc<dyn pgprox_core::cache::QueryCache>>,
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
    // One deadline for the whole handshake rather than one per read: a client
    // sending a byte a second would pass every per-read timeout and still hold
    // its slot for as long as it liked.
    //
    // It covers the steps the client owes this proxy, and stops at the last of
    // them. What the proxy then does on the client's behalf, which is resolve
    // a grant, fetch server parameters and take a connection from a pool, is
    // this node's own latency: a client made to wait by us is refused with a
    // message it can read, or served late, and never dropped in silence.
    let deadline = tokio::time::Instant::now() + context.login_timeout;
    let mut wire = Wire::new(stream, Arc::clone(&context.slab));
    let mut handshake = Handshake::new(context.handshake.clone());

    match until(deadline, Box::pin(negotiate(&mut wire, &mut handshake))).await? {
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

            // Boxed: rustls' handshake state is over a kilobyte and it is
            // finished with by the time the first query arrives.
            let upgraded = until(
                deadline,
                Box::pin(async {
                    acceptor
                        .accept(io)
                        .await
                        .map_err(|_| ShellError::Disconnected)
                }),
            )
            .await?;

            // What this client actually negotiated. The server is the only
            // side that knows it for every client: some drivers will tell you
            // their cipher and some will not, and "which suite did that driver
            // use" is a question a FIPS deployment has to be able to answer
            // about all of them. Debug rather than info because it is one line
            // per connection, and a node holding a hundred thousand of them
            // must not be logging a hundred thousand lines to say so.
            //
            // Both fields are unwrapped rather than logged as options: a
            // handshake that got this far has agreed on both, and
            // `cipher="Some(TLS13_AES_256_GCM_SHA384)"` is a value every
            // reader of this line then has to strip.
            {
                let (_, session) = upgraded.get_ref();
                if let (Some(protocol), Some(suite)) = (
                    session.protocol_version(),
                    session.negotiated_cipher_suite(),
                ) {
                    tracing::debug!(
                        protocol = ?protocol,
                        cipher = ?suite.suite(),
                        "tls handshake"
                    );
                }
            }

            let mut wire = Wire::new(upgraded, Arc::clone(&context.slab));

            // The same handshake, which is what makes "TLS was accepted"
            // survive the change of stream type.
            match until(deadline, Box::pin(negotiate(&mut wire, &mut handshake))).await? {
                Handoff::Cancel(conn) => cancel(conn, context).await,
                Handoff::Upgrade => Err(wire
                    .refuse(ClientError::ProtocolViolation("TLS was already negotiated"))
                    .await),
                Handoff::Ask(credential) => {
                    serve_client(
                        &mut wire,
                        &mut handshake,
                        credential,
                        context,
                        admitted,
                        deadline,
                    )
                    .await
                }
            }
        }
        Handoff::Cancel(conn) => cancel(conn, context).await,
        Handoff::Ask(credential) => {
            serve_client(
                &mut wire,
                &mut handshake,
                credential,
                context,
                admitted,
                deadline,
            )
            .await
        }
    }
}

/// Runs a step of the handshake, or gives up on the client.
///
/// Nothing is sent back on a timeout. A client that has not finished
/// authenticating has not been told what this server is, and an error frame
/// would be read by whatever is on the other end as an answer to a question it
/// did not ask.
async fn until<F, T>(deadline: tokio::time::Instant, step: F) -> Result<T, ShellError>
where
    F: std::future::Future<Output = Result<T, ShellError>>,
{
    tokio::time::timeout_at(deadline, step)
        .await
        .unwrap_or(Err(ShellError::Disconnected))
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

/// What authentication produced, and all a serving session needs from it.
///
/// The point of this type is what it does *not* carry: the state machine, the
/// SCRAM exchange, the sidecar call and the parameter fetch that got here are
/// all dropped by the time one of these exists. A future is the union of
/// everything alive across its awaits, so a startup that returns rather than
/// falling through into the serving loop is a connection that stops paying for
/// its own beginning.
///
/// The grant is boxed because it is 224 bytes and only one arm uses it.
#[derive(Debug)]
enum Ready {
    /// A tenant's session, which will reach a database.
    Tenant {
        /// Where that database is and what it allows.
        grant: Box<Grant>,
        /// The cancel key issued for it.
        conn: ConnId,
        /// The runtime settings the client packed into its `options` startup
        /// parameter, in the order it sent them.
        ///
        /// Carried this far because they belong to the session rather than to
        /// the handshake, and the session is built after it. `M20.2`.
        ///
        /// A boxed slice rather than a `Vec`, and moved into the relay rather
        /// than borrowed by it, for the reason `grant` beside it is boxed: this
        /// value is read once, before the first frame, and every byte it
        /// occupies is occupied for the life of the connection.
        /// `one_session_costs_less_than_the_slab_buffer_it_no_longer_holds` had
        /// 72 bytes of headroom when this was written.
        settings: Box<[(String, String)]>,
    },
    /// A static user on the `SHOW` surface, which reaches no database at all.
    Admin,
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
    deadline: tokio::time::Instant,
) -> Result<(), ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    // Boxed, and this is the point of the split. Starting up costs a couple of
    // kilobytes of state, and without the box that state sits in this future
    // for as long as the connection lives: at a hundred thousand connections,
    // hundreds of megabytes of work that finished in the first milliseconds.
    let ready = Box::pin(authenticate(wire, handshake, credential, context, deadline)).await?;

    match ready {
        Ready::Admin => {
            drop(admitted);
            crate::admin::serve(wire, &context.observatory).await
        }
        Ready::Tenant {
            grant,
            conn,
            settings,
        } => {
            // The signal a shed decision fires. Registered with the session
            // rather than held by it, because the decision is the node's and
            // the session is a task on a socket.
            let shed = crate::run::Shutdown::new();
            let _registered = context.sessions.register(
                conn,
                grant.tenant.clone(),
                context.node,
                context.clock.now(),
                // The tenant's own allowance where the grant states one. A
                // grant that does not is a tenant with no per-tenant cap, and
                // the shed decision reads a zero budget as "no headroom
                // anywhere", which refuses rather than moves. Refusing is the
                // direction that costs nobody a reconnect.
                grant.pool.max_upstream.unwrap_or(0),
                shed.clone(),
            );
            let outcome = relay(wire, context, &grant, conn, &shed, settings).await;
            drop(admitted);
            outcome
        }
    }
}

/// Authenticates the client and tells it it is in.
///
/// Returns [`Ready`] rather than carrying on into the serving loop, so that
/// everything this needed is dropped before the connection settles.
async fn authenticate<S>(
    wire: &mut Wire<S>,
    handshake: &mut Handshake,
    credential: Credential,
    context: &Context,
    deadline: tokio::time::Instant,
) -> Result<Ready, ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    // The admin arm returns rather than yielding: a static user reaches no
    // database, so it has no connection to set anything on and no settings to
    // carry.
    let (grant, settings) = match credential {
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
            until(deadline, Box::pin(authenticate_scram(wire, &mut scram))).await?;

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
            return Ok(Ready::Admin);
        }
        Credential::Jwt => {
            let startup = handshake.startup().ok_or(ShellError::Disconnected)?.clone();
            // Kept before the token exchange consumes it. These are the
            // client's own runtime settings, and until `M20.2` this was the
            // last place they existed.
            // Already in the order they apply: `StartupInfo::settings` puts the
            // plain parameters first and the ones from `options` after them, so
            // replaying in order is what makes `options` win. `M20.7`.
            let settings: Box<[(String, String)]> = startup.settings.clone().into();
            let mut auth = TokenAuth::new(&startup, std::net::IpAddr::from([0, 0, 0, 0]));
            let grant = until(
                deadline,
                Box::pin(authenticate_token(
                    wire,
                    &mut auth,
                    context.resolver.as_ref(),
                    std::time::SystemTime::now(),
                )),
            )
            .await?;
            (grant, settings)
        }
    };

    // The grant is what says where this tenant's database is, so the connector
    // learns it here: it is the only moment the credentials exist.
    context.connector.learn(&grant.primary);
    for replica in &grant.replicas {
        context.connector.learn(replica);
    }

    // Told to the client, not merely returned. `ShellError::Refused` says of
    // itself that the client was told why before the socket closed, and three
    // places built one without writing anything: a client whose grant resolved
    // and whose backend then could not be reached had its socket end in
    // silence, which every driver reports as a network fault rather than as
    // the upstream problem it is.
    let parameters = match Box::pin(
        context
            .parameters
            .ensure(context.connector.as_ref(), &grant.primary),
    )
    .await
    {
        Ok(parameters) => parameters,
        Err(err) => return Err(wire.refuse(err.into()).await),
    };

    // Refused rather than issued from a fallback: a cancel key is a bearer
    // token, and one drawn from anything predictable lets a tenant cancel its
    // neighbour's queries.
    let Some(conn) = context.cancels.issue() else {
        return Err(wire
            .refuse(ClientError::Internal("no entropy for a cancel key"))
            .await);
    };
    // Deliberately not under the login deadline. That deadline is for what the
    // client owes this proxy: a connection that has said nothing must not hold
    // a slot forever. By this point the client has authenticated and it is the
    // proxy that is keeping it waiting, for a grant, for server parameters, or
    // behind a pool. Dropping it here closed the socket of a client that did
    // everything right, with no message, which every driver reports as a
    // network fault; at a thousand connections it happened to eight of them.
    accept(wire, conn, &parameters).await?;

    Ok(Ready::Tenant {
        grant: Box::new(grant),
        conn,
        settings,
    })
}

/// Puts the client's connection-string settings on the session, before it runs.
///
/// Before the first frame, because these are what the session already is rather
/// than something it does: a `search_path` from the connection string has to be
/// on the first connection this session borrows, not the second.
///
/// Borrows the settings; the caller drops them, so nothing about them is alive
/// across the relay loop's awaits. See the field's own note.
fn apply_startup_settings(
    live: &mut Live,
    context: &Context,
    conn: ConnId,
    settings: &[(String, String)],
) {
    if let Some(reason) = live.relay.on_startup_settings(
        settings.iter().map(|(n, v)| (n.as_str(), v.as_str())),
        &mut live.session.params,
    ) {
        context.sessions.set_pinned(conn, reason.as_str());
    }
}

/// Moves frames between a client and the upstream connections it borrows.
async fn relay<S>(
    wire: &mut Wire<S>,
    context: &Context,
    grant: &Grant,
    conn: ConnId,
    shed: &crate::run::Shutdown,
    settings: Box<[(String, String)]>,
) -> Result<(), ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut body = Vec::new();
    let mut live = Live::new(context, grant);

    apply_startup_settings(&mut live, context, conn, &settings);
    // Explicitly, and this is the point rather than tidiness: a future is the
    // union of everything alive across its awaits, and the loop below is
    // nothing but awaits. Dropping here is what keeps these bytes out of every
    // one of a hundred thousand connections.
    drop(settings);

    loop {
        // A session between transactions leaves as soon as the node says it is
        // draining. One holding a connection stays until it gives it back, or
        // until the grace timer says otherwise: finishing in-flight work is
        // what a drain is for.
        let idle = live.upstream.is_none();
        let header = tokio::select! {
            result = wire.read_header(pgprox_proto::frame::DEFAULT_MAX_FRAME) => result?,
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
        let tag = header.tag;
        let tail = read_client_body(wire, header, &mut body, context.cache.is_some()).await?;

        let now = context.clock.now();
        let (message, outgoing) = match decoded(tag, &body, &mut live.session) {
            Ok(pair) => pair,
            Err(err) => return Err(wire.refuse(err).await),
        };

        // The cache, both protocols, and everything it may have to do first:
        // invalidate on a write, answer a simple query, hold an extended
        // sequence back or give one up. Not boxed, and measured rather than
        // assumed: the compiler lays this branch over the one below it, so the
        // session future is the same size either way and a box here would be an
        // allocation per frame for nothing.
        let incoming = Incoming {
            message: &message,
            tag,
            body: &body,
        };
        if cached(wire, context, grant, conn, &mut live, incoming, now).await? {
            continue;
        }

        let outcome = decide(
            &mut live.relay,
            live.watch.as_ref(),
            &live.none,
            &message,
            now,
        );
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
            ClientAction::Send { acquire, target } => {
                // Every statement is counted, not every acquire. A transaction
                // that holds its connection sends several statements to the
                // target chosen once at its first, and counting acquires would
                // call that one statement: the first version of this counter
                // did, and reported a share of acquisitions under a name that
                // said statements.
                if acquire {
                    live.serving = target;
                }
                record_statement(context, &message, live.serving);
                if acquire {
                    let taken =
                        take_connection(wire, context, grant, live.serving, conn, &live.session);
                    live.upstream = Some(taken.await?);
                    live.relay.acquired();
                }
            }
        }

        let Some((_guard, upstream)) = live.upstream.as_mut() else {
            return Err(wire.refuse(NO_CONNECTION).await);
        };

        let onward = Frame::new(tag, &outgoing);
        let sent = send_upstream(
            wire,
            upstream,
            &mut live.pumping,
            &message,
            &mut live.session,
            onward,
            tail,
        );
        if !sent.await? {
            continue;
        }

        // Nothing to read back yet: the client is mid-sequence and will say
        // when it wants an answer. See `awaits_more`.
        if awaits_more(&message) {
            continue;
        }

        let answered = read_the_answer(
            wire,
            upstream,
            &mut live.relay,
            context,
            conn,
            &mut live.pumping,
            &message,
        );
        if Box::pin(answered).await? && grant.pool.mode != pgprox_core::auth::PoolMode::Session {
            Box::pin(release(&mut live.upstream, &mut live.relay, context, conn)).await?;
        }
    }
}

/// Takes a connection for this statement and brings it up to date.
///
/// Split out of the relay loop, which clippy holds to a hundred lines, and
/// they are two steps rather than one: the pool decides which connection, and
/// the session's own parameters decide what has to be replayed onto it.
async fn take_connection<S>(
    wire: &mut Wire<S>,
    context: &Context,
    grant: &Grant,
    target: pgprox_core::route::RouteTarget,
    conn: ConnId,
    session_state: &pgprox_session::resume::SessionMemory,
) -> Result<(UpstreamGuard, Upstreamed<crate::dial::Stream>), ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut borrowed = told(wire, borrow(context, grant, target, conn).await).await?;
    // Before the client's own frame reaches the server, and only where the
    // connection does not already match: a warm pool serving one tenant
    // replays nothing.
    told(
        wire,
        Box::pin(resume(&mut borrowed.1, session_state, grant)).await,
    )
    .await?;
    Ok(borrowed)
}

/// Runs the relay's decision against whatever replica state this session has.
///
/// A grant with no replicas has no watch, and every decision for it lands on
/// the primary by the same rule that sends a read there when no replica is
/// eligible.
fn decide(
    relay: &mut Relay,
    watch: Option<&Arc<pgprox_route::poller::ReplicaWatch>>,
    none: &Replicas,
    message: &pgprox_proto::frontend::FrontendMessage<'_>,
    now: std::time::Instant,
) -> pgprox_session::relay::ClientOutcome {
    match watch {
        Some(watch) => watch.with_replicas(|replicas| relay.on_client(message, replicas, now)),
        None => relay.on_client(message, none, now),
    }
}

/// Reads the server's answer back to the client and stores it if it may be.
///
/// Returns whether the upstream connection is free, which the caller still
/// overrules for a tenant in session pooling.
///
/// The store is here rather than beside the pump's own return because it has
/// to happen whether or not the connection is released, and because the
/// recording has to be taken out of `pumping` either way: a statement that
/// must not be stored cannot leave bytes behind for the next one to pick up.
#[allow(clippy::too_many_arguments)]
async fn read_the_answer<S>(
    wire: &mut Wire<S>,
    upstream: &mut Upstreamed<crate::dial::Stream>,
    relay: &mut Relay,
    context: &Context,
    conn: ConnId,
    pumping: &mut Pumping,
    asked: &pgprox_proto::frontend::FrontendMessage<'_>,
) -> Result<bool, ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let releasable = pump(wire, upstream, relay, context, conn, pumping, asked).await?;

    // Only when there is something to store, so a node that caches nothing
    // pays one comparison here.
    if pumping.recording.is_some() {
        store_answer(context, pumping).await;
    }

    Ok(releasable)
}

/// Invalidates on a write, then answers from the cache if it can.
///
/// Returns whether the client has been answered, in which case the statement
/// never reaches `decide` and nothing is taken from the pool. That is the
/// point of the whole milestone: `M7.56` found 45% of this proxy's CPU in the
/// pool's lock, with the cost landing per connection because contention tracks
/// how many are queued, and a statement answered here never queues.
///
/// One function for both halves because they share a guard and the relay loop
/// is held to a hundred lines. They are still two rules: the invalidation must
/// happen whether or not anything is served, and it must happen before the
/// statement is sent, or a reader arriving in between would be served an entry
/// the write was about to make wrong.
async fn cache_before_sending<S>(
    wire: &mut Wire<S>,
    context: &Context,
    grant: &Grant,
    message: &pgprox_proto::frontend::FrontendMessage<'_>,
    pumping: &mut Pumping,
) -> Result<bool, ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    // The guard is here rather than at the call site, so a tenant nobody has
    // opted in for pays a read lock and a map lookup and returns. Boxing this
    // to keep it out of the session's frame was tried and cost more than it
    // saved: the borrow it needs cannot be taken lazily, so the future had to
    // be built either way.
    //
    // `serves` rather than `is_none`, because the store is now built on every
    // node and turned on by a document. A node that caches nothing has one
    // that serves nobody rather than no store at all.
    if !context
        .cache
        .as_ref()
        .is_some_and(|cache| cache.serves(&grant.tenant))
    {
        return Ok(false);
    }

    invalidate_on_write(context, message, &grant.tenant).await;

    let (Some(cache), Some(recording)) = (context.cache.as_ref(), pumping.recording.as_ref())
    else {
        return Ok(false);
    };

    // Queued and then dropped, before the flush. Held across that await the
    // result is another thirty-two bytes in every session's frame, and the
    // ceiling has none to spare.
    let served = match cache.get(&recording.key).await {
        Some(hit) => {
            // Assembled rather than replayed, and refused rather than trimmed:
            // a simple query is owed a description for the rows it is about to
            // be sent, and an entry stored by a sequence that asked for none
            // cannot give it one. The terminator is generated here because no
            // payload holds one. See `M9.27`.
            let mut assembled = Ok(());
            wire.queue(|out| {
                assembled = pgprox_session::sequence::assemble_simple(&hit.frames, out);
            });
            assembled.is_ok()
        }
        None => false,
    };

    if served {
        wire.flush().await?;
        // Or the next statement inherits this one's key.
        pumping.recording = None;
    }
    Ok(served)
}

/// The cache key for a simple query, when this statement may be cached.
///
/// `None` covers four different things and does not distinguish them: no cache
/// wired in at all, a tenant that has not opted in, a message that is not a
/// simple query, or a statement the cacheability rule refused. `M9.9` is what
/// will count them apart.
///
/// # Only the simple protocol, for now
///
/// The extended protocol's parameter values live in a `Bind`, and
/// `pgprox-proto` exposes that message's portal and statement names and not
/// its parameters: the codec has never had a reason to read them, and reading
/// them is a piece of work with its own risk.
///
/// Until it does, a bound statement is a miss rather than a wrong key.
/// `CacheKey::params` would be empty for two calls differing only in what was
/// bound, so `SELECT $1` with 1 and with 2 would share an entry. That is the
/// difference between a smaller cache and a broken one. See `M9.12`.
fn cache_key(
    context: &Context,
    grant: &Grant,
    message: &pgprox_proto::frontend::FrontendMessage<'_>,
    session: &pgprox_session::resume::SessionMemory,
    relay: &Relay,
) -> Option<Box<Recording>> {
    use pgprox_proto::frontend::FrontendMessage;

    // Before anything that allocates. `normalize` builds a string per
    // statement, and on a node where no tenant has opted in, which is the
    // default and so most nodes, that would be a string built to look up
    // nothing. `serves` is a read lock and a map lookup.
    let cache = context.cache.as_ref()?;
    if !cache.serves(&grant.tenant) {
        return None;
    }

    let FrontendMessage::Query { sql } = message else {
        return None;
    };

    // The transaction status the server last sent, not the SQL. A `COMMIT`
    // inside a failed transaction does not commit, which is the same reason the
    // release rule reads the status byte.
    let in_transaction = !matches!(relay.session().tx_status(), TxStatus::Idle);
    let facts = pgprox_cache::SessionFacts::new(
        relay.wrote(),
        relay.pin_reason().is_some(),
        in_transaction,
    );
    pgprox_cache::cacheable(sql, pgprox_route::classify(sql), facts).ok()?;

    Some(Box::new(Recording {
        // Nothing bound: a simple query has no `Bind` behind it, and an empty
        // run is the same key material as an extended statement with no
        // parameters. That is the same question asked two ways, and `M9.22` is
        // what makes one entry answer both.
        key: key_for(grant, sql, std::sync::Arc::from(&[][..]), session),
        frames: Vec::new(),
        failed: false,
    }))
}

/// One session's own state, all of it.
///
/// Held together rather than as eight locals because the cache path needs most
/// of it at once, and eight parameters that always travel together are one
/// parameter wearing a disguise. Nothing else owns one: it lives for exactly as
/// long as [`relay`] does, and its size is a per-connection cost.
struct Live {
    relay: Relay,
    /// The connection this session holds, if it holds one.
    upstream: Option<(UpstreamGuard, Upstreamed<crate::dial::Stream>)>,
    /// Where that connection points, so a statement sent on it is counted
    /// against the right target.
    serving: pgprox_core::route::RouteTarget,
    pumping: Pumping,
    /// The extended-query sequence being held back, if any.
    ///
    /// A pointer here and everything else behind it, because a session future is
    /// the union of everything alive across its awaits and the ceiling has bytes
    /// rather than kilobytes to spare. Allocated at the first sequence a tenant
    /// opts into and reused after that, so a node that caches nothing never
    /// allocates one at all.
    sequence: Option<Box<pgprox_session::sequence::HeldSequence>>,
    /// The replica state this grant is polled into.
    watch: Option<Arc<pgprox_route::poller::ReplicaWatch>>,
    /// What a grant with no replicas routes against.
    none: Replicas,
    /// What this session expects any connection it borrows to look like.
    ///
    /// A transaction-pooling proxy hands a session a different connection per
    /// transaction, so without this a `SET` is silently lost at the next
    /// boundary.
    session: pgprox_session::resume::SessionMemory,
}

impl Live {
    /// A session that has authenticated, holds nothing and has run nothing.
    fn new(context: &Context, grant: &Grant) -> Self {
        Self {
            relay: Relay::new(),
            upstream: None,
            serving: pgprox_core::route::RouteTarget::Primary,
            pumping: Pumping::default(),
            sequence: None,
            // Shared with every other session on the same primary. A grant with
            // no replicas gets none, and every route decision for it lands on
            // the primary by the same rule that sends a read there when no
            // replica is eligible.
            watch: context.replicas.watch_for(grant),
            none: Replicas::new(0, pgprox_route::replica::ReplicaConfig::default()),
            session: pgprox_session::resume::SessionMemory::default(),
        }
    }
}

/// One client frame, decoded, as the client sent it.
///
/// `body` is the client's own bytes rather than the rewritten ones a `Parse` or
/// a `Bind` goes upstream as, because the rewrite cannot be read back: it writes
/// this proxy's own global name, and no session's statement map is keyed by
/// that. See `M9.26`.
#[derive(Clone, Copy)]
struct Incoming<'a> {
    message: &'a pgprox_proto::frontend::FrontendMessage<'a>,
    tag: pgprox_proto::frame::Tag,
    body: &'a [u8],
}

/// The cache path for one frame, both protocols.
///
/// Returns whether the client has already been answered, in which case the
/// relay loop is finished with this frame and nothing was taken from the pool.
/// That is the point of the whole milestone: `M7.56` found 45% of this proxy's
/// CPU in the pool's lock, with the cost landing per connection because
/// contention tracks how many are queued, and a statement answered here never
/// queues.
///
/// One function for the two protocols because a hit is one thing and is counted
/// in one place. They arrive at it differently: a simple query is a statement on
/// its own, and an extended one is a sequence that has to be held back first,
/// because forwarding its `Parse` would acquire a connection before anything was
/// looked up. See ADR 0022.
async fn cached<S>(
    wire: &mut Wire<S>,
    context: &Context,
    grant: &Grant,
    conn: ConnId,
    live: &mut Live,
    incoming: Incoming<'_>,
    now: std::time::Instant,
) -> Result<bool, ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let message = incoming.message;
    live.pumping.recording = cache_key(context, grant, message, &live.session, &live.relay);

    let mut hit = cache_before_sending(wire, context, grant, message, &mut live.pumping).await?;
    if !hit {
        match withhold(context, grant, live, incoming) {
            Held::Withheld => return Ok(true),
            Held::Complete => hit = Box::pin(serve_held(wire, context, grant, live)).await?,
            Held::Send => {}
        }
    }
    if hit {
        // Counted here rather than left to `record_statement`, which the caller
        // skips on a hit. A hit is a statement the client asked for and got an
        // answer to, and leaving it out makes every ratio built on this counter
        // wrong in the direction that flatters the cache: its best cases vanish
        // from the denominator.
        context.routes.record_cache_hit();
        return Ok(true);
    }

    // Whatever is still held goes upstream in the order it arrived, before this
    // frame does. Both the guard and the box are deliberate: nothing is held on
    // any frame of a node that caches nothing, and boxing unconditionally would
    // be an allocation per frame for a path most frames never take.
    if live
        .sequence
        .as_deref()
        .is_some_and(|held| !held.is_empty())
    {
        Box::pin(replay_held(wire, context, grant, conn, live, now)).await?;
    }
    Ok(false)
}

/// Holds one frame back, when this session is one a sequence may be held from.
///
/// The entry condition lives here rather than in the machine, because it is the
/// caller's: the tenant's configuration, the pool, the transaction status and
/// the cacheability rule are all things this file can see and `pgprox-session`
/// cannot. See [`facts_for`].
///
/// Nothing is allocated until the first frame is actually held, so a node with
/// no cache pays one `serves` call per frame and no more.
fn withhold(context: &Context, grant: &Grant, live: &mut Live, incoming: Incoming<'_>) -> Held {
    let facts = facts_for(context, grant, &live.relay, incoming.message, &live.session);
    if live.sequence.is_none() {
        if !facts.may_begin {
            return Held::Send;
        }
        live.sequence = Some(Box::new(pgprox_session::sequence::HeldSequence::new()));
    }
    live.sequence.as_deref_mut().map_or(Held::Send, |held| {
        held.feed(incoming.message, incoming.tag, incoming.body, facts)
    })
}

/// What this file knows about a frame that the sequence machine cannot.
///
/// `may_begin` is the whole of ADR 0022's entry condition. Two of its four parts
/// are about the session rather than the statement:
///
/// - **No connection held.** A session holding one is either mid-sequence, in a
///   transaction, or on session pooling. In the first case part of the sequence
///   is already upstream and answering the rest locally would leave a bound
///   portal nobody executes.
/// - **No transaction open.** A hit generates a `ReadyForQuery('I')`, which is
///   only true for a session with nothing open. `M9.18` is the same rule for
///   the simple protocol.
fn facts_for<'a>(
    context: &Context,
    grant: &Grant,
    relay: &Relay,
    message: &pgprox_proto::frontend::FrontendMessage<'a>,
    session: &'a pgprox_session::resume::SessionMemory,
) -> Facts<'a> {
    use pgprox_proto::frontend::FrontendMessage;

    // Before anything else, because off is the default and therefore every
    // node's hot path. `serves` is a read lock and a map lookup.
    if !context
        .cache
        .as_ref()
        .is_some_and(|cache| cache.serves(&grant.tenant))
    {
        return Facts::default();
    }

    // A `Parse` carries its own SQL. A `Bind` names a statement this session
    // prepared, possibly in a round trip of its own against another connection,
    // which is what every driver with a statement cache does.
    let sql = match message {
        FrontendMessage::Parse { sql, .. } => Some(*sql),
        FrontendMessage::Bind { statement, .. } => session
            .statements
            .get(statement)
            .map(|held| held.sql.as_str()),
        _ => None,
    };
    let Some(sql) = sql else {
        return Facts::default();
    };

    let facts = pgprox_cache::SessionFacts::new(
        relay.wrote(),
        relay.pin_reason().is_some(),
        !matches!(relay.session().tx_status(), TxStatus::Idle),
    );
    Facts {
        sql: Some(sql),
        may_begin: !relay.is_holding()
            && pgprox_cache::cacheable(sql, pgprox_route::classify(sql), facts).is_ok(),
    }
}

/// Answers a complete held sequence from the cache, if the cache can.
///
/// Returns whether the client has been answered. On a miss it arms the recording
/// instead, so the answer the replayed sequence comes back with is stored:
/// `belongs_in_payload` filters the pump's copy down to exactly the payload
/// shape ADR 0022 describes, which is why there is no second recorder here.
async fn serve_held<S>(
    wire: &mut Wire<S>,
    context: &Context,
    grant: &Grant,
    live: &mut Live,
) -> Result<bool, ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (Some(cache), Some(held)) = (context.cache.as_ref(), live.sequence.as_deref()) else {
        return Ok(false);
    };
    let Some(sql) = held.sql() else {
        return Ok(false);
    };
    let key = key_for(grant, sql, held.params().clone(), &live.session);

    let Some(entry) = cache.get(&key).await else {
        // Armed for the answer the replay is about to bring back. A `Sync` is
        // what the pump will be reading when it arrives, and this is the only
        // place that knows the key it belongs under.
        live.pumping.recording = Some(Box::new(Recording {
            key,
            frames: Vec::new(),
            failed: false,
        }));
        return Ok(false);
    };

    // Written straight into the client's buffer, and nothing is written unless
    // the whole thing can be: `assemble` validates the payload before it emits a
    // byte, so a refusal here leaves the buffer as it was and the sequence goes
    // upstream instead.
    let mut assembled = Ok(());
    wire.queue(|out| assembled = held.assemble(&entry.frames, out));
    if assembled.is_err() {
        return Ok(false);
    }

    wire.flush().await?;
    if let Some(held) = live.sequence.as_deref_mut() {
        held.clear();
    }
    Ok(true)
}

/// Sends a held sequence upstream, in the order the client sent it.
///
/// Every frame goes through the relay's own decision and the same statement
/// readiness check a frame arriving now would, because that is where the pin,
/// the route and the connection's record of what it holds are kept up to date.
/// The first frame acquires and the rest travel on what it took.
///
/// A no-op when nothing is held, which is the common case and the reason it is
/// called unconditionally.
async fn replay_held<S>(
    wire: &mut Wire<S>,
    context: &Context,
    grant: &Grant,
    conn: ConnId,
    live: &mut Live,
    now: std::time::Instant,
) -> Result<(), ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let Some(sequence) = live.sequence.as_deref_mut() else {
        return Ok(());
    };

    for (tag, body) in sequence.replay() {
        let Ok(message) = pgprox_proto::frontend::decode(&Frame::new(tag, body)) else {
            // Unreachable: every held frame decoded on its way in, and nothing
            // has touched the bytes since.
            return Err(wire
                .refuse(ClientError::ProtocolViolation(
                    "a held frame stopped decoding",
                ))
                .await);
        };
        // Mapped here rather than stored mapped, because the map is one way: a
        // `Parse` stored under this proxy's own name decodes to a statement no
        // session's map contains, and what pays for that is the connection's
        // record of which statements it holds. `M9.24` found it by running.
        let Some(mapped) = map_statement_name(&message, body, &mut live.session) else {
            return Err(wire
                .refuse(ClientError::ProtocolViolation(
                    "a held frame named a statement this session never parsed",
                ))
                .await);
        };

        let outcome = decide(
            &mut live.relay,
            live.watch.as_ref(),
            &live.none,
            &message,
            now,
        );
        if let Some(reason) = outcome.pinned {
            context.sessions.set_pinned(conn, reason.as_str());
        }
        let ClientAction::Send { acquire, target } = outcome.action else {
            // Unreachable: only a `Parse`, `Bind`, `Describe` or `Execute` is
            // ever held, and the cacheability rule refuses the statements the
            // relay answers itself. Skipped rather than asserted, because a
            // panic here is on a node serving 100k other connections.
            continue;
        };
        if acquire {
            live.serving = target;
        }
        record_statement(context, &message, live.serving);
        if acquire {
            let taken =
                take_connection(wire, context, grant, live.serving, conn, &live.session).await?;
            live.upstream = Some(taken);
            live.relay.acquired();
        }

        let Some((_guard, upstream)) = live.upstream.as_mut() else {
            return Err(wire.refuse(NO_CONNECTION).await);
        };
        send_upstream(
            wire,
            upstream,
            &mut live.pumping,
            &message,
            &mut live.session,
            Frame::new(tag, &mapped),
            // No tail: these frames come from a sequence this proxy held in
            // full, not from a socket, so the body here is the whole body.
            0,
        )
        .await?;
    }

    sequence.clear();
    Ok(())
}

/// The key a statement's answer is stored under.
///
/// One place, so the two protocols cannot come to disagree about what a key is.
/// `search_path` decides what the SQL names, so it is part of it. A session that
/// never set one is on the server's default, which every session on this tenant
/// shares; the empty string stands for it and cannot collide with a real path,
/// since a real one is never empty.
fn key_for(
    grant: &Grant,
    sql: &str,
    params: std::sync::Arc<[u8]>,
    session: &pgprox_session::resume::SessionMemory,
) -> pgprox_core::cache::CacheKey {
    pgprox_core::cache::CacheKey {
        tenant: grant.tenant.clone(),
        normalized_sql: std::sync::Arc::from(pgprox_cache::normalize(sql)),
        params,
        search_path: std::sync::Arc::from(session.params.get("search_path").unwrap_or_default()),
    }
}

/// What the relay asks a cached entry to live for.
///
/// The longest there is, which is how a caller with no opinion says so: the
/// cache takes the smaller of this and what the tenant is configured for, so
/// asking for forever means asking for exactly the configured TTL. It is not a
/// request to keep anything forever, and the store could not honour one.
const NO_STALENESS_BOUND_HERE: Duration = Duration::MAX;

/// Stores what the server just said, when it may be stored.
///
/// No TTL of its own. The relay knows nothing about how stale a tenant's reads
/// may be, so it says so with [`NO_STALENESS_BOUND_HERE`] and the cache applies
/// the configured one. It used to pass the grant's TTL, which is a credential's
/// lifetime standing in for a staleness bound: two unrelated numbers that
/// happened to both be durations.
async fn store_answer(context: &Context, pumping: &mut Pumping) {
    // Taken whatever happens, so a statement that must not be stored cannot
    // leave bytes behind for the next one to pick up.
    let recorded = pumping.recording.take();

    let (Some(cache), Some(recording)) = (context.cache.as_ref(), recorded) else {
        return;
    };
    if recording.failed || recording.frames.is_empty() {
        return;
    }

    cache
        .put(
            recording.key,
            pgprox_core::cache::CachedResult {
                frames: std::sync::Arc::from(recording.frames.as_slice()),
                ttl: NO_STALENESS_BOUND_HERE,
            },
        )
        .await;
}

/// Drops a tenant's cached results when it writes.
///
/// Best-effort, and ADR 0021 says so in as many words: this node sees writes
/// that pass through it, needs gossip for writes through another node, and
/// never sees a migration or an operator with psql. The TTL is the guarantee
/// and this is an improvement on it. It is not read-your-writes.
///
/// Conservative in two directions on purpose. Anything the classifier does not
/// call read-only invalidates, including `Unknown`, because that class exists
/// so a construct nobody has taught it yet is treated as a write. And a `Parse`
/// invalidates without waiting to see whether the statement is executed, so a
/// client that prepares a write and abandons it clears the tenant's entries.
///
/// Both cost a miss. The other direction costs a wrong answer, and the
/// asymmetry is the whole argument: over-invalidating wastes a round trip the
/// client was going to make anyway, while under-invalidating serves data the
/// proxy knew was stale.
async fn invalidate_on_write(
    context: &Context,
    message: &pgprox_proto::frontend::FrontendMessage<'_>,
    tenant: &pgprox_core::ids::TenantId,
) {
    use pgprox_proto::frontend::FrontendMessage;

    // Nothing to do on a node with no cache. The caller has already checked
    // that this tenant is served, so this is the narrower guard of the two and
    // exists because a function that can drop a tenant's results should not
    // depend on where it was called from to be safe.
    let Some(cache) = context.cache.as_ref() else {
        return;
    };

    let sql = match message {
        FrontendMessage::Query { sql } | FrontendMessage::Parse { sql, .. } => *sql,
        _ => return,
    };

    if pgprox_route::classify(sql) != pgprox_core::route::StmtClass::ReadOnly {
        cache.invalidate_tenant(tenant).await;
    }
}

/// Counts a statement, and only a statement.
///
/// A `Query` in the simple protocol and an `Execute` in the extended one are
/// the two frames that run one. Counting every frame counted `Parse`, `Bind`
/// and `Sync` as statements of their own and reported four times the truth;
/// counting acquires counted a whole transaction as one. Both were tried, and
/// both were wrong under a name that said statements.
fn record_statement(
    context: &Context,
    message: &pgprox_proto::frontend::FrontendMessage<'_>,
    target: pgprox_core::route::RouteTarget,
) {
    use pgprox_proto::frontend::FrontendMessage;

    if matches!(
        message,
        FrontendMessage::Query { .. } | FrontendMessage::Execute { .. }
    ) {
        context.routes.record(target);
    }
}

/// Sends a refusal to the client before passing it on.
///
/// The functions below are about pools and parameters and hold no wire, so
/// they return the refusal and this puts it where the client can read it. A
/// refusal that never reaches the client is a socket that closes for no stated
/// reason, which is what every one of these paths used to do.
async fn told<S, T>(wire: &mut Wire<S>, result: Result<T, ShellError>) -> Result<T, ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    match result {
        Ok(value) => Ok(value),
        Err(ShellError::Refused(reason)) => Err(wire.refuse(reason).await),
        Err(other) => Err(other),
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

    // The refusal travels back to the caller, which holds the wire and tells
    // the client. This function has no wire on purpose: it is about the pool.
    let (guard, taken) = fit_connection(context, &backend.pool_key(), deadline).await?;

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

/// Takes connections from the pool until one is fit to use.
///
/// `M20.5`. Nothing reads a pooled connection while it is idle, so a server
/// that went away between borrowers is discovered here or not at all. Without
/// this, the client's own query was what discovered it, and what the client saw
/// was its statement failing on a connection that was already dead when it
/// arrived.
///
/// A connection that fails the check is dropped rather than returned:
/// `UpstreamGuard` discards unless `release_clean` was called, so letting it go
/// is what closes it.
///
/// # Why more than one attempt
///
/// The condition that produces one dead connection produces all of them. A
/// database restart or a failover leaves the whole warm pool stale, and a
/// client that got one retry would meet the second corpse. The bound exists
/// because a pool that is somehow always unfit must become a refusal the client
/// is told about rather than a loop, and because `acquire` already has the
/// deadline that makes waiting finite.
const FITNESS_ATTEMPTS: usize = 4;

async fn fit_connection(
    context: &Context,
    key: &pgprox_core::ids::PoolKey,
    deadline: std::time::Instant,
) -> Result<(UpstreamGuard, Upstreamed<crate::dial::Stream>), ShellError> {
    for attempt in 0..FITNESS_ATTEMPTS {
        let guard = context
            .pool
            .acquire(key, deadline)
            .await
            .map_err(|err| ShellError::Refused(err.into()))?;
        let mut taken = context
            .pool
            .take_connection(guard.key(), guard.id())
            .ok_or(ShellError::Disconnected)?;

        if !taken.unfit().await {
            return Ok((guard, taken));
        }

        // Dropped in this order for the reason `release` has one: the payload
        // is the socket and the guard is the pool's accounting for it, and a
        // slot freed before its socket is a slot another session can fill while
        // the upstream is still counting the old one.
        drop(taken);
        drop(guard);
        tracing::debug!(
            attempt,
            "an idle upstream connection had unread bytes or had closed, taking another"
        );
    }

    Err(ShellError::Refused(ClientError::UpstreamClosed))
}

/// What a client is told when the relay reached a frame with nothing to send
/// it on.
///
/// Unreachable as the code stands: every path that gets that far has either
/// taken a connection or returned. It is a refusal rather than an unwrap
/// because the alternative to being told is a socket that closes with nothing
/// on it, which is the hardest kind of bug to report.
const NO_CONNECTION: ClientError =
    ClientError::ProtocolViolation("a message arrived with no connection to send it on");

/// Sends one client frame to the server, or answers the client directly.
///
/// Split out of the relay loop because it is a step of its own: the
/// connection's record of what it holds has to agree with what the frame is
/// about to assume, before anything crosses.
///
/// Returns false when the frame was answered here and nothing went upstream,
/// which happens for a `Parse` naming a statement the connection already
/// holds: Postgres refuses a second `Parse` under a name it has, and the
/// client is owed a `ParseComplete` that is true either way.
async fn send_upstream<S>(
    wire: &mut Wire<S>,
    upstream: &mut Upstreamed<crate::dial::Stream>,
    pumping: &mut Pumping,
    message: &pgprox_proto::frontend::FrontendMessage<'_>,
    session: &mut pgprox_session::resume::SessionMemory,
    onward: Frame<'_>,
    tail: usize,
) -> Result<bool, ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    // `DISCARD ALL` deallocates every prepared statement on the connection this
    // is about to go to, so both maps have to hear about it. Here rather than
    // beside the parameter half in `observe`, because this is the first point
    // where the connection it will run on is known: `observe` sees the frame
    // before a connection has been acquired.
    //
    // Before the statement is forwarded, not after. Over-clearing costs a
    // re-prepare and under-clearing produces "prepared statement does not
    // exist" on a connection the proxy thought was warm, so the safe direction
    // is to forget early. `M15.3` argued that and `M16.7` connected it.
    if let pgprox_proto::frontend::FrontendMessage::Query { sql }
    | pgprox_proto::frontend::FrontendMessage::Parse { sql, .. } = message
    {
        pgprox_session::resume::observe_statement(
            &mut session.statements,
            &mut upstream.statements,
            sql,
        );
    }

    // The protocol form of the same thing, and here for the same reason: this
    // is the first point where the connection the `Close` is about to reach is
    // known. The name has already been rewritten into `onward` by `observe`, so
    // dropping the session's entry now cannot leave the frame untranslatable.
    if let pgprox_proto::frontend::FrontendMessage::Close {
        target: pgprox_proto::frontend::Target::Statement,
        name,
    } = message
    {
        pgprox_session::resume::on_close(&mut session.statements, &mut upstream.statements, name);
    }

    // A `Parse` or a `Bind` names a statement the connection this session was
    // just lent may never have seen, or may already hold.
    match ready_statement(upstream, message, session) {
        Statement::Nothing => {}
        Statement::Prepared(swallow) => pumping.swallow += swallow,
        Statement::AlreadyPrepared => {
            wire.queue(|out| out.extend_from_slice(&[b'1', 0, 0, 0, 4]));
            // Nothing goes upstream, but the rest of this message is still on
            // the client's socket and has to leave it, or the next header is
            // read from inside this body. Unreachable today, because only a
            // `Parse` reaches this arm and a `Parse` is never streamed, and
            // here anyway because the reason it is unreachable lives in another
            // function.
            stream_body(wire, &mut upstream.wire, tail, true).await?;
            return Ok(false);
        }
    }

    // Statement name mapped, nothing else touched.
    //
    // The length is what arrived plus what rewriting did to it, not the length
    // of the buffer: `onward` may be a prefix with the rest still on the
    // client's socket, and `rewrite` changes the size of the part that is here.
    // Writing the buffer's length would announce a short message and then send
    // a long one.
    forward_header(&mut upstream.wire, onward.tag(), onward.body().len() + tail);
    upstream
        .wire
        .queue(|out| out.extend_from_slice(onward.body()));
    stream_body(wire, &mut upstream.wire, tail, false).await?;
    pumping.owed.sent(message);
    upstream.wire.flush().await?;
    Ok(true)
}

/// Whether the client will send more before it expects an answer.
///
/// True for the frames in the middle of an extended-query sequence, false for
/// the ones that end one: `Sync` and `Flush` both make the server answer, and
/// a simple `Query` is a sequence of one.
///
/// Reading back after every frame instead would deadlock exactly like the
/// copy-in did, with the proxy waiting for the server and the server waiting
/// for the rest of the sequence.
///
/// `Flush` is included as an ending because a client that sends one is waiting
/// on the answer to what it has sent so far, which is the whole reason the
/// message exists.
fn awaits_more(message: &pgprox_proto::frontend::FrontendMessage<'_>) -> bool {
    use pgprox_proto::frontend::FrontendMessage as Message;

    matches!(
        message,
        Message::Parse { .. }
            | Message::Bind { .. }
            | Message::Describe { .. }
            | Message::Execute { .. }
            | Message::Close { .. }
    )
}

/// What had to happen before a `Parse` or a `Bind` could be forwarded.
enum Statement {
    /// Not a statement-bearing frame.
    Nothing,
    /// The connection is ready, and this many replies are not the client's.
    Prepared(usize),
    /// The connection already holds it, so the client's `Parse` must not go.
    AlreadyPrepared,
}

/// Brings the connection up to what this frame is about to assume.
///
/// A `Bind` for a statement the connection does not hold gets the `Parse`
/// first. A `Parse` for one it already holds gets nothing, because Postgres
/// refuses a second `Parse` under a name it holds and the caller answers the
/// client itself.
fn ready_statement(
    upstream: &mut Upstreamed<crate::dial::Stream>,
    message: &pgprox_proto::frontend::FrontendMessage<'_>,
    session: &pgprox_session::resume::SessionMemory,
) -> Statement {
    use pgprox_proto::frontend::FrontendMessage as Message;

    // The unnamed statement, which has no global name and is not one of the
    // statements the connection holds. `M20.6`. It belongs to whichever
    // connection last parsed it, and a session that moved between the `Parse`
    // and the `Bind` has to have it parsed again on the new one.
    if let Some(sql) = unnamed_statement(message, session) {
        return match message {
            // The client's own `Parse` is about to go, so the connection ends
            // up with it either way and nothing is owed.
            Message::Parse { .. } => {
                upstream.statements.note_unnamed(sql);
                Statement::Nothing
            }
            _ if upstream.statements.holds_unnamed(sql) => Statement::Nothing,
            // Re-parsed rather than refused, and legal: a `Parse` of the
            // unnamed statement replaces whatever was there, so unlike a named
            // one it cannot collide with itself.
            _ => {
                upstream.wire.queue(|out| {
                    pgprox_proto::encode_frontend::parse(out, "", sql);
                });
                upstream.statements.note_unnamed(sql);
                Statement::Prepared(1)
            }
        };
    }

    let Some((global, sql)) = statement_of(message, session) else {
        return Statement::Nothing;
    };
    let held = upstream.statements.holds(&global);

    if matches!(message, Message::Parse { .. }) {
        if held {
            return Statement::AlreadyPrepared;
        }
        // The client's own `Parse` is about to go, so the connection records
        // it and this only makes room.
        return Statement::Prepared(evict_for(upstream, &global));
    }

    if held {
        return Statement::Prepared(0);
    }

    let swallow = evict_for(upstream, &global) + 1;
    upstream.wire.queue(|out| {
        pgprox_proto::encode_frontend::parse(out, global.as_str(), &sql);
    });
    Statement::Prepared(swallow)
}

/// The SQL of the unnamed statement, when this message is about it.
///
/// `M20.6`. Separated from [`statement_of`] because the unnamed statement has no
/// global name and is not one of the statements a connection holds: it is
/// replaced rather than added to, and asking the `held` map about it would put
/// something the server does not keep under a cap that decides what it evicts.
fn unnamed_statement<'a>(
    message: &pgprox_proto::frontend::FrontendMessage<'_>,
    session: &'a pgprox_session::resume::SessionMemory,
) -> Option<&'a str> {
    use pgprox_proto::frontend::FrontendMessage as Message;

    let (Message::Parse {
        statement: name, ..
    }
    | Message::Bind {
        statement: name, ..
    }) = message
    else {
        return None;
    };
    if !name.is_empty() {
        return None;
    }
    session.statements.get("").map(|held| held.sql.as_str())
}

/// The statement a `Parse` or a `Bind` names, as this proxy calls it.
fn statement_of(
    message: &pgprox_proto::frontend::FrontendMessage<'_>,
    session: &pgprox_session::resume::SessionMemory,
) -> Option<(pgprox_pool::statements::GlobalName, String)> {
    use pgprox_proto::frontend::FrontendMessage as Message;

    let (Message::Parse {
        statement: name, ..
    }
    | Message::Bind {
        statement: name, ..
    }) = message
    else {
        return None;
    };
    let prepared = session.statements.get(name)?;
    Some((prepared.global.clone(), prepared.sql.clone()))
}

/// Makes room on the connection for one more statement.
///
/// Returns how many `CloseComplete`s the client must not be shown. Eviction is
/// what keeps a long-lived connection from accumulating every statement every
/// session that borrowed it ever prepared.
fn evict_for(
    upstream: &mut Upstreamed<crate::dial::Stream>,
    global: &pgprox_pool::statements::GlobalName,
) -> usize {
    // Already held, or a variant added later: neither is a reason to close
    // anything.
    let pgprox_pool::statements::Preparation::Replay { evict } =
        upstream.statements.prepare_for(global)
    else {
        return 0;
    };

    upstream.wire.queue(|out| {
        for victim in &evict {
            pgprox_proto::encode_frontend::close_statement(out, victim.as_str());
        }
    });
    evict.len()
}

/// Decodes one client frame and records what it does to the session.
///
/// Two steps rather than one, and they fail differently, which is why the error
/// comes back rather than a bare `None`: a frame this proxy cannot decode is not
/// the same problem as a `Bind` naming a statement this session never parsed,
/// and an operator reading the log needs to know which.
fn decoded<'a>(
    tag: pgprox_proto::frame::Tag,
    body: &'a [u8],
    session: &mut pgprox_session::resume::SessionMemory,
) -> Result<(pgprox_proto::frontend::FrontendMessage<'a>, Vec<u8>), ClientError> {
    let frame = Frame::new(tag, body);
    let message = pgprox_proto::frontend::decode(&frame)
        .map_err(|_| ClientError::ProtocolViolation("undecodable message"))?;
    let outgoing = observe(&message, body, session).ok_or(ClientError::ProtocolViolation(
        "a statement name this session never parsed",
    ))?;
    Ok((message, outgoing))
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
            .observe_statement(sql, pgprox_pool::pin::Replayable::DEFAULT);
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
        // The unnamed statement keeps its own name, which is no name. `M20.6`:
        // renaming it made it a named one, and the two are different things.
        // The unnamed statement is replaced by the next `Parse` of it and does
        // not survive a `Close`; a named one persists until it is closed, so
        // every one-shot query a driver sent through the unnamed statement
        // became a permanent entry under `per_connection_cap`, evicting real
        // statements and paying a `Close` round trip to do it. pgcat carries an
        // `anonymous()` on four message types for the same reason.
        //
        // The session still records the SQL: a `Bind` of it may land on a
        // connection whose unnamed statement is something else, and
        // `ready_statement` needs to know what to re-parse.
        Message::Parse {
            statement: statement @ "",
            sql,
        } => {
            session.statements.parse(statement, sql);
            Some(body.to_vec())
        }
        Message::Parse { statement, sql } => {
            let global = session.statements.parse(statement, sql);
            rewrite::parse_statement(body, global.as_str())
        }
        // And every other message naming it travels as it arrived, for the same
        // reason: the name it uses is the one the server knows it by.
        Message::Bind { statement: "", .. } => Some(body.to_vec()),
        Message::Bind { statement, .. } => {
            let prepared = session.statements.get(statement)?;
            rewrite::bind_statement(body, prepared.global.as_str())
        }
        Message::Describe { name, .. } | Message::Close { name, .. }
            if name.is_empty() && rewrite::describes_statement(body) =>
        {
            Some(body.to_vec())
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
    grant: &Grant,
) -> Result<(), ShellError> {
    use pgprox_session::resume::Step;

    // The tenant's own cap on its runaway queries, which the sidecar sends and
    // which is per connection rather than per session: a connection borrowed
    // from the pool carries whatever the last borrower set.
    let mut statements: Vec<String> = grant
        .pool
        .statement_timeout
        .map(|timeout| {
            format!(
                "SET statement_timeout = {}",
                u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX)
            )
        })
        .into_iter()
        .collect();

    // The connection's own memory is not tracked across borrows yet, so this
    // replays onto a connection assumed to carry nothing. Correct and
    // occasionally wasteful: a `SET` applied twice is the same as once, where
    // a `SET` skipped is a session that forgot something. `M6.49` gives the
    // pool the memory that makes the skip safe.
    let connection = pgprox_session::resume::ConnectionMemory::default();

    statements.extend(
        pgprox_session::resume::on_acquire(session, &connection)
            .into_iter()
            .filter_map(|step| match step {
                Step::Run(sql) => Some(sql),
                _ => None,
            }),
    );

    for sql in statements {
        upstream
            .wire
            .queue(|out| pgprox_proto::encode_frontend::query(out, &sql));
        upstream.wire.flush().await?;

        let mut body = Vec::new();
        loop {
            let tag = upstream
                .wire
                .read_tagged(&mut body, pgprox_proto::frame::DEFAULT_MAX_FRAME)
                .await?;
            if tag == pgprox_proto::frame::Tag::READY_FOR_QUERY {
                break;
            }
            if tag == pgprox_proto::frame::Tag::ERROR_RESPONSE {
                // A parameter the server refuses is the session's problem
                // rather than this connection's, and the client is about to
                // send a statement that will fail in a way it can read. The
                // replay stops here rather than pretending it worked.
                // Carried back to the caller, which holds the wire and tells
                // the client: it is about to send a statement that depends on
                // a parameter this connection does not have.
                return Err(ShellError::Refused(ClientError::ProtocolViolation(
                    "a replayed session parameter was refused",
                )));
            }
        }
    }
    Ok(())
}

/// What one session carries from one server answer to the next.
///
/// Two counters that only the response pump reads, kept together so the pump
/// takes one argument for them rather than one each.
#[derive(Default)]
struct Pumping {
    /// Completions for frames the proxy sent on the client's behalf.
    ///
    /// The client sent neither the `Parse` nor the `Close`, and must not see
    /// their completions, or every reply after them is one out of step.
    swallow: usize,
    /// What the server still owes the client.
    ///
    /// Only a `Flush` reads it. A `Sync` and a simple `Query` end with a
    /// `ReadyForQuery`, which is a terminator anything can wait for; a `Flush`
    /// has none, so the only way to know it has been answered is to have
    /// counted what was asked.
    owed: pgprox_session::flush::Outstanding,
    /// The statement in flight, when its answer may be cached.
    ///
    /// `None` on every statement the cache would refuse and on every node
    /// without one, which keeps this off the path of anyone not using it.
    ///
    /// Boxed, and one object rather than three fields, because all of it lives
    /// across the await that reads the server's answer, and a session future
    /// is the union of everything alive across its awaits. Inline it cost 152
    /// bytes and put the future over its ceiling; as a pointer it costs eight,
    /// and the allocation happens only on the path that was already
    /// normalising SQL into a fresh `String`.
    recording: Option<Box<Recording>>,
}

impl Pumping {
    /// Whether this frame is a completion the client must not see, taking it
    /// off the count if it is.
    ///
    /// One rule in one place. It was written out twice, once on the streaming
    /// path and once on the buffering one, and `M17.4` found five mutants
    /// living across the two copies. Each is a session that desynchronises and
    /// never recovers: `+=` for `-=` makes the count grow with every
    /// completion, so every reply after the first is swallowed and the client
    /// waits forever; `/=` never reaches zero from one, which is the same;
    /// and `<` for `>` swallows nothing, so the client sees a `ParseComplete`
    /// it never asked for and reads every answer after it one frame out of
    /// step.
    ///
    /// Consuming rather than a predicate plus a decrement, because the two
    /// have to happen together: the old shape allowed a caller to ask twice.
    fn swallow_one(&mut self, tag: pgprox_proto::frame::Tag) -> bool {
        if matches!(
            tag,
            pgprox_proto::frame::Tag::PARSE_COMPLETE | pgprox_proto::frame::Tag::CLOSE_COMPLETE
        ) && self.swallow > 0
        {
            self.swallow -= 1;
            return true;
        }
        false
    }
}

/// What the cache path needs while a statement is in flight.
struct Recording {
    /// Where the answer will be stored.
    key: pgprox_core::cache::CacheKey,
    /// A copy of what went back to the client.
    ///
    /// The bytes forwarded, not the ones received: a `Parse` the proxy issued
    /// on the client's behalf has its completion swallowed before it gets
    /// here, and a recording including it would replay a frame the client
    /// never asked for.
    frames: Vec<u8>,
    /// Whether the server said no at any point in this answer.
    ///
    /// An error makes the whole thing unfit to store. Part of it may already
    /// be recorded, and replaying that part later would be a result nobody
    /// ever received.
    failed: bool,
}

/// Copies the server's answer back, returning whether the connection is free.
///
/// "Free" is the relay's judgement, and the caller still overrules it for a
/// tenant in session pooling: that tenant asked to keep its connection, and
/// one silently given transaction pooling loses its temporary tables and
/// advisory locks between statements.
///
/// `asked` is the client's last frame, and it decides what ends the answer: a
/// `Flush` is answered when nothing is outstanding, everything else by a
/// `ReadyForQuery`.
async fn pump<S>(
    wire: &mut Wire<S>,
    upstream: &mut Upstreamed<crate::dial::Stream>,
    relay: &mut Relay,
    context: &Context,
    conn: ConnId,
    pumping: &mut Pumping,
    asked: &pgprox_proto::frontend::FrontendMessage<'_>,
) -> Result<bool, ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let flushing = matches!(asked, pgprox_proto::frontend::FrontendMessage::Flush);
    // A `Flush` with nothing outstanding. Postgres answers a lone `Flush` with
    // silence, correctly, so reading even one frame here would block on a
    // message that is never sent. Checked before the loop rather than inside
    // it, because the check inside only runs after a read.
    if flushing && pumping.owed.settled() {
        wire.flush().await?;
        return Ok(false);
    }

    let mut body = Vec::new();
    loop {
        // An upstream that goes away mid-session is not this client's doing,
        // and it is told rather than having its socket closed underneath it.
        // Passing the disconnect straight through made a database restart look
        // to every driver like a network fault against the proxy.
        //
        // The header first, and the body only if something here needs it.
        // `M16.1` measured what reading it unconditionally costs: one 16 MiB
        // DataRow was held in full, twice, once in `body` and once again in the
        // write buffer that `forward` copies it into.
        //
        // Splitting the read is safe here and would not be in the relay loop
        // above. `read_header` consumes five bytes before the body arrives, so
        // a future dropped between the two leaves the wire inside a message.
        // This loop has no `select!` and nothing races it; the relay loop does,
        // which is why `read_tagged` stays atomic and is still what it calls.
        let header = match upstream
            .wire
            .read_header(pgprox_proto::frame::DEFAULT_MAX_FRAME)
            .await
        {
            Ok(header) => header,
            Err(ShellError::Disconnected | ShellError::Io(_)) => {
                return Err(wire.refuse(ClientError::UpstreamClosed).await);
            }
            Err(other) => return Err(other),
        };
        let tag = header.tag;

        // Everything this loop does with a frame besides forwarding it is
        // decided by the tag, except recording for the cache, which is the one
        // thing that wants the bytes. And an uninspected tag decodes to
        // `Opaque` without reading a body at all, which
        // `what_is_not_inspected_is_not_decoded_either` states as a property
        // rather than leaving to two lists that happen to agree.
        //
        // So when both hold, the body is moved straight from one socket to the
        // other and never lands anywhere. That is every DataRow and every
        // CopyData of every uncached statement.
        if pgprox_proto::frame::inspect_policy(pgprox_proto::frame::Direction::Backend, tag)
            == pgprox_proto::frame::Inspect::None
            && pumping.recording.is_none()
        {
            let server = relay.on_server(&BackendMessage::Opaque(tag));
            if let Some(reason) = server.pinned {
                context.sessions.set_pinned(conn, reason.as_str());
            }

            // The swallowed completions are uninspected too, and both have
            // empty bodies, so this is the same check the buffering path makes
            // and it still has to happen before anything is queued.
            let swallowed = pumping.swallow_one(tag);
            if !swallowed {
                forward_header(wire, tag, header.body_len);
            }
            stream_body(&mut upstream.wire, wire, header.body_len, swallowed).await?;

            // `Opaque` is never a ReadyForQuery and never a copy response, so
            // the two terminators below cannot apply to it. What remains is the
            // Flush case, which is the same here as there.
            pumping.owed.received(tag);
            if flushing && pumping.owed.settled() {
                wire.flush().await?;
                return Ok(false);
            }
            continue;
        }

        let wanted = wanted_body(tag, header.body_len, pumping.recording.is_some());
        let tail = header.body_len - wanted;

        upstream.wire.read_body_into(&mut body, wanted).await?;
        let frame = Frame::new(tag, &body);
        let decoded = backend::decode(&frame).unwrap_or(BackendMessage::Opaque(tag));
        let server = relay.on_server(&decoded);
        if let Some(reason) = server.pinned {
            context.sessions.set_pinned(conn, reason.as_str());
        }

        // The replies to the `Close` and `Parse` this proxy sent on the
        // client's behalf. The client sent neither and must not see their
        // completions, or every reply after them is one out of step.
        if pumping.swallow_one(tag) {
            // The tail still has to leave the socket even though nothing will
            // see it, or the next header would be read from inside this body.
            stream_body(&mut upstream.wire, wire, tail, true).await?;
            continue;
        }

        // The header from what the header said, then the part that was read,
        // then whatever was not. The bytes on the wire are the same either way;
        // only where they were held differs.
        forward_header(wire, tag, header.body_len);
        wire.queue(|out| out.extend_from_slice(&body));
        stream_body(&mut upstream.wire, wire, tail, false).await?;

        // The same bytes, kept for the cache, filtered to the ones that answer
        // the statement rather than the client's framing. A `ReadyForQuery`
        // arrives once per simple query and once per `Sync`, so an entry
        // carrying one could only serve the protocol that filled it; ADR 0022
        // makes the payload a description, the rows and the completion, and
        // both hit paths generate their own terminator.
        //
        // `belongs_in_payload` is the assembler's own list. Two lists here
        // would drift, and the one nobody remembers to fix is always the second.
        record_frame(&mut pumping.recording, tag, &body);

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
            Box::pin(copying(wire, upstream, &mut body)).await?;
            continue;
        }

        if matches!(decoded, BackendMessage::ReadyForQuery(_)) {
            pumping.owed.received(tag);
            wire.flush().await?;
            return Ok(server.release);
        }

        // The other way a client can be waiting. A `Flush` makes the server
        // answer everything outstanding and then say nothing at all, so there
        // is no terminator to read until: the proxy has to know when it has
        // forwarded the last answer and go back to the client itself. Reading
        // on would block on a message that is not coming, with the client
        // blocked on the answer already sitting in this proxy.
        //
        // The connection is not released: the sequence is still open, which is
        // the whole reason the client used a `Flush` rather than a `Sync`.
        pumping.owed.received(tag);
        if flushing && pumping.owed.settled() {
            wire.flush().await?;
            return Ok(false);
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

    // `body` is still taken so the caller keeps owning the buffer, and it is
    // still used: everything that ends a copy is small and read whole. What
    // does not land in it is the copy data itself, which is the whole traffic
    // of a `COPY ... FROM STDIN` and the one message here a client chooses the
    // size of. `M16.3` did the other direction; this is the same split, on the
    // side where the sender is the untrusted one.
    //
    // Safe for the same reason it is safe there. Nothing races this loop: the
    // module comment above says why the direction is one-way, so there is no
    // cancellation point between the header and its body.
    loop {
        let header = wire
            .read_header(pgprox_proto::frame::DEFAULT_MAX_FRAME)
            .await?;
        let tag = header.tag;

        if tag == Tag::COPY_DATA {
            forward_header(&mut upstream.wire, tag, header.body_len);
            stream_body(wire, &mut upstream.wire, header.body_len, false).await?;
            upstream.wire.flush().await?;
            continue;
        }

        // `CopyDone`, `CopyFail`, `Terminate`, or a stray `Query`. All of them
        // end the copy, and the caller's loop needs the frame, so this is the
        // path that reads one.
        wire.read_body_into(body, header.body_len).await?;
        forward(&mut upstream.wire, tag, body);
        upstream.wire.flush().await?;
        return Ok(());
    }
}

/// Keeps one frame of an answer, and gives up if the answer gets too big.
///
/// `belongs_in_payload` is the assembler's own list. Two lists here would
/// drift, and the one nobody remembers to fix is always the second.
///
/// Giving up means dropping the recording, not truncating it. A partial
/// recording stored would be a wrong answer served later, and there is no half
/// measure between keeping all of an answer and keeping none. The rest of the
/// answer then takes the streaming path, because `wanted_body` and the pump's
/// streaming branch both key off whether a recording is live, so giving up here
/// gives up the buffering with it.
fn record_frame(
    recording: &mut Option<Box<Recording>>,
    tag: pgprox_proto::frame::Tag,
    body: &[u8],
) {
    let Some(live) = recording.as_mut() else {
        return;
    };

    if pgprox_session::sequence::belongs_in_payload(tag) {
        live.frames.push(tag.get());
        let len = u32::try_from(body.len() + 4).unwrap_or(u32::MAX);
        live.frames.extend_from_slice(&len.to_be_bytes());
        live.frames.extend_from_slice(body);
    }
    if tag == pgprox_proto::frame::Tag::ERROR_RESPONSE {
        live.failed = true;
    }
    if live.frames.len() > MAX_RECORDED_ANSWER {
        *recording = None;
    }
}

/// Reads as much of a client message as anything here will read.
///
/// Returns how much of the body is still on the wire, for the caller to forward
/// straight upstream once it knows where upstream is. That order is forced:
/// the destination comes from routing, routing needs the prefix, and the prefix
/// is what this reads. Until then the tail waits in the socket, which is
/// backpressure rather than a buffer.
///
/// `M16.6`. A `Bind` carries the client's parameter values, which can be
/// hundreds of megabytes, and the only part of it anything here reads is the
/// two names at the front.
async fn read_client_body<S>(
    wire: &mut Wire<S>,
    header: pgprox_proto::frame::FrameHeader,
    body: &mut Vec<u8>,
    caching: bool,
) -> Result<usize, ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let wanted = client_body_wanted(header.tag, header.body_len, caching);
    wire.read_body_into(body, wanted).await?;
    let tail = header.body_len - wanted;

    // A name that runs past the prefix is rare and legal, so the rest is read
    // rather than the client refused. `decode` is pure, so asking it twice
    // costs a parse of a few kilobytes on a path almost nothing takes.
    if tail > 0 && pgprox_proto::frontend::decode(&Frame::new(header.tag, body)).is_err() {
        wire.append_body(body, tail).await?;
        return Ok(0);
    }
    Ok(tail)
}

/// How much of a client message must be in memory before it can be acted on.
///
/// `inspect_policy` is the answer for most of them, and for a `Bind` it is four
/// kilobytes of a message that may be hundreds of megabytes: the two names are
/// at the front and the rest is parameter values nothing here reads.
///
/// # Query and Parse are read whole whatever the policy says
///
/// Their SQL is scanned for pin triggers, and `pin_reason` looks at *every*
/// statement in it rather than the first, because the simple query protocol
/// allows several in one message and `SELECT 1; LISTEN c` would otherwise go
/// through unpinned. A truncated scan is a missed pin, and a missed pin hands
/// one client another client's state. `Parse` has a second reason: the global
/// statement name is derived from the SQL, so two long statements sharing a
/// prefix would collide on one name.
///
/// # And nothing is streamed while the cache is on
///
/// A cache key for a `Bind` is built from its parameter values, so the whole
/// body has to be there. Coarse on purpose: this asks whether the node has a
/// cache at all rather than whether this tenant's statement is cacheable, which
/// is not known until the message is decoded. Erring toward reading is the
/// direction that cannot be wrong.
fn client_body_wanted(tag: pgprox_proto::frame::Tag, body_len: usize, caching: bool) -> usize {
    use pgprox_proto::frame::{DEFAULT_MAX_INSPECT, Direction, Tag};

    if caching || matches!(tag, Tag::QUERY | Tag::PARSE) {
        return body_len;
    }
    pgprox_proto::relay::inspect_budget(Direction::Frontend, tag, body_len, DEFAULT_MAX_INSPECT)
}

/// The largest answer this proxy will hold on the chance it is cacheable.
///
/// The query cache rejects an entry bigger than its budget, and that guard is
/// at `put`, which is the end. The pump accumulates the whole answer first, so
/// until `M17.1` a 500 MB result was 500 MB held and then thrown away: the
/// cache's guard protected the cache and nothing protected the proxy.
///
/// Not the cache's budget, and deliberately not asked for. That is one global
/// figure for a store. This is per session, spent while an answer is in
/// flight, and multiplied by however many sessions are recording at once,
/// which is the same arithmetic that makes `DEFAULT_MAX_INSPECT` small. Two
/// resources, two guards.
///
/// A megabyte because the cache is for small repeated reads: ADR 0007's case
/// is a point select answered thousands of times, and an answer that does not
/// fit here was never going to earn its place in a shared budget.
const MAX_RECORDED_ANSWER: usize = 1024 * 1024;

/// How much of a server message the pump actually reads.
///
/// `inspect_policy` is the answer, and it is small: eight kilobytes for an
/// `ErrorResponse`, one byte for a `ReadyForQuery`. Reading `body_len` instead
/// bounded it by `DEFAULT_MAX_FRAME`, which is a gigabyte, and that is
/// `M15.1`'s defect one layer up: the relay was given the inspect cap and the
/// pump was not, because the pump does not use the relay.
///
/// A recording session reads whole and is the exception. A truncated cache
/// entry is a wrong answer rather than a smaller one.
fn wanted_body(tag: pgprox_proto::frame::Tag, body_len: usize, recording: bool) -> usize {
    use pgprox_proto::frame::{DEFAULT_MAX_INSPECT, Direction};

    if recording {
        return body_len;
    }
    // The rule lives in pgprox-proto, next to the policy it reads and the relay
    // that applies it to a byte stream. It was written out here as well until
    // `M16.10`, which is one edit away from a proxy that buffers more than the
    // component documenting the bound.
    pgprox_proto::relay::inspect_budget(Direction::Backend, tag, body_len, DEFAULT_MAX_INSPECT)
}

/// Queues a message header, with the body still to come.
///
/// The other half of [`forward`], for the path that never holds a body.
fn forward_header<S>(wire: &mut Wire<S>, tag: pgprox_proto::frame::Tag, body_len: usize)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    wire.queue(|out| {
        out.push(tag.get());
        out.extend_from_slice(
            &u32::try_from(body_len + 4)
                .unwrap_or(u32::MAX)
                .to_be_bytes(),
        );
    });
}

/// Moves `remaining` body bytes from one wire to the other without holding them.
///
/// Flushed per chunk rather than at the end, which is the point: queueing the
/// whole body and flushing once would move the buffer from the read side to the
/// write side rather than removing it.
///
/// `discard` drops the bytes instead of forwarding them, for a message the
/// proxy asked for on the client's behalf and the client must not see. They
/// still have to be read, or the next header would be taken from the middle of
/// this body.
async fn stream_body<A, B>(
    from: &mut Wire<A>,
    to: &mut Wire<B>,
    mut remaining: usize,
    discard: bool,
) -> Result<(), ShellError>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    while remaining > 0 {
        let chunk = from.take_body(remaining).await?;
        let taken = chunk.len();
        if !discard {
            to.queue(|out| out.extend_from_slice(chunk));
        }
        remaining -= taken;
        if !discard {
            to.flush().await?;
        }
    }
    Ok(())
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
                let peers = self.peers.peers();
                if let Some(peer) = peers.get(&node) {
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
pub async fn refuse_full<S>(
    stream: S,
    cap: u32,
    slab: Arc<pgprox_core::buf::BufferSlab>,
) -> Result<(), ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut wire = Wire::new(stream, slab);
    // Read whatever the client opened with, so the error lands where a driver
    // expects one rather than in the middle of its startup.
    let mut body = Vec::new();
    let _ = wire
        .read_untagged(&mut body, pgprox_session::shell::MAX_HANDSHAKE_FRAME)
        .await;

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
                let _ = refuse_draining(socket, Arc::clone(&context.slab)).await;
                return;
            }
            // Then admission, and that refusal is a message too. A driver told
            // 53300 reports it; a driver whose socket vanished reports a
            // network error.
            let Some(admitted) = gate.admit() else {
                // Warn rather than debug: this is the node at its ceiling,
                // which is a capacity decision somebody has to take.
                let ceiling = gate.ceiling();
                tracing::warn!(ceiling, "refused a client: at the connection ceiling");
                let _ = refuse_full(socket, ceiling, Arc::clone(&context.slab)).await;
                return;
            };
            // A session that ends badly says so. The result used to be
            // discarded, so a node that dropped a client's socket left no
            // trace of having done it, and the only evidence was on the
            // client's side. `Disconnected` at the end of a healthy session is
            // the ordinary case and stays quiet.
            match session(socket, context.as_ref(), admitted).await {
                Ok(()) | Err(ShellError::Disconnected) => {}
                Err(ShellError::Refused(reason)) => {
                    tracing::debug!(%reason, "a client was refused");
                }
                Err(error) => {
                    tracing::warn!(%error, "a client session ended badly");
                }
            }
        });
    }
}

/// Tells a client the node is going away, then closes.
///
/// # Errors
///
/// Fails when the socket does.
pub async fn refuse_draining<S>(
    stream: S,
    slab: Arc<pgprox_core::buf::BufferSlab>,
) -> Result<(), ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut wire = Wire::new(stream, slab);
    let mut body = Vec::new();
    let _ = wire
        .read_untagged(&mut body, pgprox_session::shell::MAX_HANDSHAKE_FRAME)
        .await;

    wire.queue(|out| encode::error_response(out, &ClientError::Draining));
    wire.flush().await
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
    use crate::fakepg::{
        PRIMARY_WRITTEN, fake_postgres, fake_postgres_after, row_description, statements_seen,
        text_row,
    };
    use pgprox_proto::frame::Tag;

    /// A recording armed for an answer, with a key nothing here reads.
    fn recording() -> Recording {
        Recording {
            key: pgprox_core::cache::CacheKey {
                tenant: pgprox_core::ids::TenantId::new("acme"),
                normalized_sql: std::sync::Arc::from("select $1"),
                params: std::sync::Arc::from(&b""[..]),
                search_path: std::sync::Arc::from("public"),
            },
            frames: Vec::new(),
            failed: false,
        }
    }

    #[test]
    fn an_answer_too_big_to_cache_is_dropped_rather_than_held() {
        // `M17.1`. The cache rejects an oversized entry at `put`, which is the
        // end of the answer. Until this existed the pump accumulated the whole
        // thing first, so a large result was held in full and then thrown away:
        // the cache's guard protected the cache and nothing protected the
        // proxy.
        let mut held = Some(Box::new(recording()));

        // Rows until it gives up. Each is 64 KiB, so this is bounded.
        let row = vec![b'x'; 64 * 1024];
        for _ in 0..64 {
            record_frame(&mut held, Tag::DATA_ROW, &row);
            if held.is_none() {
                break;
            }
        }

        assert!(
            held.is_none(),
            "the pump kept accumulating past {MAX_RECORDED_ANSWER} bytes"
        );
    }

    #[test]
    fn an_answer_that_fits_is_kept_whole() {
        // The cost side. Giving up too eagerly would make the cache miss
        // everything, which is worse than not having one: the work is done and
        // the entry is thrown away.
        let mut held = Some(Box::new(recording()));
        let row = vec![b'x'; 1024];
        for _ in 0..16 {
            record_frame(&mut held, Tag::DATA_ROW, &row);
        }

        let kept = held.expect("an answer well under the bound was dropped");
        // Each frame is a tag, a four-byte length and the body.
        assert_eq!(kept.frames.len(), 16 * (1 + 4 + 1024));
        assert!(!kept.failed);
    }

    #[test]
    fn only_the_frames_that_answer_the_statement_are_kept() {
        // `belongs_in_payload` decides, and a `ReadyForQuery` arrives once per
        // simple query and once per `Sync`, so an entry carrying one could only
        // serve the protocol that filled it.
        let mut held = Some(Box::new(recording()));
        record_frame(&mut held, Tag::DATA_ROW, b"row");
        record_frame(&mut held, Tag::READY_FOR_QUERY, b"I");
        record_frame(&mut held, Tag::PARAMETER_STATUS, b"x\0y\0");

        let kept = held.expect("nothing here should have dropped the recording");
        assert_eq!(kept.frames.len(), 1 + 4 + 3, "a framing message was kept");
    }

    #[test]
    fn an_error_marks_the_recording_failed_rather_than_dropping_it() {
        // The distinction the storing side needs: a failed answer is one that
        // must not be stored, and it is not the same as no answer at all.
        let mut held = Some(Box::new(recording()));
        record_frame(&mut held, Tag::ERROR_RESPONSE, b"SERROR\0\0");

        let kept = held.expect("an error should not drop the recording here");
        assert!(kept.failed);
    }

    #[test]
    fn recording_nothing_stays_nothing() {
        // A session with no recording armed must not acquire one by being fed
        // frames, which is what the pump does for every uncached statement.
        let mut none: Option<Box<Recording>> = None;
        record_frame(&mut none, Tag::DATA_ROW, b"row");
        assert!(none.is_none());
    }

    #[tokio::test]
    async fn a_name_past_the_prefix_is_topped_up_rather_than_refused() {
        // `read_client_body`'s fallback, and `M17.3` found it unconstrained:
        // all four mutants of its guard survived, because nothing here ever
        // sent a message whose names ran past the prefix.
        //
        // A `Bind` whose statement name is longer than the 4 KiB
        // `inspect_policy` allots. Legal, rare, and the client must not be
        // refused over it.
        let slab = test_slab();
        let (server, client) = tokio::io::duplex(64 * 1024);
        let mut wire = Wire::new(server, slab);
        let mut peer = client;

        let long_name = "s".repeat(6000);
        let mut body = Vec::new();
        body.extend_from_slice(b"portal\0");
        body.extend_from_slice(long_name.as_bytes());
        body.push(0);
        body.extend_from_slice(&[0, 0, 0, 0, 0, 0]);

        let mut frame = vec![Tag::BIND.get()];
        frame.extend_from_slice(&u32::try_from(body.len() + 4).expect("fits").to_be_bytes());
        frame.extend_from_slice(&body);

        let sent = frame.clone();
        let writer = tokio::spawn(async move {
            tokio::io::AsyncWriteExt::write_all(&mut peer, &sent)
                .await
                .expect("the frame is written");
            peer
        });

        let header = wire
            .read_header(pgprox_proto::frame::DEFAULT_MAX_FRAME)
            .await
            .expect("the header decodes");
        let mut got = Vec::new();
        let tail = read_client_body(&mut wire, header, &mut got, false)
            .await
            .expect("the body reads");
        drop(writer.await.expect("the writer finishes"));

        // The whole body, and no tail, because the name did not fit the prefix
        // and the rest was read rather than the client refused.
        assert_eq!(tail, 0, "a name past the prefix left a tail behind");
        assert_eq!(got.len(), body.len(), "the top-up did not read the rest");
        assert_eq!(got, body);

        // And it decodes now, which is the point of having read more.
        let decoded = pgprox_proto::frontend::decode(&Frame::new(Tag::BIND, &got))
            .expect("a complete Bind decodes");
        assert_eq!(
            decoded,
            pgprox_proto::frontend::FrontendMessage::Bind {
                portal: "portal",
                statement: &long_name,
            }
        );
    }

    #[tokio::test]
    async fn a_bind_whose_names_fit_keeps_its_tail_on_the_wire() {
        // The other side of the same guard. A `Bind` with ordinary names and a
        // large parameter must read the prefix and leave the rest, which is the
        // whole of `M16.6`. Without this the guard could top up every message
        // and nothing would notice except the memory.
        let slab = test_slab();
        let (server, client) = tokio::io::duplex(64 * 1024);
        let mut wire = Wire::new(server, slab);
        let mut peer = client;

        let body_len = 40_000;
        let mut frame = vec![Tag::BIND.get()];
        frame.extend_from_slice(&u32::try_from(body_len + 4).expect("fits").to_be_bytes());
        frame.extend_from_slice(b"p\0s\0");
        frame.resize(5 + body_len, b'v');

        let writer = tokio::spawn(async move {
            tokio::io::AsyncWriteExt::write_all(&mut peer, &frame)
                .await
                .expect("the frame is written");
            peer
        });

        let header = wire
            .read_header(pgprox_proto::frame::DEFAULT_MAX_FRAME)
            .await
            .expect("the header decodes");
        let mut got = Vec::new();
        let tail = read_client_body(&mut wire, header, &mut got, false)
            .await
            .expect("the prefix reads");

        assert_eq!(got.len(), 4096, "the prefix was not the prefix");
        assert_eq!(tail, body_len - 4096, "the tail was read instead of left");

        // Drain so the writer can finish.
        let mut sink = Vec::new();
        wire.append_body(&mut sink, tail)
            .await
            .expect("the tail drains");
        drop(writer.await.expect("the writer finishes"));
    }

    #[test]
    fn the_recorded_bound_is_a_ceiling_rather_than_a_limit_it_may_reach() {
        // `record_frame`'s bound at exactly the limit. `>` becoming `>=`
        // survived, because the test above it loops until the recording gives
        // up and never lands on the boundary.
        let mut held = Some(Box::new(recording()));

        // One frame that takes the recording to exactly the bound. A frame is a
        // tag, four length bytes and the body.
        let body = vec![b'x'; MAX_RECORDED_ANSWER - 5];
        record_frame(&mut held, Tag::DATA_ROW, &body);

        let kept = held.expect("an answer of exactly the bound was dropped");
        assert_eq!(kept.frames.len(), MAX_RECORDED_ANSWER);

        // And one byte more gives up.
        let mut held = Some(kept);
        record_frame(&mut held, Tag::DATA_ROW, b"x");
        assert!(held.is_none(), "one byte past the bound was kept");
    }

    #[tokio::test]
    async fn a_streamed_body_reaches_the_peer_while_it_is_still_streaming() {
        // `stream_body` flushes per chunk, and that is the whole point: queuing
        // the body and flushing once would move the buffer from the read side
        // to the write side rather than removing it. Nothing asserted it, so
        // deleting the `!` on the flush guard survived. The bytes come out the
        // same either way; what changes is how many are held to get them there.
        //
        // The property, stated so a machine can check it: with a receiving
        // window smaller than the body, streaming can only finish if the peer
        // is draining as it goes. Queue-then-flush would return having written
        // nothing, and the reader below would wait for bytes that are sitting
        // in a buffer.
        let slab = test_slab();
        let (from_io, mut source) = tokio::io::duplex(128 * 1024);
        let mut from = Wire::new(from_io, std::sync::Arc::clone(&slab));
        let (to_io, mut peer) = tokio::io::duplex(4096);
        let mut to = Wire::new(to_io, slab);

        let body_len = 32 * 1024;
        let writer = tokio::spawn(async move {
            tokio::io::AsyncWriteExt::write_all(&mut source, &vec![b'z'; body_len])
                .await
                .expect("the body is written");
            source
        });

        let mut got = vec![0_u8; body_len];
        let both = async {
            let (streamed, read) = tokio::join!(
                stream_body(&mut from, &mut to, body_len, false),
                tokio::io::AsyncReadExt::read_exact(&mut peer, &mut got),
            );
            streamed.expect("the body streams");
            read.expect("the body arrives");
        };

        tokio::time::timeout(std::time::Duration::from_secs(10), both)
            .await
            .expect("streaming did not reach the peer; it was queued instead");
        drop(writer.await.expect("the writer finishes"));

        assert!(got.iter().all(|byte| *byte == b'z'));
    }

    #[tokio::test]
    async fn a_whole_frame_announces_the_length_it_carries() {
        // `forward` has computed `body.len() + 4` since M6 and nothing checked
        // it: `+` became `*` and every test stayed green. `M16.3` added
        // `forward_header` beside it and tested that one, which is how the gap
        // showed up.
        let slab = test_slab();
        let (io, mut peer) = tokio::io::duplex(64 * 1024);
        let mut wire = Wire::new(io, slab);

        let body = b"SELECT 1\0";
        forward(&mut wire, Tag::QUERY, body);
        wire.flush().await.expect("the frame goes out");

        let mut got = vec![0_u8; 5 + body.len()];
        tokio::io::AsyncReadExt::read_exact(&mut peer, &mut got)
            .await
            .expect("the frame arrives");

        assert_eq!(got[0], Tag::QUERY.get());
        assert_eq!(
            u32::from_be_bytes(got[1..5].try_into().expect("four bytes")),
            u32::try_from(body.len() + 4).expect("fits"),
            "the length announced does not match the body behind it"
        );
        assert_eq!(&got[5..], body);
    }

    #[test]
    fn a_bind_is_read_by_its_names_and_a_query_is_read_whole() {
        // `M16.6`. A `Bind` is the case this exists for: the two names are at
        // the front and the rest is parameter values nothing here reads, so a
        // 100 MB parameter should cost four kilobytes.
        use pgprox_proto::frame::DEFAULT_MAX_INSPECT;
        let huge = 100 * 1024 * 1024;

        assert_eq!(client_body_wanted(Tag::BIND, huge, false), 4096);
        assert_eq!(client_body_wanted(Tag::BIND, 300, false), 300);

        // Query and Parse are read whole whatever inspect_policy says, and this
        // is the assertion that keeps them that way. `pin_reason` scans every
        // statement in the SQL, so a truncated scan is a missed pin, and a
        // missed pin hands one client another client's state. `Parse` has a
        // second reason: its global name is derived from the SQL, so two long
        // statements sharing a prefix would collide on one name.
        assert_eq!(
            client_body_wanted(Tag::QUERY, huge, false),
            huge,
            "a Query was truncated, so a LISTEN past the prefix would be missed"
        );
        assert_eq!(client_body_wanted(Tag::PARSE, huge, false), huge);

        // Bulk frontend messages read nothing at all.
        assert_eq!(client_body_wanted(Tag::COPY_DATA, huge, false), 0);
        assert_eq!(client_body_wanted(Tag::FUNCTION_CALL, huge, false), 0);

        // Nothing is streamed while a cache is present, because a cache key for
        // a Bind is built from its parameter values.
        assert_eq!(client_body_wanted(Tag::BIND, huge, true), huge);
        assert_eq!(client_body_wanted(Tag::COPY_DATA, huge, true), huge);

        // And whatever is read stays under the ceiling.
        assert!(client_body_wanted(Tag::SYNC, huge, false) <= DEFAULT_MAX_INSPECT);
    }

    #[tokio::test]
    async fn a_rewritten_prefix_and_its_tail_announce_one_length() {
        // The hazard that made this task worth designing before writing. The
        // forwarded header is built from what arrived plus what rewriting did
        // to the part that is here, not from the length of the buffer. Writing
        // the buffer's length would announce a short message and then send a
        // long one, and every frame after it would be read from the wrong
        // offset.
        use tokio::io::AsyncReadExt;

        let slab = test_slab();
        let (from_io, mut client_side) = tokio::io::duplex(64 * 1024);
        let mut from = Wire::new(from_io, std::sync::Arc::clone(&slab));
        let (to_io, mut server_side) = tokio::io::duplex(64 * 1024);
        let mut to = Wire::new(to_io, slab);

        // A tail waiting on the client's socket, as it would be mid-message.
        let tail_bytes = vec![b'v'; 5000];
        let sent = tail_bytes.clone();
        let writer = tokio::spawn(async move {
            tokio::io::AsyncWriteExt::write_all(&mut client_side, &sent)
                .await
                .expect("the tail is written");
            client_side
        });

        // A prefix that rewriting made longer than what arrived.
        let prefix = b"portal\0pgprox_a_much_longer_global_name\0".to_vec();
        forward_header(&mut to, Tag::BIND, prefix.len() + tail_bytes.len());
        to.queue(|out| out.extend_from_slice(&prefix));
        stream_body(&mut from, &mut to, tail_bytes.len(), false)
            .await
            .expect("the tail streams");
        to.flush().await.expect("the message goes out");
        drop(writer.await.expect("the writer finishes"));

        let mut got = vec![0_u8; 5 + prefix.len() + tail_bytes.len()];
        server_side
            .read_exact(&mut got)
            .await
            .expect("the message arrives");

        assert_eq!(got[0], Tag::BIND.get());
        let declared = u32::from_be_bytes(got[1..5].try_into().expect("four bytes")) as usize;
        assert_eq!(
            declared,
            prefix.len() + tail_bytes.len() + 4,
            "the length announced does not match the bytes behind it"
        );
        assert_eq!(&got[5..5 + prefix.len()], &prefix[..]);
        assert_eq!(&got[5 + prefix.len()..], &tail_bytes[..]);
    }

    #[test]
    fn an_inspected_body_is_bounded_by_the_inspect_cap_not_the_relay_cap() {
        // `M16.11`. The pump read every inspected body whole, and whole means
        // whatever the peer declared, up to DEFAULT_MAX_FRAME. That is a
        // gigabyte, and inspect_policy wants eight kilobytes of an
        // ErrorResponse and one byte of a ReadyForQuery.
        use pgprox_proto::frame::{DEFAULT_MAX_FRAME, DEFAULT_MAX_INSPECT};

        // A server declaring a gigabyte for a message that is one byte long.
        assert_eq!(
            wanted_body(Tag::READY_FOR_QUERY, DEFAULT_MAX_FRAME, false),
            DEFAULT_MAX_INSPECT,
            "a Whole-policy body was read against the relay cap"
        );

        // A prefix is the policy's number, not the body's, whichever is smaller.
        assert_eq!(
            wanted_body(Tag::ERROR_RESPONSE, DEFAULT_MAX_FRAME, false),
            8192
        );
        assert_eq!(wanted_body(Tag::ERROR_RESPONSE, 100, false), 100);

        // The ordinary cases are untouched, which is what says the cap costs
        // nothing where it is not needed.
        assert_eq!(wanted_body(Tag::READY_FOR_QUERY, 1, false), 1);
        assert_eq!(wanted_body(Tag::COMMAND_COMPLETE, 9, false), 9);

        // Uninspected messages read nothing at all: that is M16.3's path.
        assert_eq!(wanted_body(Tag::DATA_ROW, DEFAULT_MAX_FRAME, false), 0);

        // And a recording session reads whole, because a truncated cache entry
        // is a wrong answer rather than a smaller one.
        assert_eq!(
            wanted_body(Tag::DATA_ROW, 50_000_000, true),
            50_000_000,
            "a recorded frame was truncated, which corrupts the entry"
        );
    }

    #[tokio::test]
    async fn a_body_streams_across_without_being_held() {
        // `M16.3`. The mechanism the pump now uses for every uninspected
        // message: the header is queued from what the header said, and the body
        // is moved a chunk at a time. Nothing here ever holds the message.
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let body = vec![b'r'; 400 * 1024];
        let slab = test_slab();

        // The upstream side, holding a body the pump is about to move.
        let (from_io, mut server) = tokio::io::duplex(64 * 1024);
        let mut from = Wire::new(from_io, std::sync::Arc::clone(&slab));
        // The client side, which reads what comes out.
        let (to_io, mut client) = tokio::io::duplex(1024 * 1024);
        let mut to = Wire::new(to_io, slab);

        let sent = body.clone();
        let writer = tokio::spawn(async move {
            server.write_all(&sent).await.expect("the body is written");
            server
        });

        forward_header(&mut to, Tag::DATA_ROW, body.len());
        stream_body(&mut from, &mut to, body.len(), false)
            .await
            .expect("the body streams");
        to.flush().await.expect("the last chunk goes out");
        drop(writer.await.expect("the writer finishes"));

        // What arrived is the message, byte for byte, with a length prefix that
        // agrees with what follows it. A header written from `body_len` and a
        // body streamed separately are two places the count could disagree.
        let mut got = vec![0_u8; 5 + body.len()];
        client
            .read_exact(&mut got)
            .await
            .expect("the message arrives");
        assert_eq!(got[0], Tag::DATA_ROW.get());
        assert_eq!(
            u32::from_be_bytes(got[1..5].try_into().expect("four bytes")),
            u32::try_from(body.len() + 4).expect("the fixture fits"),
        );
        assert_eq!(&got[5..], &body[..], "the body did not arrive intact");
    }

    #[tokio::test]
    async fn a_discarded_body_is_still_read_off_the_wire() {
        // The swallow case. A `ParseComplete` the proxy asked for has an empty
        // body, but the rule has to hold for any length: bytes the client must
        // not see still have to leave the socket, or the next header would be
        // taken from the middle of this body.
        use tokio::io::AsyncWriteExt;

        let slab = test_slab();
        let (from_io, mut server) = tokio::io::duplex(64 * 1024);
        let mut from = Wire::new(from_io, std::sync::Arc::clone(&slab));
        let (to_io, _client) = tokio::io::duplex(64 * 1024);
        let mut to = Wire::new(to_io, slab);

        // A body to discard, then a header that must be readable afterwards.
        let writer = tokio::spawn(async move {
            server.write_all(&vec![b'x'; 8192]).await.expect("body");
            let mut next = vec![Tag::READY_FOR_QUERY.get()];
            next.extend_from_slice(&5_u32.to_be_bytes());
            next.push(b'I');
            server.write_all(&next).await.expect("the next header");
            server
        });

        stream_body(&mut from, &mut to, 8192, true)
            .await
            .expect("the body is consumed");
        drop(writer.await.expect("the writer finishes"));

        let header = from
            .read_header(pgprox_proto::frame::DEFAULT_MAX_FRAME)
            .await
            .expect("the next header decodes");
        assert_eq!(
            header.tag,
            Tag::READY_FOR_QUERY,
            "the discarded body left the wire mid-message"
        );
        assert_eq!(header.body_len, 1);
    }

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
    fn a_gate_follows_a_ceiling_that_moves() {
        // An operator raising the limit is usually doing so while the node is
        // refusing connections, which is the worst moment to need a restart.
        let gate = Arc::new(Gate::new(1));
        let first = gate.admit().expect("room for one");
        assert!(gate.admit().is_none());

        gate.set_ceiling(2);
        let second = gate.admit().expect("the raised ceiling was not read");

        // And lowering it refuses the next client rather than closing one that
        // already has a connection: taking those away is a drain, which is a
        // different thing with its own sequence.
        gate.set_ceiling(1);
        assert!(gate.admit().is_none());
        assert_eq!(gate.live(), 2, "an established client was closed");

        drop((first, second));
    }

    #[test]
    fn a_gate_of_zero_admits_nobody() {
        // Configuration validation refuses this, and the gate must not be the
        // thing that decides otherwise.
        assert!(Arc::new(Gate::new(0)).admit().is_none());
    }

    #[tokio::test]
    async fn a_client_that_says_nothing_is_closed_and_gives_its_slot_back() {
        // The cheapest denial of service there is: open the ceiling's worth of
        // sockets, send nothing, and the node is out of service with no
        // credentials and no traffic.
        let addr = fake_postgres().await;
        let mut context = context_for(addr);
        context.login_timeout = Duration::from_millis(150);
        let context = Arc::new(context);

        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();
        let (ours, _client) = tokio::io::duplex(8192);
        let held = Arc::clone(&context);

        let outcome = tokio::time::timeout(
            Duration::from_secs(5),
            session(ours, held.as_ref(), admitted),
        )
        .await
        .expect("a silent client was served forever");

        assert!(outcome.is_err());
        assert_eq!(gate.live(), 0, "its place under the ceiling was not freed");
        assert!(context.sessions.is_empty());
    }

    #[tokio::test]
    async fn a_client_that_authenticates_is_not_closed_by_the_login_timeout() {
        // The timeout ends where the client is told it is in. After that the
        // session is the client's to keep idle for as long as it likes, which
        // is what a connection pool is for.
        let addr = fake_postgres().await;
        let mut context = context_for(addr);
        context.login_timeout = Duration::from_millis(300);
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
        for _ in 0..5 {
            expect(&mut client).await;
        }

        // Well past the login timeout, doing nothing.
        tokio::time::sleep(Duration::from_millis(600)).await;

        let mut query = Vec::new();
        pgprox_proto::encode_frontend::query(&mut query, "SELECT 1");
        client.write_all(&query).await.unwrap();
        let answer = expect_answer(&mut client).await;
        assert_eq!(
            answer.first().map(|frame| frame.0),
            Some(Tag::ROW_DESCRIPTION),
            "a query with rows was answered without a description"
        );
        assert_eq!(answer[answer.len() - 2].0, Tag::COMMAND_COMPLETE);

        drop(client);
        let _ = served.await;
    }

    #[tokio::test]
    async fn a_client_the_proxy_kept_waiting_is_served_rather_than_dropped() {
        // The login deadline is for what the client owes this proxy. Once it
        // has authenticated, the waiting is the proxy's own: a grant, server
        // parameters, a connection from a pool. Ending its socket there closes
        // on a client that did everything right, with no message, which every
        // driver reports as a network fault. At a thousand connections that
        // was eight of them per run.
        let addr = fake_postgres_after(Duration::from_millis(400)).await;
        let mut context = context_for(addr);
        context.login_timeout = Duration::from_millis(150);
        let context = Arc::new(context);

        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();
        // Small on purpose. The proxy's parameter burst does not fit, so the
        // write that tells the client it is in has to wait for the client to
        // read, which is what makes an expired deadline reachable here at all:
        // a write that completes on the first poll never sees one.
        let (ours, mut client) = tokio::io::duplex(64);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();

        // The upstream takes longer than the login timeout to answer, so the
        // deadline is long past by the time this client can be told it is in.
        //
        // This test does not fail deterministically against the old code: the
        // silent drop needed the write that tells the client it is in to be
        // pending on its first poll, and over a duplex that is a race. The
        // evidence for the fix is the stack: eight of a thousand clients per
        // run were dropped this way, and none are now.
        for _ in 0..5 {
            expect(&mut client).await;
        }

        let mut query = Vec::new();
        pgprox_proto::encode_frontend::query(&mut query, "SELECT 1");
        client.write_all(&query).await.unwrap();
        let answer = expect_answer(&mut client).await;
        assert_eq!(
            answer.first().map(|frame| frame.0),
            Some(Tag::ROW_DESCRIPTION),
            "a query with rows was answered without a description"
        );
        assert_eq!(answer[answer.len() - 2].0, Tag::COMMAND_COMPLETE);

        drop(client);
        let _ = served.await;
    }

    #[tokio::test]
    async fn a_refusal_reaches_the_client_and_anything_else_passes_through() {
        // `told` is the one place a refusal from the pool or the parameter
        // fetch becomes a message on the wire. Both of its branches matter:
        // one has to write, and the other has to not invent a message for a
        // client that has already gone.
        use tokio::io::AsyncReadExt;

        let (ours, mut client) = tokio::io::duplex(4096);
        let mut wire = Wire::new(ours, test_slab());

        let refused: Result<(), ShellError> = Err(ShellError::Refused(ClientError::Draining));
        let error = told(&mut wire, refused).await.unwrap_err();
        assert!(matches!(error, ShellError::Refused(ClientError::Draining)));

        let mut header = [0_u8; 5];
        client.read_exact(&mut header).await.unwrap();
        assert_eq!(Tag(header[0]), Tag::ERROR_RESPONSE);
        let len = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
        let mut body = vec![0; len - 4];
        client.read_exact(&mut body).await.unwrap();
        assert!(
            String::from_utf8_lossy(&body).contains("57P01"),
            "the client was not told why"
        );

        // A disconnect is not a refusal: there is nobody to tell.
        let gone: Result<(), ShellError> = Err(ShellError::Disconnected);
        assert!(matches!(
            told(&mut wire, gone).await.unwrap_err(),
            ShellError::Disconnected
        ));

        // And a success passes through untouched.
        assert_eq!(told(&mut wire, Ok(7)).await.unwrap(), 7);
    }

    #[tokio::test]
    async fn one_session_costs_less_than_the_slab_buffer_it_no_longer_holds() {
        // Every connection is one spawned task holding one of these futures,
        // so its size is a per-connection cost that no buffer pool reduces.
        // It was 11,640 bytes and is 2,352. A future is the union of
        // everything alive across its awaits, and what was alive included a
        // 4 KiB stack array in `Wire::fill`, the startup negotiation, the
        // authentication exchange, and the frames of the two functions that
        // ran them, none of which a connection needs once it is serving.
        // The ceiling is 5 KiB, which the comment said was 3 for two
        // milestones after M7.50 lowered it from 6. A change that adds a
        // kilobyte fails this; one that adds a pointer does not.
        //
        // It is 5,048 bytes as `M9.23` left it, and that is 16 fewer than
        // before it, which is not luck: holding one session's state in one
        // struct costs less across an await than the same fields as eight
        // locals, and it paid for the sequence the cache holds back.
        let context = Arc::new(context_for("127.0.0.1:1".parse().unwrap()));
        let gate = Arc::new(Gate::new(1));
        let admitted = gate.admit().unwrap();
        let (ours, _theirs) = tokio::io::duplex(64);

        let future = session(ours, context.as_ref(), admitted);
        let bytes = std::mem::size_of_val(&future);
        assert!(
            bytes < 5 * 1024,
            "the session future is {bytes} bytes, so a hundred thousand of them is {} MB \
             before a single buffer, socket or registry entry",
            bytes * 100_000 / 1024 / 1024
        );
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

        refuse_full(ours, 10, test_slab()).await.unwrap();
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
        let connector = Arc::new(PgConnector::new(TcpUpstream::new(tls), test_slab()));

        Context {
            cache: None,
            slab: test_slab(),
            routes: Arc::new(crate::routes::RouteCounts::new()),
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
            login_timeout: Duration::from_secs(30),
            statics: None,
            observatory: pgprox_core::admin::FakeObservatory::new(NodeId::new(1)),
            tls: None,
            draining: crate::run::Shutdown::new(),
            closing: crate::run::Shutdown::new(),
            peers: pgprox_core::cluster::StaticPeers::new(std::collections::BTreeMap::new()),
            replicas: Arc::new(crate::replicas::ReplicaSets::new(
                TcpUpstream::new(
                    pgprox_tls::client_config(tokio_rustls::rustls::RootCertStore::empty())
                        .unwrap(),
                ),
                Arc::new(FakeClock::new()),
                crate::run::Shutdown::new(),
                test_slab(),
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
        startup_with_options(token, None)
    }

    /// The same, with runtime settings packed into `options` the way libpq
    /// packs a connection string's `options=` into the startup packet.
    fn startup_with_options(token: &str, options: Option<&str>) -> Vec<u8> {
        startup_with(token, options, &[])
    }

    /// The same, plus plain startup parameters the way libpq sends
    /// `client_encoding` and `application_name`.
    fn startup_with(token: &str, options: Option<&str>, extra: &[(&str, &str)]) -> Vec<u8> {
        let mut params = vec![("user", "acme_app"), ("database", "acme")];
        params.extend_from_slice(extra);
        if let Some(options) = options {
            params.push(("options", options));
        }
        let mut out = Vec::new();
        pgprox_proto::encode_frontend::startup_message(
            &mut out,
            pgprox_proto::encode::PROTOCOL_3_0,
            &params,
        );
        pgprox_proto::encode_frontend::password_message(&mut out, token);
        out
    }

    /// A context with a cache in it, and the cache, so a test can look inside.
    use pgprox_core::cache::{FakeQueryCache, QueryCache};

    fn context_with_cache(addr: SocketAddr) -> (Context, Arc<FakeQueryCache>) {
        let cache = FakeQueryCache::new();
        let mut context = context_for(addr);
        context.cache = Some(cache.clone());
        (context, cache)
    }

    /// A context with the real store in it, configured for `tenants`.
    ///
    /// The fake serves everybody, which is right for the tests that are about
    /// the relay and wrong for the ones that are about who the cache is for.
    /// Its clock comes back so a test can walk past a TTL.
    fn context_with_store(
        addr: SocketAddr,
        tenants: &[&str],
        ttl: Duration,
    ) -> (Context, Arc<pgprox_cache::Store>, Arc<FakeClock>) {
        use pgprox_core::config::{QueryCacheConfig, TenantCache};

        let clock = Arc::new(FakeClock::new());
        let store = pgprox_cache::Store::new(clock.clone());
        store.reconfigure(&QueryCacheConfig {
            max_bytes: 1024 * 1024,
            ttl_cap: Duration::from_secs(60),
            tenants: tenants
                .iter()
                .map(|name| (pgprox_core::ids::TenantId::new(name), TenantCache { ttl }))
                .collect(),
        });

        let mut context = context_for(addr);
        context.cache = Some(store.clone());
        (context, store, clock)
    }

    /// An entry for `acme`, so a test can watch it disappear.
    fn seed(cache: &Arc<FakeQueryCache>) -> pgprox_core::cache::CacheKey {
        let key = pgprox_core::cache::CacheKey {
            tenant: pgprox_core::ids::TenantId::new("acme"),
            normalized_sql: Arc::from("select 1"),
            params: Arc::from(&[][..]),
            search_path: Arc::from("public"),
        };
        let value = pgprox_core::cache::CachedResult {
            frames: Arc::from([0_u8; 4].as_slice()),
            ttl: Duration::from_secs(60),
        };
        futures_lite_block_on(cache.put(key.clone(), value));
        key
    }

    /// Runs a future to completion on the current thread.
    ///
    /// The fake cache's methods are async because the trait is, and these two
    /// helpers are called from synchronous setup. Nothing here yields.
    fn futures_lite_block_on<F: std::future::Future>(future: F) -> F::Output {
        use std::task::{Context as TaskContext, Poll, Waker};
        let mut future = Box::pin(future);
        let mut cx = TaskContext::from_waker(Waker::noop());
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("the fake cache yielded, which it must not"),
        }
    }

    /// Drives a session far enough to have authenticated, then runs `sql`.
    async fn query_through_a_session(context: Arc<Context>, sql: &str) {
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();
        let (ours, mut client) = tokio::io::duplex(8192);
        let served = tokio::spawn(async move { session(ours, context.as_ref(), admitted).await });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut client).await;
        }

        let mut out = Vec::new();
        pgprox_proto::encode_frontend::query(&mut out, sql);
        client.write_all(&out).await.unwrap();
        expect(&mut client).await;

        drop(client);
        let _ = served.await;
    }

    /// Reads one query's whole answer, up to and including its `ReadyForQuery`.
    ///
    /// By terminator rather than by a frame count, because how many frames a
    /// query's answer has is the server's business: a `SELECT` carries a row
    /// description and an `UPDATE` does not, and a test that counted them was
    /// asserting the shape of the fake rather than the behaviour under test.
    async fn expect_answer<S: AsyncRead + Unpin>(io: &mut S) -> Vec<(Tag, Vec<u8>)> {
        let mut frames = Vec::new();
        loop {
            let frame = expect(io).await;
            let last = frame.0 == Tag::READY_FOR_QUERY;
            frames.push(frame);
            if last {
                return frames;
            }
        }
    }

    /// Runs `sql` through a session and returns what the client was sent back.
    async fn query_and_collect(context: Arc<Context>, sql: &str) -> Vec<(Tag, Vec<u8>)> {
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();
        let (ours, mut client) = tokio::io::duplex(8192);
        let served = tokio::spawn(async move { session(ours, context.as_ref(), admitted).await });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut client).await;
        }

        let mut out = Vec::new();
        pgprox_proto::encode_frontend::query(&mut out, sql);
        client.write_all(&out).await.unwrap();

        let got = expect_answer(&mut client).await;

        drop(client);
        let _ = served.await;
        got
    }

    #[tokio::test]
    async fn a_hit_is_served_without_taking_anything_from_the_pool() {
        // The property the whole milestone is for. M7.56 found 45% of this
        // proxy's CPU in the pool's lock, with the cost landing per connection
        // because contention tracks how many are queued. A statement answered
        // here never queues.
        let addr = fake_postgres().await;
        let (context, cache) = context_with_cache(addr);
        let context = Arc::new(context);

        // First time through, from the server: a description, a completion and
        // a ReadyForQuery, which is what a real server answers a `SELECT` with.
        let first = query_and_collect(Arc::clone(&context), "SELECT 1").await;
        assert_eq!(first[0].0, Tag::ROW_DESCRIPTION);
        assert_eq!(cache.len(), 1, "the answer was not stored");

        let before = statements_seen(addr).len();

        // Second time, from the cache: the same bytes, and the fake server
        // never hears about it.
        let second = query_and_collect(Arc::clone(&context), "SELECT 1").await;
        assert_eq!(
            second, first,
            "the cached answer differed from the stored one"
        );
        assert_eq!(
            statements_seen(addr).len(),
            before,
            "a hit still sent the statement upstream"
        );

        // And it is still a statement. A hit that went uncounted makes every
        // ratio built on this counter wrong in the direction that flatters the
        // cache: the statements it served best are the ones missing from the
        // denominator, which is what `M9.16` was opened for.
        assert_eq!(
            context.routes.cache(),
            1,
            "a hit was answered and not counted"
        );
        assert_eq!(
            context.routes.primary(),
            1,
            "the miss that filled the cache was not counted as a primary statement"
        );
    }

    #[tokio::test]
    async fn what_is_stored_is_the_statements_answer_and_not_the_sessions() {
        // ADR 0022's payload shape. A `ReadyForQuery` answers the client's
        // framing rather than its question: the simple protocol gets one per
        // statement, an extended sequence gets one per `Sync`, and an entry
        // carrying one could only ever serve the protocol that filled it.
        let addr = fake_postgres().await;
        let (context, cache) = context_with_cache(addr);
        let context = Arc::new(context);
        query_and_collect(Arc::clone(&context), "SELECT 1").await;

        let key = pgprox_core::cache::CacheKey {
            tenant: pgprox_core::ids::TenantId::new("acme"),
            normalized_sql: Arc::from("select 1"),
            params: Arc::from(&[][..]),
            // Never set, so the server's own default, which every session on
            // this tenant shares.
            search_path: Arc::from(""),
        };
        let stored = futures_lite_block_on(cache.get(&key)).expect("nothing was stored");

        // What the fake answers a query with, and nothing else: the description
        // and the completion, and no `ReadyForQuery`.
        let text = b"SELECT 1\0";
        let mut want = row_description();
        want.push(Tag::COMMAND_COMPLETE.get());
        want.extend_from_slice(&u32::try_from(text.len() + 4).unwrap().to_be_bytes());
        want.extend_from_slice(text);

        assert_eq!(
            stored.frames.as_ref(),
            want.as_slice(),
            "the stored payload is not the statement's answer alone"
        );
    }

    #[tokio::test]
    async fn a_read_inside_a_transaction_is_neither_served_nor_stored() {
        // Two sessions rather than one, because a `BEGIN` drops the tenant's
        // entries on its way past: the entry this session could be wrongly
        // served has to be stored by somebody else after that.
        //
        // What the bug looked like. A cached answer ends in the transaction
        // status the server sent when it was recorded, which for an entry
        // stored by an idle session is `I`. Served to a session with a
        // transaction open, that tells the client its transaction ended while
        // the proxy goes on holding a connection with an open one on it.
        let addr = fake_postgres().await;
        let (context, cache) = context_with_cache(addr);
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
        for _ in 0..5 {
            expect(&mut client).await;
        }

        let mut out = Vec::new();
        pgprox_proto::encode_frontend::query(&mut out, "BEGIN");
        client.write_all(&out).await.unwrap();
        expect(&mut client).await;
        assert_eq!(
            expect(&mut client).await.1,
            vec![b'T'],
            "the fake did not open a transaction"
        );

        // Somebody else fills the cache while that transaction is open.
        query_through_a_session(Arc::clone(&context), "SELECT 1").await;
        assert_eq!(cache.len(), 1, "the other session stored nothing");

        let before = statements_seen(addr).len();
        let mut out = Vec::new();
        pgprox_proto::encode_frontend::query(&mut out, "SELECT 1");
        client.write_all(&out).await.unwrap();
        let answer = expect_answer(&mut client).await;
        assert_eq!(
            answer.last().map(|frame| frame.1.clone()),
            Some(vec![b'T']),
            "a hit told the client its transaction had ended"
        );
        assert!(
            statements_seen(addr).len() > before,
            "a read inside a transaction was answered from the cache"
        );

        // And nothing new was stored: its answer is visible only to this
        // transaction, and it ends in `T` rather than `I`.
        assert_eq!(
            cache.len(),
            1,
            "a read inside a transaction was stored for everyone else"
        );

        drop(client);
        let _ = served.await;
    }

    #[tokio::test]
    async fn a_tenant_nobody_opted_in_for_is_never_cached_for() {
        // ADR 0021's first consequence, at the layer a client would notice it.
        // Every node has a store now, so this is what makes "off by default"
        // true: the store is there and it serves nobody.
        let addr = fake_postgres().await;
        let (context, store, _clock) = context_with_store(addr, &[], Duration::from_secs(5));
        let context = Arc::new(context);

        query_and_collect(Arc::clone(&context), "SELECT 1").await;
        let after_first = statements_seen(addr).len();
        query_and_collect(Arc::clone(&context), "SELECT 1").await;

        assert_eq!(store.stats().entries, 0, "a result was stored for nobody");
        assert!(
            statements_seen(addr).len() > after_first,
            "the second statement was answered from a cache serving nobody"
        );
    }

    #[tokio::test]
    async fn an_entry_lives_for_the_configured_ttl_rather_than_the_grants() {
        // The relay has no opinion about staleness and says so. It used to
        // pass the grant's TTL, which is how long a credential may be trusted:
        // a different question that happens to have the same units.
        let addr = fake_postgres().await;
        let (context, store, clock) = context_with_store(addr, &["acme"], Duration::from_secs(5));
        let context = Arc::new(context);

        query_and_collect(Arc::clone(&context), "SELECT 1").await;
        assert_eq!(store.stats().entries, 1, "the answer was not stored");
        let after_first = statements_seen(addr).len();

        // Inside the TTL, the server hears nothing.
        query_and_collect(Arc::clone(&context), "SELECT 1").await;
        assert_eq!(statements_seen(addr).len(), after_first);
        assert_eq!(store.stats().hits, 1);

        // Past it, the statement goes upstream again. Whatever the grant said,
        // this is the number the document chose.
        clock.advance(Duration::from_secs(5));
        query_and_collect(Arc::clone(&context), "SELECT 1").await;
        assert!(
            statements_seen(addr).len() > after_first,
            "an entry outlived the configured TTL"
        );
        assert_eq!(store.stats().expired, 1);
    }

    #[tokio::test]
    async fn a_hit_survives_a_difference_in_layout_and_case() {
        // What normalisation buys. Two clients asking the same question in
        // different words share an entry.
        let addr = fake_postgres().await;
        let (context, _cache) = context_with_cache(addr);
        let context = Arc::new(context);

        query_and_collect(Arc::clone(&context), "SELECT 1").await;
        let before = statements_seen(addr).len();
        query_and_collect(Arc::clone(&context), "select   1").await;

        assert_eq!(
            statements_seen(addr).len(),
            before,
            "a differently spelled statement missed"
        );
    }

    #[tokio::test]
    async fn a_statement_the_rule_refuses_is_never_stored() {
        // Read-only, replica-safe, and never the same answer twice. The
        // cacheability rule is what keeps it out, and without that check this
        // would be stored and served.
        let addr = fake_postgres().await;
        let (context, cache) = context_with_cache(addr);

        query_and_collect(Arc::new(context), "SELECT random()").await;

        assert_eq!(cache.len(), 0, "a volatile statement was cached");
    }

    #[tokio::test]
    async fn a_write_through_the_relay_drops_the_tenants_entries() {
        // ADR 0021's improvement on the TTL bound, and the half that has to
        // land before anything is ever served: a cache that could serve but
        // never invalidate would hand out data this node watched go stale.
        let addr = fake_postgres().await;
        let (context, cache) = context_with_cache(addr);
        let key = seed(&cache);
        assert_eq!(cache.len(), 1);

        query_through_a_session(Arc::new(context), "UPDATE t SET x = 1").await;

        assert!(
            futures_lite_block_on(cache.get(&key)).is_none(),
            "a write left the tenant's entries in place"
        );
    }

    #[tokio::test]
    async fn a_read_through_the_relay_leaves_the_cache_alone() {
        // The other half of the same claim. If every statement invalidated,
        // the test above would pass for a cache that was simply never used.
        let addr = fake_postgres().await;
        let (context, cache) = context_with_cache(addr);
        let key = seed(&cache);

        query_through_a_session(Arc::new(context), "SELECT 1").await;

        assert!(
            futures_lite_block_on(cache.get(&key)).is_some(),
            "a read invalidated the cache"
        );
    }

    #[tokio::test]
    async fn a_write_that_rolls_back_still_invalidates() {
        // Deliberately conservative, and worth a test so it is deliberate.
        // Waiting for the commit would buy a better hit rate and would mean
        // detecting a commit correctly on every path; getting that wrong means
        // not invalidating when we should, which is the unsafe direction.
        // Throwing entries away for a transaction that changed nothing costs a
        // miss.
        let addr = fake_postgres().await;
        let (context, cache) = context_with_cache(addr);
        let key = seed(&cache);

        let context = Arc::new(context);
        query_through_a_session(Arc::clone(&context), "BEGIN; DELETE FROM t; ROLLBACK").await;

        assert!(
            futures_lite_block_on(cache.get(&key)).is_none(),
            "a rolled-back write left the entries in place"
        );
    }

    #[tokio::test]
    async fn a_node_with_no_cache_serves_a_write_without_noticing() {
        // The default, and the guard that keeps the feature free for a node
        // that never asked for it: with no cache there is no classification
        // either.
        let addr = fake_postgres().await;
        let context = context_for(addr);
        assert!(context.cache.is_none(), "the default is not off");

        query_through_a_session(Arc::new(context), "UPDATE t SET x = 1").await;
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

        // A row description first, as a real server sends for a query with
        // rows.
        assert_eq!(expect(&mut client).await.0, Tag::ROW_DESCRIPTION);
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
            test_slab(),
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
    async fn a_statement_is_prepared_once_per_connection_rather_than_once_per_bind() {
        // The whole point of a prepared statement. The connection carries its
        // own record of what it holds, so the second transaction to bind the
        // same SQL sends no extra messages.
        let addr = fake_postgres().await;
        let context = Arc::new(context_for(addr));
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(16384);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut client).await;
        }

        let mut first = Vec::new();
        pgprox_proto::encode_frontend::parse(&mut first, "s1", "SELECT $1");
        pgprox_proto::encode_frontend::bind(&mut first, "", "s1");
        pgprox_proto::encode_frontend::sync(&mut first);
        client.write_all(&first).await.unwrap();
        expect(&mut client).await;
        expect(&mut client).await;

        let after_first = statements_seen(addr).len();
        assert!(
            after_first > 0,
            "nothing reached the server for the first statement"
        );

        // The same statement again, on a connection that has now seen it.
        let mut again = Vec::new();
        pgprox_proto::encode_frontend::bind(&mut again, "", "s1");
        pgprox_proto::encode_frontend::sync(&mut again);
        client.write_all(&again).await.unwrap();
        expect(&mut client).await;
        expect(&mut client).await;

        // Counting `Parse`s rather than frames: the second `Bind` and its
        // `Sync` are new frames and are supposed to be.
        let parses = |from: usize| {
            statements_seen(addr)
                .into_iter()
                .skip(from)
                .filter(|sql| sql.contains("SELECT $1"))
                .count()
        };
        assert_eq!(
            parses(after_first),
            0,
            "the statement was prepared again on a connection that already held it: {:?}",
            statements_seen(addr)
        );

        drop(client);
        let _ = served.await;
    }

    /// A fake upstream that speaks the extended protocol the way Postgres does.
    ///
    /// The one property that matters: it sends no `ReadyForQuery` until it
    /// sees a `Sync`. A `Flush` gets nothing of its own, because there is no
    /// message meaning "that was all". `fake_postgres` answers every frame as
    /// though it were a simple query, which is enough for the rest of the
    /// suite and cannot show this.
    /// One frame's answer from the fake extended-protocol server.
    ///
    /// Split out of the accept loop, which clippy holds to a hundred lines.
    /// `parsed` is what this connection has been asked to prepare, by name.
    fn extended_answer(
        tag: u8,
        body: &[u8],
        parsed: &mut std::collections::BTreeSet<String>,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        let completion = |out: &mut Vec<u8>| {
            out.push(Tag::COMMAND_COMPLETE.get());
            let text = b"SELECT 1\0";
            out.extend_from_slice(&u32::try_from(text.len() + 4).unwrap().to_be_bytes());
            out.extend_from_slice(text);
        };

        // The position the proxy asks the primary for after a write, before it
        // lets the connection go. A fake that answered it with a bare completion
        // left every session that had written looking like one that still had:
        // `relay.wrote()` never cleared, so nothing after a write was cacheable
        // and `M9.26`'s test could not reach the path it was written for.
        if tag == b'Q' && String::from_utf8_lossy(body).contains("pg_current_wal_insert_lsn") {
            out.extend_from_slice(&text_row(&[Some(PRIMARY_WRITTEN)]));
            encode::ready_for_query(&mut out, TxStatus::Idle);
            return out;
        }

        match tag {
            // Parse, answered with ParseComplete unless this connection already
            // holds the name. Postgres refuses that with 42P05, and a fake that
            // accepted it let the desync `M9.24` found pass every test in this
            // file: the proxy's record of what a connection holds is only
            // correct if something notices when it is not.
            b'P' => {
                let name = String::from_utf8_lossy(
                    &body[..body.iter().position(|b| *b == 0).unwrap_or(0)],
                )
                .into_owned();
                if parsed.contains(&name) {
                    let text = format!(
                        "SERROR\0C42P05\0Mprepared statement \"{name}\" already exists\0\0"
                    );
                    out.push(Tag::ERROR_RESPONSE.get());
                    out.extend_from_slice(&u32::try_from(text.len() + 4).unwrap().to_be_bytes());
                    out.extend_from_slice(text.as_bytes());
                } else {
                    parsed.insert(name);
                    out.extend_from_slice(&[b'1', 0, 0, 0, 4]);
                }
            }
            // Describe of a statement: the parameters it takes, then the row it
            // returns. Two messages, one of which ends the exchange.
            b'D' => {
                out.extend_from_slice(&[b't', 0, 0, 0, 6, 0, 0]);
                out.extend_from_slice(&[b'n', 0, 0, 0, 4]);
            }
            // Close of a statement. The server deallocates it, so this
            // connection no longer holds it and a later `Bind` naming it must
            // fail. Modelled because the proxy's record of what a connection
            // holds is only correct if something notices when it is not, which
            // is the argument the `42P05` arm above already makes for `Parse`.
            b'C' if body.first() == Some(&b'S') => {
                // The target byte, then the name.
                let rest = &body[1..];
                let name = String::from_utf8_lossy(
                    &rest[..rest.iter().position(|b| *b == 0).unwrap_or(0)],
                )
                .into_owned();
                parsed.remove(&name);
                out.extend_from_slice(&[b'3', 0, 0, 0, 4]);
            }
            b'B' => {
                // The statement name is the second string: the portal comes
                // first. A `Bind` naming a statement this connection does not
                // hold is `26000`, which is what Postgres answers and what a
                // proxy that lost track of a `Close` would provoke.
                let after_portal = body.iter().position(|b| *b == 0).map_or(0, |at| at + 1);
                let rest = &body[after_portal..];
                let name = String::from_utf8_lossy(
                    &rest[..rest.iter().position(|b| *b == 0).unwrap_or(0)],
                )
                .into_owned();
                if name.is_empty() || parsed.contains(&name) {
                    out.extend_from_slice(&[b'2', 0, 0, 0, 4]);
                } else {
                    let text = format!(
                        "SERROR\0C26000\0Mprepared statement \"{name}\" does not exist\0\0"
                    );
                    out.push(Tag::ERROR_RESPONSE.get());
                    out.extend_from_slice(&u32::try_from(text.len() + 4).unwrap().to_be_bytes());
                    out.extend_from_slice(text.as_bytes());
                }
            }
            b'E' => completion(&mut out),
            // Sync, and only Sync, produces a ReadyForQuery.
            b'S' => encode::ready_for_query(&mut out, TxStatus::Idle),
            // Flush. Postgres pushes out what it has and says nothing else,
            // which is what an empty buffer does here.
            b'H' => {}
            // A simple query, which a real server answers with a description
            // for anything that returns rows.
            _ => {
                if String::from_utf8_lossy(body)
                    .trim_start()
                    .to_uppercase()
                    .starts_with("SELECT")
                {
                    out.extend_from_slice(&row_description());
                }
                completion(&mut out);
                encode::ready_for_query(&mut out, TxStatus::Idle);
            }
        }
        out
    }

    async fn fake_postgres_extended() -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
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

                    // What this connection has been asked to prepare, by name.
                    let mut parsed: std::collections::BTreeSet<String> =
                        std::collections::BTreeSet::new();
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

                        // One entry per frame, by tag, so a test can see what
                        // reached the server. A held sequence that was answered
                        // from the cache leaves nothing here at all, which is
                        // the property `M9.23` is about.
                        crate::fakepg::record(addr, (header[0] as char).to_string());

                        let out = extended_answer(header[0], &body, &mut parsed);
                        if !out.is_empty() && socket.write_all(&out).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });

        addr
    }

    /// An authenticated client on a session of its own, for tests that send
    /// extended-protocol frames by hand.
    ///
    /// The gate lives inside the admitted guard, so the caller keeps only the
    /// two ends it needs: the client's socket and the session's task.
    async fn extended_client(
        context: &Arc<Context>,
    ) -> (tokio::io::DuplexStream, tokio::task::JoinHandle<()>) {
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();
        let (ours, mut client) = tokio::io::duplex(8192);
        let held = Arc::clone(context);
        let served = tokio::spawn(async move {
            let _ = session(ours, held.as_ref(), admitted).await;
        });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut client).await;
        }
        (client, served)
    }

    /// `Parse`, `Bind`, `Execute`, `Sync`: what a driver sends the first time it
    /// runs a parameterised statement.
    fn one_binding(sql: &str, value: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        pgprox_proto::encode_frontend::parse(&mut out, "s1", sql);
        pgprox_proto::encode_frontend::bind_with_parameters(&mut out, "", "s1", &[Some(value)]);
        pgprox_proto::encode_frontend::execute(&mut out, "");
        pgprox_proto::encode_frontend::sync(&mut out);
        out
    }

    /// `Bind`, `Execute`, `Sync`: what a driver sends for a statement it has
    /// already parsed on this connection, which is every run after the first.
    ///
    /// The shape `M9.26` was wrong about. It carries no `Parse`, so nothing in
    /// the sequence names the statement by the client's own name, and a replay
    /// that could not read the name back had nothing to fall back on.
    fn a_binding(value: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        pgprox_proto::encode_frontend::bind_with_parameters(&mut out, "", "s1", &[Some(value)]);
        pgprox_proto::encode_frontend::execute(&mut out, "");
        pgprox_proto::encode_frontend::sync(&mut out);
        out
    }

    /// The four frames the client is owed for [`one_binding`].
    async fn four_answers(client: &mut tokio::io::DuplexStream) -> Vec<Tag> {
        let mut tags = Vec::new();
        for _ in 0..4 {
            tags.push(
                tokio::time::timeout(Duration::from_secs(5), expect(client))
                    .await
                    .expect("the sequence was never answered")
                    .0,
            );
        }
        tags
    }

    #[test]
    fn an_unnamed_parse_keeps_its_name_and_a_named_one_does_not() {
        // `M20.6`, and the assertion the end-to-end test below cannot make: it
        // is about which name left this process, and both behaviours produce a
        // working sequence.
        let mut session = pgprox_session::resume::SessionMemory::default();

        let mut named = Vec::new();
        pgprox_proto::encode_frontend::parse(&mut named, "s1", "SELECT $1");
        let body = named[5..].to_vec();
        let frame = Frame::new(Tag::PARSE, &body);
        let message = pgprox_proto::frontend::decode(&frame).unwrap();
        let mapped = map_statement_name(&message, &body, &mut session).unwrap();
        assert!(
            mapped.starts_with(b"pgprox_"),
            "a named statement was not rewritten: {:?}",
            String::from_utf8_lossy(&mapped)
        );

        // The unnamed one goes as it arrived. Renaming it would make it a named
        // statement, which persists until it is closed, occupies a slot under
        // `per_connection_cap` and costs a `Close` round trip to evict.
        let mut unnamed = Vec::new();
        pgprox_proto::encode_frontend::parse(&mut unnamed, "", "SELECT $1");
        let body = unnamed[5..].to_vec();
        let frame = Frame::new(Tag::PARSE, &body);
        let message = pgprox_proto::frontend::decode(&frame).unwrap();
        let mapped = map_statement_name(&message, &body, &mut session).unwrap();
        assert_eq!(
            mapped, body,
            "the unnamed statement was given a name, which makes it a different statement"
        );

        // And the session still knows its SQL, which is what lets a `Bind` of
        // it be re-parsed on a connection that has something else unnamed.
        assert_eq!(
            session.statements.get("").map(|held| held.sql.as_str()),
            Some("SELECT $1")
        );
    }

    #[test]
    fn a_bind_of_the_unnamed_statement_keeps_its_name_too() {
        let mut session = pgprox_session::resume::SessionMemory::default();
        session.statements.parse("", "SELECT $1");

        let mut out = Vec::new();
        pgprox_proto::encode_frontend::bind(&mut out, "", "");
        let body = out[5..].to_vec();
        let frame = Frame::new(Tag::BIND, &body);
        let message = pgprox_proto::frontend::decode(&frame).unwrap();

        assert_eq!(
            map_statement_name(&message, &body, &mut session).unwrap(),
            body,
            "a bind of the unnamed statement was pointed at a global name"
        );
    }

    #[tokio::test]
    async fn the_unnamed_statement_stays_unnamed_on_the_wire() {
        // `M20.6`. The unnamed statement and a named one are different things.
        // The unnamed one is replaced by the next `Parse` of it and does not
        // survive a `Close`; a named one persists until it is closed. Renaming
        // it made every one-shot query a driver sent through it a permanent
        // entry under `per_connection_cap`, evicting real statements and paying
        // a `Close` round trip to do it. pgcat excludes anonymous statements
        // from its own rewriting for the same reason.
        let addr = fake_postgres_extended().await;
        let context = Arc::new(context_for(addr));
        let (mut client, served) = extended_client(&context).await;

        let mut out = Vec::new();
        pgprox_proto::encode_frontend::parse(&mut out, "", "SELECT $1");
        pgprox_proto::encode_frontend::bind_with_parameters(
            &mut out,
            "",
            "",
            &[Some(b"1".as_slice())],
        );
        pgprox_proto::encode_frontend::execute(&mut out, "");
        pgprox_proto::encode_frontend::sync(&mut out);
        client.write_all(&out).await.unwrap();

        let mut saw = Vec::new();
        loop {
            let (tag, body) = expect(&mut client).await;
            saw.push((tag, String::from_utf8_lossy(&body).into_owned()));
            if tag == Tag::READY_FOR_QUERY {
                break;
            }
        }
        assert!(
            !saw.iter().any(|(tag, _)| *tag == Tag::ERROR_RESPONSE),
            "the unnamed sequence failed: {saw:?}"
        );

        drop(client);
        let _ = served.await;
    }

    #[tokio::test]
    async fn a_statement_the_client_closed_is_prepared_again_before_the_next_bind() {
        // A driver rotating its statement cache sends `Close` and then re-uses
        // the name. The `Close` is rewritten to this proxy's global name and
        // forwarded, so the server deallocates it; if nothing tells the
        // connection's record that, the next `Bind` names a statement the
        // server no longer has.
        let addr = fake_postgres_extended().await;
        let context = Arc::new(context_for(addr));
        let (mut client, served) = extended_client(&context).await;

        let mut first = Vec::new();
        pgprox_proto::encode_frontend::parse(&mut first, "s1", "SELECT $1");
        pgprox_proto::encode_frontend::bind_with_parameters(
            &mut first,
            "",
            "s1",
            &[Some(b"1".as_slice())],
        );
        pgprox_proto::encode_frontend::execute(&mut first, "");
        pgprox_proto::encode_frontend::sync(&mut first);
        client.write_all(&first).await.unwrap();
        loop {
            if expect(&mut client).await.0 == Tag::READY_FOR_QUERY {
                break;
            }
        }

        // The client gives the name back.
        let mut closing = Vec::new();
        pgprox_proto::encode_frontend::close_statement(&mut closing, "s1");
        pgprox_proto::encode_frontend::sync(&mut closing);
        client.write_all(&closing).await.unwrap();
        loop {
            if expect(&mut client).await.0 == Tag::READY_FOR_QUERY {
                break;
            }
        }

        // And prepares it again, which is the whole point of having closed it.
        let mut again = Vec::new();
        pgprox_proto::encode_frontend::parse(&mut again, "s1", "SELECT $1");
        pgprox_proto::encode_frontend::bind_with_parameters(
            &mut again,
            "",
            "s1",
            &[Some(b"1".as_slice())],
        );
        pgprox_proto::encode_frontend::execute(&mut again, "");
        pgprox_proto::encode_frontend::sync(&mut again);
        client.write_all(&again).await.unwrap();

        let mut saw = Vec::new();
        loop {
            let (tag, body) = expect(&mut client).await;
            saw.push((tag, String::from_utf8_lossy(&body).into_owned()));
            if tag == Tag::READY_FOR_QUERY {
                break;
            }
        }
        assert!(
            !saw.iter()
                .any(|(tag, body)| *tag == Tag::ERROR_RESPONSE && body.contains("26000")),
            "the bind named a statement the server had deallocated: {saw:?}"
        );

        drop(client);
        let _ = served.await;
    }

    #[tokio::test]
    async fn two_bindings_of_one_statement_are_served_separately() {
        // The acceptance criterion `M9.17` was opened for. Until the parameters
        // reached the key, `SELECT $1` with 1 and with 2 shared an entry, which
        // is one question answered with another's rows.
        let addr = fake_postgres_extended().await;
        let (context, cache) = context_with_cache(addr);
        let context = Arc::new(context);
        let (mut client, served) = extended_client(&context).await;

        // A miss: the whole sequence goes upstream and its answer is stored.
        client
            .write_all(&one_binding("SELECT $1", b"1"))
            .await
            .unwrap();
        assert_eq!(
            four_answers(&mut client).await,
            vec![
                Tag(b'1'),
                Tag(b'2'),
                Tag::COMMAND_COMPLETE,
                Tag::READY_FOR_QUERY
            ]
        );
        assert_eq!(cache.len(), 1, "the sequence's answer was not stored");

        // The same binding again: answered here, and the server never hears it.
        let before = statements_seen(addr).len();
        client
            .write_all(&one_binding("SELECT $1", b"1"))
            .await
            .unwrap();
        assert_eq!(
            four_answers(&mut client).await,
            vec![
                Tag(b'1'),
                Tag(b'2'),
                Tag::COMMAND_COMPLETE,
                Tag::READY_FOR_QUERY
            ],
            "the assembled hit is not the answer the client was owed"
        );
        assert_eq!(context.routes.cache(), 1, "the hit was not counted");
        assert_eq!(
            statements_seen(addr).len(),
            before,
            "a hit still sent the sequence upstream"
        );

        // A different binding is a different question, so it goes upstream and
        // gets an entry of its own rather than the first one's rows.
        client
            .write_all(&one_binding("SELECT $1", b"2"))
            .await
            .unwrap();
        four_answers(&mut client).await;
        assert_eq!(
            context.routes.cache(),
            1,
            "a second binding was served the first's answer"
        );
        assert_eq!(cache.len(), 2, "two bindings shared one entry");

        drop(client);
        let _ = served.await;
    }

    #[tokio::test]
    async fn a_replayed_sequence_leaves_the_connection_recorded_correctly() {
        // What `M9.24`'s first run found, at 177 errors in fifteen seconds.
        //
        // A held frame was stored after the statement-name rewrite, so the
        // replay decoded a `Parse` whose name was already this proxy's own.
        // `statement_of` looks that name up in a map keyed by the client's name,
        // found nothing, and the connection's record of what it holds was never
        // updated. The server held the statement and the proxy believed it did
        // not, so the next `Bind` on that connection prepared it again:
        // `prepared statement "pgprox_..." already exists`, and after that
        // `does not exist` as the two sides diverged further.
        //
        // The second sequence is the assertion. It only reaches the server
        // because the write in between drops the entry the first one stored.
        let addr = fake_postgres_extended().await;
        let (context, _cache) = context_with_cache(addr);
        let context = Arc::new(context);
        let (mut client, served) = extended_client(&context).await;

        client
            .write_all(&one_binding("SELECT $1", b"1"))
            .await
            .unwrap();
        assert_eq!(
            four_answers(&mut client).await,
            vec![
                Tag(b'1'),
                Tag(b'2'),
                Tag::COMMAND_COMPLETE,
                Tag::READY_FOR_QUERY
            ]
        );

        // A write, which drops what the read stored.
        let mut write = Vec::new();
        pgprox_proto::encode_frontend::query(&mut write, "UPDATE t SET a = 1");
        client.write_all(&write).await.unwrap();
        for _ in 0..2 {
            expect(&mut client).await;
        }

        // The same sequence again, now a miss, and it is replayed onto the same
        // connection the first one prepared the statement on.
        client
            .write_all(&one_binding("SELECT $1", b"1"))
            .await
            .unwrap();
        assert_eq!(
            four_answers(&mut client).await,
            vec![
                Tag(b'1'),
                Tag(b'2'),
                Tag::COMMAND_COMPLETE,
                Tag::READY_FOR_QUERY
            ],
            "the replayed statement was prepared twice on one connection"
        );

        // And the shape with no `Parse` in it at all, which is every run after
        // a driver's first: another write to drop the entry, then a `Bind` for
        // the statement this session parsed earlier. Nothing in this sequence
        // carries the client's name for it, so a replay that reads the name out
        // of the frames it holds has to have held the client's own bytes.
        client.write_all(&write).await.unwrap();
        for _ in 0..2 {
            expect(&mut client).await;
        }

        client.write_all(&a_binding(b"1")).await.unwrap();
        let mut tags = Vec::new();
        for _ in 0..3 {
            tags.push(
                tokio::time::timeout(Duration::from_secs(5), expect(&mut client))
                    .await
                    .expect("the second binding was never answered")
                    .0,
            );
        }
        assert_eq!(
            tags,
            vec![Tag(b'2'), Tag::COMMAND_COMPLETE, Tag::READY_FOR_QUERY],
            "a sequence with no Parse in it could not be replayed"
        );

        drop(client);
        let _ = served.await;
    }

    #[tokio::test]
    async fn a_sequence_with_no_parse_in_it_can_still_be_replayed() {
        // `M9.26`, and the ordering is the whole test.
        //
        // A held frame was stored after the statement-name rewrite, so a replay
        // decoded a `Bind` naming this proxy's own global name and looked it up
        // in a map keyed by the client's name. It only shows when the sequence
        // that would have inserted an alias never ran: a hit replays nothing, so
        // the next sequence is the first thing to reach the server, and a
        // `Bind`, `Execute`, `Sync` is what every driver sends once it believes
        // the statement is prepared.
        //
        // The simple query is how the entry gets there without a replay: `M9.22`
        // made both protocols store the same payload, so a `Bind` with nothing
        // bound has the same key as the simple query of the same SQL.
        let addr = fake_postgres_extended().await;
        let (context, cache) = context_with_cache(addr);
        let context = Arc::new(context);
        let (mut client, served) = extended_client(&context).await;

        let mut simple = Vec::new();
        pgprox_proto::encode_frontend::query(&mut simple, "SELECT 1");
        client.write_all(&simple).await.unwrap();
        expect_answer(&mut client).await;
        assert_eq!(cache.len(), 1, "the simple query stored nothing");

        // The extended sequence for the same statement: a hit, so nothing is
        // replayed and nothing upstream learns the name.
        let mut prepared = Vec::new();
        pgprox_proto::encode_frontend::parse(&mut prepared, "s1", "SELECT 1");
        pgprox_proto::encode_frontend::bind(&mut prepared, "", "s1");
        pgprox_proto::encode_frontend::execute(&mut prepared, "");
        pgprox_proto::encode_frontend::sync(&mut prepared);
        client.write_all(&prepared).await.unwrap();
        assert_eq!(
            four_answers(&mut client).await,
            vec![
                Tag(b'1'),
                Tag(b'2'),
                Tag::COMMAND_COMPLETE,
                Tag::READY_FOR_QUERY
            ]
        );
        assert_eq!(context.routes.cache(), 1, "the sequence was not a hit");

        // A write drops it, and now the `Bind` has to go upstream.
        let mut write = Vec::new();
        pgprox_proto::encode_frontend::query(&mut write, "UPDATE t SET a = 1");
        client.write_all(&write).await.unwrap();
        for _ in 0..2 {
            expect(&mut client).await;
        }

        let mut rebind = Vec::new();
        pgprox_proto::encode_frontend::bind(&mut rebind, "", "s1");
        pgprox_proto::encode_frontend::execute(&mut rebind, "");
        pgprox_proto::encode_frontend::sync(&mut rebind);
        client.write_all(&rebind).await.unwrap();

        let mut tags = Vec::new();
        for _ in 0..3 {
            tags.push(
                tokio::time::timeout(Duration::from_secs(5), expect(&mut client))
                    .await
                    .expect("the replayed sequence was never answered")
                    .0,
            );
        }
        assert_eq!(
            tags,
            vec![Tag(b'2'), Tag::COMMAND_COMPLETE, Tag::READY_FOR_QUERY],
            "a sequence with no Parse in it was refused rather than replayed"
        );
        assert_eq!(
            context.routes.cache(),
            1,
            "the replayed sequence was answered from the cache after a write"
        );

        drop(client);
        let _ = served.await;
    }

    #[tokio::test]
    async fn a_simple_query_is_not_served_a_sequences_payload_without_a_description() {
        // `M9.27`. ADR 0022 says one payload serves both protocols, and `M9.22`
        // built it, but the two do not store the same thing: a server sends a
        // `RowDescription` for every simple query with rows, and for an
        // `Execute` only if the client sent a `Describe`. So a sequence that
        // asked for none stores a payload with none, and a simple query served
        // that entry would get its rows with nothing describing them, which no
        // driver can read.
        //
        // The extended sequence goes first, so the entry in the cache is the
        // one without a description.
        let addr = fake_postgres_extended().await;
        let (context, cache) = context_with_cache(addr);
        let context = Arc::new(context);
        let (mut client, served) = extended_client(&context).await;

        let mut prepared = Vec::new();
        pgprox_proto::encode_frontend::parse(&mut prepared, "s1", "SELECT 1");
        pgprox_proto::encode_frontend::bind(&mut prepared, "", "s1");
        pgprox_proto::encode_frontend::execute(&mut prepared, "");
        pgprox_proto::encode_frontend::sync(&mut prepared);
        client.write_all(&prepared).await.unwrap();
        four_answers(&mut client).await;
        assert_eq!(cache.len(), 1, "the sequence stored nothing");

        // The same statement, asked the simple way. It is owed a description
        // and the entry has none, so this has to reach the server.
        let mut simple = Vec::new();
        pgprox_proto::encode_frontend::query(&mut simple, "SELECT 1");
        client.write_all(&simple).await.unwrap();
        let answer = expect_answer(&mut client).await;

        assert_eq!(
            answer.first().map(|frame| frame.0),
            Some(Tag::ROW_DESCRIPTION),
            "a simple query was served a payload with nothing describing its rows"
        );
        assert_eq!(
            context.routes.cache(),
            0,
            "a simple query was answered from an entry it could not use"
        );

        drop(client);
        let _ = served.await;
    }

    #[tokio::test]
    async fn a_served_sequence_takes_nothing_from_the_pool() {
        // The whole point of holding the sequence back rather than serving at
        // the `Execute`. Forwarding a `Parse` acquires, so a hit there would
        // have paid for the pool before it looked anything up, and `M7.56` put
        // 45% of this proxy's CPU in that lock.
        // The pool is emptied and capped before the hit, so a sequence that
        // needed a connection could not have one: the assertion is that the
        // client is answered anyway.
        let addr = fake_postgres_extended().await;
        let (mut context, _cache) = context_with_cache(addr);
        // Short, so a version of this that did acquire fails on the answer
        // rather than on the clock.
        context.acquire_timeout = Duration::from_millis(250);
        let context = Arc::new(context);
        let key = grant_for(addr).primary.pool_key();

        // Fill the cache, on a session that then goes away and leaves its
        // connection idle in the pool.
        let (mut first, first_task) = extended_client(&context).await;
        first
            .write_all(&one_binding("SELECT $1", b"1"))
            .await
            .unwrap();
        four_answers(&mut first).await;
        drop(first);
        let _ = first_task.await;

        // A second session takes that idle connection and keeps it: a `Parse`
        // the cacheability rule refuses, ended with a `Flush` rather than a
        // `Sync`, so the sequence stays open and the connection stays out.
        // `random()` rather than a write, because a write would drop the entry
        // this test is about.
        let (mut holding, holding_task) = extended_client(&context).await;
        let mut open = Vec::new();
        pgprox_proto::encode_frontend::parse(&mut open, "volatile", "SELECT random()");
        open.extend_from_slice(&[b'H', 0, 0, 0, 4]);
        holding.write_all(&open).await.unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), expect(&mut holding))
                .await
                .expect("the holding session was never answered")
                .0,
            Tag(b'1')
        );
        assert_eq!(
            context.pool.stats(&key),
            pgprox_core::pool::PoolStats {
                active: 1,
                idle: 0,
                waiting: 0,
                limit: 20,
            },
            "the holding session did not take the only connection"
        );

        // And now nothing more may be opened.
        context.pool.set_limit(&key, 1);

        let (mut client, served) = extended_client(&context).await;
        client
            .write_all(&one_binding("SELECT $1", b"1"))
            .await
            .unwrap();
        assert_eq!(
            four_answers(&mut client).await,
            vec![
                Tag(b'1'),
                Tag(b'2'),
                Tag::COMMAND_COMPLETE,
                Tag::READY_FOR_QUERY
            ],
            "a hit could not be served without a connection to take"
        );
        assert_eq!(
            context.pool.stats(&key).active,
            1,
            "the served session is holding a connection of its own"
        );

        drop(client);
        drop(holding);
        let _ = served.await;
        let _ = holding_task.await;
    }

    #[tokio::test]
    async fn a_sequence_the_cache_gives_up_on_is_answered_as_it_always_was() {
        // asyncpg's prepare round trip, on a tenant that has opted in. A
        // statement `Describe` is not held, so the `Parse` that was held has to
        // reach the server before the `Describe` does, in that order, and the
        // client sees exactly what it saw before any of this existed.
        let addr = fake_postgres_extended().await;
        let (context, _cache) = context_with_cache(addr);
        let context = Arc::new(context);
        let (mut client, served) = extended_client(&context).await;

        let mut prepare = Vec::new();
        pgprox_proto::encode_frontend::parse(&mut prepare, "s1", "SELECT $1");
        prepare.push(b'D');
        let described = b"S\x73\x31\x00";
        prepare.extend_from_slice(&u32::try_from(described.len() + 4).unwrap().to_be_bytes());
        prepare.extend_from_slice(described);
        prepare.extend_from_slice(&[b'H', 0, 0, 0, 4]);
        client.write_all(&prepare).await.unwrap();

        let mut tags = Vec::new();
        for _ in 0..3 {
            tags.push(
                tokio::time::timeout(Duration::from_secs(5), expect(&mut client))
                    .await
                    .expect("the proxy never answered the Flush")
                    .0,
            );
        }
        assert_eq!(
            tags,
            vec![Tag(b'1'), Tag(b't'), Tag(b'n')],
            "the held Parse did not reach the server before the Describe"
        );

        drop(client);
        let _ = served.await;
    }

    #[test]
    fn a_session_that_never_binds_holds_nothing() {
        // "Allocates nothing new", at the line that would do the allocating. A
        // node with no cache pays one `serves` call per frame and never builds
        // the buffer a sequence would need.
        let context = context_for("127.0.0.1:1".parse().unwrap());
        let grant = grant_for("127.0.0.1:1".parse().unwrap());

        let mut out = Vec::new();
        pgprox_proto::encode_frontend::query(&mut out, "SELECT 1");
        let body = out[5..].to_vec();
        let frame = Frame::new(Tag::QUERY, &body);
        let message = pgprox_proto::frontend::decode(&frame).unwrap();

        let mut live = Live::new(&context, &grant);
        let incoming = Incoming {
            message: &message,
            tag: Tag::QUERY,
            body: &body,
        };
        assert_eq!(withhold(&context, &grant, &mut live, incoming), Held::Send);
        assert!(
            live.sequence.is_none(),
            "a session that never binds allocated a sequence buffer"
        );
    }

    #[tokio::test]
    async fn a_flush_is_answered_without_waiting_for_a_ready_for_query() {
        // The asyncpg deadlock. It prepares with Parse, Describe, Flush rather
        // than with a Sync, and the relay read until ReadyForQuery, which a
        // Flush never produces: the server had answered, the answer was inside
        // the proxy, and both ends waited.
        //
        // The timeout is the assertion. Without it a regression here does not
        // fail the test, it hangs the suite.
        let addr = fake_postgres_extended().await;
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

        let mut prepare = Vec::new();
        pgprox_proto::encode_frontend::parse(&mut prepare, "s1", "SELECT $1");
        // Describe of a statement, hand-encoded: 'D', the target byte, the
        // name. `encode_frontend` has no helper for it because nothing in the
        // proxy has ever needed to send one.
        prepare.push(b'D');
        let described = b"S\x73\x31\x00";
        prepare.extend_from_slice(&u32::try_from(described.len() + 4).unwrap().to_be_bytes());
        prepare.extend_from_slice(described);
        prepare.extend_from_slice(&[b'H', 0, 0, 0, 4]);
        client.write_all(&prepare).await.unwrap();

        let answered = tokio::time::timeout(Duration::from_secs(5), async {
            let mut tags = Vec::new();
            for _ in 0..3 {
                tags.push(expect(&mut client).await.0);
            }
            tags
        })
        .await
        .expect("the proxy never answered the Flush");

        assert_eq!(
            answered,
            vec![Tag(b'1'), Tag(b't'), Tag(b'n')],
            "the client did not get its ParseComplete and description"
        );

        // And the sequence is still open: a Sync after it gets the
        // ReadyForQuery, which is what tells the client the exchange is over.
        client.write_all(&[b'S', 0, 0, 0, 4]).await.unwrap();
        let (tag, _) = tokio::time::timeout(Duration::from_secs(5), expect(&mut client))
            .await
            .expect("the Sync after the Flush was never answered");
        assert_eq!(tag, Tag::READY_FOR_QUERY);

        drop(client);
        let _ = served.await;
    }

    #[tokio::test]
    async fn a_flush_with_nothing_outstanding_returns_rather_than_blocking() {
        // Postgres answers a lone Flush with silence, correctly. A proxy that
        // read one frame anyway would block on a message that is not coming,
        // and the client's next statement would never be looked at.
        let addr = fake_postgres_extended().await;
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

        client.write_all(&[b'H', 0, 0, 0, 4]).await.unwrap();

        let mut query = Vec::new();
        pgprox_proto::encode_frontend::query(&mut query, "SELECT 1");
        client.write_all(&query).await.unwrap();

        let (tag, _) = tokio::time::timeout(Duration::from_secs(5), expect(&mut client))
            .await
            .expect("a lone Flush wedged the session");
        assert_eq!(tag, Tag::ROW_DESCRIPTION);

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

    /// A fake that answers one statement per connection and then goes away.
    ///
    /// The shape a `pg_terminate_backend`, an `idle_session_timeout` or a
    /// database restart leaves behind: the connection the pool is holding is
    /// dead, and nothing on this side has looked at it since it went idle.
    async fn fake_postgres_that_dies_when_idle() -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
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

                    // One statement, answered, and then the backend is gone.
                    let mut header = [0_u8; 5];
                    if socket.read_exact(&mut header).await.is_err() {
                        return;
                    }
                    let len = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
                    let mut body = vec![0; len - 4];
                    if socket.read_exact(&mut body).await.is_err() {
                        return;
                    }
                    crate::fakepg::record(
                        addr,
                        String::from_utf8_lossy(&body)
                            .trim_end_matches('\0')
                            .to_owned(),
                    );

                    let mut out = Vec::new();
                    out.extend_from_slice(&crate::fakepg::row_description());
                    out.push(Tag::COMMAND_COMPLETE.get());
                    let text = b"SELECT 1\0";
                    out.extend_from_slice(&u32::try_from(text.len() + 4).unwrap().to_be_bytes());
                    out.extend_from_slice(text);
                    encode::ready_for_query(&mut out, TxStatus::Idle);
                    let _ = socket.write_all(&out).await;
                });
            }
        });

        addr
    }

    #[tokio::test]
    async fn a_connection_that_died_while_idle_is_not_handed_to_a_client() {
        // `M20.5`. Nothing reads a pooled connection while it is idle: the pool
        // holds them in a `VecDeque` and no task polls it. So a server that
        // went away between borrowers was discovered by the next client's own
        // query, and what that client saw was its statement failing on a
        // connection that was already dead when it arrived.
        let addr = fake_postgres_that_dies_when_idle().await;
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

        let mut query = Vec::new();
        pgprox_proto::encode_frontend::query(&mut query, "SELECT 1");
        client.write_all(&query).await.unwrap();
        loop {
            if expect(&mut client).await.0 == Tag::READY_FOR_QUERY {
                break;
            }
        }
        assert_eq!(
            context.pool.all_stats()[0].1.idle,
            1,
            "the connection did not go back to the pool"
        );

        // The backend goes while the connection sits in the pool. Parked on
        // rather than yielded to: the close has to reach this process's I/O
        // driver, and on a current-thread runtime that only runs when the
        // runtime parks. See the note in
        // `a_reaped_connection_says_goodbye_rather_than_vanishing`.
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        // The second statement. It must be answered, from a fresh connection.
        let mut query = Vec::new();
        pgprox_proto::encode_frontend::query(&mut query, "SELECT 2");
        client.write_all(&query).await.unwrap();
        let mut saw = Vec::new();
        loop {
            let (tag, body) = expect(&mut client).await;
            saw.push((tag, String::from_utf8_lossy(&body).into_owned()));
            if tag == Tag::READY_FOR_QUERY {
                break;
            }
        }
        assert!(
            !saw.iter().any(|(tag, _)| *tag == Tag::ERROR_RESPONSE),
            "a client was handed a connection that had already died: {saw:?}"
        );

        // And it really did run, on a connection this fake opened for it.
        assert!(
            crate::fakepg::statements_seen(addr).contains(&"SELECT 2".to_owned()),
            "the second statement never reached a server: {:?}",
            crate::fakepg::statements_seen(addr)
        );

        drop(client);
        let _ = served.await;
    }

    #[tokio::test]
    async fn a_reaped_connection_says_goodbye_rather_than_vanishing() {
        // `M20.4`. Postgres logs a client that disappears without a
        // `Terminate`, and this node reaps idle connections after thirty
        // seconds with `min_pool` at zero, deliberately. So reaping is the
        // steady state, and without a goodbye every routine close is a line on
        // the database that reads like a crash, which makes a real fault
        // indistinguishable from housekeeping.
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

        // One statement, so a connection exists and goes back to the pool.
        let mut query = Vec::new();
        pgprox_proto::encode_frontend::query(&mut query, "SELECT 1");
        client.write_all(&query).await.unwrap();
        expect(&mut client).await;
        expect(&mut client).await;

        drop(client);
        let _ = served.await;

        assert!(
            !crate::fakepg::statements_seen(addr).contains(&crate::fakepg::TERMINATED.to_owned()),
            "the connection was said goodbye to before anything reaped it"
        );

        // Now retire it, which is what the node's own timer does every second.
        let reaped = context.pool.reap_idle(&pgprox_pool::reap::ReapConfig {
            idle_timeout: std::time::Duration::ZERO,
            ..pgprox_pool::reap::ReapConfig::default()
        });
        assert_eq!(reaped.len(), 1, "nothing was idle to reap");
        crate::dial::retire(reaped).await;

        // The fake records a `Terminate` by name, because its body is empty and
        // recording that would record the empty string.
        //
        // Polled rather than waited on, and the interval is not a deadline this
        // test depends on. The bytes have already left this side: `goodbye`
        // flushed and returned `Ok`. What is outstanding is the fake's own task
        // being woken to read them, and on a current-thread runtime that needs
        // the runtime to park so the I/O driver runs at all. `yield_now` does
        // not park, which is why it is not what is here. The loop stops on the
        // first sight of the goodbye, so the interval is what a *failure* costs
        // rather than what a pass does, and widening it would fix nothing.
        for _ in 0..100 {
            if crate::fakepg::statements_seen(addr).contains(&crate::fakepg::TERMINATED.to_owned())
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        let seen = crate::fakepg::statements_seen(addr);
        assert!(
            seen.contains(&crate::fakepg::TERMINATED.to_owned()),
            "the socket was dropped without a Terminate: {seen:?}"
        );
    }

    #[tokio::test]
    async fn a_plain_startup_parameter_reaches_the_server_too() {
        // `M20.7`. `M20.2` did the `options` half. libpq sends
        // `client_encoding` and `application_name` as plain parameters, and
        // `StartupInfo` did not even carry them: the upstream startup packet
        // was `user`, `database` and a hard-coded `application_name=pgprox`.
        //
        // The client's `application_name` is honoured, which reverses what the
        // proxy used to put on the connection. A connection actively serving a
        // tenant showing that tenant's application is the more useful of the
        // two facts available to a DBA, and which node holds it is already in
        // the pool key. `probe.rs`'s separate rule stands: `pgprox` is still
        // not reported back to the client as its own application name.
        let addr = fake_postgres().await;
        let context = Arc::new(context_for(addr));
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(8192);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        client
            .write_all(&startup_with(
                "good.token",
                None,
                &[("application_name", "reporting")],
            ))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut client).await;
        }

        // The client sends no `SET`, so anything naming it came from the
        // startup packet.
        let mut query = Vec::new();
        pgprox_proto::encode_frontend::query(&mut query, "SELECT 1");
        client.write_all(&query).await.unwrap();
        expect(&mut client).await;
        expect(&mut client).await;

        assert!(
            statements_seen(addr)
                .iter()
                .any(|sql| sql.contains("application_name") && sql.contains("reporting")),
            "a plain startup parameter never reached the server: {:?}",
            statements_seen(addr)
        );

        drop(client);
        let _ = served.await;
    }

    #[tokio::test]
    async fn a_search_path_from_the_connection_string_reaches_the_server() {
        // `M20.2`. libpq packs `options=-c search_path=...` into the startup
        // packet, and the proxy parsed it into `StartupInfo::options` and read
        // it nowhere. So every statement ran under whatever `search_path` the
        // pooled connection happened to carry, which in a proxy that separates
        // tenants is the wrong schema and no error.
        let addr = fake_postgres().await;
        let context = Arc::new(context_for(addr));
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(8192);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        client
            .write_all(&startup_with_options(
                "good.token",
                Some("-c search_path=tenant_acme"),
            ))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut client).await;
        }

        // The client never sends a `SET`. Anything on the wire naming
        // `search_path` came from the startup packet by way of the replay.
        let mut query = Vec::new();
        pgprox_proto::encode_frontend::query(&mut query, "SELECT 1");
        client.write_all(&query).await.unwrap();
        expect(&mut client).await;
        expect(&mut client).await;

        assert!(
            statements_seen(addr)
                .iter()
                .any(|sql| sql.contains("search_path") && sql.contains("tenant_acme")),
            "the connection-string search_path never reached the server: {:?}",
            statements_seen(addr)
        );

        drop(client);
        let _ = served.await;
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
    async fn a_tenant_s_statement_timeout_reaches_the_connection_it_borrows() {
        // The sidecar sends it and nothing applied it, so a tenant's cap on
        // its own runaway queries did nothing. It is per connection, and a
        // connection from the pool carries whatever the last borrower set.
        let addr = fake_postgres().await;
        let mut context = context_for(addr);
        let mut grant = grant_for(addr);
        grant.pool.statement_timeout = Some(Duration::from_secs(7));
        context.resolver = Arc::new(FakeCredentialResolver::new().with_grant("good.token", grant));
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
        for _ in 0..5 {
            expect(&mut client).await;
        }

        let mut query = Vec::new();
        pgprox_proto::encode_frontend::query(&mut query, "SELECT 1");
        client.write_all(&query).await.unwrap();
        expect(&mut client).await;
        expect(&mut client).await;

        let seen = statements_seen(addr);
        assert!(
            seen.iter()
                .any(|sql| sql.contains("statement_timeout = 7000")),
            "the tenant's statement timeout never reached the server: {seen:?}"
        );

        drop(client);
        let _ = served.await;
    }

    #[tokio::test]
    async fn a_tenant_that_asked_for_session_pooling_keeps_its_connection() {
        // The sidecar sends the mode and nothing read it, so every tenant got
        // transaction pooling. One that asked for session pooling and did not
        // get it loses temporary tables and advisory locks between statements,
        // which the pin list catches only for the cases it knows.
        let addr = fake_postgres().await;
        let mut context = context_for(addr);
        let mut grant = grant_for(addr);
        grant.pool.mode = pgprox_core::auth::PoolMode::Session;
        context.resolver = Arc::new(FakeCredentialResolver::new().with_grant("good.token", grant));
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
        for _ in 0..5 {
            expect(&mut client).await;
        }

        let mut query = Vec::new();
        pgprox_proto::encode_frontend::query(&mut query, "SELECT 1");
        client.write_all(&query).await.unwrap();
        expect(&mut client).await;
        expect(&mut client).await;

        // The transaction ended and the connection did not go back.
        let key = PoolKey::new(ServerId::new("127.0.0.1", addr.port()), "acme", "acme_app");
        let stats = pgprox_core::pool::UpstreamPool::stats(context.pool.as_ref(), &key);
        assert_eq!(
            stats.idle, 0,
            "a session-pooled connection was returned at a transaction boundary: {stats:?}"
        );
        assert_eq!(stats.active, 1);

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
        tokio::spawn(accept_loop(listener, Arc::clone(&context), gate));

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
        assert_eq!(expect(&mut tls).await.0, Tag::ROW_DESCRIPTION);
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
        //
        // `M17.4`: this asserted only that the session finished, which it does
        // whether or not anything was cancelled, so replacing `cancel` with
        // `Ok(())` survived and the test's own name was the thing it did not
        // check. It now runs against the catcher rather than the fake server,
        // because a cancel arrives on its own connection carrying no startup
        // packet and what proves it arrived is the bytes.
        let (addr, caught) = cancel_catcher().await;
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

        let key = tokio::time::timeout(Duration::from_secs(5), caught)
            .await
            .expect("the cancel never reached the server")
            .unwrap();
        assert_eq!(
            key,
            (4242, 99),
            "the cancel carried a key the server never issued"
        );
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
        elsewhere.peers = pgprox_core::cluster::StaticPeers::new(std::collections::BTreeMap::from(
            [(NodeId::new(1), gossip_at.to_string())],
        ));
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
        context.peers =
            pgprox_core::cluster::StaticPeers::new(std::collections::BTreeMap::from([(
                NodeId::new(1),
                listener.local_addr().unwrap().to_string(),
            )]));

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
        let answer = expect_answer(&mut first).await;
        assert_eq!(
            answer.first().map(|frame| frame.0),
            Some(Tag::ROW_DESCRIPTION),
            "a query with rows was answered without a description"
        );
        assert_eq!(answer[answer.len() - 2].0, Tag::COMMAND_COMPLETE);
    }

    #[test]
    fn a_static_session_is_told_it_reached_the_proxy_and_not_a_database() {
        // `M17.4`: all four mutants of this list survived, so it could have
        // answered a single empty pair, or `xyzzy`, to every admin session.
        // The version is what a driver reads before deciding which syntax to
        // use, and the encodings are what it decodes bytes with, so a wrong
        // answer here is a client that mis-parses everything that follows.
        // Named `(pgprox)` on purpose: this session has no upstream, and a
        // version copied from one would name a server it cannot reach.
        assert_eq!(
            proxy_parameters(),
            vec![
                ("server_version".to_owned(), "17.0 (pgprox)".to_owned()),
                ("server_encoding".to_owned(), "UTF8".to_owned()),
                ("client_encoding".to_owned(), "UTF8".to_owned()),
                ("DateStyle".to_owned(), "ISO, MDY".to_owned()),
            ]
        );
    }

    #[tokio::test]
    async fn a_context_prints_which_node_it_is_and_no_credential() {
        // `M17.4`: this `Debug` could return an empty string, because nothing
        // read it. It holds a resolver, and a resolver holds tokens, so both
        // halves matter: it must say enough to identify the node in a panic
        // and it must not say what the node authenticated with.
        let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let rendered = format!("{:?}", context_for(addr));

        assert!(
            rendered.contains("Context") && rendered.contains("NodeId"),
            "the debug output names neither the type nor the node: {rendered}"
        );
        assert!(!rendered.to_lowercase().contains("token"), "{rendered}");
        assert!(!rendered.to_lowercase().contains("good."), "{rendered}");
    }

    #[tokio::test]
    async fn an_answer_the_server_failed_is_not_stored_however_much_of_it_arrived() {
        // `M17.4`: `||` for `&&` here stores a recording that carries both a
        // failure and frames, which is exactly the shape a partial answer has
        // when the server raised an error partway. Replaying it later serves a
        // truncated result nobody ever received, with no error attached, and
        // the cache keeps doing so until the TTL runs out.
        let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let (context, cache) = context_with_cache(addr);
        let mut pumping = Pumping {
            swallow: 0,
            owed: pgprox_session::flush::Outstanding::new(),
            recording: None,
        };

        // Failed, and with bytes already recorded.
        let mut failed = recording();
        failed.failed = true;
        failed.frames = vec![b'D', 0, 0, 0, 4];
        pumping.recording = Some(Box::new(failed));
        store_answer(&context, &mut pumping).await;
        assert_eq!(cache.len(), 0, "a failed answer reached the cache");

        // An answer that carries nothing is not an answer either, whatever it
        // says about failing.
        pumping.recording = Some(Box::new(recording()));
        store_answer(&context, &mut pumping).await;
        assert_eq!(cache.len(), 0, "an empty answer reached the cache");

        // And the one that is fit to store is stored, so the two assertions
        // above are about the condition rather than about a path that never
        // stores anything.
        let mut good = recording();
        good.frames = vec![b'D', 0, 0, 0, 4];
        pumping.recording = Some(Box::new(good));
        store_answer(&context, &mut pumping).await;
        assert_eq!(cache.len(), 1, "a complete answer was not stored");

        // Taken whatever happens, so nothing is left for the next statement.
        assert!(pumping.recording.is_none());
    }

    #[test]
    fn a_completion_the_proxy_asked_for_is_swallowed_exactly_once_each() {
        // `M17.4`. Five mutants lived in the two copies of this rule, and each
        // desynchronises a session past recovery. The count is what the proxy
        // is owed: it sent a `Parse` and a `Close` the client never sent, so
        // it must eat exactly that many completions and no more.
        let mut pumping = Pumping {
            swallow: 2,
            owed: pgprox_session::flush::Outstanding::new(),
            recording: None,
        };

        assert!(pumping.swallow_one(Tag::PARSE_COMPLETE));
        assert_eq!(pumping.swallow, 1, "the count did not come down by one");
        assert!(pumping.swallow_one(Tag::CLOSE_COMPLETE));
        assert_eq!(pumping.swallow, 0);

        // The third is the client's own, and swallowing it would leave the
        // client reading every answer after it one frame out of step.
        assert!(
            !pumping.swallow_one(Tag::PARSE_COMPLETE),
            "a completion the client asked for was eaten"
        );
        assert_eq!(pumping.swallow, 0, "the count went below zero");

        // And nothing else is ever swallowed, however much is owed.
        let mut owing = Pumping {
            swallow: 5,
            owed: pgprox_session::flush::Outstanding::new(),
            recording: None,
        };
        for tag in [Tag::ROW_DESCRIPTION, Tag::DATA_ROW, Tag::COMMAND_COMPLETE] {
            assert!(!owing.swallow_one(tag), "{tag:?} was swallowed");
        }
        assert_eq!(owing.swallow, 5, "an unrelated frame moved the count");
    }

    #[test]
    fn a_describe_of_a_statement_is_renamed_and_one_of_a_portal_is_not() {
        // `M17.4`: the `describes_statement` guard could be forced `false`,
        // which sends the client's own name upstream for a statement this
        // proxy prepared under a global one. The server answers "prepared
        // statement does not exist" on a connection the proxy thought was
        // warm, which is the failure `M15.3` and `M16.7` were both about.
        //
        // The two bodies differ in one byte: `S` names a statement, `P` names
        // a portal. A portal is the client's own name for a result set and
        // this proxy does not rename it, so forcing the guard `true` is the
        // opposite failure and equally silent.
        use pgprox_proto::frontend::FrontendMessage as Message;

        let mut session = pgprox_session::resume::SessionMemory::default();
        let global = session
            .statements
            .parse("s1", "SELECT $1")
            .as_str()
            .to_owned();
        assert_ne!(global, "s1", "the global name is the client's own");

        let describe_statement = Message::Describe {
            name: "s1",
            target: pgprox_proto::frontend::Target::Statement,
        };
        let mapped = map_statement_name(&describe_statement, b"Ss1\0", &mut session)
            .expect("a statement this session prepared was not mapped");
        assert_eq!(
            mapped,
            [b"S".as_slice(), global.as_bytes(), b"\0"].concat(),
            "the client's own name went upstream"
        );

        // A portal of the same name travels as it arrived.
        let describe_portal = Message::Describe {
            name: "s1",
            target: pgprox_proto::frontend::Target::Portal,
        };
        assert_eq!(
            map_statement_name(&describe_portal, b"Ps1\0", &mut session),
            Some(b"Ps1\0".to_vec()),
            "a portal was renamed"
        );
    }

    #[tokio::test]
    async fn a_bind_larger_than_the_inspect_prefix_announces_its_whole_length() {
        // `M17.4`. `send_upstream` builds the forwarded header from the part
        // that is here plus the part still on the client's socket, and `+`
        // could become `-` with nothing to notice: every test's `Bind` was
        // small enough to arrive whole, so the tail was always zero and the
        // two spellings agreed.
        //
        // With a tail they do not. A header short by twice the tail makes the
        // server read a truncated body and then take the next header from
        // inside the bytes it did not read, which desynchronises the
        // connection for good. `a_rewritten_prefix_and_its_tail_announce_one_length`
        // asserts the same rule against `forward_header` directly; this is the
        // caller that has to get it right, driven end to end.
        let addr = fake_postgres().await;
        let context = Arc::new(context_for(addr));
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(256 * 1024);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut client).await;
        }

        // A `Bind` well past the 4 KiB prefix, so `read_client_body` leaves a
        // tail and `send_upstream` has to account for it.
        let mut extended = Vec::new();
        pgprox_proto::encode_frontend::parse(&mut extended, "s1", "SELECT $1");
        let mut bind = Vec::new();
        pgprox_proto::encode_frontend::bind(&mut bind, "", "s1");
        let padding = vec![b'v'; 40_000];
        let body_len = bind.len() - 5 + padding.len();
        let mut big = vec![Tag::BIND.get()];
        big.extend_from_slice(&u32::try_from(body_len + 4).expect("fits").to_be_bytes());
        big.extend_from_slice(&bind[5..]);
        big.extend_from_slice(&padding);
        extended.extend_from_slice(&big);
        pgprox_proto::encode_frontend::sync(&mut extended);
        client.write_all(&extended).await.unwrap();

        // The session stays in step: the server answered and the answer came
        // back. Under a short header the fake reads into the next frame and
        // nothing arrives.
        let answered = tokio::time::timeout(Duration::from_secs(5), async {
            expect(&mut client).await;
            expect(&mut client).await;
        })
        .await;
        assert!(
            answered.is_ok(),
            "the connection desynchronised on a bind with a tail"
        );

        drop(client);
        let _ = served.await;
    }

    #[tokio::test]
    async fn a_client_at_the_pool_cap_waits_for_the_deadline_rather_than_being_refused() {
        // `M17.4`: `now + acquire_timeout` could become `now - acquire_timeout`
        // and every test passed, because the pool in a test always had room
        // and `acquire` consults the deadline only when a caller has to wait.
        // A deadline already in the past refuses every waiting client the
        // instant it arrives, which turns "wait up to five seconds for a
        // connection" into "fail immediately whenever the pool is busy". That
        // is the pool's entire contract with a client under load.
        let addr = fake_postgres().await;
        let context = Arc::new(context_for(addr));
        let key = grant_for(addr).primary.pool_key();
        // One connection for the whole node, so the second client must wait.
        context.pool.set_limit(&key, 1);

        let gate = Arc::new(Gate::new(10));

        // The first client opens a transaction and keeps it open, so the
        // connection is not released at a statement boundary.
        let (first_io, mut first) = tokio::io::duplex(64 * 1024);
        let held = Arc::clone(&context);
        let holding = gate.admit().unwrap();
        let one = tokio::spawn(async move { session(first_io, held.as_ref(), holding).await });
        first
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut first).await;
        }
        let mut begin = Vec::new();
        pgprox_proto::encode_frontend::query(&mut begin, "BEGIN");
        first.write_all(&begin).await.unwrap();
        expect_answer(&mut first).await;

        // The second finds the pool full.
        let (second_io, mut second) = tokio::io::duplex(64 * 1024);
        let also = Arc::clone(&context);
        let waiting = gate.admit().unwrap();
        let two = tokio::spawn(async move { session(second_io, also.as_ref(), waiting).await });
        second
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut second).await;
        }
        let mut query = Vec::new();
        pgprox_proto::encode_frontend::query(&mut query, "SELECT 1");
        second.write_all(&query).await.unwrap();

        // One-sided on purpose. Correct code *cannot* answer here: the only
        // connection is held and the deadline is five seconds away, so
        // nothing arrives however slow the machine is. A deadline in the past
        // answers with a refusal at once, and 300ms is a long time for a
        // duplex to carry an error that has already been written.
        let early = tokio::time::timeout(Duration::from_millis(300), expect(&mut second)).await;
        assert!(
            early.is_err(),
            "a client at the cap was refused instead of waiting: {early:?}"
        );

        // And it is served once the first client lets go, which is what makes
        // the assertion above about the deadline rather than about a hang.
        let mut commit = Vec::new();
        pgprox_proto::encode_frontend::query(&mut commit, "COMMIT");
        first.write_all(&commit).await.unwrap();
        expect_answer(&mut first).await;

        let answer = tokio::time::timeout(Duration::from_secs(5), expect_answer(&mut second))
            .await
            .expect("the waiting client was never served");
        assert_eq!(
            answer.first().map(|frame| frame.0),
            Some(Tag::ROW_DESCRIPTION),
            "the waiting client got something other than its answer"
        );

        drop(first);
        drop(second);
        let _ = one.await;
        let _ = two.await;
    }

    #[tokio::test]
    async fn a_volatile_statement_in_the_extended_protocol_is_never_held() {
        // `M17.4`: `may_begin`'s `&&` could become `||`, and the simple-query
        // test that covers the same rule cannot see it. `facts_for` returns
        // nothing at all for a `Query`, because only a `Parse` carries SQL and
        // only a `Bind` names a statement the session prepared, so
        // `a_statement_the_rule_refuses_is_never_stored` exercises a path
        // where the condition is never reached.
        //
        // With `||`, a session holding no connection begins a sequence for
        // *any* statement, including one the cacheability rule refuses. What
        // gets stored is then a volatile answer that will be served to the
        // next caller as though it were stable, which is the one failure ADR
        // 0022's entry condition exists to prevent.
        let addr = fake_postgres().await;
        let (context, cache) = context_with_cache(addr);
        let context = Arc::new(context);
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(64 * 1024);
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
        pgprox_proto::encode_frontend::parse(&mut extended, "s1", "SELECT random()");
        pgprox_proto::encode_frontend::bind(&mut extended, "", "s1");
        pgprox_proto::encode_frontend::execute(&mut extended, "");
        pgprox_proto::encode_frontend::sync(&mut extended);
        client.write_all(&extended).await.unwrap();

        // Drained rather than counted: what the fake answers with is not the
        // point, only that the exchange completed before the cache is read.
        let drained = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if expect(&mut client).await.0 == Tag::READY_FOR_QUERY {
                    return;
                }
            }
        })
        .await;
        assert!(drained.is_ok(), "the extended sequence never finished");

        assert_eq!(
            cache.len(),
            0,
            "a volatile statement was held and stored through the extended protocol"
        );

        drop(client);
        let _ = served.await;
    }

    #[tokio::test]
    async fn a_peer_asking_this_node_for_its_clients_gets_them() {
        // `M17.4`, and a correction: the baseline entry accepting the *trait
        // default* for `CancelSink::clients` claimed this override "is a
        // separate mutant and is caught". It was not. Returning an empty list
        // here makes `SHOW CLIENTS` at cluster scope report every peer's
        // sessions and none of this node's, which reads as a node serving
        // nobody while it serves thousands.
        use crate::gossip::CancelSink as _;

        let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let context = context_for(addr);
        assert!(
            context.clients().is_empty(),
            "a node serving nobody listed somebody"
        );

        let now = context.clock.now();
        let _held = context.sessions.register(
            ConnId::new(NodeId::new(1), 1),
            pgprox_core::ids::TenantId::new("acme"),
            NodeId::new(1),
            now,
            16,
            crate::run::Shutdown::new(),
        );
        let _also = context.sessions.register(
            ConnId::new(NodeId::new(1), 2),
            pgprox_core::ids::TenantId::new("globex"),
            NodeId::new(1),
            now,
            16,
            crate::run::Shutdown::new(),
        );

        let listed = context.clients();
        assert_eq!(listed.len(), 2, "a peer was told this node serves nobody");
        assert!(
            listed
                .iter()
                .any(|view| view.tenant == pgprox_core::ids::TenantId::new("acme"))
        );
    }

    #[tokio::test]
    async fn a_cancel_for_a_node_added_after_startup_is_forwarded_to_it() {
        // `M19.3`. The seam's whole point, asked of the consumer that would
        // have been silently wrong: a cancel arrives on whichever pod the
        // client's second connection reached, so a node holding a table taken
        // at startup drops cancels for every node that joined after it did, and
        // cancelling a query works one time in N. That is the defect `M6.30`
        // fixed once already, and a stale table would have brought it back
        // without touching the routing.
        let (upstream, _caught) = cancel_catcher().await;
        let mut context = context_for(upstream);
        context.node = NodeId::new(2);
        context.cancels = Arc::new(Registry::new(NodeId::new(2), Box::new(Fixed)));

        // Built knowing about nobody, which is what a node that started first
        // sees.
        let source = pgprox_core::cluster::FakePeerSource::new(std::collections::BTreeMap::new());
        context.peers = source.clone();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let peer_at = listener.local_addr().unwrap();
        let (caught, catch) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            use tokio::io::{AsyncBufReadExt, BufReader};
            let Ok((socket, _)) = listener.accept().await else {
                return;
            };
            let mut lines = BufReader::new(socket).lines();
            if let Ok(Some(line)) = lines.next_line().await {
                let _ = caught.send(line);
            }
        });

        // Node 1 joins. Nothing is rebuilt and nothing is restarted.
        source.publish(std::collections::BTreeMap::from([(
            NodeId::new(1),
            peer_at.to_string(),
        )]));

        context
            .deliver(ConnId::new(NodeId::new(1), 0x00AB_CDEF))
            .await;

        let line = tokio::time::timeout(Duration::from_secs(5), catch)
            .await
            .expect("the cancel was not forwarded to a node that joined after startup")
            .unwrap();
        assert!(
            line.contains(r#""kind":"cancel""#),
            "the peer got something other than a cancel: {line}"
        );
    }
}
