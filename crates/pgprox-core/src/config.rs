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
use crate::ids::{ServerId, TenantId};

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
pub struct NodeOverride {
    /// Active, or draining.
    pub mode: NodeMode,
}

/// What one tenant asked the query cache for.
///
/// A tenant opting in is stating something about its own workload: that data
/// this old is acceptable for these reads. Nobody else can make that judgement,
/// which is why the number is here and not a global.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TenantCache {
    /// How stale this tenant accepts its cached reads being.
    ///
    /// Bounded from above by [`QueryCacheConfig::ttl_cap`]. Read it through
    /// [`QueryCacheConfig::ttl_for`] rather than directly, which applies the
    /// bound; this field is what was asked for, not what is granted.
    pub ttl: Duration,
}

/// What the query cache does on this node.
///
/// ADR 0021's shape. The cache promises bounded staleness and the TTL is the
/// bound, so the TTL is doing real safety work and an operator holds a ceiling
/// over it, exactly as [`Config::grant_ttl_cap`] holds one over a sidecar's.
///
/// # Off is an empty map, and that is the only way to be off
///
/// There is no `enabled` flag beside the tenant list. Two representations of
/// off is a bug with no right answer the first time a document sets one and
/// not the other, and the question the cache actually asks is never "is this
/// node's cache on" but "may this tenant be served from it".
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct QueryCacheConfig {
    /// Bytes of results this node may hold.
    ///
    /// A byte budget rather than an entry count, because nothing bounds the
    /// size of one result and a count would bound nothing.
    pub max_bytes: usize,
    /// The largest single answer the proxy will hold on the chance it is
    /// cacheable.
    ///
    /// # Two resources, two guards
    ///
    /// Not [`QueryCacheConfig::max_bytes`] and deliberately not derived from
    /// it. That is one figure for one store. This is per session, spent while
    /// an answer is in flight, and multiplied by however many sessions are
    /// recording at once, which is the same arithmetic that makes the protocol
    /// crate's inspect cap small.
    ///
    /// The guard is applied while the answer is still arriving, so an answer
    /// past it is abandoned mid-flight and falls back to the streaming path
    /// rather than being assembled and then refused. Until `M17.1` there was no
    /// guard here at all and a 500 MB result was 500 MB held and thrown away:
    /// the cache's own check is at `put`, which is the end.
    ///
    /// Settable since `M25.2`, which is pgpool-II's `memqcache_maxcache`. It
    /// was a constant while the budget it interacts with was configuration, so
    /// an operator who raised the budget to a gigabyte still could not cache a
    /// five megabyte result and nothing they could read said why.
    pub max_entry_bytes: usize,
    /// The longest TTL any tenant may have, whatever it asked for.
    pub ttl_cap: Duration,
    /// Tenants that have opted in. Empty is a cache that serves nobody.
    pub tenants: BTreeMap<TenantId, TenantCache>,
}

impl Default for QueryCacheConfig {
    fn default() -> Self {
        Self {
            // Enough to be worth having on a node that has opted in, small
            // enough that it is not a surprise: the milestone's own memory
            // argument is about what a connection costs, and a cache that
            // defaulted to a gigabyte would undo it.
            max_bytes: 64 * 1024 * 1024,
            // A megabyte, because the cache is for small repeated reads: ADR
            // 0007's case is a point select answered thousands of times, and an
            // answer that does not fit here was never going to earn its place
            // in a shared budget. pgpool-II defaults its own to 400 KB.
            max_entry_bytes: 1024 * 1024,
            // Nothing chose this number from measurement; it is a ceiling, so
            // it only has to be short enough that an operator who never
            // thought about it has not accidentally allowed a stale hour.
            ttl_cap: Duration::from_secs(30),
            // Off. ADR 0021 makes that the default and a tenant has to ask.
            tenants: BTreeMap::new(),
        }
    }
}

impl QueryCacheConfig {
    /// How long this tenant's results may be served, or `None` if it has not
    /// opted in.
    ///
    /// The cap is applied here rather than at the point a document is read, so
    /// that lowering the cap takes effect on the tenants already configured
    /// instead of only on the next document that mentions them.
    #[must_use]
    pub fn ttl_for(&self, tenant: &TenantId) -> Option<Duration> {
        self.tenants
            .get(tenant)
            .map(|asked| asked.ttl.min(self.ttl_cap))
    }

    /// Whether this tenant may be served from the cache at all.
    #[must_use]
    pub fn serves(&self, tenant: &TenantId) -> bool {
        self.tenants.contains_key(tenant)
    }

    /// Whether the cache serves nobody, which is the default.
    #[must_use]
    pub fn is_off(&self) -> bool {
        self.tenants.is_empty()
    }
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
    /// What the query cache does. Serves nobody by default.
    pub query_cache: QueryCacheConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            servers: Vec::new(),
            nodes: BTreeMap::new(),
            max_client_conns: 10_000,
            drain_grace: Duration::from_secs(60),
            grant_ttl_cap: Duration::from_secs(300),
            query_cache: QueryCacheConfig::default(),
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

        // Checked whether or not any tenant has opted in. A cache section
        // saying something impossible is worth refusing before the tenant that
        // makes it load-bearing is added, rather than at the moment somebody
        // adds one and cannot see why the node stopped.
        let cache = &self.query_cache;
        if cache.ttl_cap.is_zero() {
            return Err(ConfigError::Invalid {
                field: "query_cache.ttl_cap".into(),
                reason: "must be greater than zero; a cap of zero caches nothing at any TTL, \
                         which is what an empty tenant list already says"
                    .into(),
            });
        }
        if cache.max_bytes == 0 && !cache.is_off() {
            return Err(ConfigError::Invalid {
                field: "query_cache.max_bytes".into(),
                reason: "must be greater than zero when a tenant has opted in, \
                         or every result is refused as larger than the budget"
                    .into(),
            });
        }
        // The same sentence one field along. A cap of zero records nothing, so
        // a document that lists tenants and sets it is a cache that is off
        // while saying it is on. `M25.3`.
        if cache.max_entry_bytes == 0 && !cache.is_off() {
            return Err(ConfigError::Invalid {
                field: "query_cache.max_entry_bytes".into(),
                reason: "must be greater than zero when a tenant has opted in, \
                         or no answer is ever held long enough to be stored"
                    .into(),
            });
        }
        // The two against each other, which neither check above can see. A
        // node with a cap above its budget records answers to the cap and then
        // rejects every one of them at the moment it tries to store them: work
        // done, memory held, nothing kept, and two counters that each look
        // explainable on their own.
        //
        // Refused only above, not at, the budget. An answer of exactly the
        // budget is still turned away, because the key and the two structs
        // weigh something, but that is a matter of bytes rather than a
        // configuration that can never store anything.
        //
        // Only while the cache is on, unlike the `ttl_cap` check above it. A
        // budget of zero is deliberately allowed while nothing is cached, so
        // an operator can write the section down before deciding who gets it,
        // and a pair check that fired on that would refuse the case the check
        // above it exists to permit.
        if !cache.is_off() && cache.max_entry_bytes > cache.max_bytes {
            return Err(ConfigError::Invalid {
                field: "query_cache.max_entry_bytes".into(),
                reason: format!(
                    "{} is larger than query_cache.max_bytes, which is {}: \
                     every answer that fits the cap would be refused by the \
                     budget, so nothing would ever be cached",
                    cache.max_entry_bytes, cache.max_bytes
                ),
            });
        }
        for (tenant, asked) in &cache.tenants {
            if asked.ttl.is_zero() {
                return Err(ConfigError::Invalid {
                    field: format!("query_cache.tenants.{tenant}.ttl"),
                    reason: "must be greater than zero; remove the tenant to turn its cache off"
                        .into(),
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

    /// Whether the last attempt to read the configuration succeeded.
    ///
    /// Defaulted to true, because a source with no loop cannot fail between
    /// reads. The file provider overrides it: a node serving a stale document
    /// looks exactly like one serving the current one, which is when an
    /// operator most needs to be told which they have.
    fn is_healthy(&self) -> bool {
        true
    }

    /// Runs whatever loop this source needs to notice a change, until dropped.
    ///
    /// Defaulted to never returning, because most sources have no loop: the
    /// fake publishes when a test tells it to, and a source that had to be
    /// driven would make every test drive it. The file provider overrides
    /// this with its poll.
    ///
    /// It exists so the composition root can start the loop without knowing
    /// which source it holds. Without it, `FileSource::run` was reachable only
    /// by downcasting, and in practice was not started at all: a `ConfigMap`
    /// edit never reached a running node.
    async fn run_loop(self: Arc<Self>) {
        std::future::pending::<()>().await;
    }
}

#[async_trait::async_trait]
impl<T: ConfigSource + ?Sized> ConfigSource for Arc<T> {
    // Forwarded rather than defaulted: an `Arc` around a source that can go
    // stale can go stale, and taking the default here would report every
    // wrapped source as healthy forever.
    fn is_healthy(&self) -> bool {
        (**self).is_healthy()
    }

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
    /// `M14.34`. Four mutants survived in this file, and two of them are the
    /// same method on the `Arc` forwarding impl: `is_healthy` could be replaced
    /// by `true` *and* by `false`. A method whose mutants both survive is a
    /// method nothing calls through the trait at all.
    ///
    /// It is the staleness signal. `FileSource` overrides it because a node
    /// serving a stale document looks exactly like one serving the current one,
    /// which is when an operator most needs to be told which they have.
    #[test]
    fn health_is_forwarded_through_an_arc_rather_than_defaulted() {
        #[derive(Debug)]
        struct Fixed(bool);

        #[async_trait::async_trait]
        impl ConfigSource for Fixed {
            async fn load(&self) -> Result<Config, ConfigError> {
                Ok(Config::default())
            }
            fn watch(&self) -> watch::Receiver<Arc<Config>> {
                watch::channel(Arc::new(Config::default())).1
            }
            fn is_healthy(&self) -> bool {
                self.0
            }
        }

        // Both directions, or a constant in either position would pass.
        let healthy: Arc<dyn ConfigSource> = Arc::new(Fixed(true));
        assert!(healthy.is_healthy());
        let stale: Arc<dyn ConfigSource> = Arc::new(Fixed(false));
        assert!(
            !stale.is_healthy(),
            "an Arc reported a stale source as healthy"
        );
    }

    #[test]
    fn a_source_with_no_loop_reports_itself_healthy() {
        // The trait default, which is what a source written outside this
        // repository gets. `false` would make every such node report its
        // configuration as stale for ever.
        #[derive(Debug)]
        struct Minimal;

        #[async_trait::async_trait]
        impl ConfigSource for Minimal {
            async fn load(&self) -> Result<Config, ConfigError> {
                Ok(Config::default())
            }
            fn watch(&self) -> watch::Receiver<Arc<Config>> {
                watch::channel(Arc::new(Config::default())).1
            }
        }

        assert!(
            Minimal.is_healthy(),
            "a source with no loop cannot fail between reads, so it is healthy"
        );
    }

    #[test]
    fn the_default_loop_never_returns() {
        // `run_loop` defaults to `pending()`, and could be replaced with `()`.
        // The composition root starts this without knowing which source it
        // holds, so a default that returns immediately turns "start the config
        // loop" into a no-op that looks like it ran.
        //
        // Polled by hand rather than with a timeout, because this crate depends
        // on tokio only for `sync`: pulling in the time driver to assert that a
        // future is pending would be a dependency added for one test.
        #[derive(Debug)]
        struct Minimal;

        #[async_trait::async_trait]
        impl ConfigSource for Minimal {
            async fn load(&self) -> Result<Config, ConfigError> {
                Ok(Config::default())
            }
            fn watch(&self) -> watch::Receiver<Arc<Config>> {
                watch::channel(Arc::new(Config::default())).1
            }
        }

        let mut loop_future = Box::pin(ConfigSource::run_loop(Arc::new(Minimal)));
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(
            std::future::Future::poll(loop_future.as_mut(), &mut cx).is_pending(),
            "the default run_loop completed; the composition root would treat that as the loop having ended"
        );
    }

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

    /// The field a validation error names.
    fn field_of(err: &ConfigError) -> String {
        match err {
            ConfigError::Invalid { field, .. } => field.clone(),
            other => unreachable!("wrong variant: {other:?}"),
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

    /// A configuration with one tenant opted in.
    fn with_cache(tenant: &str, ttl: Duration, ttl_cap: Duration) -> Config {
        Config {
            query_cache: QueryCacheConfig {
                ttl_cap,
                tenants: [(TenantId::new(tenant), TenantCache { ttl })]
                    .into_iter()
                    .collect(),
                ..QueryCacheConfig::default()
            },
            ..valid()
        }
    }

    #[test]
    fn the_cache_serves_nobody_by_default() {
        // ADR 0021's first consequence, as a property of the type rather than
        // of the document that produces it: whatever else a configuration says,
        // a tenant that has not opted in is not served.
        let config = Config::default();
        assert!(config.query_cache.is_off());
        assert!(!config.query_cache.serves(&TenantId::new("acme")));
        assert_eq!(config.query_cache.ttl_for(&TenantId::new("acme")), None);
    }

    #[test]
    fn a_tenant_that_opted_in_gets_the_staleness_it_asked_for() {
        let config = with_cache("acme", Duration::from_secs(5), Duration::from_secs(30));
        assert!(config.query_cache.serves(&TenantId::new("acme")));
        assert_eq!(
            config.query_cache.ttl_for(&TenantId::new("acme")),
            Some(Duration::from_secs(5))
        );
        // And nobody else is served by its opting in.
        assert_eq!(config.query_cache.ttl_for(&TenantId::new("globex")), None);
    }

    #[test]
    fn a_tenant_asking_for_a_day_gets_the_cap() {
        // The TTL is the whole guarantee, so it is bounded by something the
        // operator controls rather than by what the tenant asked for. The same
        // relationship `grant_ttl_cap` has to a sidecar's TTL.
        let config = with_cache("acme", Duration::from_secs(86_400), Duration::from_secs(30));
        assert_eq!(
            config.query_cache.ttl_for(&TenantId::new("acme")),
            Some(Duration::from_secs(30))
        );
    }

    #[test]
    fn lowering_the_cap_reaches_the_tenants_already_configured() {
        // The reason the cap is applied on read rather than when a document is
        // resolved. An operator lowering it during an incident means it to
        // apply now, not to the next document that happens to mention a tenant.
        let mut config = with_cache("acme", Duration::from_secs(20), Duration::from_secs(30));
        config.query_cache.ttl_cap = Duration::from_secs(1);
        assert_eq!(
            config.query_cache.ttl_for(&TenantId::new("acme")),
            Some(Duration::from_secs(1))
        );
    }

    #[test]
    fn a_cache_setting_that_says_nothing_useful_is_refused_by_field() {
        let zero_cap = Config {
            query_cache: QueryCacheConfig {
                ttl_cap: Duration::ZERO,
                ..QueryCacheConfig::default()
            },
            ..valid()
        };
        let err = zero_cap.validate().unwrap_err();
        assert_eq!(field_of(&err), "query_cache.ttl_cap");

        let no_budget = Config {
            query_cache: QueryCacheConfig {
                max_bytes: 0,
                ..with_cache("acme", Duration::from_secs(5), Duration::from_secs(30)).query_cache
            },
            ..valid()
        };
        let err = no_budget.validate().unwrap_err();
        assert_eq!(field_of(&err), "query_cache.max_bytes");

        let zero_ttl = with_cache("acme", Duration::ZERO, Duration::from_secs(30));
        let err = zero_ttl.validate().unwrap_err();
        assert_eq!(field_of(&err), "query_cache.tenants.acme.ttl");
        assert!(err.to_string().contains("remove the tenant"), "got {err}");
    }

    #[test]
    fn a_per_answer_cap_above_the_budget_is_refused() {
        // `M25.3`. A node in this state records answers to the cap and then
        // rejects every one of them at `put`: work done, memory held, nothing
        // stored, and two counters that each look explainable on their own.
        // pgpool-II documents the same interaction between memqcache_maxcache
        // and memqcache_cache_block_size and leaves it to the operator.
        let over = Config {
            query_cache: QueryCacheConfig {
                max_bytes: 1024 * 1024,
                max_entry_bytes: 2 * 1024 * 1024,
                ..with_cache("acme", Duration::from_secs(5), Duration::from_secs(30)).query_cache
            },
            ..valid()
        };
        let err = over.validate().unwrap_err();
        assert_eq!(field_of(&err), "query_cache.max_entry_bytes");
        let text = err.to_string();
        assert!(
            text.contains("max_bytes"),
            "the reason names one field and the operator has to change one of \
             two: {text}"
        );

        // Equal is the boundary and it is accepted. An answer of exactly the
        // budget is still refused at `put`, because the key and the two structs
        // weigh something, but that is a matter of bytes rather than a
        // configuration that can never store anything.
        let equal = Config {
            query_cache: QueryCacheConfig {
                max_bytes: 1024 * 1024,
                max_entry_bytes: 1024 * 1024,
                ..with_cache("acme", Duration::from_secs(5), Duration::from_secs(30)).query_cache
            },
            ..valid()
        };
        assert!(equal.validate().is_ok(), "the boundary itself was refused");

        // And the default pair, which is what most nodes run.
        assert!(valid().validate().is_ok());

        // A cap above a budget of zero is allowed while nothing is cached, for
        // the reason the budget itself is: the section may be written down
        // before anybody is opted in. Checked here rather than left implicit,
        // because an unconditional pair check refuses exactly the case
        // `a_budget_of_zero_is_allowed_while_nothing_is_cached` exists to
        // permit, and it did until this line was written.
        let idle = Config {
            query_cache: QueryCacheConfig {
                max_bytes: 0,
                max_entry_bytes: 1024 * 1024,
                ..QueryCacheConfig::default()
            },
            ..valid()
        };
        assert!(idle.validate().is_ok());
    }

    #[test]
    fn a_per_answer_cap_of_zero_is_refused_the_way_a_budget_of_zero_is() {
        // The same check one field along. A cap of zero records nothing, so a
        // document listing tenants and setting it is a cache that is off while
        // saying it is on, which is exactly what the `max_bytes` check above
        // exists to refuse.
        let none = Config {
            query_cache: QueryCacheConfig {
                max_entry_bytes: 0,
                ..with_cache("acme", Duration::from_secs(5), Duration::from_secs(30)).query_cache
            },
            ..valid()
        };
        let err = none.validate().unwrap_err();
        assert_eq!(field_of(&err), "query_cache.max_entry_bytes");

        // Allowed while nothing is cached, for the reason a zero budget is:
        // an operator may write the section down before deciding who gets it.
        let idle = Config {
            query_cache: QueryCacheConfig {
                max_entry_bytes: 0,
                ..QueryCacheConfig::default()
            },
            ..valid()
        };
        assert!(idle.validate().is_ok());
    }

    #[test]
    fn a_budget_of_zero_is_allowed_while_nothing_is_cached() {
        // Only a cache with a tenant in it needs a budget. Refusing this would
        // mean an operator could not write the section down before deciding
        // who gets it.
        let idle = Config {
            query_cache: QueryCacheConfig {
                max_bytes: 0,
                ..QueryCacheConfig::default()
            },
            ..valid()
        };
        assert!(idle.validate().is_ok());
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
