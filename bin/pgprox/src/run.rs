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
use std::time::Duration;

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
fn warn_about_descriptors(ceiling: u32) {
    let Some(limit) = descriptor_limit() else {
        return;
    };
    let needed = u64::from(ceiling) + DESCRIPTOR_HEADROOM;
    if limit < needed {
        tracing::warn!(
            soft_limit = limit,
            max_client_conns = ceiling,
            needed,
            "the file descriptor limit is below this node's client ceiling: \
             connections past it will fail at accept, which reads as a network fault"
        );
    }
}

/// The process's soft descriptor limit, if it can be read.
///
/// From `/proc`, because reading `RLIMIT_NOFILE` needs libc and this binary
/// has none: a proc read is a smaller thing to own than a new dependency on a
/// connection path. Returns `None` where the file is absent, which is every
/// platform that is not Linux and is not a reason to fail.
fn descriptor_limit() -> Option<u64> {
    let limits = std::fs::read_to_string("/proc/self/limits").ok()?;
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
        // Off. ADR 0021 makes that the default, and `M9.8` is what a config
        // document will use to say otherwise.
        cache: None,
        slab: Arc::clone(&app.slab),
        routes: Arc::clone(&app.routes),
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
        peers: BTreeMap::new(),
        replicas: Arc::new(crate::replicas::ReplicaSets::new(
            crate::dial::TcpUpstream::new(Arc::clone(&app.deps.tls)),
            Arc::clone(&app.deps.clock),
            shutdown.clone(),
            Arc::clone(&app.slab),
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
    run_with_peers(app, listeners, BTreeMap::new(), shutdown).await
}

/// Runs the node, gossiping to `peers`, until the signal fires.
///
/// # Errors
///
/// As [`run`].
pub async fn run_with_peers(
    app: App,
    listeners: Listeners,
    peers: BTreeMap<pgprox_core::ids::NodeId, String>,
    shutdown: Shutdown,
) -> std::io::Result<()> {
    // Set here rather than at build time: the peer table is a deployment fact
    // and `App::build` opens no sockets. A node with no peers keeps the
    // fallback it had, which is its guaranteed share.
    app.cluster
        .set_transport(Arc::new(crate::gossip::GossipTransport::new(peers.clone())));
    // The same table, to the one read that fans out.
    app.observatory.set_peers(peers.clone());
    let addresses: Vec<String> = peers.values().cloned().collect();
    warn_about_descriptors(app.config.max_client_conns);
    let gate = Arc::new(Gate::new(app.config.max_client_conns));
    let context = Arc::new(Context {
        peers: peers.clone(),
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
async fn apply_quota(app: &App) {
    let config = app.deps.config.watch().borrow().clone();

    for server in &config.servers {
        let keys: Vec<pgprox_core::ids::PoolKey> = app
            .pool
            .all_stats()
            .into_iter()
            .map(|(key, _)| key)
            .filter(|key| key.server == server.server)
            .collect();
        if keys.is_empty() {
            continue;
        }

        let mut allowance = app.cluster.allowance(&server.server);
        // The guaranteed share needs no coordination. More than that is the
        // leader's to grant, and a refusal leaves the node on its share, which
        // is the direction that cannot breach the cap.
        let held: u32 = app
            .pool
            .all_stats()
            .into_iter()
            .filter(|(key, _)| key.server == server.server)
            .map(|(_, stats)| stats.active + stats.idle + stats.waiting)
            .sum();
        if held >= allowance.guaranteed + allowance.leased {
            let want = held.saturating_sub(allowance.guaranteed) + 1;
            // Logged either way. The result used to be discarded with
            // `.is_ok()`, so a node pinned at its guaranteed share while
            // clients queued behind it looked exactly like a node that had
            // never asked, and the difference is the whole diagnosis.
            match pgprox_core::cluster::ClusterCoordinator::request_quota(
                app.cluster.as_ref(),
                &server.server,
                want,
            )
            .await
            {
                Ok(lease) => {
                    allowance = app.cluster.allowance(&server.server);
                    tracing::info!(
                        server = %server.server,
                        want,
                        granted = lease.count(app.deps.clock.now()),
                        guaranteed = allowance.guaranteed,
                        leased = allowance.leased,
                        "quota lease granted"
                    );
                }
                Err(reason) => {
                    tracing::warn!(
                        server = %server.server,
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

        let total = allowance.guaranteed + allowance.leased;
        let each = (total / u32::try_from(keys.len()).unwrap_or(u32::MAX)).max(1);
        for key in keys {
            app.pool.set_limit(&key, each);
        }
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
async fn follow_drain(app: &App, probes: &Arc<Probes>, drainer: &Drainer<'_>) {
    let draining = probes.is_draining();
    let signalled = drainer.context.draining.fired();

    if draining && !signalled {
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
    } else if !draining && signalled {
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
        // usually doing so while the node is refusing connections.
        gate.set_ceiling(app.deps.config.watch().borrow().max_client_conns);
        app.cluster.tick();
        app.cluster.report(
            app.sessions.len(),
            app.pool
                .all_stats()
                .into_iter()
                .map(|(key, stats)| (key.server, stats.active + stats.idle))
                .collect(),
        );
        let per_tenant = app.sessions.per_tenant();
        app.cluster.report_tenants(per_tenant.clone());

        // A tenant this node no longer serves is one it should stop reserving
        // for. Without this the tracked set only ever grows, which in a proxy
        // built for five thousand tenants is a leak with a slow fuse, and the
        // reservations it holds are capacity peers could have used.
        for tenant in tracked.drain(..) {
            if !per_tenant.iter().any(|(seen, _)| seen == &tenant) {
                app.cluster.forget_tenant(&tenant);
            }
        }
        tracked.extend(per_tenant.into_iter().map(|(tenant, _)| tenant));

        // A node serving a stale document looks exactly like one serving the
        // current document, which is when an operator most needs to be told
        // which they have. Every tick, because the condition persists until
        // somebody fixes the file, and a single line at the moment it broke
        // would have scrolled away.
        if !app.deps.config.is_healthy() {
            tracing::warn!("the configuration could not be re-read: serving the last good one");
        }

        // Before the reap, so a limit that just dropped is what the reaper
        // measures against.
        apply_quota(app).await;

        // Idle connections cost the database a slot for as long as the node
        // runs, so this is not housekeeping: it is the other half of the
        // promise that a quiet node holds nothing. `reap_idle` has existed
        // since M5.13 with no caller on a timer.
        app.pool.reap_idle(&ReapConfig::default());

        // After reporting, so a peer hears this tick's numbers rather than the
        // last one's, and awaited rather than spawned: a round that took longer
        // than a tick would otherwise pile up one task per second against a
        // peer that is already too slow to answer.
        let reached = crate::gossip::round(peers, &app.cluster).await;
        if reached < peers.len() {
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

        // Never while draining: a draining node's clients are leaving anyway,
        // and shedding them toward a home node would move work twice.
        if !probes.is_draining() {
            let shed = shed_pass(app, &app.sessions);
            if shed > 0 {
                tracing::info!(shed, "shed clients toward their home nodes");
            }
        }

        // Last, so a node that has just been told to drain has already
        // reported its final numbers to the fleet.
        follow_drain(app, probes, drainer).await;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::wiring::Deps;
    use pgprox_core::auth::FakeCredentialResolver;
    use pgprox_core::clock::SystemClock;
    use pgprox_core::config::{Config, FakeConfigSource, ServerConfig};
    use pgprox_core::ids::{NodeId, ServerId};

    fn deps() -> Deps {
        Deps {
            listener_tls: None,
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
        }
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
        tokio::time::timeout(Duration::from_secs(5), running)
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

        tokio::time::timeout(Duration::from_secs(5), shutdown.waited())
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
                ticker(&app, &probes, &[], &drainer, &shutdown).await
            }
        });

        tokio::time::sleep(Duration::from_millis(1200)).await;
        shutdown.fire();

        let ran = tokio::time::timeout(Duration::from_secs(5), ticked)
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
            BTreeMap::from([(NodeId::new(2), two_at.gossip.to_string())]),
            shutdown.clone(),
        ));
        let peer = tokio::spawn(run_with_peers(
            second,
            two,
            BTreeMap::from([(NodeId::new(1), one_at.gossip.to_string())]),
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
        let _ = tokio::time::timeout(Duration::from_secs(5), running).await;
        let _ = tokio::time::timeout(Duration::from_secs(5), peer).await;

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
        let _ = tokio::time::timeout(Duration::from_secs(5), running).await;
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
        assert!(limit > 0, "the soft limit read as zero");

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
            ticker(&app, &probes, &[], &drainer, &shutdown),
        )
        .await
        .expect("the tick stopped when the reaper was added");

        assert!(ran >= 1);
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

        let reloaded = tokio::time::timeout(Duration::from_secs(10), async {
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
        let _ = tokio::time::timeout(Duration::from_secs(5), running).await;
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
        let _ = tokio::time::timeout(Duration::from_secs(5), running).await;
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
            ticker(&app, &probes, &[], &drainer, &shutdown),
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
        apply_quota(&app).await;

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
        let shed = shed_pass(&app, &app.sessions);

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
}
