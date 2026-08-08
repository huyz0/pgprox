//! The run loop: what a built node actually does.
//!
//! [`crate::wiring::App::build`] assembles a node and opens nothing. This binds
//! the two ports, starts the work that has to happen on a timer, and returns
//! when it is told to stop.
//!
//! # Two ports, on purpose
//!
//! Clients speak Postgres on one and operators speak HTTP on the other, and a
//! deployment is expected to expose them differently: the client port to the
//! world, the admin port to a cluster-internal Service. Serving both on one
//! port would make that a decision nobody can take later.
//!
//! # Stopping is a signal rather than an abort
//!
//! Every long-lived task here selects on the same [`Shutdown`], so stopping is
//! something each of them observes at a point it chose. Aborting the tasks
//! instead would cut a relay mid-frame, which is the one thing a proxy holding
//! somebody's transaction must not do. `M6.22` builds the ordered sequence on
//! top of this; what is here is the mechanism it needs.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use pgprox_session::cancel::Registry;
use pgprox_session::probe::ParameterCache;
use pgprox_session::state::HandshakeConfig;
use tokio::sync::watch;

use crate::entropy::SystemEntropy;
use crate::http::{self, Probes};
use crate::serve::{Context, Gate, accept_loop};
use crate::sessions::Sessions;
use pgprox_pool::reap::ReapConfig;

use crate::wiring::App;

/// How often the periodic work runs.
///
/// One second: gossip freshness and the liveness heartbeat are both measured in
/// tens of seconds, so this is frequent enough to be invisible in both and
/// cheap enough to ignore.
const TICK: Duration = Duration::from_secs(1);

/// How long a client waits for an upstream connection before being refused.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a client has to finish authenticating.
///
/// Generous against a real handshake, which is two round trips and a sidecar
/// call, and short against a socket that will never say anything. pgbouncer's
/// `client_login_timeout` defaults to a minute; this is tighter because the
/// ceiling here is the thing being protected.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(30);

/// The stop signal every long-lived task watches.
///
/// A watch channel rather than a `CancellationToken` from a new dependency, and
/// rather than dropping a sender, because a task has to be able to ask whether
/// the signal has fired as well as wait for it: the drain sequence checks it at
/// a transaction boundary rather than at an await point of its own choosing.
#[derive(Clone, Debug)]
pub struct Shutdown(watch::Sender<bool>);

impl Default for Shutdown {
    fn default() -> Self {
        Self::new()
    }
}

impl Shutdown {
    /// A signal that has not fired.
    #[must_use]
    pub fn new() -> Self {
        Self(watch::channel(false).0)
    }

    /// Fires it. Idempotent, because two things asking a node to stop is
    /// normal: a `SIGTERM` and a drain that ran out of grace.
    pub fn fire(&self) {
        self.0.send_replace(true);
    }

    /// Unfires it.
    ///
    /// For the drain signals only, which an undrain has to be able to take
    /// back: a node whose drain expired or was cancelled goes back to serving.
    /// The process shutdown is never cleared, because a process that has begun
    /// stopping does not un-stop.
    pub fn clear(&self) {
        self.0.send_replace(false);
    }

    /// Whether it has fired.
    #[must_use]
    pub fn fired(&self) -> bool {
        *self.0.borrow()
    }

    /// Resolves when it fires, immediately if it already has.
    pub async fn waited(&self) {
        let mut rx = self.0.subscribe();
        if *rx.borrow_and_update() {
            return;
        }
        // Only fails when every sender is gone, and this holds one, so the
        // result cannot be an error. Treated as a stop either way rather than
        // unwrapped: a panic in the shutdown path is the worst place for one.
        let _ = rx.changed().await;
    }
}

/// Where a node listens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Addrs {
    /// The Postgres port.
    pub client: SocketAddr,
    /// The HTTP port: the probes and the admin API.
    pub admin: SocketAddr,
    /// The gossip port, where peers arrive.
    pub gossip: SocketAddr,
}

/// Both ports, already bound.
///
/// Bound before anything is served so a port already in use is a failed start
/// rather than a node that came up serving half its surface.
#[derive(Debug)]
pub struct Listeners {
    /// Where clients arrive.
    pub client: tokio::net::TcpListener,
    /// Where operators and kubelet arrive.
    pub admin: tokio::net::TcpListener,
    /// Where peers arrive.
    pub gossip: tokio::net::TcpListener,
}

impl Listeners {
    /// Binds both ports.
    ///
    /// # Errors
    ///
    /// Fails when either port cannot be bound.
    pub async fn bind(addrs: Addrs) -> std::io::Result<Self> {
        Ok(Self {
            client: bind_client(addrs.client)?,
            admin: tokio::net::TcpListener::bind(addrs.admin).await?,
            gossip: tokio::net::TcpListener::bind(addrs.gossip).await?,
        })
    }

    /// The addresses actually bound, which is what a test needs when it asked
    /// for port zero.
    ///
    /// # Errors
    ///
    /// Fails when a socket cannot report its own address.
    pub fn addrs(&self) -> std::io::Result<Addrs> {
        Ok(Addrs {
            client: self.client.local_addr()?,
            admin: self.admin.local_addr()?,
            gossip: self.gossip.local_addr()?,
        })
    }
}

/// Descriptors a node needs beyond its client connections.
///
/// Three listeners, the sidecar socket, the upstream pool, the peers, and the
/// usual handful. Deliberately generous: the point is to warn early rather
/// than to predict exactly.
const DESCRIPTOR_HEADROOM: u64 = 256;

/// Says so when the client ceiling exceeds what the process may open.
///
/// A node configured for twenty thousand clients under a soft limit of 1024
/// fails at `accept`, and the failure reads as a network fault to whoever is
/// looking. This turns it into a line in the log at start, before any client
/// arrives.
///
/// A warning rather than a refusal to start: the ceiling is a maximum, not a
/// promise, and a node that serves nine hundred clients under a limit of 1024
/// is working. Refusing to start would take a running deployment down on a
/// configuration change that had not hurt it yet.
/// How many descriptors a node with this ceiling needs, and whether it has them.
///
/// Separated from the warning so the arithmetic can be tested. `M17.4` found
/// every mutant of it surviving, including replacing the whole function with
/// nothing: a warning is invisible to a test that does not read logs, so the
/// decision had to stop being wrapped in one.
fn descriptors_are_short(limit: u64, ceiling: u32) -> Option<u64> {
    let needed = u64::from(ceiling) + DESCRIPTOR_HEADROOM;
    (limit < needed).then_some(needed)
}

fn warn_about_descriptors(ceiling: u32) {
    let Some(limit) = descriptor_limit() else {
        return;
    };
    if let Some(needed) = descriptors_are_short(limit, ceiling) {
        tracing::warn!(
            soft_limit = limit,
            max_client_conns = ceiling,
            needed,
            "the file descriptor limit is below this node's client ceiling: \
             connections past it will fail at accept, which reads as a network fault"
        );
    }
}

/// How much more quota to ask for, or `None` when this node is inside its own.
///
/// The guaranteed share needs no coordination; more than that is the leader's
/// to grant. Asking for one past what is held is deliberate: it is the smallest
/// request that changes anything, and a refusal leaves the node on its share,
/// which is the direction that cannot breach the cap.
///
/// Extracted by `M17.4`, which found five mutants of this arithmetic surviving
/// inside an async function nothing could call without a cluster.
fn wants_more_quota(held: u32, guaranteed: u32, leased: u32) -> Option<u32> {
    (held >= guaranteed + leased).then(|| held.saturating_sub(guaranteed) + 1)
}

/// This node's pools for one server, and the connections they account for.
///
/// The count includes waiters, which is why it is not [`PoolStats::total`]: a
/// caller queued for a connection is demand this node has to ask the leader
/// about, and a quota decision that ignored it would leave a node asking for
/// nothing while clients waited behind a cap.
///
/// Extracted by `M17.4`. Five mutants lived in the two loops this replaces:
/// both `==` filters could become `!=`, and both `+` in the sum could become
/// `-` or `*`, with nothing able to tell. A node that counted another server's
/// connections against this one would ask the leader for capacity on the wrong
/// server, which is the one mistake the cap has no tolerance for.
///
/// Also one read of the pool map rather than two per configured server: it was
/// locked, cloned and filtered twice for every server, every tick.
fn pools_for(
    all: &[(pgprox_core::ids::PoolKey, pgprox_core::pool::PoolStats)],
    server: &pgprox_core::ids::ServerId,
) -> (Vec<pgprox_core::ids::PoolKey>, u32) {
    let mine = all.iter().filter(|(key, _)| &key.server == server);
    let mut keys = Vec::new();
    let mut held = 0;
    for (key, stats) in mine {
        keys.push(key.clone());
        held += stats.active + stats.idle + stats.waiting;
    }
    (keys, held)
}

/// Every distinct upstream this node currently holds pools for.
///
/// The loop iterates these rather than the document's `servers:` list, and that
/// is `M70.0`. Iterating the document meant a pool whose server the document
/// does not name was never passed to `set_limit` at all, so it kept the limit
/// `PoolConfig` gave it at startup and no allowance was ever applied to it.
/// Replicas are the case that matters: they arrive from the sidecar at runtime
/// and an operator cannot list hosts it has not been told about yet.
///
/// Order is the map's, which is arbitrary and does not matter: every server gets
/// the same treatment and none depends on another.
fn servers_with_pools(
    all: &[(pgprox_core::ids::PoolKey, pgprox_core::pool::PoolStats)],
) -> Vec<pgprox_core::ids::ServerId> {
    let mut seen = std::collections::HashSet::new();
    let mut servers = Vec::new();
    for (key, _) in all {
        if seen.insert(key.server.clone()) {
            servers.push(key.server.clone());
        }
    }
    servers
}

/// The cap and split the document declares for a server, directly or inherited.
///
/// Direct first. Failing that, the entry for the primary this server is a
/// replica of, because a replica set is learned from a grant and an operator
/// has no way to list a host the sidecar has not named yet. Replicas of a
/// primary are provisioned alike often enough that its cap is the right guess,
/// and guessing here is bounded: the fleet still coordinates on whatever number
/// this returns.
///
/// `None` means nothing declares a cap for this server, which is a
/// misconfiguration rather than a default to invent.
fn declared_quota(
    config: &pgprox_core::config::Config,
    replicas: &crate::replicas::ReplicaSets,
    server: &pgprox_core::ids::ServerId,
) -> Option<pgprox_cluster::coordinator::ServerQuota> {
    let entry = config.server(server).or_else(|| {
        let primary = replicas.primary_of(server)?;
        config.server(&primary)
    })?;
    Some(pgprox_cluster::coordinator::ServerQuota {
        cap: entry.max_connections,
        guaranteed_fraction: entry.guaranteed_fraction,
    })
}

/// Holds every pool for a server nothing declares a cap for at zero.
///
/// Refusing rather than defaulting, because the cap is the one property the
/// mission gives no graceful degradation to and a number nobody wrote down is
/// not a cap. A zero-limit pool waits rather than opening, so the symptom is
/// clients queueing on that server and the log line says which and why.
///
/// Logged on the transition rather than every tick. `apply_quota` runs once a
/// second and a misconfiguration that persists would otherwise write a line a
/// second for as long as it lasts.
fn hold_at_nothing(
    app: &App,
    all: &[(pgprox_core::ids::PoolKey, pgprox_core::pool::PoolStats)],
    keys: &[pgprox_core::ids::PoolKey],
    server: &pgprox_core::ids::ServerId,
) {
    let was_open = all
        .iter()
        .any(|(key, stats)| &key.server == server && stats.limit > 0);
    if was_open {
        tracing::warn!(
            %server,
            "no cap is declared for this server, so its pools are held at zero: \
             add it to `servers:` in the configuration document, or give the \
             primary it replicates an entry for it to inherit"
        );
    }
    for key in keys {
        app.pool.set_limit(key, 0);
    }
}

/// The per-pool limit an allowance divides into.
///
/// At least one, because a node holding an allowance it cannot spend on any
/// pool is a node that refuses every client while the cluster believes it is
/// serving them.
fn share_per_key(guaranteed: u32, leased: u32, keys: usize) -> u32 {
    let total = guaranteed + leased;
    (total / u32::try_from(keys).unwrap_or(u32::MAX)).max(1)
}

/// The process's soft descriptor limit, if it can be read.
///
/// From `/proc`, because reading `RLIMIT_NOFILE` needs libc and this binary
/// has none: a proc read is a smaller thing to own than a new dependency on a
/// connection path. Returns `None` where the file is absent, which is every
/// platform that is not Linux and is not a reason to fail.
fn descriptor_limit() -> Option<u64> {
    parse_descriptor_limit(&std::fs::read_to_string("/proc/self/limits").ok()?)
}

/// The soft limit out of the text `/proc/self/limits` holds.
///
/// Split from the read by `M17.4`, so the column arithmetic has something to
/// assert against. The soft limit is the fourth whitespace-separated field of
/// the `Max open files` line, and the hard limit is the fifth: reading the
/// wrong one reports a ceiling this process cannot actually reach, which turns
/// the warning this feeds into the opposite of a warning.
fn parse_descriptor_limit(limits: &str) -> Option<u64> {
    limits
        .lines()
        .find(|line| line.starts_with("Max open files"))
        .and_then(|line| line.split_whitespace().nth(3))
        .and_then(|soft| soft.parse().ok())
}

/// How many connections may sit in the kernel's accept queue.
///
/// The default is 1024, and a scale run at a thousand connections overflowed
/// it: `ListenOverflows` on the node counted the drops, and the clients saw a
/// socket that closed with nothing on it, which looks exactly like a proxy
/// bug and is not one. A reconnect storm after a node restarts is the same
/// shape and is the case that matters in production.
///
/// The kernel caps this at `net.core.somaxconn`, so a deployment aiming at the
/// roadmap's 100k raises that too; asking for more than the cap is not an
/// error, it is silently trimmed.
const LISTEN_BACKLOG: u32 = 8192;

/// Binds the client port with a backlog deep enough for a connection storm.
///
/// `TcpListener::bind` does not take one, so the socket is built explicitly.
fn bind_client(addr: SocketAddr) -> std::io::Result<tokio::net::TcpListener> {
    let socket = match addr {
        SocketAddr::V4(_) => tokio::net::TcpSocket::new_v4()?,
        SocketAddr::V6(_) => tokio::net::TcpSocket::new_v6()?,
    };
    // Same as `TcpListener::bind` does, and for the same reason: a node that
    // restarts must not wait out `TIME_WAIT` before it can serve again.
    socket.set_reuseaddr(true)?;
    socket.bind(addr)?;
    socket.listen(LISTEN_BACKLOG)
}

/// What one session needs, built from a node.
///
/// Here rather than in `serve`, because it is the one place the node's parts
/// and a session's needs meet, and two places building it would be two nodes.
#[must_use]
pub fn context(app: &App, shutdown: &Shutdown) -> Context {
    Context {
        // Present on every node and serving nobody until a document names a
        // tenant. ADR 0021 makes off the default; what makes it off is an
        // empty tenant list rather than an absent store, so an operator can
        // turn it on without a restart.
        cache: Some(Arc::clone(&app.cache) as Arc<dyn pgprox_core::cache::QueryCache>),
        slab: Arc::clone(&app.slab),
        routes: Arc::clone(&app.routes),
        recordings: Arc::clone(&app.recordings),
        statics: app.statics.clone(),
        observatory: Arc::clone(&app.observatory) as Arc<dyn pgprox_core::admin::Observatory>,
        // A node with certificates terminates TLS; one without answers `N` and
        // the handshake config decides whether that is allowed.
        tls: app
            .listener_tls
            .clone()
            .map(tokio_rustls::TlsAcceptor::from),
        draining: Shutdown::new(),
        closing: Shutdown::new(),
        node: app.deps.node,
        clock: Arc::clone(&app.deps.clock),
        handshake: HandshakeConfig {
            tls: app.tls_posture(),
            // The names the handshake answers with SASL rather than a token
            // request. A user not in this list is a tenant, whatever it is
            // called.
            static_users: app
                .statics
                .as_ref()
                .map(|statics| vec![statics.user().to_owned()])
                .unwrap_or_default(),
        },
        resolver: Arc::clone(&app.deps.resolver),
        connector: Arc::clone(&app.connector),
        pool: Arc::clone(&app.pool),
        parameters: Arc::new(ParameterCache::new()),
        sessions: Arc::clone(&app.sessions),
        cancels: Arc::new(Registry::new(app.deps.node, Box::new(SystemEntropy))),
        acquire_timeout: ACQUIRE_TIMEOUT,
        login_timeout: LOGIN_TIMEOUT,
        client_idle_timeout: app.config.client_idle_timeout,
        peers: pgprox_core::cluster::StaticPeers::new(BTreeMap::new()),
        replicas: Arc::new(crate::replicas::ReplicaSets::new(
            crate::dial::TcpUpstream::new(Arc::clone(&app.deps.tls)),
            Arc::clone(&app.deps.clock),
            shutdown.clone(),
            Arc::clone(&app.slab),
        )),
        primaries: Arc::new(crate::primary_watch::PrimaryWatches::new(
            crate::dial::TcpUpstream::new(Arc::clone(&app.deps.tls)),
            shutdown.clone(),
            Arc::clone(&app.slab),
            app.deps.invalidation.clone(),
            app.deps.topology.clone(),
        )),
    }
}

/// The probes for a node.
#[must_use]
pub fn probes(app: &App) -> Arc<Probes> {
    Arc::new(Probes::new(
        Arc::clone(&app.health),
        Arc::clone(&app.drain),
        Arc::clone(&app.deps.config),
        Arc::clone(&app.deps.clock),
    ))
}

/// Runs the node until the signal fires.
///
/// # Errors
///
/// Fails when a listening socket does, which means there is nothing left to
/// serve. A failure serving one client is that client's and never reaches here.
pub async fn run(app: App, listeners: Listeners, shutdown: Shutdown) -> std::io::Result<()> {
    run_with_peers(
        app,
        listeners,
        pgprox_core::cluster::StaticPeers::new(BTreeMap::new()),
        shutdown,
    )
    .await
}

/// Runs the node, gossiping to `peers`, until the signal fires.
///
/// # Errors
///
/// As [`run`].
pub async fn run_with_peers(
    app: App,
    listeners: Listeners,
    source: Arc<dyn pgprox_core::cluster::PeerSource>,
    shutdown: Shutdown,
) -> std::io::Result<()> {
    // Set here rather than at build time: the peer table is a deployment fact
    // and `App::build` opens no sockets. A node with no peers keeps the
    // fallback it had, which is its guaranteed share.
    // Read once here, which is deliberate and temporary. `M19.2` changes the
    // signature and nothing else, so the three consumers below still receive a
    // table taken at startup; `M19.3` is the task that makes them read the
    // current one. Splitting it that way keeps the widest diff away from the
    // semantic change.
    app.cluster
        .set_transport(Arc::new(crate::gossip::GossipTransport::new(Arc::clone(
            &source,
        ))));
    // The same source, to the one read that fans out.
    app.observatory.set_peers(Arc::clone(&source));
    // The gossip round and the drain announcement still take a list, and they
    // take the current one: read here, per tick, rather than once at startup.
    let addresses: Vec<String> = source.peers().values().cloned().collect();
    warn_about_descriptors(app.config.max_client_conns);
    let gate = Arc::new(Gate::new(app.config.max_client_conns));
    let context = Arc::new(Context {
        peers: Arc::clone(&source),
        ..context(&app, &shutdown)
    });
    let addresses_for_drain = addresses.clone();
    let probes = probes(&app);

    let admin = tokio::spawn(http::serve(
        listeners.admin,
        http::router(
            Arc::clone(&app.observatory) as pgprox_admin::Shared,
            Arc::clone(&probes),
            app.deps.node,
            Arc::clone(&app.tenants),
            Arc::clone(&app.slab),
            Arc::clone(&app.routes),
        ),
        {
            let shutdown = shutdown.clone();
            async move { shutdown.waited().await }
        },
    ));

    // The configuration is polled rather than watched, and this is the loop
    // that does it. Without it a `ConfigMap` edit reaches a running node never:
    // M4.3 built the poll and M4.4 built the validate-then-swap rule, and both
    // were reachable only from their own tests.
    let reloading = tokio::spawn({
        let source = Arc::clone(&app.deps.config);
        let shutdown = shutdown.clone();
        async move {
            tokio::select! {
                () = source.run_loop() => {}
                () = shutdown.waited() => {}
            }
        }
    });

    let gossiping = tokio::spawn({
        let shutdown = shutdown.clone();
        let cluster = Arc::clone(&app.cluster);
        // The session context is the cancel sink: it holds the registry that
        // knows which upstream connection a key names, and a second registry
        // would be a second answer to that question.
        let cancels = Arc::clone(&context) as Arc<dyn crate::gossip::CancelSink>;
        crate::gossip::serve(listeners.gossip, cluster, cancels, async move {
            shutdown.waited().await;
        })
    });

    let accepting = tokio::spawn({
        let shutdown = shutdown.clone();
        let context = Arc::clone(&context);
        let gate = Arc::clone(&gate);
        async move {
            tokio::select! {
                // Accepting stops the moment the signal fires. The sessions
                // already accepted keep running: they hold transactions, and
                // ending them here is what the drain sequence exists to avoid.
                result = accept_loop(listeners.client, context, gate) => result,
                () = shutdown.waited() => Ok(()),
            }
        }
    });

    let _ticks = ticker(
        &app,
        &context.replicas,
        &probes,
        &addresses,
        &Drainer {
            context: &context,
            gate: &gate,
            addresses: &addresses_for_drain,
            grace: app.config.drain_grace,
        },
        &shutdown,
    )
    .await;

    // Whatever the run loop is stopping for, the clients that are still here
    // are told rather than cut: the signal fires, and their sockets close on a
    // frame boundary in `session` rather than mid-frame.
    context.closing.fire();

    // Both were told to stop by the same signal, so this is waiting rather than
    // stopping. A task that panicked is reported as an error rather than
    // silently ignored: the node is going away either way, but which way is the
    // difference between a clean rollout and a bug nobody saw.
    if let Ok(Err(err)) = accepting.await {
        return Err(err);
    }
    if let Ok(Err(err)) = admin.await {
        return Err(err);
    }
    if let Ok(Err(err)) = gossiping.await {
        return Err(err);
    }
    reloading.abort();
    Ok(())
}

/// The work that happens on a timer, until the signal fires.
///
/// Returns how many ticks it ran. A count rather than nothing, because the
/// work it does is reported to peers and to probes rather than returned, and a
/// loop that silently never ran would look exactly like one that did.
/// Ticks between certificate reloads.
///
/// Derived from the two constants rather than written a third time, so
/// `pgprox_tls::RELOAD_INTERVAL` is the one place the interval is decided. A
/// floor of one, because a `TICK` longer than the interval would otherwise
/// produce zero and a modulo by zero.
const TICKS_PER_RELOAD: u64 = {
    let ticks = pgprox_tls::RELOAD_INTERVAL.as_secs() / TICK.as_secs();
    if ticks == 0 { 1 } else { ticks }
};

/// Whether this tick is one that re-reads the certificate.
///
/// A pure function of the tick count so it can be tested without a runtime, a
/// clock or a file. `M24.9`.
const fn due_for_reload(ran: u64) -> bool {
    ran.is_multiple_of(TICKS_PER_RELOAD)
}

/// Re-reads the listener's certificate, if there is one.
///
/// A node with no certificate has nothing to reload, and a node whose files
/// have not changed does nothing visible. Both are silent; only a rotation and
/// a failure say anything, because a log line a minute is a log nobody reads.
///
/// A failure is logged and swallowed. Certificates are rotated by machines and
/// a half-written file is a normal thing to read: `CertReloader::reload` leaves
/// the previous certificate serving, and the next tick tries again.
fn reload_certificate(reloader: Option<&Arc<pgprox_tls::CertReloader>>) {
    let Some(reloader) = reloader else {
        return;
    };
    match reloader.reload() {
        Ok(true) => tracing::info!("the listener certificate was rotated"),
        Ok(false) => {}
        Err(err) => tracing::warn!(
            %err,
            "could not re-read the listener certificate; the previous one is still serving"
        ),
    }
}

/// Asks each idle client whether it belongs on another node, and closes the
/// ones that do.
///
/// M3.7 built the decision and every guard rail on it, and nothing in the
/// binary ever took one, so tenant affinity was a property of the cluster
/// crate's tests. Nothing here decides: `shed::decide` does, and this only
/// gathers what it needs and acts on the answer.
fn shed_pass(app: &App, sessions: &Arc<Sessions>) -> usize {
    use pgprox_cluster::shed::{self, ShedCtx, ShedDecision};
    use pgprox_core::admin::ClientState;

    let config = shed::ShedConfig::default();
    let now = app.deps.clock.now();
    let mut shed_count = 0;

    for view in sessions.views(now) {
        // The tenant's own allowance, from its grant. A made-up budget here
        // sends a client to a node with no room for it, and it comes straight
        // back.
        let budget = sessions.budget_for(&view.tenant);
        // Tracked so the home node's reservation exists to weigh against: an
        // untracked tenant has no reservation and reads as having no headroom
        // anywhere, which refuses every shed for the wrong reason.
        app.cluster.track_tenant(view.tenant.clone());
        let placement = app.cluster.placement(&view.tenant, budget);
        let decision = shed::decide(
            &config,
            &ShedCtx {
                idle_for: view.since,
                on_home_node: placement.on_home_node,
                home_has_headroom: placement.home_has_headroom,
                home_draining: placement.home_draining,
                pinned: view.pinned.is_some(),
                // The registry knows what a client is doing, and `Active`
                // means it holds a connection. A session between transactions
                // is the only one worth moving and the only one it is safe to.
                in_transaction: view.state != ClientState::Idle,
                since_membership_change: placement.since_membership_change,
                // The window the registry keeps, which is what turns "move
                // this client once" into something other than "move this
                // client every time the tick runs".
                recent_sheds: sessions.recent_sheds(&view.tenant, now),
            },
        );

        if matches!(decision, ShedDecision::Shed) && sessions.shed(view.conn, now) {
            shed_count += 1;
        }
    }
    shed_count
}

/// Holds every pool to what the cluster layer says this node may have.
///
/// The invariant the whole of `pgprox-cluster` exists to protect is that
/// guaranteed plus leased never exceeds a server's cap. It was enforced in a
/// ledger nothing consulted: the pool's limit came from the configuration
/// document and the allowance was read only by the admin surface, so three
/// nodes could each open fifty connections to a server capped at sixty.
///
/// Divided between the pools that exist for that server, because a pool is one
/// database and user and the cap is per server. A node with no pool for a
/// server sets nothing: there is nothing to hold.
async fn apply_quota(app: &App, replicas: &crate::replicas::ReplicaSets) {
    let config = app.deps.config.watch().borrow().clone();
    let all = app.pool.all_stats();

    for server in servers_with_pools(&all) {
        let (keys, held) = pools_for(&all, &server);

        let Some(quota) = declared_quota(&config, replicas, &server) else {
            hold_at_nothing(app, &all, &keys, &server);
            continue;
        };

        // Every tick, from the document currently loaded. Caps used to be
        // registered once during `App::build`, so a reload that raised or
        // lowered one never reached the cluster layer at all and the fleet went
        // on dividing the number it started with. `M70.0`.
        app.cluster.set_cap(server.clone(), quota);

        let mut allowance = app.cluster.allowance(&server);
        // The guaranteed share needs no coordination. More than that is the
        // leader's to grant, and a refusal leaves the node on its share, which
        // is the direction that cannot breach the cap.
        if let Some(want) = wants_more_quota(held, allowance.guaranteed, allowance.leased) {
            // Logged either way. The result used to be discarded with
            // `.is_ok()`, so a node pinned at its guaranteed share while
            // clients queued behind it looked exactly like a node that had
            // never asked, and the difference is the whole diagnosis.
            match pgprox_core::cluster::ClusterCoordinator::request_quota(
                app.cluster.as_ref(),
                &server,
                want,
            )
            .await
            {
                Ok(lease) => {
                    allowance = app.cluster.allowance(&server);
                    tracing::info!(
                        server = %server,
                        want,
                        granted = lease.count(app.deps.clock.now()),
                        guaranteed = allowance.guaranteed,
                        leased = allowance.leased,
                        "quota lease granted"
                    );
                }
                Err(reason) => {
                    tracing::warn!(
                        server = %server,
                        want,
                        held,
                        guaranteed = allowance.guaranteed,
                        leased = allowance.leased,
                        %reason,
                        "quota lease refused: this node stays on its share"
                    );
                }
            }
        }

        let each = share_per_key(allowance.guaranteed, allowance.leased, keys.len());
        for key in keys {
            app.pool.set_limit(&key, each);
        }
    }
}

/// The tenants tracked last tick that this node no longer serves.
///
/// Extracted by `M17.4`, which found the `!` deletable and the `==` flippable
/// with nothing to notice. Both invert the loop into forgetting exactly the
/// tenants this node *is* serving, which drops their reservations while their
/// clients are still connected, and the symptom is a peer opening connections
/// this node had promised to hold.
fn tenants_to_forget(
    tracked: &[pgprox_core::ids::TenantId],
    serving: &[(pgprox_core::ids::TenantId, u32)],
) -> Vec<pgprox_core::ids::TenantId> {
    tracked
        .iter()
        .filter(|tenant| !serving.iter().any(|(seen, _)| seen == *tenant))
        .cloned()
        .collect()
}

/// Whether a gossip round left a peer unanswered.
///
/// A predicate rather than the comparison inline, for the reason
/// [`descriptors_are_short`] is one: `M17.4` found `<` interchangeable with
/// `>`, `==` and `<=` here, and a log line is still behaviour. `<=` warns
/// every round of a healthy fleet, which trains an operator to ignore the one
/// round that mattered; `>` never warns at all, and a node that has lost sight
/// of its peers falls back to its guaranteed share with nothing said.
const fn peers_went_unanswered(reached: usize, peers: usize) -> bool {
    reached < peers
}

/// Whether a shed pass did anything worth a line.
///
/// Same shape and same reason: `>=` reports "shed 0 clients" once a second on
/// every node in the fleet, and `<` never reports a shed at all, which is the
/// event an operator is looking for when a tenant's clients move.
const fn something_happened(shed: usize) -> bool {
    shed > 0
}

/// Moves clients toward their home nodes, unless this node is draining.
///
/// Never while draining: a draining node's clients are leaving anyway, and
/// shedding them toward a home node would move work twice, which is the one
/// thing a drain is supposed to avoid doing to a client that is already going.
///
/// The guard sits here rather than around the call, because `M17.4` found the
/// `!` deletable inside the tick with nothing to notice, and what that leaves
/// is the exact inversion: a draining node that sheds and a healthy one that
/// never does. Beside the operation it guards, it has something to assert.
fn shed_pass_unless_draining(app: &App, sessions: &Arc<Sessions>, draining: bool) -> usize {
    if draining {
        return 0;
    }
    shed_pass(app, sessions)
}

/// Closes clients that have been idle longer than `timeout`.
///
/// A tick-loop pass rather than a per-connection timer, for the size budget
/// `M9.23` set and `M74.0` would otherwise have spent: a `tokio::time::Sleep`
/// held across the relay loop's awaits costs roughly 176 bytes per connection,
/// whether or not that connection ever configures a timeout, because the
/// state machine's size is the union of every branch a `select!` can take.
/// This costs one `Instant` comparison per idle client per second instead, in
/// a walk the shed pass already does.
///
/// Only between transactions, the same guard `shed_pass` uses and for the same
/// reason: a session holding a connection is doing something, whatever the
/// client on the other end of it is doing, and closing it mid-transaction is
/// the one thing this proxy does not do for a reason under its own control.
/// See ADR 0030.
fn idle_timeout_pass(sessions: &Arc<Sessions>, timeout: Duration, now: Instant) -> usize {
    use pgprox_core::admin::ClientState;

    let mut closed = 0;
    for view in sessions.views(now) {
        if view.state == ClientState::Idle
            && view.since >= timeout
            && sessions.close_idle(view.conn)
        {
            closed += 1;
        }
    }
    closed
}

/// Says so when the document in force is not the one on disk.
///
/// A node serving a stale document looks exactly like one serving the current
/// document, which is when an operator most needs to be told which they have.
/// Every tick, because the condition persists until somebody fixes the file,
/// and a single line at the moment it broke would have scrolled away.
///
/// A function for the reason [`warn_about_descriptors`] is one: `M17.4` found
/// the `!` deletable, and the inversion warns on every healthy tick and stays
/// silent on the one that matters.
fn warn_about_stale_config(healthy: bool) {
    if !healthy {
        tracing::warn!("the configuration could not be re-read: serving the last good one");
    }
}

/// What the tick needs to start or reverse a drain.
struct Drainer<'a> {
    context: &'a Arc<Context>,
    /// The admission gate, so the tick can follow the document's ceiling.
    gate: &'a Arc<Gate>,
    addresses: &'a [String],
    grace: Duration,
}

/// Starts or reverses the drain sequence when the node's mode changes.
///
/// Driven from the tick rather than from each caller, because a drain can
/// arrive three ways: the admin API, the configuration document, and a drain
/// TTL expiring underneath both. One place that notices the state changed is
/// what stops those three paths behaving differently.
/// What a tick should do about the drain state, if anything.
///
/// Two facts and four combinations, and only the two where they disagree do
/// anything: the sequence runs when the node has been told to drain and has
/// not yet been, and reverses when it has been drained and no longer should
/// be. The other two are a node that is already in the state it should be in.
///
/// Extracted by `M17.4`. All three mutants of the conditions survived, and
/// each is a fleet-level failure with no local symptom: `||` for `&&` reruns
/// the whole drain sequence every tick of a draining node, and dropping the
/// `!` never starts it at all, so a node told to drain keeps taking traffic
/// while the operator watches for it to go quiet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrainStep {
    /// Run the drain sequence.
    Start,
    /// Undo one that ran.
    Reverse,
    /// The node is already where it should be.
    Nothing,
}

const fn drain_step(draining: bool, signalled: bool) -> DrainStep {
    match (draining, signalled) {
        (true, false) => DrainStep::Start,
        (false, true) => DrainStep::Reverse,
        _ => DrainStep::Nothing,
    }
}

async fn follow_drain(app: &App, probes: &Arc<Probes>, drainer: &Drainer<'_>) {
    let step = drain_step(probes.is_draining(), drainer.context.draining.fired());

    if matches!(step, DrainStep::Start) {
        tracing::warn!(node_id = app.deps.node.get(), "draining");
        let steps = crate::drain::Drain {
            cluster: &app.cluster,
            sessions: &app.sessions,
            peers: drainer.addresses,
            draining: &drainer.context.draining,
            closing: &drainer.context.closing,
            grace: drainer.grace,
        }
        .run()
        .await;
        // The order is the property the sequence guarantees, so it is what the
        // line carries: an operator reading it afterwards can see whether the
        // fleet was told before anyone was closed.
        tracing::info!(steps = ?steps, "drained");
    } else if matches!(step, DrainStep::Reverse) {
        tracing::info!(node_id = app.deps.node.get(), "undraining");
        crate::drain::undrain(
            &app.cluster,
            &drainer.context.draining,
            &drainer.context.closing,
        );
    }
}

async fn ticker(
    app: &App,
    replicas: &Arc<crate::replicas::ReplicaSets>,
    probes: &Arc<Probes>,
    peers: &[String],
    drainer: &Drainer<'_>,
    shutdown: &Shutdown,
) -> u64 {
    let gate = &drainer.gate;
    let mut ticks = tokio::time::interval(TICK);
    let mut ran = 0;
    // The tenants reported last tick, so one that has gone can be forgotten.
    let mut tracked: Vec<pgprox_core::ids::TenantId> = Vec::new();
    loop {
        tokio::select! {
            () = shutdown.waited() => return ran,
            _ = ticks.tick() => ran += 1,
        }

        // Liveness first: a node whose gossip is failing is still alive, and
        // restarting it would drop every client on it.
        probes.beat();

        // The ceiling follows the document, because an operator raising it is
        // usually doing so while the node is refusing connections. The cache
        // follows it for the same reason and in the same place: a tenant added
        // to a ConfigMap starts being served on the next tick, and one removed
        // has its results dropped on it.
        {
            let live = app.deps.config.watch();
            let live = live.borrow();
            gate.set_ceiling(live.max_client_conns);
            app.cache.reconfigure(&live.query_cache);
            // The other half of the same section, and it goes to a different
            // place because it bounds a different resource: `max_bytes` is one
            // figure for the store, `max_entry_bytes` is a buffer held per
            // session while an answer is in flight. `M25.2`.
            app.recordings
                .set_max_bytes(live.query_cache.max_entry_bytes);
        }
        app.cluster.tick();
        app.cluster.report(
            app.sessions.len(),
            app.pool
                .all_stats()
                .into_iter()
                .map(|(key, stats)| (key.server, stats.total()))
                .collect(),
        );
        let per_tenant = app.sessions.per_tenant();
        app.cluster.report_tenants(per_tenant.clone());

        // A tenant this node no longer serves is one it should stop reserving
        // for. Without this the tracked set only ever grows, which in a proxy
        // built for five thousand tenants is a leak with a slow fuse, and the
        // reservations it holds are capacity peers could have used.
        for tenant in tenants_to_forget(&tracked, &per_tenant) {
            app.cluster.forget_tenant(&tenant);
        }
        tracked.clear();
        tracked.extend(per_tenant.into_iter().map(|(tenant, _)| tenant));

        warn_about_stale_config(app.deps.config.is_healthy());

        // Before the reap, so a limit that just dropped is what the reaper
        // measures against.
        apply_quota(app, replicas).await;

        // Idle connections cost the database a slot for as long as the node
        // runs, so this is not housekeeping: it is the other half of the
        // promise that a quiet node holds nothing. `reap_idle` has existed
        // since M5.13 with no caller on a timer.
        // Said goodbye to rather than dropped: `M20.4`. The reaper decides
        // under a lock it may not await inside, so it hands the sockets back
        // and this is where they are told.
        crate::dial::retire(app.pool.reap_idle(&ReapConfig::default())).await;

        // After reporting, so a peer hears this tick's numbers rather than the
        // last one's, and awaited rather than spawned: a round that took longer
        // than a tick would otherwise pile up one task per second against a
        // peer that is already too slow to answer.
        let reached = crate::gossip::round(peers, &app.cluster).await;
        if peers_went_unanswered(reached, peers.len()) {
            // A node that cannot see its peers falls back to its guaranteed
            // share and stops being able to lead, which shows up as capacity
            // that has gone missing. Saying so once a second is noisy; saying
            // nothing leaves the operator to infer it from a quota error.
            tracing::warn!(
                reached,
                peers = peers.len(),
                "some peers did not answer the gossip round"
            );
        }

        let shed = shed_pass_unless_draining(app, &app.sessions, probes.is_draining());
        if something_happened(shed) {
            tracing::info!(shed, "shed clients toward their home nodes");
        }

        if let Some(timeout) = app.config.client_idle_timeout {
            let closed = idle_timeout_pass(&app.sessions, timeout, app.deps.clock.now());
            if closed > 0 {
                tracing::info!(closed, "closed clients idle past the configured timeout");
            }
        }

        // Not every tick. A certificate is rotated on the order of weeks and
        // two files read a minute is a cost nobody has to reason about, where
        // two files read a second for a file that changes monthly is a thing
        // somebody eventually asks about.
        if due_for_reload(ran) {
            reload_certificate(app.deps.listener_certificate.as_ref());
        }

        // Last, so a node that has just been told to drain has already
        // reported its final numbers to the fleet.
        follow_drain(app, probes, drainer).await;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    /// How long a test waits for something real to happen across a socket.
    ///
    /// Not a threshold and not a measurement. It is how long a test is willing
    /// to wait before calling a hang a hang, and every use of it drives real
    /// I/O between spawned nodes, so `start_paused` cannot help: tokio only
    /// auto-advances virtual time when every task is idle, and a socket keeps
    /// them awake.
    ///
    /// Five seconds was chosen on a twenty-core developer machine. On a
    /// two-core runner under llvm-cov instrumentation it is not enough:
    /// `a_cancel_for_a_peers_connection_is_forwarded_from_a_running_node`
    /// failed at 5.085s in CI having asserted nothing wrong, and three tests
    /// in this file had already been raised to ten seconds one at a time,
    /// which is the shape of a number nobody owns.
    ///
    /// Thirty is generous on purpose. Being generous costs one genuinely hung
    /// test thirty seconds instead of five, once. Being tight costs a red
    /// build on work that was correct, which is the expensive kind of wrong.
    const PATIENCE: Duration = Duration::from_secs(30);

    #[test]
    fn a_ceiling_above_the_descriptor_limit_is_reported_with_what_it_needs() {
        // `M17.4`. Every mutant of this survived, including replacing the whole
        // function with nothing, because a warning is invisible to a test that
        // does not read logs. The decision is separate from the warning now, so
        // there is something to assert.
        //
        // The headroom is the point: a node does not need one descriptor per
        // client, it needs those plus what the process already holds, so a
        // ceiling exactly equal to the limit is already short.
        assert_eq!(
            descriptors_are_short(1024, 1024),
            Some(1024 + DESCRIPTOR_HEADROOM),
            "a ceiling equal to the limit was called sufficient"
        );
        assert_eq!(
            descriptors_are_short(1024, 900),
            Some(900 + DESCRIPTOR_HEADROOM)
        );

        // And a limit with room to spare says nothing.
        assert_eq!(descriptors_are_short(65_536, 1024), None);
        assert_eq!(
            descriptors_are_short(1024 + DESCRIPTOR_HEADROOM, 1024),
            None,
            "exactly enough was reported as short"
        );
    }

    /// What one call writes to the log.
    ///
    /// `M17.4`. Every branch in this crate whose only effect is a log line was
    /// untestable, so each one's mutants survived and the argument for
    /// accepting them would have been "it only logs" — which
    /// `docs/internal/standards/observability.md` does not allow, because the line is the
    /// contract. A scoped subscriber is thread-local, so it captures the
    /// calling thread's events and leaves the process-wide one alone.
    fn logged(f: impl FnOnce()) -> String {
        use std::sync::Mutex;

        #[derive(Clone)]
        struct Sink(Arc<Mutex<Vec<u8>>>);

        impl std::io::Write for Sink {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Sink {
            type Writer = Self;
            fn make_writer(&'a self) -> Self::Writer {
                self.clone()
            }
        }

        let buffer = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_writer(Sink(Arc::clone(&buffer)))
            .with_ansi(false)
            .finish();
        tracing::subscriber::with_default(subscriber, f);

        let held = buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        String::from_utf8_lossy(&held).into_owned()
    }

    #[test]
    fn a_ceiling_the_process_cannot_reach_is_warned_about_and_a_reachable_one_is_not() {
        // `M17.4`: replacing this whole function with nothing survived. What
        // it does is emit one line at startup, and that line is the only
        // thing standing between an operator and a node that fails at
        // `accept` for a reason that reads as a network fault.
        let noisy = logged(|| warn_about_descriptors(u32::MAX));
        assert!(
            noisy.contains("file descriptor limit"),
            "a ceiling of four billion clients said nothing: {noisy}"
        );
        // The numbers an operator acts on, not just the prose.
        assert!(noisy.contains("max_client_conns"), "{noisy}");
        assert!(noisy.contains("needed"), "{noisy}");

        // And a ceiling this process can serve says nothing at all. A warning
        // that fires either way is one an operator learns to ignore.
        let quiet = logged(|| warn_about_descriptors(1));
        assert!(quiet.is_empty(), "a ceiling of one client warned: {quiet}");
    }

    #[test]
    fn a_node_serving_a_stale_document_says_so_and_one_serving_the_current_one_does_not() {
        // `M17.4`: the `!` on this was deletable inside the tick, and the
        // inversion warns on every healthy tick of every node in the fleet
        // while staying silent on the one node that is serving a document
        // somebody thought they had replaced. A stale node looks exactly like
        // a current one, which is why the line exists at all.
        let stale = logged(|| warn_about_stale_config(false));
        assert!(
            stale.contains("could not be re-read"),
            "a stale document said nothing: {stale}"
        );

        let current = logged(|| warn_about_stale_config(true));
        assert!(
            current.is_empty(),
            "a node serving the current document warned: {current}"
        );
    }

    #[test]
    fn a_node_asks_for_quota_only_once_it_is_at_its_allowance() {
        // The guaranteed share needs no coordination, so asking below it would
        // be a round trip for capacity the node already has.
        assert_eq!(wants_more_quota(3, 10, 0), None);
        assert_eq!(wants_more_quota(9, 10, 0), None);

        // At the allowance, and past it, it asks for one more than it holds
        // beyond its guarantee. One, because that is the smallest request that
        // changes anything.
        assert_eq!(wants_more_quota(10, 10, 0), Some(1));
        assert_eq!(wants_more_quota(14, 10, 0), Some(5));

        // A lease already granted counts toward the allowance, so a node that
        // has been given room does not immediately ask for more.
        assert_eq!(wants_more_quota(10, 10, 5), None);
        assert_eq!(wants_more_quota(15, 10, 5), Some(6));
    }

    #[test]
    fn an_allowance_divides_across_pools_and_never_reaches_zero() {
        assert_eq!(share_per_key(10, 0, 1), 10);
        assert_eq!(share_per_key(10, 10, 2), 10);
        assert_eq!(share_per_key(9, 0, 2), 4);

        // The floor. A node holding an allowance it cannot spend on any pool
        // refuses every client while the cluster believes it is serving them,
        // which is worse than overshooting a division by one.
        assert_eq!(
            share_per_key(1, 0, 8),
            1,
            "a pool was given a limit of zero"
        );
        assert_eq!(share_per_key(0, 0, 1), 1);
    }

    #[test]
    fn a_servers_pools_are_its_own_and_its_count_includes_the_waiters() {
        // `M17.4`. Both loops inside `apply_quota` were untestable, and five
        // mutants lived there: either `==` filter could become `!=`, and
        // either `+` in the sum could become `-` or `*`.
        let stats = |active, idle, waiting| pgprox_core::pool::PoolStats {
            active,
            idle,
            waiting,
            limit: 50,
        };
        let key = |server, database| pgprox_core::ids::PoolKey::new(server, database, "acme_app");
        let db1 = pgprox_core::ids::ServerId::new("db-1", 5432);
        let db2 = pgprox_core::ids::ServerId::new("db-2", 5432);
        let all = vec![
            (key(db1.clone(), "acme"), stats(3, 4, 5)),
            (key(db2.clone(), "globex"), stats(90, 90, 90)),
            (key(db1.clone(), "initech"), stats(2, 0, 1)),
        ];

        let (keys, held) = pools_for(&all, &db1);
        assert_eq!(keys.len(), 2, "the other server's pool was counted");
        assert!(keys.iter().all(|key| key.server == db1));
        // Three, four and five, then two and one. Waiters included: a caller
        // queued behind the cap is demand the leader has to hear about.
        assert_eq!(held, 15);

        let (keys, held) = pools_for(&all, &db2);
        assert_eq!(keys.len(), 1);
        assert_eq!(held, 270);

        // A server this node holds nothing for asks for nothing.
        let (keys, held) = pools_for(&all, &pgprox_core::ids::ServerId::new("db-3", 5432));
        assert!(keys.is_empty());
        assert_eq!(held, 0);
    }

    #[test]
    fn a_drain_step_is_taken_only_when_the_two_facts_disagree() {
        // `M17.4`: three mutants of the conditions this replaces survived.
        assert_eq!(drain_step(true, false), DrainStep::Start);
        assert_eq!(drain_step(false, true), DrainStep::Reverse);

        // Already there, both ways. Rerunning the sequence every tick is what
        // `||` for `&&` does, and the sequence tells the fleet and closes
        // clients.
        assert_eq!(drain_step(true, true), DrainStep::Nothing);
        assert_eq!(drain_step(false, false), DrainStep::Nothing);
    }

    #[test]
    fn a_tenant_that_has_gone_is_forgotten_and_one_still_here_is_not() {
        // `M17.4`. The inverted forms of this forget every tenant the node is
        // serving, which drops the reservations holding capacity for their
        // clients while those clients are still connected.
        let tenant = pgprox_core::ids::TenantId::new;
        let tracked = vec![tenant("acme"), tenant("globex"), tenant("initech")];
        let serving = vec![(tenant("globex"), 4)];

        assert_eq!(
            tenants_to_forget(&tracked, &serving),
            vec![tenant("acme"), tenant("initech")]
        );

        // Nothing tracked, nothing to forget, whatever is being served.
        assert!(tenants_to_forget(&[], &serving).is_empty());
        // And a node still serving all of them forgets none.
        let all_served: Vec<_> = tracked.iter().cloned().map(|t| (t, 1)).collect();
        assert!(tenants_to_forget(&tracked, &all_served).is_empty());
    }

    #[test]
    fn the_ticks_two_log_gates_fire_on_the_event_and_not_on_the_quiet_case() {
        // `M17.4`: six mutants, every relational operator interchangeable with
        // every other, because a log line is invisible to a test that does not
        // read logs. A gate that fires on the quiet case is a line a second
        // per node, and one that never fires loses the event.
        assert!(peers_went_unanswered(0, 1));
        assert!(peers_went_unanswered(2, 3));
        assert!(!peers_went_unanswered(3, 3), "a full round warned");
        assert!(!peers_went_unanswered(0, 0), "a node alone warned");

        assert!(something_happened(1));
        assert!(something_happened(9));
        assert!(!something_happened(0), "a pass that shed nothing spoke");
    }

    #[test]
    fn the_soft_descriptor_limit_is_the_fourth_column_and_not_the_hard_one() {
        // `M17.4`: `descriptor_limit` returning `Some(1)` survived, because
        // nothing had ever parsed a `/proc/self/limits` at all. The line's
        // shape is what matters: name, then soft, then hard, then units, and
        // reading the hard limit would report a ceiling this process cannot
        // reach, which turns the warning it feeds into the opposite of one.
        let limits = "Limit                     Soft Limit           Hard Limit           Units\n\
             Max open files            1024                 524288               files\n\
             Max locked memory         8388608              8388608              bytes\n";
        assert_eq!(parse_descriptor_limit(limits), Some(1024));

        // `unlimited` does not parse as a number, and a limit that cannot be
        // read is `None` rather than a guess.
        let unlimited =
            "Max open files            unlimited            unlimited            files\n";
        assert_eq!(parse_descriptor_limit(unlimited), None);
        assert_eq!(parse_descriptor_limit(""), None);
        assert_eq!(parse_descriptor_limit("Max open files\n"), None);
    }

    use super::*;
    use crate::wiring::Deps;
    use pgprox_core::auth::FakeCredentialResolver;
    use pgprox_core::clock::SystemClock;
    use pgprox_core::config::{Config, FakeConfigSource, ServerConfig};
    use pgprox_core::ids::{NodeId, ServerId};

    /// A backend on `host`, with the fields a quota test does not care about.
    fn test_backend(host: &str) -> pgprox_core::auth::Backend {
        pgprox_core::auth::Backend {
            server: ServerId::new(host, 5432),
            database: "acme".into(),
            user: "acme_app".into(),
            password: pgprox_core::secret::SecretString::new("hunter2"),
            tls: pgprox_core::auth::TlsMode::Disabled,
        }
    }

    fn deps() -> Deps {
        Deps {
            listener_tls: None,
            listener_certificate: None,
            require_tls: false,
            statics: None,
            node: NodeId::new(1),
            node_name: "pgprox-1".to_owned(),
            clock: Arc::new(SystemClock),
            tls: pgprox_tls::client_config(tokio_rustls::rustls::RootCertStore::empty()).unwrap(),
            config: FakeConfigSource::new(Config {
                servers: vec![ServerConfig {
                    server: ServerId::new("db-1", 5432),
                    max_connections: 10,
                    guaranteed_fraction: 0.5,
                }],
                max_client_conns: 10,
                ..Config::default()
            })
            .unwrap(),
            resolver: Arc::new(FakeCredentialResolver::new()),
            invalidation: None,
            topology: None,
        }
    }

    #[test]
    fn idle_timeout_pass_closes_only_what_has_been_idle_long_enough() {
        use pgprox_core::clock::Clock as _;
        let sessions = Sessions::new();
        let clock = pgprox_core::clock::FakeClock::new();
        let start = clock.now();

        // Idle since the start, so once the clock moves past the timeout this
        // one is a candidate.
        let stale_close = Shutdown::new();
        let _stale = sessions.register(
            pgprox_core::ids::ConnId::new(NodeId::new(1), 1),
            pgprox_core::ids::TenantId::new("acme"),
            NodeId::new(1),
            start,
            16,
            stale_close.clone(),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );

        clock.advance(Duration::from_secs(30));

        // Registered after the clock moved, so it has not been idle long
        // enough even though the stale one has: this is the case that proves
        // the pass reads each client's own age rather than a fleet-wide clock.
        let fresh_close = Shutdown::new();
        let _fresh = sessions.register(
            pgprox_core::ids::ConnId::new(NodeId::new(1), 2),
            pgprox_core::ids::TenantId::new("acme"),
            NodeId::new(1),
            clock.now(),
            16,
            fresh_close.clone(),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );

        let closed = idle_timeout_pass(&sessions, Duration::from_secs(30), clock.now());

        assert_eq!(
            closed, 1,
            "the pass closed {closed}, expected exactly the stale one"
        );
        assert!(
            stale_close.fired(),
            "the client idle long enough was not closed"
        );
        assert!(
            !fresh_close.fired(),
            "a client idle for less than the timeout was closed anyway"
        );
        assert_eq!(sessions.idle_timeouts(), 1);
    }

    #[test]
    fn idle_timeout_pass_leaves_a_session_holding_a_connection_alone() {
        // The same guard `shed_pass` uses, and for the same reason: a session
        // is doing something, whatever the client on the other end of it is
        // doing, and closing it mid-transaction is the one thing this proxy
        // does not do for a reason under its own control.
        use pgprox_core::clock::Clock as _;
        let sessions = Sessions::new();
        let clock = pgprox_core::clock::FakeClock::new();
        let conn = pgprox_core::ids::ConnId::new(NodeId::new(1), 1);
        let close = Shutdown::new();
        let _held = sessions.register(
            conn,
            pgprox_core::ids::TenantId::new("acme"),
            NodeId::new(1),
            clock.now(),
            16,
            close.clone(),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        sessions.set_state(conn, pgprox_core::admin::ClientState::Active, clock.now());
        clock.advance(Duration::from_secs(3_600));

        let closed = idle_timeout_pass(&sessions, Duration::from_secs(30), clock.now());

        assert_eq!(closed, 0);
        assert!(
            !close.fired(),
            "an active session was closed for being idle"
        );
    }

    fn loopback() -> Addrs {
        Addrs {
            client: "127.0.0.1:0".parse().unwrap(),
            admin: "127.0.0.1:0".parse().unwrap(),
            gossip: "127.0.0.1:0".parse().unwrap(),
        }
    }

    #[tokio::test]
    async fn a_running_node_serves_both_ports_and_stops_when_told() {
        let app = App::build(deps()).await.unwrap();
        let listeners = Listeners::bind(loopback()).await.unwrap();
        let addrs = listeners.addrs().unwrap();
        let shutdown = Shutdown::new();

        let running = tokio::spawn(run(app, listeners, shutdown.clone()));

        // The admin port answers.
        let mut probe = tokio::net::TcpStream::connect(addrs.admin).await.unwrap();
        tokio::io::AsyncWriteExt::write_all(
            &mut probe,
            b"GET /readyz HTTP/1.1\r\nHost: p\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
        let mut answer = String::new();
        tokio::io::AsyncReadExt::read_to_string(&mut probe, &mut answer)
            .await
            .unwrap();
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");

        // And the client port is accepting, which is what a driver's connect
        // succeeding means.
        tokio::net::TcpStream::connect(addrs.client).await.unwrap();

        shutdown.fire();
        tokio::time::timeout(PATIENCE, running)
            .await
            .expect("the run loop did not return when signalled")
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn a_signal_that_already_fired_is_observed_rather_than_waited_for() {
        // The drain sequence fires the signal before the tasks that watch it
        // have necessarily reached their await, and one that missed it would
        // hang until the grace timer.
        let shutdown = Shutdown::new();
        assert!(!shutdown.fired());
        shutdown.fire();
        assert!(shutdown.fired());

        tokio::time::timeout(PATIENCE, shutdown.waited())
            .await
            .expect("a signal that had already fired was waited on forever");
    }

    #[tokio::test]
    async fn a_port_already_in_use_fails_the_start() {
        // Rather than a node that came up serving half its surface.
        let held = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let taken = held.local_addr().unwrap();

        let clash = Addrs {
            client: taken,
            ..loopback()
        };
        assert!(Listeners::bind(clash).await.is_err());
    }

    #[tokio::test]
    async fn the_periodic_work_runs_and_stops_with_the_signal() {
        // A node that never ticks stops observing liveness and lease expiry,
        // and looks dead to its peers while believing itself healthy.
        let app = App::build(deps()).await.unwrap();
        let probes = probes(&app);
        let shutdown = Shutdown::new();

        let ticked = tokio::spawn({
            let shutdown = shutdown.clone();
            async move {
                let context = Arc::new(context(&app, &shutdown));
                let gate = Arc::new(Gate::new(10));
                let drainer = Drainer {
                    context: &context,
                    gate: &gate,
                    addresses: &[],
                    grace: Duration::from_millis(50),
                };
                ticker(&app, &context.replicas, &probes, &[], &drainer, &shutdown).await
            }
        });

        tokio::time::sleep(Duration::from_millis(1200)).await;
        shutdown.fire();

        let ran = tokio::time::timeout(PATIENCE, ticked)
            .await
            .expect("the ticker did not stop when signalled")
            .unwrap();
        assert!(ran >= 2, "the periodic work ran {ran} times in 1.2 seconds");
    }

    #[tokio::test]
    async fn two_running_nodes_learn_about_each_other() {
        // The acceptance for M6.29, driven through the run loop rather than
        // through the transport, because the failure it fixes was that nothing
        // called the transport at all.
        let first = App::build(deps()).await.unwrap();
        let second = App::build(Deps {
            node: NodeId::new(2),
            node_name: "pgprox-2".to_owned(),
            ..deps()
        })
        .await
        .unwrap();

        let (one, two) = (
            Listeners::bind(loopback()).await.unwrap(),
            Listeners::bind(loopback()).await.unwrap(),
        );
        let (one_at, two_at) = (one.addrs().unwrap(), two.addrs().unwrap());
        let shutdown = Shutdown::new();

        let cluster = Arc::clone(&first.cluster);
        let running = tokio::spawn(run_with_peers(
            first,
            one,
            pgprox_core::cluster::StaticPeers::new(BTreeMap::from([(
                NodeId::new(2),
                two_at.gossip.to_string(),
            )])),
            shutdown.clone(),
        ));
        let peer = tokio::spawn(run_with_peers(
            second,
            two,
            pgprox_core::cluster::StaticPeers::new(BTreeMap::from([(
                NodeId::new(1),
                one_at.gossip.to_string(),
            )])),
            shutdown.clone(),
        ));

        // Two ticks' worth: the first fires immediately, so one round is
        // enough, and the margin is for a loaded machine rather than for the
        // protocol.
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let learned = cluster
            .digests()
            .iter()
            .any(|digest| digest.node == NodeId::new(2));

        shutdown.fire();
        let _ = tokio::time::timeout(PATIENCE, running).await;
        let _ = tokio::time::timeout(PATIENCE, peer).await;

        assert!(learned, "a running node never heard from its peer");
    }

    #[tokio::test]
    async fn a_drain_through_the_admin_api_takes_the_node_out_of_service() {
        // End to end and in order: the probe fails, gossip says draining, and
        // the listener refuses the next client with 57P01 rather than dropping
        // its socket. Driven through the API because that is one of the three
        // ways a drain arrives, and the run loop is what makes all three
        // behave the same.
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let app = App::build(deps()).await.unwrap();
        let cluster = Arc::clone(&app.cluster);
        let listeners = Listeners::bind(loopback()).await.unwrap();
        let addrs = listeners.addrs().unwrap();
        let shutdown = Shutdown::new();
        let running = tokio::spawn(run(app, listeners, shutdown.clone()));

        assert_eq!(probe(addrs.admin, "GET", "/readyz").await.0, 200);
        assert_eq!(probe(addrs.admin, "POST", "/v1/drain").await.0, 200);

        // The tick is what notices, so this waits for one.
        tokio::time::sleep(Duration::from_millis(1500)).await;

        let (status, body) = probe(addrs.admin, "GET", "/readyz").await;
        assert_eq!(status, 503, "{body}");
        assert_eq!(
            cluster.outgoing().digest.mode,
            pgprox_core::cluster::NodeMode::Draining,
            "the fleet was never told"
        );

        // A client arriving now is told why rather than having its socket
        // dropped, which every driver reports as a network fault instead.
        let mut late = tokio::net::TcpStream::connect(addrs.client).await.unwrap();
        let mut packet = Vec::new();
        pgprox_proto::encode_frontend::startup_message(
            &mut packet,
            pgprox_proto::encode::PROTOCOL_3_0,
            &[("user", "acme_app")],
        );
        late.write_all(&packet).await.unwrap();

        let mut header = [0_u8; 5];
        late.read_exact(&mut header).await.unwrap();
        let len = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
        let mut body = vec![0; len - 4];
        late.read_exact(&mut body).await.unwrap();

        assert_eq!(header[0], pgprox_proto::frame::Tag::ERROR_RESPONSE.get());
        let rendered = String::from_utf8_lossy(&body);
        assert!(rendered.contains("57P01"), "{rendered}");

        shutdown.fire();
        let _ = tokio::time::timeout(PATIENCE, running).await;
    }

    /// One HTTP request against the admin port.
    async fn probe(addr: SocketAddr, method: &str, path: &str) -> (u16, String) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
        socket
            .write_all(
                format!("{method} {path} HTTP/1.1\r\nHost: p\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        let mut answer = String::new();
        socket.read_to_string(&mut answer).await.unwrap();

        let status = answer
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or(0);
        (status, answer)
    }

    #[test]
    fn the_descriptor_limit_is_read_from_the_process() {
        // A node configured for more clients than it has descriptors fails at
        // `accept`, and that failure reads as a network fault. The check has
        // to see a real number for the warning to mean anything.
        let limit = descriptor_limit().expect("no /proc/self/limits on this machine");
        // `M17.4`: `> 0` let `descriptor_limit -> Some(1)` survive, and a
        // ceiling of one descriptor would warn on every node in the fleet
        // forever. Loose on purpose: what is asserted is that a real number
        // came back, not which one. No supported target sets this below 64.
        assert!(limit >= 64, "an implausible soft limit: {limit}");

        // Neither of these asserts a log line; they assert the arithmetic runs
        // on both sides of the comparison without panicking.
        warn_about_descriptors(1);
        warn_about_descriptors(u32::MAX);
    }

    #[tokio::test]
    async fn the_tick_reaps_idle_upstream_connections() {
        // M5.13 made a quiet pool drop to zero and nothing called it on a
        // timer, so it was true of the type and false of the process. A proxy
        // that never lets go holds the database's connection budget for as
        // long as it runs.
        let app = App::build(deps()).await.unwrap();
        let probes = probes(&app);
        let shutdown = Shutdown::new();
        let context = Arc::new(context(&app, &shutdown));
        let gate = Arc::new(Gate::new(10));
        let drainer = Drainer {
            context: &context,
            gate: &gate,
            addresses: &[],
            grace: Duration::from_millis(50),
        };

        // Nothing to reap, which is the case worth checking here: the tick has
        // to survive an empty pool rather than the reaper being exercised
        // twice. `pgprox-pool` owns what reaping decides.
        tokio::spawn({
            let shutdown = shutdown.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(1200)).await;
                shutdown.fire();
            }
        });

        let ran = tokio::time::timeout(
            Duration::from_secs(5),
            ticker(&app, &context.replicas, &probes, &[], &drainer, &shutdown),
        )
        .await
        .expect("the tick stopped when the reaper was added");

        assert!(ran >= 1);
    }

    #[tokio::test]
    async fn a_document_that_adds_a_tenant_turns_the_cache_on_without_a_restart() {
        // `M9.13`'s acceptance. The store is built once, at startup, when no
        // tenant has opted in; what a document changes is who it serves, and
        // the tick loop is what carries that across. A node that had to be
        // restarted to start caching would make every opt-in a deploy.
        use pgprox_core::cache::QueryCache;

        let acme = pgprox_core::ids::TenantId::new("acme");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, "max_client_conns: 100\n").unwrap();

        let source = pgprox_config::provider::FileSource::new(
            pgprox_config::provider::FileConfig::at(&path),
        )
        .unwrap();
        let app = App::build(Deps {
            config: Arc::clone(&source) as Arc<dyn pgprox_core::config::ConfigSource>,
            ..deps()
        })
        .await
        .unwrap();

        assert!(
            !app.cache.serves(&acme),
            "a document with no query_cache section built a node that caches"
        );

        let probes = probes(&app);
        let shutdown = Shutdown::new();
        let context = Arc::new(context(&app, &shutdown));
        let gate = Arc::new(Gate::new(10));
        let drainer = Drainer {
            context: &context,
            gate: &gate,
            addresses: &[],
            grace: Duration::from_millis(50),
        };

        // The poll is what notices the file, and nothing starts it here: in a
        // running node `run` does, and this test drives the tick loop alone.
        let polling = tokio::spawn(pgprox_core::config::ConfigSource::run_loop(Arc::clone(
            &source,
        )));

        std::fs::write(
            &path,
            "max_client_conns: 100\nquery_cache:\n  tenants:\n    acme: { ttl: 5s }\n",
        )
        .unwrap();

        let turned_on = tokio::time::timeout(PATIENCE, async {
            tokio::select! {
                () = async {
                    loop {
                        if app.cache.serves(&acme) {
                            return;
                        }
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                } => {}
                _ = ticker(&app, &context.replicas, &probes, &[], &drainer, &shutdown) => {}
            }
        })
        .await;

        shutdown.fire();
        polling.abort();
        turned_on.expect("a tenant added to the document never reached the running cache");
        assert!(app.cache.serves(&acme));
    }

    #[tokio::test]
    async fn the_per_answer_cap_reaches_a_running_node_from_the_document() {
        // `M25.2`. It was a `const` in `serve.rs` while `max_bytes`, the budget
        // it interacts with, was configuration that reloads live. An operator
        // who raised the budget still could not cache a larger result, and
        // nothing they could read said why.
        //
        // The same route `max_client_conns` takes to the gate, because the
        // reason is the same: `Context` is built once, and a value that only
        // reached it at startup is a value an operator restarts a pod to change.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, "max_client_conns: 100\n").unwrap();

        let source = pgprox_config::provider::FileSource::new(
            pgprox_config::provider::FileConfig::at(&path),
        )
        .unwrap();
        let app = App::build(Deps {
            config: Arc::clone(&source) as Arc<dyn pgprox_core::config::ConfigSource>,
            ..deps()
        })
        .await
        .unwrap();

        let default = pgprox_core::config::QueryCacheConfig::default().max_entry_bytes;
        assert_eq!(
            app.recordings.max_bytes(),
            default,
            "a document with no query_cache section changed the bound"
        );

        let probes = probes(&app);
        let shutdown = Shutdown::new();
        let context = Arc::new(context(&app, &shutdown));
        let gate = Arc::new(Gate::new(10));
        let drainer = Drainer {
            context: &context,
            gate: &gate,
            addresses: &[],
            grace: Duration::from_millis(50),
        };

        let polling = tokio::spawn(pgprox_core::config::ConfigSource::run_loop(Arc::clone(
            &source,
        )));

        std::fs::write(
            &path,
            "max_client_conns: 100\nquery_cache:\n  max_bytes: 64MiB\n  max_entry_bytes: 4MiB\n",
        )
        .unwrap();

        let raised = tokio::time::timeout(PATIENCE, async {
            tokio::select! {
                () = async {
                    loop {
                        if app.recordings.max_bytes() == 4 * 1024 * 1024 {
                            return;
                        }
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                } => {}
                _ = ticker(&app, &context.replicas, &probes, &[], &drainer, &shutdown) => {}
            }
        })
        .await;

        shutdown.fire();
        polling.abort();
        raised.expect("a raised per-answer cap never reached the running recorder");
        assert_eq!(app.recordings.max_bytes(), 4 * 1024 * 1024);
    }

    #[tokio::test]
    async fn a_document_rewritten_on_disk_reaches_a_running_node() {
        // M4.3 built the poll and M4.4 built validate-then-swap, and nothing
        // started either, so a ConfigMap edit reached a running node never.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, "max_client_conns: 100\n").unwrap();

        let source = pgprox_config::provider::FileSource::new(
            pgprox_config::provider::FileConfig::at(&path),
        )
        .unwrap();
        let app = App::build(Deps {
            config: Arc::clone(&source) as Arc<dyn pgprox_core::config::ConfigSource>,
            ..deps()
        })
        .await
        .unwrap();

        let watching = app.deps.config.watch();
        let listeners = Listeners::bind(loopback()).await.unwrap();
        let shutdown = Shutdown::new();
        let running = tokio::spawn(run(app, listeners, shutdown.clone()));

        std::fs::write(&path, "max_client_conns: 250\n").unwrap();

        let reloaded = tokio::time::timeout(PATIENCE, async {
            let mut watching = watching;
            loop {
                if watching.borrow_and_update().max_client_conns == 250 {
                    return;
                }
                let _ = watching.changed().await;
            }
        })
        .await;

        shutdown.fire();
        let _ = tokio::time::timeout(PATIENCE, running).await;
        reloaded.expect("a rewritten document never reached the running node");
    }

    #[tokio::test]
    async fn a_broken_document_leaves_the_previous_one_serving() {
        // A typo in a ConfigMap is routine. Taking a node with clients on it
        // down for one would make every config edit a deploy.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, "max_client_conns: 100\n").unwrap();

        let source = pgprox_config::provider::FileSource::new(
            pgprox_config::provider::FileConfig::at(&path),
        )
        .unwrap();
        let app = App::build(Deps {
            config: Arc::clone(&source) as Arc<dyn pgprox_core::config::ConfigSource>,
            ..deps()
        })
        .await
        .unwrap();

        let watching = app.deps.config.watch();
        let listeners = Listeners::bind(loopback()).await.unwrap();
        let shutdown = Shutdown::new();
        let running = tokio::spawn(run(app, listeners, shutdown.clone()));

        std::fs::write(&path, "max_client_conns: not a number\n").unwrap();
        tokio::time::sleep(Duration::from_millis(2500)).await;

        assert_eq!(
            watching.borrow().max_client_conns,
            100,
            "a broken document replaced a good one"
        );
        assert!(!source.is_healthy(), "the failure was not reported");

        shutdown.fire();
        let _ = tokio::time::timeout(PATIENCE, running).await;
    }

    #[tokio::test]
    async fn a_tenant_that_has_gone_is_forgotten_by_the_next_tick() {
        // Without this the tracked set only ever grows, which in a proxy built
        // for five thousand tenants is a leak with a slow fuse, and the
        // reservations it holds are capacity peers could have used.
        let app = App::build(deps()).await.unwrap();
        let probes = probes(&app);
        let shutdown = Shutdown::new();
        let context = Arc::new(context(&app, &shutdown));
        let gate = Arc::new(Gate::new(10));
        let drainer = Drainer {
            context: &context,
            gate: &gate,
            addresses: &[],
            grace: Duration::from_millis(50),
        };

        let tenant = pgprox_core::ids::TenantId::new("acme");
        let held = app.sessions.register(
            pgprox_core::ids::ConnId::new(NodeId::new(1), 1),
            tenant.clone(),
            NodeId::new(1),
            app.deps.clock.now(),
            16,
            Shutdown::new(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );

        tokio::spawn({
            let shutdown = shutdown.clone();
            async move {
                // Two ticks: the first sees the tenant, the second sees it
                // gone. One tick could not tell the two states apart.
                tokio::time::sleep(Duration::from_millis(1100)).await;
                drop(held);
                tokio::time::sleep(Duration::from_millis(1100)).await;
                shutdown.fire();
            }
        });

        let ran = tokio::time::timeout(
            Duration::from_secs(10),
            ticker(&app, &context.replicas, &probes, &[], &drainer, &shutdown),
        )
        .await
        .expect("the tick did not stop");

        assert!(ran >= 2, "the tick ran {ran} times");
        assert!(
            !app.cluster
                .outgoing()
                .digest
                .tenant_usage
                .iter()
                .any(|(seen, _)| seen == &tenant),
            "a tenant with no sessions was still being reported"
        );
    }

    #[tokio::test]
    async fn the_pools_are_held_to_what_the_cluster_layer_allows() {
        // The invariant the whole of pgprox-cluster exists to protect, which
        // was enforced in a ledger nothing consulted: the pool's limit came
        // from the configuration document and the allowance was read only by
        // the admin surface.
        let app = App::build(deps()).await.unwrap();
        let key = pgprox_core::ids::PoolKey::new(
            pgprox_core::ids::ServerId::new("db-1", 5432),
            "acme",
            "acme_app",
        );

        // A pool with a limit far above anything the cluster would allow,
        // which is what the configuration document alone would have given it.
        app.pool.set_limit(&key, 999);
        let ctx = context(&app, &Shutdown::new());
        apply_quota(&app, &ctx.replicas).await;

        let allowance = app
            .cluster
            .allowance(&pgprox_core::ids::ServerId::new("db-1", 5432));
        let limit = app
            .pool
            .all_stats()
            .into_iter()
            .find(|(held, _)| held == &key)
            .map(|(_, stats)| stats.limit)
            .expect("the pool vanished");

        assert!(
            limit <= allowance.guaranteed + allowance.leased,
            "a pool was allowed {limit} against an allowance of {allowance:?}"
        );
        assert!(limit > 0, "a pool was allowed nothing at all");
    }

    #[tokio::test]
    async fn a_pool_for_a_server_the_document_never_named_is_held_at_zero() {
        // `M70.0`. The loop iterated `config.servers`, so a pool whose server
        // the document does not name was never passed to `set_limit` at all: it
        // kept the default `PoolConfig` gave it and no allowance ever reached
        // it. Three nodes each holding that default is a cap nobody declared
        // being exceeded by a factor nobody chose.
        let app = App::build(deps()).await.unwrap();
        let stranger = pgprox_core::ids::PoolKey::new(
            pgprox_core::ids::ServerId::new("db-unknown", 5432),
            "acme",
            "acme_app",
        );
        app.pool.set_limit(&stranger, 50);

        let ctx = context(&app, &Shutdown::new());
        apply_quota(&app, &ctx.replicas).await;

        let limit = app
            .pool
            .all_stats()
            .into_iter()
            .find(|(key, _)| key == &stranger)
            .map(|(_, stats)| stats.limit)
            .expect("the pool vanished");
        assert_eq!(
            limit, 0,
            "a server with no declared cap was allowed {limit} connections"
        );
    }

    #[tokio::test]
    async fn a_replica_inherits_the_cap_of_the_primary_it_replicates() {
        // The other half, and the reason zero is not simply the answer.
        // Replicas arrive from the sidecar at runtime, so an operator cannot
        // list a host it has not been told about. Holding every replica at
        // zero would make read routing configuration-impossible rather than
        // safe.
        let app = App::build(deps()).await.unwrap();
        let ctx = context(&app, &Shutdown::new());

        // A grant naming db-1, which the document does declare, and a replica
        // it does not.
        let grant = pgprox_core::auth::Grant {
            tenant: pgprox_core::ids::TenantId::new("acme"),
            primary: test_backend("db-1"),
            replicas: vec![test_backend("db-replica")],
            pool: pgprox_core::auth::PoolHints::default(),
            ttl: Duration::from_secs(60),
            claims: pgprox_core::auth::ClaimSet::default(),
        };
        let _watch = ctx.replicas.watch_for(&grant).expect("a watch");

        let key = pgprox_core::ids::PoolKey::new(
            pgprox_core::ids::ServerId::new("db-replica", 5432),
            "acme",
            "acme_app",
        );
        app.pool.set_limit(&key, 50);
        apply_quota(&app, &ctx.replicas).await;

        let limit = app
            .pool
            .all_stats()
            .into_iter()
            .find(|(seen, _)| seen == &key)
            .map(|(_, stats)| stats.limit)
            .expect("the pool vanished");
        assert!(
            limit > 0,
            "a replica of a declared primary inherited no cap, so read routing cannot open one"
        );
        let allowance = app
            .cluster
            .allowance(&pgprox_core::ids::ServerId::new("db-replica", 5432));
        assert!(
            limit <= allowance.guaranteed + allowance.leased,
            "an inherited cap was not coordinated: {limit} against {allowance:?}"
        );
    }

    #[tokio::test]
    async fn a_cap_the_document_changes_reaches_the_cluster() {
        // Caps were registered once, during `App::build`. A reload that raised
        // or lowered one never reached the cluster layer, so the fleet went on
        // dividing the number it started with while the admin surface reported
        // the new one. `M70.0`.
        let server = pgprox_core::ids::ServerId::new("db-1", 5432);
        let source = FakeConfigSource::new(Config {
            servers: vec![ServerConfig {
                server: server.clone(),
                max_connections: 10,
                guaranteed_fraction: 0.5,
            }],
            max_client_conns: 10,
            ..Config::default()
        })
        .unwrap();
        let app = App::build(Deps {
            config: Arc::clone(&source) as Arc<dyn pgprox_core::ConfigSource>,
            ..deps()
        })
        .await
        .unwrap();
        let key = pgprox_core::ids::PoolKey::new(server.clone(), "acme", "acme_app");
        app.pool.set_limit(&key, 1);

        let before = app.cluster.allowance(&server);
        source
            .publish(Config {
                servers: vec![ServerConfig {
                    server: server.clone(),
                    max_connections: 200,
                    guaranteed_fraction: 0.5,
                }],
                max_client_conns: 10,
                ..Config::default()
            })
            .unwrap();

        let ctx = context(&app, &Shutdown::new());
        apply_quota(&app, &ctx.replicas).await;

        let after = app.cluster.allowance(&server);
        assert!(
            after.guaranteed > before.guaranteed,
            "a raised cap did not reach the cluster: {before:?} then {after:?}"
        );
    }

    #[tokio::test]
    async fn a_client_whose_tenant_belongs_elsewhere_is_shed() {
        // M3.7 built the decision and every guard rail on it, and nothing in
        // the binary ever took one, so tenant affinity was a property of one
        // crate's tests rather than of a running fleet.
        let clock = pgprox_core::clock::FakeClock::new();
        let app = App::build(Deps {
            clock: Arc::new(clock.clone()),
            ..deps()
        })
        .await
        .unwrap();
        let close = Shutdown::new();

        // A peer, gossiping every second as the real loop does. A membership
        // of one homes every tenant here, and advancing the clock in one jump
        // would let the peer go silent and take the membership back to one.
        let peer = |round: u64| pgprox_cluster::digest::VersionedDigest {
            digest: pgprox_core::cluster::ClusterDigest {
                node: NodeId::new(2),
                ..pgprox_core::cluster::ClusterDigest::default()
            },
            version: round,
        };
        app.cluster.gossip(peer(1));
        // Ticked before anything is chosen, so the membership the selection
        // sees is the membership the assertion sees. Without it the fleet is
        // one node during selection and two after the first tick, and the
        // entitlement each node gets is divided by a different number in the
        // two places.
        app.cluster.tick();

        // Homed elsewhere *and* with room at that home, which are two
        // conditions and not one: a client is not shed toward a node that
        // cannot take it. Selecting on the first alone passed only while
        // `active_count` undercounted the fleet by one, which is the bug the
        // self-heartbeat fixed.
        let tenant = (1..200)
            .map(|n| pgprox_core::ids::TenantId::new(format!("tenant-{n}")))
            .find(|tenant| {
                // Tracked first, exactly as `shed_pass` does: an untracked
                // tenant has no reservation at its home and reads as having no
                // room there, which would make this loop reject every
                // candidate for a reason that has nothing to do with placement.
                app.cluster.track_tenant(tenant.clone());
                let placement = app.cluster.placement(tenant, 16);
                !placement.on_home_node && placement.home_has_headroom
            })
            .expect("no tenant of two hundred homed elsewhere with room, which is not hashing");

        let _held = app.sessions.register(
            pgprox_core::ids::ConnId::new(NodeId::new(1), 1),
            tenant,
            NodeId::new(1),
            app.deps.clock.now(),
            16,
            close.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );

        // Past the idle threshold and past the settle window: both guard rails
        // are about time, and a test that did not move it would be asserting
        // that they refuse rather than that the decision works.
        for round in 2..=45 {
            app.cluster.gossip(peer(round));
            clock.advance(Duration::from_secs(1));
        }
        app.cluster.tick();

        let view = app.sessions.views(app.deps.clock.now()).remove(0);
        let placement = app.cluster.placement(&view.tenant, 1);

        // A draining node leaves it alone, and this is the only place the two
        // can be compared: the same client, the same instant, the same
        // placement. `M17.4` found the `!` on this guard deletable inside the
        // tick, and the inversion is a draining node shedding clients that
        // were already leaving while a healthy one moves nobody.
        assert_eq!(
            shed_pass_unless_draining(&app, &app.sessions, true),
            0,
            "a draining node shed a client that was on its way out anyway"
        );
        assert!(!close.fired(), "a draining node asked a client to leave");
        assert_eq!(app.sessions.sheds(), 0);

        let shed = shed_pass_unless_draining(&app, &app.sessions, false);

        assert_eq!(
            shed, 1,
            "an idle client of another node's tenant was kept: idle_for={:?}, {placement:?}",
            view.since
        );
        assert!(close.fired(), "the session was never asked to leave");
        assert_eq!(app.sessions.sheds(), 1);

        // And the next tick sees that it happened, which is what the rate
        // limit weighs. Before M6.46 this was always zero and the limit could
        // never refuse.
        assert_eq!(
            app.sessions
                .recent_sheds(&view.tenant, app.deps.clock.now()),
            1
        );
    }

    #[tokio::test]
    async fn a_client_of_a_tenant_this_node_homes_is_kept() {
        // Moving it achieves nothing, and every move costs a reconnect.
        let app = App::build(deps()).await.unwrap();
        let close = Shutdown::new();
        // A node is in the membership because somebody gossiped about it, and
        // a node does not gossip to itself, so without this the fleet has no
        // members and no tenant has a home at all.
        app.cluster.gossip(pgprox_cluster::digest::VersionedDigest {
            digest: pgprox_core::cluster::ClusterDigest {
                node: NodeId::new(1),
                ..pgprox_core::cluster::ClusterDigest::default()
            },
            version: 1,
        });

        let tenant = (1..200)
            .map(|n| pgprox_core::ids::TenantId::new(format!("tenant-{n}")))
            .find(|tenant| app.cluster.placement(tenant, 16).on_home_node)
            .expect("a fleet of one homes every tenant");

        let _held = app.sessions.register(
            pgprox_core::ids::ConnId::new(NodeId::new(1), 1),
            tenant,
            NodeId::new(1),
            app.deps.clock.now(),
            16,
            close.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );

        assert_eq!(shed_pass(&app, &app.sessions), 0);
        assert!(!close.fired());
    }

    #[tokio::test]
    async fn a_session_context_carries_the_node_s_own_parts() {
        // Two pools or two connectors would mean the grant path teaching one
        // and the sessions using the other.
        let app = App::build(deps()).await.unwrap();
        let context = context(&app, &Shutdown::new());

        assert!(Arc::ptr_eq(&context.pool, &app.pool));
        assert!(Arc::ptr_eq(&context.connector, &app.connector));
        assert!(Arc::ptr_eq(&context.sessions, &app.sessions));
    }

    #[tokio::test]
    async fn a_cancel_for_a_peers_connection_is_forwarded_from_a_running_node() {
        // `M17.4`: deleting `peers` from the `Context` this builds survived.
        // The peer table reaches three places from here, and two of them are
        // separate calls that a test could see: the quota transport and the
        // observatory. The third is this field, and it is the one the serving
        // path reads, so a node built without it accepts a cancel for another
        // pod and drops it. Cancelling a query then works one time in N, which
        // is the exact defect `M6.30` fixed and nothing would have caught its
        // return.
        //
        // The peer is a plain listener rather than a second node: what is
        // under test is that this node forwards at all, and the forwarded
        // message is a line on the gossip socket.
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let peer = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let peer_at = peer.local_addr().unwrap();
        // Every connection, not the first. The node's own gossip round dials
        // this same address once a tick, so the first thing to arrive is
        // whichever of the two the scheduler ran: taking one connection and
        // asserting on one line made this test fail about one run in eight,
        // with a digest where the cancel should have been. `M17.5` is the
        // reason that was checked rather than shipped.
        let (caught, catch) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Ok((socket, _)) = peer.accept().await {
                let caught = caught.clone();
                tokio::spawn(async move {
                    let (read, mut write) = tokio::io::split(socket);
                    let mut lines = BufReader::new(read).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        // Recorded before answering, and the order is the whole
                        // point. `gossip::forward` connects, writes the cancel,
                        // flushes and drops the stream, so by the time this
                        // replies the far end is often already gone and the
                        // write fails with a broken pipe. Answering first meant
                        // returning on that failure and discarding the line
                        // just read, which is the one the test is waiting for.
                        //
                        // It passed locally because a write to a socket the
                        // peer has closed succeeds until the RST lands, and it
                        // failed every time on a GitHub runner, where it does
                        // not. `M56.0` raised the timeout from five seconds to
                        // thirty on the theory that the runner was slow; the
                        // test then failed at exactly thirty, which is what
                        // ruled slowness out and pointed here. `M57.0`.
                        if caught.send(line).is_err() {
                            return;
                        }
                        // Answered like a peer, not just recorded. A listener
                        // that accepts and says nothing makes every gossip
                        // round wait out `PEER_TIMEOUT`, and the node is inside
                        // one when the shutdown lands: that alone was two to
                        // five seconds of this test, which is `M17.7`'s whole
                        // subject.
                        //
                        // A failed reply ends this connection and nothing else.
                        // The cancel path never reads one, and a digest round
                        // whose peer has hung up has no answer to wait for.
                        let digest = br#"{"kind":"digest","node":2,"mode":"active","version":1,"client_conns":0,"upstream_conns":[],"tenant_usage":[]}"#;
                        if write.write_all(digest).await.is_err()
                            || write.write_all(b"\n").await.is_err()
                        {
                            return;
                        }
                    }
                });
            }
        });

        let app = App::build(deps()).await.unwrap();
        let listeners = Listeners::bind(loopback()).await.unwrap();
        let addrs = listeners.addrs().unwrap();
        let shutdown = Shutdown::new();
        let running = tokio::spawn(run_with_peers(
            app,
            listeners,
            pgprox_core::cluster::StaticPeers::new(BTreeMap::from([(
                NodeId::new(2),
                peer_at.to_string(),
            )])),
            shutdown.clone(),
        ));

        // A key node 2 issued, arriving at node 1. A `CancelRequest` carries
        // no startup packet and gets no answer, so the socket is all it is.
        let conn = pgprox_core::ids::ConnId::new(NodeId::new(2), 0x00AB_CDEF);
        let (process_id, secret) = pgprox_proto::backend::key_from_conn_id(conn);
        let mut packet = Vec::new();
        pgprox_proto::encode_frontend::cancel_request(&mut packet, process_id, secret);

        let mut client = tokio::net::TcpStream::connect(addrs.client).await.unwrap();
        client.write_all(&packet).await.unwrap();

        let mut catch = catch;
        let forwarded = tokio::time::timeout(PATIENCE, async {
            while let Some(line) = catch.recv().await {
                if line.contains(r#""kind":"cancel""#) {
                    return line;
                }
            }
            unreachable!("the peer listener stopped before the cancel arrived")
        })
        .await
        .expect("the cancel was never forwarded to the peer");

        assert_eq!(
            forwarded, r#"{"kind":"cancel","node":2,"secret":11259375}"#,
            "the forwarded cancel named the wrong connection"
        );

        drop(client);
        shutdown.fire();
        let _ = tokio::time::timeout(PATIENCE, running).await;
    }
    #[test]
    fn the_certificate_is_re_read_on_a_minute_rather_than_a_tick() {
        // `M24.9`. Two small files a minute is a cost nobody has to reason
        // about; two files a second, for a file that changes monthly, is a
        // thing somebody eventually asks about.
        assert_eq!(
            TICKS_PER_RELOAD,
            pgprox_tls::RELOAD_INTERVAL.as_secs() / TICK.as_secs(),
            "the two constants disagree, so the interval is decided twice"
        );
        const {
            assert!(TICKS_PER_RELOAD > 1, "every tick re-reads the certificate");
        }

        assert!(due_for_reload(0));
        assert!(due_for_reload(TICKS_PER_RELOAD));
        assert!(due_for_reload(TICKS_PER_RELOAD * 2));
        for ran in 1..TICKS_PER_RELOAD {
            assert!(!due_for_reload(ran), "tick {ran} re-read the certificate");
        }
    }

    #[tokio::test]
    async fn a_rotated_certificate_reaches_a_running_listener() {
        // The wiring half. `pgprox-tls` proves the reloader notices a rewrite;
        // this proves the node asks it to. Until `M24.9` nothing did, and
        // architecture.md had credited the crate with hot reload since M-1.
        let dir = std::env::temp_dir().join(format!("pgprox-reload-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cert_path = dir.join("cert.pem");
        let key_path = dir.join("key.pem");
        write_cert(&cert_path, &key_path, "before.example");

        let reloader = pgprox_tls::CertReloader::new(&cert_path, &key_path).unwrap();
        let before = reloader.serving();

        // What the tick does, on a tick that is due.
        reload_certificate(Some(&reloader));
        assert_eq!(reloader.serving(), before, "nothing changed and it changed");

        write_cert(&cert_path, &key_path, "after.example");
        reload_certificate(Some(&reloader));
        assert_ne!(
            reloader.serving(),
            before,
            "the tick did not carry the rotation to the listener"
        );

        // A node with no certificate has nothing to reload, and must not
        // panic on the tick that would have.
        reload_certificate(None);

        // A half-written file leaves the previous certificate serving rather
        // than taking the listener down.
        let rotated = reloader.serving();
        std::fs::write(&cert_path, b"-----BEGIN CERTIFICATE-----\nhalf").unwrap();
        reload_certificate(Some(&reloader));
        assert_eq!(
            reloader.serving(),
            rotated,
            "a half-written rotation took the live certificate with it"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Writes a self-signed certificate and its key to two paths.
    fn write_cert(cert_path: &std::path::Path, key_path: &std::path::Path, name: &str) {
        let cert = rcgen::generate_simple_self_signed(vec![name.into()]).unwrap();
        std::fs::write(cert_path, cert.cert.pem()).unwrap();
        std::fs::write(key_path, cert.signing_key.serialize_pem()).unwrap();
    }
}
