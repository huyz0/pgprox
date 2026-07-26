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
use pgprox_session::state::{HandshakeConfig, TlsPosture};
use tokio::sync::watch;

use crate::entropy::SystemEntropy;
use crate::http::{self, Probes};
use crate::serve::{Context, Gate, accept_loop};
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
        node: app.deps.node,
        clock: Arc::clone(&app.deps.clock),
        handshake: HandshakeConfig {
            // Client-side TLS termination is not built, so the listener says so
            // rather than requiring something it cannot do. `M6.23` gives the
            // stack a certificate and this becomes configuration.
            tls: TlsPosture::Optional,
            static_users: Vec::new(),
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
    peers: BTreeMap<pgprox_core::ids::NodeId, SocketAddr>,
    shutdown: Shutdown,
) -> std::io::Result<()> {
    // Set here rather than at build time: the peer table is a deployment fact
    // and `App::build` opens no sockets. A node with no peers keeps the
    // fallback it had, which is its guaranteed share.
    app.cluster
        .set_transport(Arc::new(crate::gossip::GossipTransport::new(peers.clone())));
    let addresses: Vec<SocketAddr> = peers.values().copied().collect();
    let ceiling = app.config.max_client_conns;
    let gate = Arc::new(Gate::new(ceiling));
    let context = Arc::new(Context {
        peers: peers.clone(),
        ..context(&app, &shutdown)
    });
    let probes = probes(&app);

    let admin = tokio::spawn(http::serve(
        listeners.admin,
        http::router(
            Arc::clone(&app.observatory) as pgprox_admin::Shared,
            Arc::clone(&probes),
        ),
        {
            let shutdown = shutdown.clone();
            async move { shutdown.waited().await }
        },
    ));

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

    let _ticks = ticker(&app, &probes, &addresses, &shutdown).await;

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
    Ok(())
}

/// The work that happens on a timer, until the signal fires.
///
/// Returns how many ticks it ran. A count rather than nothing, because the
/// work it does is reported to peers and to probes rather than returned, and a
/// loop that silently never ran would look exactly like one that did.
async fn ticker(app: &App, probes: &Arc<Probes>, peers: &[SocketAddr], shutdown: &Shutdown) -> u64 {
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

        // After reporting, so a peer hears this tick's numbers rather than the
        // last one's, and awaited rather than spawned: a round that took longer
        // than a tick would otherwise pile up one task per second against a
        // peer that is already too slow to answer.
        crate::gossip::round(peers, &app.cluster).await;
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
            async move { ticker(&app, &probes, &[], &shutdown).await }
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
            BTreeMap::from([(NodeId::new(2), two_at.gossip)]),
            shutdown.clone(),
        ));
        let peer = tokio::spawn(run_with_peers(
            second,
            two,
            BTreeMap::from([(NodeId::new(1), one_at.gossip)]),
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
