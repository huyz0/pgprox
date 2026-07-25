//! Configuration contract.
//!
//! Config is pulled, not pushed, and drain is desired state rather than a
//! command. A drained node stays drained across a restart, and the intent is
//! visible in whatever the config lives in rather than being a side effect
//! somebody ran once.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

use crate::cluster::NodeMode;
use crate::ids::ServerId;

/// Per-upstream-server limits.
///
/// No `Eq`: `guaranteed_fraction` is a float, and an `Eq` on a config struct
/// invites comparing two configurations for exact equality, which is not what
/// float comparison means.
#[derive(Clone, PartialEq, Debug)]
pub struct ServerConfig {
    /// The server these limits apply to.
    pub server: ServerId,
    /// Connections the cluster may hold in total.
    ///
    /// Set this to the server's `max_connections` minus a reserve for superuser
    /// and maintenance sessions. Using the raw value risks locking out the
    /// operator at exactly the moment they need to intervene.
    pub max_connections: u32,
    /// Fraction of the cap distributed as guaranteed per-node share, with the
    /// remainder held as a leasable free pool.
    pub guaranteed_fraction: f64,
}

/// What a node should be doing.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
#[non_exhaustive]
pub struct NodeOverride {
    /// Active, or draining.
    pub mode: NodeMode,
}

/// The whole configuration.
#[derive(Clone, PartialEq, Debug)]
pub struct Config {
    /// Limits per upstream server.
    pub servers: Vec<ServerConfig>,
    /// Per-node overrides, keyed by node name. This is where drain lives.
    pub nodes: BTreeMap<String, NodeOverride>,
    /// Client connections one node will accept.
    pub max_client_conns: u32,
    /// How long a draining node waits before force-closing what remains.
    pub drain_grace: Duration,
    /// Upper bound on how long a resolved grant may be cached, whatever the
    /// sidecar says.
    pub grant_ttl_cap: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            servers: Vec::new(),
            nodes: BTreeMap::new(),
            max_client_conns: 10_000,
            drain_grace: Duration::from_secs(60),
            grant_ttl_cap: Duration::from_secs(300),
        }
    }
}

impl Config {
    /// Checks the configuration makes sense.
    ///
    /// Every error names the offending field, because a config error found at
    /// startup with no field name means reading the whole file to guess.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.max_client_conns == 0 {
            return Err(ConfigError::Invalid {
                field: "max_client_conns".into(),
                reason: "must be greater than zero, or the node accepts nothing".into(),
            });
        }

        let mut seen = std::collections::BTreeSet::new();
        for server in &self.servers {
            if server.max_connections == 0 {
                return Err(ConfigError::Invalid {
                    field: format!("servers[{}].max_connections", server.server),
                    reason: "must be greater than zero".into(),
                });
            }
            if !(0.0..=1.0).contains(&server.guaranteed_fraction) {
                return Err(ConfigError::Invalid {
                    field: format!("servers[{}].guaranteed_fraction", server.server),
                    reason: format!(
                        "must be between 0.0 and 1.0, got {}",
                        server.guaranteed_fraction
                    ),
                });
            }
            if !seen.insert(server.server.clone()) {
                return Err(ConfigError::Invalid {
                    field: format!("servers[{}]", server.server),
                    reason: "listed twice; two caps for one server is ambiguous".into(),
                });
            }
        }

        Ok(())
    }

    /// What mode a node should be in.
    #[must_use]
    pub fn mode_for(&self, node_name: &str) -> NodeMode {
        self.nodes
            .get(node_name)
            .map_or(NodeMode::Active, |override_| override_.mode)
    }

    /// The cap configured for a server, if any.
    #[must_use]
    pub fn server(&self, server: &ServerId) -> Option<&ServerConfig> {
        self.servers.iter().find(|s| &s.server == server)
    }
}

/// Why configuration could not be loaded or accepted.
#[derive(Clone, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// A field is present but wrong.
    #[error("config field {field} is invalid: {reason}")]
    Invalid {
        /// Which field, in a form that can be found in the source.
        field: String,
        /// What is wrong with it.
        reason: String,
    },
    /// The source could not be read.
    #[error("could not read configuration: {reason}")]
    Unreadable {
        /// What went wrong.
        reason: String,
    },
}

/// Somewhere configuration comes from.
///
/// Implementations watch a mounted directory, an etcd prefix, or an HTTP
/// endpoint. The file implementation must watch the *directory*: a `ConfigMap`
/// update swaps a symlink, so watching the file itself misses every change.
///
/// See ADR 0006.
#[async_trait::async_trait]
pub trait ConfigSource: Send + Sync + fmt::Debug {
    /// Reads the current configuration.
    async fn load(&self) -> Result<Config, ConfigError>;

    /// Observes changes. The receiver always holds the latest value.
    fn watch(&self) -> watch::Receiver<Arc<Config>>;
}

#[async_trait::async_trait]
impl<T: ConfigSource + ?Sized> ConfigSource for Arc<T> {
    async fn load(&self) -> Result<Config, ConfigError> {
        (**self).load().await
    }

    fn watch(&self) -> watch::Receiver<Arc<Config>> {
        (**self).watch()
    }
}

#[cfg(any(test, feature = "test-fakes"))]
pub use fake::FakeConfigSource;

#[cfg(any(test, feature = "test-fakes"))]
mod fake {
    use super::{Arc, Config, ConfigError, ConfigSource, watch};

    /// An in-memory [`ConfigSource`] for tests.
    ///
    /// Validates on publish, exactly as a real source must, so a test cannot
    /// push a configuration through the fake that the real one would reject.
    #[derive(Debug)]
    pub struct FakeConfigSource {
        tx: watch::Sender<Arc<Config>>,
    }

    impl FakeConfigSource {
        /// Builds a source serving `initial`.
        ///
        /// # Errors
        ///
        /// Returns the validation error if `initial` is not valid.
        pub fn new(initial: Config) -> Result<Arc<Self>, ConfigError> {
            initial.validate()?;
            let (tx, _) = watch::channel(Arc::new(initial));
            Ok(Arc::new(Self { tx }))
        }

        /// Publishes a new configuration to every watcher.
        ///
        /// # Errors
        ///
        /// Returns the validation error and publishes nothing if `next` is not
        /// valid, so watchers never observe a configuration a real source would
        /// have refused.
        pub fn publish(&self, next: Config) -> Result<(), ConfigError> {
            next.validate()?;
            self.tx.send_replace(Arc::new(next));
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl ConfigSource for FakeConfigSource {
        async fn load(&self) -> Result<Config, ConfigError> {
            Ok((**self.tx.borrow()).clone())
        }

        fn watch(&self) -> watch::Receiver<Arc<Config>> {
            self.tx.subscribe()
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn server(name: &str, cap: u32) -> ServerConfig {
        ServerConfig {
            server: ServerId::new(name, 5432),
            max_connections: cap,
            guaranteed_fraction: 0.5,
        }
    }

    fn valid() -> Config {
        Config {
            servers: vec![server("db-1", 4000)],
            ..Config::default()
        }
    }

    #[test]
    fn a_default_config_is_valid() {
        assert!(Config::default().validate().is_ok());
    }

    #[test]
    fn validation_names_the_offending_field() {
        // A config error with no field name means reading the whole file to
        // guess which line is wrong.
        let bad = Config {
            max_client_conns: 0,
            ..valid()
        };
        let err = bad.validate().unwrap_err();
        let ConfigError::Invalid { field, .. } = &err else {
            unreachable!("wrong variant: {err:?}");
        };
        assert_eq!(field, "max_client_conns");
        assert!(err.to_string().contains("max_client_conns"));
    }

    #[test]
    fn a_zero_cap_is_rejected_and_identified_by_server() {
        let bad = Config {
            servers: vec![server("db-7", 0)],
            ..valid()
        };
        let err = bad.validate().unwrap_err();
        assert!(err.to_string().contains("db-7:5432"), "got {err}");
        assert!(err.to_string().contains("max_connections"), "got {err}");
    }

    #[test]
    fn an_out_of_range_fraction_is_rejected_with_its_value() {
        for bad_fraction in [-0.1, 1.5] {
            let bad = Config {
                servers: vec![ServerConfig {
                    guaranteed_fraction: bad_fraction,
                    ..server("db-1", 100)
                }],
                ..valid()
            };
            let err = bad.validate().unwrap_err();
            assert!(err.to_string().contains("guaranteed_fraction"), "got {err}");
            assert!(
                err.to_string().contains(&bad_fraction.to_string()),
                "error should show the offending value, got {err}"
            );
        }
    }

    #[test]
    fn a_server_listed_twice_is_rejected() {
        // Two caps for one server is ambiguous, and silently taking the last
        // one would be a cap breach waiting to happen.
        let bad = Config {
            servers: vec![server("db-1", 100), server("db-1", 200)],
            ..valid()
        };
        let err = bad.validate().unwrap_err();
        assert!(err.to_string().contains("twice"), "got {err}");
    }

    #[test]
    fn drain_is_expressed_as_desired_state() {
        let mut config = valid();
        config.nodes.insert(
            "pgprox-2".into(),
            NodeOverride {
                mode: NodeMode::Draining,
            },
        );

        assert_eq!(config.mode_for("pgprox-2"), NodeMode::Draining);
        // A node with no entry is active, so forgetting to list one cannot
        // accidentally drain it.
        assert_eq!(config.mode_for("pgprox-0"), NodeMode::Active);
    }

    #[test]
    fn server_lookup_finds_the_cap() {
        let config = valid();
        let found = config.server(&ServerId::new("db-1", 5432)).unwrap();
        assert_eq!(found.max_connections, 4000);
        assert!(config.server(&ServerId::new("db-9", 5432)).is_none());
    }

    #[tokio::test]
    async fn the_fake_publishes_to_watchers() {
        let source = FakeConfigSource::new(valid()).unwrap();
        let mut rx = source.watch();
        assert_eq!(rx.borrow_and_update().max_client_conns, 10_000);

        source
            .publish(Config {
                max_client_conns: 250,
                ..valid()
            })
            .unwrap();

        rx.changed().await.unwrap();
        assert_eq!(rx.borrow().max_client_conns, 250);
        assert_eq!(source.load().await.unwrap().max_client_conns, 250);
    }

    #[tokio::test]
    async fn the_fake_refuses_to_publish_an_invalid_config() {
        // A fake that accepted what the real source rejects would let a test
        // pass against a configuration that cannot exist.
        let source = FakeConfigSource::new(valid()).unwrap();
        let mut rx = source.watch();
        rx.borrow_and_update();

        let err = source
            .publish(Config {
                max_client_conns: 0,
                ..valid()
            })
            .unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }));

        assert!(
            !rx.has_changed().unwrap(),
            "invalid config reached watchers"
        );
        assert_eq!(source.load().await.unwrap().max_client_conns, 10_000);
    }

    #[test]
    fn constructing_the_fake_with_an_invalid_config_fails() {
        let err = FakeConfigSource::new(Config {
            max_client_conns: 0,
            ..valid()
        })
        .unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }));
    }

    #[tokio::test]
    async fn config_source_works_through_an_arc_dyn() {
        let source: Arc<dyn ConfigSource> = FakeConfigSource::new(valid()).unwrap();
        assert!(source.load().await.is_ok());
        assert_eq!(source.watch().borrow().max_client_conns, 10_000);
    }

    #[test]
    fn unreadable_sources_report_why() {
        let err = ConfigError::Unreadable {
            reason: "no such file".into(),
        };
        assert!(err.to_string().contains("no such file"));
    }
}
