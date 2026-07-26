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
use crate::observatory::NodeObservatory;
use crate::sessions::Sessions;

use pgprox_cluster::coordinator::CoordinatorConfig;
use pgprox_cluster::service::GossipCoordinator;
use pgprox_config::drain::{DrainConfig, DrainState};
use pgprox_core::auth::CredentialResolver;
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

        let connector = Arc::new(PgConnector::new(TcpUpstream::new(Arc::clone(&deps.tls))));
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
        let observatory = Arc::new(NodeObservatory::new(
            deps.node,
            Arc::clone(&deps.clock),
            Arc::clone(&deps.config),
            Arc::clone(&cluster),
            Arc::clone(&pool),
            Arc::clone(&sessions),
            Arc::clone(&drain),
        ));

        // Started, because a node reaches this line only with a configuration
        // that loaded and validated, which is exactly what `Starting` means it
        // has not done.
        let health = Health::new(HealthConfig::default());
        let health: SharedHealth = Arc::new(Mutex::new(health));
        lock(&health).started();

        Ok(Self {
            connector,
            pool,
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
            node: NodeId::new(1),
            node_name: "pgprox-1".to_owned(),
            clock: Arc::new(FakeClock::new()),
            tls: pgprox_tls::client_config(tokio_rustls::rustls::RootCertStore::empty())
                .expect("an empty root store is a valid client config"),
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
