//! Building one node out of the pieces every other crate provides.
//!
//! # The shape
//!
//! [`Deps`] holds what a node cannot make for itself: a clock, an identity, a
//! configuration source, and a credential resolver. [`App::build`] turns those
//! into the running parts. A test passes fakes for all four and gets a real
//! `App`, which is what makes this file testable at all.
//!
//! # What build does not do
//!
//! It opens no socket. Building and running are separate so a configuration
//! mistake is found before anything is bound, and so the wiring itself can be
//! exercised without a port. The run loop is a later task's problem.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::dial::TcpUpstream;

/// The fewest buffers a node's slab holds, whatever its client ceiling says.
///
/// A node configured for a handful of clients still has a handshake, a probe
/// and a pool behind it, and a slab of two would make them wait on each other.
const BUFFER_FLOOR: usize = 256;

use crate::observatory::{NodeObservatory, NodeParts};
use crate::sessions::Sessions;

use pgprox_cluster::coordinator::CoordinatorConfig;
use pgprox_cluster::service::GossipCoordinator;
use pgprox_config::drain::{DrainConfig, DrainState};
use pgprox_core::auth::CredentialResolver;
use pgprox_core::buf::{BufferSlab, DEFAULT_BUFFER_SIZE};
use pgprox_core::clock::Clock;
use pgprox_core::config::{Config, ConfigError, ConfigSource};
use pgprox_core::ids::NodeId;
use pgprox_observe::health::{Health, HealthConfig};
use pgprox_pool::live::LivePool;
use pgprox_pool::pool::PoolConfig;
use pgprox_route::poller::ReplicaWatch;
use pgprox_route::replica::ReplicaConfig;
use pgprox_session::connect::PgConnector;

/// Why a node could not be built.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StartupError {
    /// The command line was.
    #[error("{detail}")]
    Arguments {
        /// What was wrong with it.
        detail: String,
    },

    /// The credential sidecar could not be reached.
    ///
    /// A startup failure rather than a degraded state: a node that cannot
    /// resolve credentials can authenticate nobody.
    #[error("sidecar: {detail}")]
    Sidecar {
        /// What went wrong.
        detail: String,
    },

    /// The configuration was unusable.
    ///
    /// Reported before anything binds, so a mistake costs a failed start
    /// rather than a node that accepts connections it cannot serve.
    #[error("configuration: {0}")]
    Config(#[from] ConfigError),
}

/// What a node cannot build for itself.
///
/// Every field is a trait object or an identity, so a test supplies fakes and
/// gets the same `App` production does.
pub struct Deps {
    /// This node's number, which its cancel keys and its leadership depend on.
    pub node: NodeId,
    /// This node's name in the configuration document, which is what a drain
    /// is addressed to.
    pub node_name: String,
    /// Time. Injected so tests never sleep.
    pub clock: Arc<dyn Clock>,
    /// How upstream TLS is verified.
    pub tls: Arc<tokio_rustls::rustls::ClientConfig>,
    /// What this node presents to clients, if it has a certificate.
    pub listener_tls: Option<Arc<tokio_rustls::rustls::ServerConfig>>,
    /// Whether a client may authenticate without TLS.
    ///
    /// Separate from having a certificate, because the two failures are
    /// different: a node with no certificate cannot offer TLS at all, and a
    /// node that requires it must refuse rather than serve in the clear.
    pub require_tls: bool,
    /// The static user this node accepts over SCRAM, if one is configured.
    pub statics: Option<Arc<crate::admin::StaticAdmin>>,
    /// Where configuration comes from.
    pub config: Arc<dyn ConfigSource>,
    /// Who resolves a token into a backend.
    pub resolver: Arc<dyn CredentialResolver>,
}

impl std::fmt::Debug for Deps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Deps")
            .field("node", &self.node)
            .field("node_name", &self.node_name)
            .finish_non_exhaustive()
    }
}

/// The connector this node opens upstream connections through.
pub type NodeConnector = Arc<PgConnector<TcpUpstream>>;

/// The pool this node holds.
pub type NodePool = LivePool<NodeConnector>;

/// The one drain overlay in the process.
///
/// Shared rather than copied: `/readyz`, the admin API and the drain sequence
/// all read and write it, and two of them holding different answers is a node
/// that reports itself out of rotation while still taking work.
pub type SharedDrain = Arc<Mutex<DrainState>>;

/// What the probes answer, shared for the same reason.
pub type SharedHealth = Arc<Mutex<Health>>;

/// Locks a shared value, ignoring poisoning.
///
/// A panicked holder leaves the drain overlay readable, because the alternative
/// is a probe that panics in turn and a node that can no longer say whether it
/// is draining.
pub fn lock<T>(shared: &Mutex<T>) -> MutexGuard<'_, T> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One built node.
#[derive(Debug)]
pub struct App {
    /// The configuration in force when the node was built.
    ///
    /// Not the live one: watchers get that from the source. This is what the
    /// sizes below were derived from.
    pub config: Arc<Config>,
    /// What this node cannot build for itself, kept for the run loop.
    pub deps: Deps,
    /// Membership, quota and leases.
    pub cluster: Arc<GossipCoordinator>,
    /// Replica positions, polled on their own schedule.
    pub replicas: Arc<ReplicaWatch>,
    /// Whether this node is draining, and until when.
    pub drain: SharedDrain,
    /// What the probes answer.
    pub health: SharedHealth,
    /// What this node presents to clients, carried from [`Deps`].
    pub listener_tls: Option<Arc<tokio_rustls::rustls::ServerConfig>>,
    /// The static user this node accepts, carried from [`Deps`].
    pub statics: Option<Arc<crate::admin::StaticAdmin>>,
    /// Which tenants get their own metric series.
    ///
    /// Empty by default, which aggregates every tenant under one label. That
    /// is the direction M4.9 chose: a `tenant` label taken from the data is
    /// one series per tenant, and a proxy is built for five thousand of them.
    pub tenants: Arc<pgprox_observe::tenants::TenantAllowlist>,
    /// Who this node is serving.
    pub sessions: Arc<Sessions>,
    /// What the admin surfaces read.
    pub observatory: Arc<NodeObservatory>,
    /// Where upstream connections come from.
    ///
    /// Held alongside the pool rather than only inside it, because the grant
    /// path has to teach it which backend a pool key means and the pool has no
    /// opinion about that.
    pub connector: NodeConnector,
    /// The upstream connections this node holds.
    pub pool: Arc<NodePool>,
    /// Where statements went, which is how the share a replica served is
    /// reported.
    pub routes: Arc<crate::routes::RouteCounts>,
    /// Where every connection's buffers come from.
    ///
    /// One slab for the whole node, shared by client and upstream
    /// connections, so the bound is on the node's buffer memory rather than on
    /// each side of it separately.
    pub slab: Arc<BufferSlab>,
    /// The query cache.
    ///
    /// Built on every node and serving nobody until a document says otherwise,
    /// rather than built only when one does. An empty store is a lock and two
    /// empty maps, and building it conditionally would mean a node could not
    /// be told to start caching without being restarted, which is the one
    /// thing ADR 0006 says configuration must never require.
    pub cache: Arc<pgprox_cache::Store>,
}

impl App {
    /// Builds a node.
    ///
    /// # Errors
    ///
    /// Fails when the configuration cannot be loaded or does not validate.
    /// Both are startup failures rather than degraded running: a node with no
    /// server caps would open unlimited upstream connections, which is the
    /// failure the whole quota layer exists to prevent.
    pub async fn build(deps: Deps) -> Result<Self, StartupError> {
        let config = Arc::new(deps.config.load().await?);
        config.validate()?;

        // Refused here rather than at the first client. A node told to require
        // TLS with nothing to offer would refuse every client for a reason
        // nobody would find in a connection error.
        if deps.require_tls && deps.listener_tls.is_none() {
            return Err(StartupError::Arguments {
                detail: "--require-tls needs --tls-cert and --tls-key".to_owned(),
            });
        }

        let cluster = GossipCoordinator::new(
            deps.node,
            CoordinatorConfig {
                // One node's view of how many there are. Configured rather
                // than observed, because a node that counted its peers would
                // shrink the majority it needs at exactly the moment a
                // partition made that dangerous.
                fleet_size: u32::try_from(config.nodes.len().max(1)).unwrap_or(u32::MAX),
                ..CoordinatorConfig::default()
            },
            Arc::clone(&deps.clock),
        );

        // Caps come from the document rather than from the servers, because a
        // server's max_connections includes the reserve an operator needs to
        // be able to log in and intervene.
        for server in &config.servers {
            cluster.set_cap(server.server.clone(), server.max_connections);
        }

        // Buffers are borrowed while a connection has something to say and
        // returned when it goes quiet, so the slab is sized for the
        // connections active at once rather than for the connections open. A
        // tenth of the client ceiling is generous against the workload this is
        // measured on, where a connection spends milliseconds busy and
        // hundreds of milliseconds thinking, and the floor keeps a small
        // deployment from being bounded at nothing.
        //
        // Being wrong here costs latency, not correctness: an exhausted slab
        // makes a connection wait, which is the direction ADR 0004 says to
        // fail in.
        let buffers = usize::try_from(config.max_client_conns / 10)
            .unwrap_or(BUFFER_FLOOR)
            .max(BUFFER_FLOOR);
        let slab = BufferSlab::new(DEFAULT_BUFFER_SIZE, buffers);

        let connector = Arc::new(PgConnector::new(
            TcpUpstream::new(Arc::clone(&deps.tls)),
            Arc::clone(&slab),
        ));
        let pool = LivePool::new(
            Arc::clone(&connector),
            Arc::clone(&deps.clock),
            PoolConfig {
                // Per pool, and a pool is one server, database and user. The
                // cluster layer holds the cap that actually matters; this stops
                // one tenant's pool from being the whole node's.
                max_size: config
                    .servers
                    .first()
                    .map_or(50, |server| server.max_connections.min(50)),
                ..PoolConfig::default()
            },
        );

        let drain: SharedDrain = Arc::new(Mutex::new(DrainState::new(
            deps.node_name.clone(),
            DrainConfig::default(),
        )));
        let sessions = Sessions::new();

        // Configured from the document that just loaded, so a node that starts
        // with a cache section is caching from its first client rather than
        // from its first tick.
        let cache = pgprox_cache::Store::new(Arc::clone(&deps.clock));
        cache.reconfigure(&config.query_cache);

        let observatory = Arc::new(NodeObservatory::new(NodeParts {
            node: deps.node,
            clock: Arc::clone(&deps.clock),
            config: Arc::clone(&deps.config),
            cluster: Arc::clone(&cluster),
            pool: Arc::clone(&pool),
            sessions: Arc::clone(&sessions),
            drain: Arc::clone(&drain),
            cache: Arc::clone(&cache),
        }));

        // Started, because a node reaches this line only with a configuration
        // that loaded and validated, which is exactly what `Starting` means it
        // has not done.
        let health = Health::new(HealthConfig::default());
        let health: SharedHealth = Arc::new(Mutex::new(health));
        lock(&health).started();

        Ok(Self {
            cache,
            listener_tls: deps.listener_tls.clone(),
            statics: deps.statics.clone(),
            tenants: Arc::new(pgprox_observe::tenants::TenantAllowlist::new()),
            connector,
            pool,
            slab,
            routes: Arc::new(crate::routes::RouteCounts::new()),
            replicas: ReplicaWatch::new(0, ReplicaConfig::default(), Arc::clone(&deps.clock)),
            drain,
            health,
            sessions,
            observatory,
            config,
            cluster,
            deps,
        })
    }

    /// Whether a static user could authenticate here.
    #[must_use]
    pub fn has_static_users(&self) -> bool {
        self.statics.is_some()
    }

    /// What the handshake tells a client about TLS.
    ///
    /// `Required` refuses a client that authenticates without it, `Optional`
    /// offers it, and `Disabled` is a node with no certificate to offer. A
    /// node told to require TLS without one cannot start, so that pair is
    /// rejected before this is reached.
    #[must_use]
    pub fn tls_posture(&self) -> pgprox_session::state::TlsPosture {
        use pgprox_session::state::TlsPosture;

        match (self.listener_tls.is_some(), self.deps.require_tls) {
            (true, true) => TlsPosture::Required,
            (true, false) => TlsPosture::Optional,
            (false, _) => TlsPosture::Disabled,
        }
    }

    /// Whether this node is draining, from either the document or the API.
    #[must_use]
    pub fn is_draining(&self) -> bool {
        // The live document rather than the one the node booted with: a drain
        // added to the `ConfigMap` after start is the ordinary way one is
        // requested.
        let config = self.deps.config.watch().borrow().clone();
        lock(&self.drain).is_draining(&config, self.deps.clock.now())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use pgprox_core::auth::FakeCredentialResolver;
    use pgprox_core::clock::FakeClock;
    use pgprox_core::cluster::NodeMode;
    use pgprox_core::config::{FakeConfigSource, NodeOverride, ServerConfig};
    use pgprox_core::ids::ServerId;

    fn config() -> Config {
        Config {
            servers: vec![ServerConfig {
                server: ServerId::new("db-1", 5432),
                max_connections: 100,
                guaranteed_fraction: 0.5,
            }],
            ..Config::default()
        }
    }

    fn deps(config: Config) -> Deps {
        Deps {
            listener_tls: None,
            require_tls: false,
            node: NodeId::new(1),
            node_name: "pgprox-1".to_owned(),
            clock: Arc::new(FakeClock::new()),
            tls: pgprox_tls::client_config(tokio_rustls::rustls::RootCertStore::empty())
                .expect("an empty root store is a valid client config"),
            statics: None,
            config: FakeConfigSource::new(config).expect("the test's config is valid"),
            resolver: Arc::new(FakeCredentialResolver::new()),
        }
    }

    #[tokio::test]
    async fn a_node_builds_from_a_valid_configuration() {
        let app = App::build(deps(config())).await.unwrap();

        assert_eq!(app.config.servers.len(), 1);
        assert!(!app.is_draining());
    }

    #[tokio::test]
    async fn the_server_caps_reach_the_cluster_layer() {
        // The wiring failure that would matter most and show least: a node
        // whose quota layer knows no caps opens connections without limit,
        // which is the failure the whole layer exists to prevent.
        let app = App::build(deps(config())).await.unwrap();

        let allowance = app.cluster.allowance(&ServerId::new("db-1", 5432));
        assert!(
            allowance.guaranteed > 0,
            "the cluster layer was given no cap for a configured server"
        );
    }

    #[tokio::test]
    async fn a_server_the_document_does_not_mention_has_no_allowance() {
        // Proves the assertion above is about the wiring rather than about a
        // default that would pass whatever was configured.
        let app = App::build(deps(config())).await.unwrap();

        let allowance = app.cluster.allowance(&ServerId::new("db-9", 5432));
        assert_eq!(allowance.guaranteed, 0);
    }

    /// A source that cannot produce a configuration.
    ///
    /// The fake in `pgprox-core` validates what it is given, so it cannot
    /// serve something invalid. This can, which is the case that matters: a
    /// document that reached the mount and does not parse.
    #[derive(Debug)]
    struct Broken;

    #[async_trait::async_trait]
    impl ConfigSource for Broken {
        async fn load(&self) -> Result<Config, ConfigError> {
            Err(ConfigError::Invalid {
                field: "max_client_conns".into(),
                reason: "must be greater than zero".into(),
            })
        }

        fn watch(&self) -> tokio::sync::watch::Receiver<Arc<Config>> {
            let (_, rx) = tokio::sync::watch::channel(Arc::new(Config::default()));
            rx
        }
    }

    #[tokio::test]
    async fn an_invalid_configuration_fails_the_start_rather_than_degrading() {
        // A node that started anyway would accept connections it cannot serve,
        // and the operator would find out from a client rather than from a
        // failed rollout.
        let broken = Deps {
            config: Arc::new(Broken),
            ..deps(config())
        };

        assert!(matches!(
            App::build(broken).await,
            Err(StartupError::Config(_))
        ));
    }

    #[tokio::test]
    async fn a_drain_in_the_document_is_in_force_from_the_start() {
        // Drain is desired state, so a node that restarts into a draining
        // document must not come back taking work.
        let mut draining = config();
        draining.nodes.insert(
            "pgprox-1".to_owned(),
            NodeOverride {
                mode: NodeMode::Draining,
            },
        );

        let app = App::build(deps(draining)).await.unwrap();
        assert!(
            app.is_draining(),
            "a node restarted into a draining document came back active"
        );
    }

    #[tokio::test]
    async fn a_drain_addressed_to_another_node_is_not_this_one() {
        let mut elsewhere = config();
        elsewhere.nodes.insert(
            "pgprox-2".to_owned(),
            NodeOverride {
                mode: NodeMode::Draining,
            },
        );

        let app = App::build(deps(elsewhere)).await.unwrap();
        assert!(!app.is_draining());
    }

    #[tokio::test]
    async fn the_fleet_size_comes_from_the_document_rather_than_from_gossip() {
        // A node that counted its live peers would shrink the majority it
        // needs at exactly the moment a partition made that dangerous.
        let mut sized = config();
        for name in ["pgprox-1", "pgprox-2", "pgprox-3"] {
            sized.nodes.insert(name.to_owned(), NodeOverride::default());
        }

        let app = App::build(deps(sized)).await.unwrap();
        // Three nodes, half the cap guaranteed: sixteen each.
        assert_eq!(
            app.cluster
                .allowance(&ServerId::new("db-1", 5432))
                .guaranteed,
            16
        );
    }

    #[tokio::test]
    async fn deps_print_no_credentials() {
        // It holds a resolver, and a resolver holds tokens.
        let rendered = format!("{:?}", deps(config()));
        assert!(!rendered.to_lowercase().contains("token"), "{rendered}");
        assert!(!rendered.to_lowercase().contains("password"), "{rendered}");

        // And prints something. `M17.4`: this whole `Debug` could return an
        // empty string and every assertion above would pass, because all
        // three are about what is absent. A redaction that redacts everything
        // is not a redaction, and this is the type an operator sees in a
        // startup panic.
        assert!(
            rendered.contains("Deps") && rendered.contains("pgprox-1"),
            "the debug output names neither the type nor the node: {rendered}"
        );
    }

    #[tokio::test]
    async fn the_sizes_derived_at_build_are_the_configured_ones() {
        // `M17.4`. Three survivors lived here, and all three are a number
        // reaching production wrong with nothing to say so: the slab's
        // divisor, and the per-pool cap's whole field.
        let app = App::build(deps(config())).await.unwrap();

        // A tenth of the client ceiling, which is 10,000 by default. `* 10`
        // and `% 10` both survived: one asks for a slab forty times the size
        // intended, the other collapses to the floor and makes every
        // connection past the 256th wait for a buffer.
        assert_eq!(app.slab.capacity(), 1_000);

        // And the per-pool cap is the configured one clamped to 50, not
        // `PoolConfig::default()`'s 20. Deleting the field survived because
        // nothing asked a pool what it was allowed to reach. `set_limit`
        // creates the pool and clamps to `max_size`, so asking for more than
        // any cap reports the cap itself.
        let key = pgprox_core::ids::PoolKey::new(ServerId::new("db-1", 5432), "acme", "acme_app");
        app.pool.set_limit(&key, u32::MAX);
        assert_eq!(
            pgprox_core::pool::UpstreamPool::stats(app.pool.as_ref(), &key).limit,
            50
        );
    }

    #[tokio::test]
    async fn a_node_says_whether_a_static_user_could_authenticate() {
        // `M17.4`: both `true` and `false` survived, so this could have
        // answered either way for every node. It gates whether the admin
        // console accepts a password at all, and a node answering `true` with
        // no static user configured offers a login nothing can satisfy.
        let plain = App::build(deps(config())).await.unwrap();
        assert!(!plain.has_static_users());

        let mut with_user = deps(config());
        with_user.statics = Some(Arc::new(
            crate::admin::StaticAdmin::new("pgprox_admin", "hunter2", b"salt".to_vec())
                .expect("the crypto provider derives keys"),
        ));
        let configured = App::build(with_user).await.unwrap();
        assert!(configured.has_static_users());
    }

    #[tokio::test]
    async fn the_pool_opens_through_the_connector_the_node_holds() {
        // Two connectors would mean the grant path teaching one of them where
        // a database lives while the pool opened through the other, and every
        // connection failing for a reason nothing reported.
        let app = App::build(deps(config())).await.unwrap();
        assert_eq!(app.connector.known(), 0);

        app.connector.learn(&pgprox_core::auth::Backend {
            server: ServerId::new("db-1", 5432),
            database: "acme".into(),
            user: "acme_app".into(),
            password: pgprox_core::secret::SecretString::new("hunter2"),
            tls: pgprox_core::auth::TlsMode::Disabled,
        });

        assert_eq!(
            app.connector.known(),
            1,
            "the node's connector did not learn the backend"
        );
    }

    #[tokio::test]
    async fn a_fresh_node_holds_no_upstream_connections() {
        // The property the whole design rests on: a node that has started and
        // been connected to by nobody costs the database nothing.
        let app = App::build(deps(config())).await.unwrap();
        let key = pgprox_core::ids::PoolKey::new(ServerId::new("db-1", 5432), "acme", "acme_app");
        assert_eq!(
            pgprox_core::pool::UpstreamPool::stats(app.pool.as_ref(), &key).active,
            0
        );
    }
}
