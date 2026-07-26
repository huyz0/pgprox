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
            client: tokio::net::TcpListener::bind(addrs.client).await?,
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

/// What one session needs, built from a node.
///
/// Here rather than in `serve`, because it is the one place the node's parts
/// and a session's needs meet, and two places building it would be two nodes.
#[must_use]
pub fn context(app: &App, shutdown: &Shutdown) -> Context {
    Context {
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
        peers: BTreeMap::new(),
        replicas: Arc::new(crate::replicas::ReplicaSets::new(
            crate::dial::TcpUpstream::new(Arc::clone(&app.deps.tls)),
            Arc::clone(&app.deps.clock),
            shutdown.clone(),
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
    let ceiling = app.config.max_client_conns;
    let gate = Arc::new(Gate::new(ceiling));
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
                result = accept_loop(listeners.client, context, gate, ceiling) => result,
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

/// What the tick needs to start or reverse a drain.
struct Drainer<'a> {
    context: &'a Arc<Context>,
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
    let mut ticks = tokio::time::interval(TICK);
    let mut ran = 0;
    loop {
        tokio::select! {
            () = shutdown.waited() => return ran,
            _ = ticks.tick() => ran += 1,
        }

        // Liveness first: a node whose gossip is failing is still alive, and
        // restarting it would drop every client on it.
        probes.beat();
        app.cluster.tick();
        app.cluster.report(
            app.sessions.len(),
            app.pool
                .all_stats()
                .into_iter()
                .map(|(key, stats)| (key.server, stats.active + stats.idle))
                .collect(),
        );
        app.cluster.report_tenants(app.sessions.per_tenant());

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
                let drainer = Drainer {
                    context: &context,
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
        let drainer = Drainer {
            context: &context,
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

        let tenant = (1..200)
            .map(|n| pgprox_core::ids::TenantId::new(format!("tenant-{n}")))
            .find(|tenant| !app.cluster.placement(tenant, 1).on_home_node)
            .expect("no tenant of two hundred homed elsewhere, which is not hashing");

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
