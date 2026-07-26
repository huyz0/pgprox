//! What the admin surface reads, and what it may change.
//!
//! # Why this is a contract rather than a set of imports
//!
//! The admin API reports on pools, tenants, clients and cluster state, which
//! live in `pgprox-pool`, `pgprox-session` and `pgprox-cluster`. `pgprox-admin`
//! is not allowed to depend on any of them: only `pgprox-session` and
//! `bin/pgprox` compose crates, and widening that rule to let one HTTP handler
//! reach into three subsystems would dissolve the boundary that lets the tracks
//! be built independently at all.
//!
//! So the fan-in happens once, in the composition root, behind
//! [`Observatory`]. `pgprox-admin` renders what it is given. That also means the
//! HTTP layer and the `SHOW` layer read the same data by construction rather
//! than by discipline, so the two cannot drift into disagreeing about the same
//! question.
//!
//! # Cluster-scoped by default
//!
//! Hitting any pod gives the whole cluster's truth. That works because every
//! node already carries a gossip digest for every other, so an aggregate is a
//! local read. Only drill-downs fan out, which is why [`Observatory::clients`]
//! is the async one and the rest are not: the signature says which questions
//! cost a round trip. See ADR 0007.
//!
//! # Nothing here holds a credential
//!
//! Not by convention, by construction. No type in this module contains a
//! [`crate::secret::SecretString`] or a password field, so a handler cannot leak
//! one however carelessly it renders. There is a test asserting it, because the
//! guarantee is only as good as the next person adding a field.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use crate::cluster::{ClusterDigest, MembershipView};
use crate::config::Config;
use crate::ids::{ConnId, NodeId, PoolKey, ServerId, TenantId};
use crate::pool::PoolStats;

/// How much of the fleet a read covers.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[non_exhaustive]
pub enum Scope {
    /// The whole cluster. The default, because an operator asking a question
    /// almost never means "on whichever pod my request happened to reach".
    #[default]
    Cluster,
    /// This node only, for the times they do.
    Local,
}

impl Scope {
    /// Parses the `?scope=` query parameter.
    ///
    /// An unrecognised value is [`None`] rather than the default, so a typo is
    /// reportable. Quietly answering for the cluster when someone asked for
    /// something they spelled wrong is how an operator draws the wrong
    /// conclusion from a real number.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "cluster" | "" => Some(Self::Cluster),
            "local" => Some(Self::Local),
            _ => None,
        }
    }

    /// Whether this scope covers the whole fleet.
    #[must_use]
    pub const fn is_cluster(self) -> bool {
        matches!(self, Self::Cluster)
    }
}

/// The cluster as this node sees it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ClusterView {
    /// Which node answered.
    pub node: NodeId,
    /// Membership, including modes.
    pub membership: MembershipView,
    /// Every node's last digest, this one included.
    pub digests: Vec<ClusterDigest>,
    /// The hash of the membership view.
    ///
    /// Exported so a mismatch between two pods surfaces split brain directly
    /// rather than being inferred from two lists that look similar.
    pub view_hash: u64,
}

/// One upstream pool.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PoolView {
    /// Which node holds it.
    pub node: NodeId,
    /// Which pool.
    pub key: PoolKey,
    /// Its counts.
    pub stats: PoolStats,
}

/// One upstream server, across the fleet.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ServerView {
    /// Which server.
    pub server: ServerId,
    /// The configured cap for the whole cluster.
    pub cap: u32,
    /// Connections every node reports holding.
    pub in_use: u32,
    /// What the answering node may open without asking.
    pub guaranteed: u32,
    /// What it currently holds on lease beyond that.
    pub leased: u32,
}

impl ServerView {
    /// Headroom against the cap, which is the number an operator actually wants.
    ///
    /// Saturating, so a momentarily inconsistent set of digests reports zero
    /// headroom rather than wrapping to four billion and looking healthy.
    #[must_use]
    pub const fn headroom(&self) -> u32 {
        self.cap.saturating_sub(self.in_use)
    }
}

/// One tenant, across the fleet.
///
/// This is where per-tenant detail lives. It is deliberately not in Prometheus:
/// at five thousand tenants a `tenant` label is a series count that takes a
/// Prometheus down. See ADR 0007.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TenantView {
    /// Which tenant.
    pub tenant: TenantId,
    /// The node that homes it, or [`None`] if every node is draining.
    pub home: Option<NodeId>,
    /// Client connections it has, fleet-wide or local depending on scope.
    pub client_conns: u32,
    /// Upstream connections held on its behalf.
    pub upstream_conns: u32,
}

/// What a client connection is doing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum ClientState {
    /// Connected, no transaction, no upstream connection held.
    Idle,
    /// Holding an upstream connection.
    Active,
    /// Waiting for one.
    Waiting,
}

impl ClientState {
    /// The name used in `SHOW CLIENTS`, matching `PgBouncer`'s.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Active => "active",
            Self::Waiting => "waiting",
        }
    }
}

/// One client connection.
///
/// Listing these is the drill-down that fans out, since a node knows only its
/// own clients.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ClientView {
    /// Which connection, and which node, since the ID carries the node.
    pub conn: ConnId,
    /// Which tenant it belongs to.
    pub tenant: TenantId,
    /// The node serving it.
    pub node: NodeId,
    /// What it is doing.
    pub state: ClientState,
    /// How long it has been in that state.
    pub since: Duration,
    /// Why it is pinned, or [`None`] if it is not.
    ///
    /// A label rather than an enum, because the reasons are `pgprox-pool`'s and
    /// this crate must not learn them to report them.
    pub pinned: Option<String>,
}

/// Fleet counters, for `SHOW STATS`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Stats {
    /// Client connections.
    pub client_conns: u32,
    /// Upstream connections.
    pub upstream_conns: u32,
    /// Transactions served since start.
    pub transactions: u64,
    /// Sessions pinned since start.
    pub pins: u64,
    /// Clients shed since start.
    pub sheds: u64,
    /// Callers currently waiting for an upstream connection.
    pub waiting: u32,
}

/// Why an admin action could not be carried out.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AdminError {
    /// The thing asked about does not exist.
    #[error("no such {kind}: {name}")]
    NotFound {
        /// What kind of thing.
        kind: &'static str,
        /// Which one, as the caller named it.
        name: String,
    },
    /// The request was understood and refused.
    #[error("refused: {reason}")]
    Refused {
        /// Why, in terms an operator can act on.
        reason: String,
    },
    /// A peer could not be reached during a fan-out.
    ///
    /// Distinct from the others because the answer is incomplete rather than
    /// wrong, and an operator deciding whether to act needs to know which.
    #[error("could not reach every node: {reason}")]
    Partial {
        /// What went wrong.
        reason: String,
    },
}

/// What the admin surface reads and changes.
///
/// Implemented once, by the composition root, which is the only place that can
/// see every subsystem. `pgprox-admin` and the `SHOW` parser both render this,
/// so the HTTP and SQL surfaces agree by construction.
#[async_trait::async_trait]
pub trait Observatory: Send + Sync + fmt::Debug {
    /// The cluster as this node sees it. Always local: this *is* the local
    /// view, and comparing two of them is how split brain is found.
    fn cluster(&self) -> ClusterView;

    /// The configuration currently in force.
    fn config(&self) -> Arc<Config>;

    /// Whether that configuration is the current document.
    ///
    /// False when the source last failed to re-read it, which is a node
    /// serving the last good configuration. Defaulted to true, because a
    /// source that cannot go stale cannot report that it has, and because
    /// every existing implementation should keep compiling.
    fn config_is_current(&self) -> bool {
        true
    }

    /// Upstream pools.
    fn pools(&self, scope: Scope) -> Vec<PoolView>;

    /// Upstream servers and their caps.
    fn servers(&self, scope: Scope) -> Vec<ServerView>;

    /// Tenants.
    fn tenants(&self, scope: Scope) -> Vec<TenantView>;

    /// One tenant.
    fn tenant(&self, tenant: &TenantId) -> Option<TenantView>;

    /// Counters.
    fn stats(&self, scope: Scope) -> Stats;

    /// Client connections.
    ///
    /// The one read that fans out, because a node knows only its own clients.
    /// Async so the signature says so, rather than a caller discovering the
    /// cost in a flame graph.
    ///
    /// # Errors
    ///
    /// [`AdminError::Partial`] when a peer could not be reached, carrying the
    /// clients it did gather. An incomplete answer is still useful; an
    /// incomplete answer presented as complete is not.
    async fn clients(&self, scope: Scope) -> Result<Vec<ClientView>, AdminError>;

    /// Drains this node for `ttl`, after which it returns to what the config
    /// document says.
    ///
    /// The expiry is not optional, and that is the point. This is the
    /// imperative path; the declarative one is the config document, which
    /// persists because it is reviewed and survives a restart. A drain here
    /// that never lapsed would be indistinguishable from one somebody meant to
    /// make permanent, and the only way to tell would be to ask whoever ran it.
    ///
    /// An implementation may shorten an over-long `ttl` and reports what it
    /// actually applied.
    ///
    /// # Errors
    ///
    /// Fails when the drain cannot be written.
    async fn drain(&self, ttl: Duration) -> Result<Duration, AdminError>;

    /// Clears an imperative drain, returning to what the document says.
    ///
    /// Cannot undo a drain the document asked for: that would reverse a
    /// reviewed change, and the next config poll would put it back, so the node
    /// would oscillate rather than do either thing.
    ///
    /// # Errors
    ///
    /// Fails when the overlay cannot be cleared.
    async fn undrain(&self) -> Result<(), AdminError>;

    /// Closes idle connections in a pool, returning how many.
    ///
    /// Idle only. Connections in use are finishing real transactions, and an
    /// operator asking for a reset is not asking for those to fail.
    ///
    /// # Errors
    ///
    /// [`AdminError::NotFound`] if there is no such pool.
    async fn reset_pool(&self, key: &PoolKey) -> Result<u32, AdminError>;
}

#[async_trait::async_trait]
impl<T: Observatory + ?Sized> Observatory for Arc<T> {
    fn cluster(&self) -> ClusterView {
        (**self).cluster()
    }
    fn config(&self) -> Arc<Config> {
        (**self).config()
    }
    fn pools(&self, scope: Scope) -> Vec<PoolView> {
        (**self).pools(scope)
    }
    fn servers(&self, scope: Scope) -> Vec<ServerView> {
        (**self).servers(scope)
    }
    fn tenants(&self, scope: Scope) -> Vec<TenantView> {
        (**self).tenants(scope)
    }
    fn tenant(&self, tenant: &TenantId) -> Option<TenantView> {
        (**self).tenant(tenant)
    }
    fn stats(&self, scope: Scope) -> Stats {
        (**self).stats(scope)
    }
    async fn clients(&self, scope: Scope) -> Result<Vec<ClientView>, AdminError> {
        (**self).clients(scope).await
    }
    async fn drain(&self, ttl: Duration) -> Result<Duration, AdminError> {
        (**self).drain(ttl).await
    }
    async fn undrain(&self) -> Result<(), AdminError> {
        (**self).undrain().await
    }
    async fn reset_pool(&self, key: &PoolKey) -> Result<u32, AdminError> {
        (**self).reset_pool(key).await
    }
}

#[cfg(any(test, feature = "test-fakes"))]
pub use fake::FakeObservatory;

#[cfg(any(test, feature = "test-fakes"))]
mod fake {
    use std::collections::BTreeMap;
    use std::sync::{Mutex, PoisonError};

    use super::{
        AdminError, Arc, ClientView, ClusterDigest, ClusterView, Config, Duration, MembershipView,
        NodeId, Observatory, PoolKey, PoolStats, PoolView, Scope, ServerView, Stats, TenantId,
        TenantView,
    };
    use crate::cluster::{Member, NodeMode};

    /// State the fake can be told to serve.
    #[derive(Debug, Default)]
    struct State {
        pools: Vec<PoolView>,
        servers: Vec<ServerView>,
        tenants: Vec<TenantView>,
        clients: Vec<ClientView>,
        /// Peers this fake pretends it cannot reach during a fan-out.
        unreachable: bool,
        mode: NodeMode,
        drained_for: Option<Duration>,
        config: Arc<Config>,
        digests: BTreeMap<NodeId, ClusterDigest>,
    }

    /// An in-memory [`Observatory`] for tests.
    ///
    /// Actually honours scope and actually refuses what the real one refuses,
    /// rather than recording calls. A fake that returned the same rows for
    /// `Scope::Local` and `Scope::Cluster` would let the whole cluster-scoped
    /// design go untested.
    #[derive(Debug)]
    pub struct FakeObservatory {
        node: NodeId,
        state: Mutex<State>,
    }

    impl FakeObservatory {
        /// A fake answering as `node`, with nothing in it.
        #[must_use]
        pub fn new(node: NodeId) -> Arc<Self> {
            Arc::new(Self {
                node,
                state: Mutex::new(State::default()),
            })
        }

        fn lock(&self) -> std::sync::MutexGuard<'_, State> {
            self.state.lock().unwrap_or_else(PoisonError::into_inner)
        }

        /// Sets the pools it reports.
        pub fn set_pools(&self, pools: Vec<PoolView>) {
            self.lock().pools = pools;
        }

        /// Sets the servers it reports.
        pub fn set_servers(&self, servers: Vec<ServerView>) {
            self.lock().servers = servers;
        }

        /// Sets the tenants it reports.
        pub fn set_tenants(&self, tenants: Vec<TenantView>) {
            self.lock().tenants = tenants;
        }

        /// Sets the clients it reports.
        pub fn set_clients(&self, clients: Vec<ClientView>) {
            self.lock().clients = clients;
        }

        /// Sets the configuration it reports.
        pub fn set_config(&self, config: Config) {
            self.lock().config = Arc::new(config);
        }

        /// Records a node's digest, as gossip would.
        pub fn set_digest(&self, digest: ClusterDigest) {
            self.lock().digests.insert(digest.node, digest);
        }

        /// Makes fan-out reads report a partial answer.
        pub fn set_unreachable(&self, unreachable: bool) {
            self.lock().unreachable = unreachable;
        }

        /// The mode this node was last put into, and for how long.
        ///
        /// `None` for the duration means active: there is no such thing as a
        /// drain without one.
        #[must_use]
        pub fn mode(&self) -> (NodeMode, Option<Duration>) {
            let state = self.lock();
            (state.mode, state.drained_for)
        }

        /// The longest drain this fake will accept, so the clamp is testable.
        pub const MAX_TTL: Duration = Duration::from_secs(4 * 60 * 60);

        /// Keeps only what this node owns, when the scope says so.
        fn narrow<T: Clone>(
            &self,
            scope: Scope,
            all: &[T],
            node_of: impl Fn(&T) -> NodeId,
        ) -> Vec<T> {
            all.iter()
                .filter(|item| scope.is_cluster() || node_of(item) == self.node)
                .cloned()
                .collect()
        }
    }

    #[async_trait::async_trait]
    impl Observatory for FakeObservatory {
        fn cluster(&self) -> ClusterView {
            let state = self.lock();
            let members: Vec<Member> = state
                .digests
                .values()
                .map(|digest| Member {
                    id: digest.node,
                    mode: digest.mode,
                })
                .collect();
            let membership = MembershipView::new(self.node, members);
            // Order-independent, like the real digest store's, so two nodes
            // that know the same things agree.
            let view_hash = state
                .digests
                .values()
                .map(|digest| u64::from(digest.node.get()))
                .fold(0_u64, u64::wrapping_add);

            ClusterView {
                node: self.node,
                membership,
                digests: state.digests.values().cloned().collect(),
                view_hash,
            }
        }

        fn config(&self) -> Arc<Config> {
            Arc::clone(&self.lock().config)
        }

        fn pools(&self, scope: Scope) -> Vec<PoolView> {
            let state = self.lock();
            self.narrow(scope, &state.pools, |pool| pool.node)
        }

        fn servers(&self, scope: Scope) -> Vec<ServerView> {
            let state = self.lock();
            if scope.is_cluster() {
                return state.servers.clone();
            }
            // Locally, only what this node itself may hold is meaningful.
            state
                .servers
                .iter()
                .map(|server| ServerView {
                    in_use: server.guaranteed.saturating_add(server.leased),
                    ..server.clone()
                })
                .collect()
        }

        fn tenants(&self, scope: Scope) -> Vec<TenantView> {
            let state = self.lock();
            if scope.is_cluster() {
                return state.tenants.clone();
            }
            state
                .tenants
                .iter()
                .filter(|tenant| tenant.home == Some(self.node))
                .cloned()
                .collect()
        }

        fn tenant(&self, tenant: &TenantId) -> Option<TenantView> {
            self.lock()
                .tenants
                .iter()
                .find(|view| &view.tenant == tenant)
                .cloned()
        }

        fn stats(&self, scope: Scope) -> Stats {
            let clients = self.clients_now(scope);
            let pools = self.pools(scope);
            Stats {
                client_conns: u32::try_from(clients.len()).unwrap_or(u32::MAX),
                upstream_conns: pools
                    .iter()
                    .map(|pool| pool.stats.total())
                    .fold(0, u32::saturating_add),
                waiting: pools
                    .iter()
                    .map(|pool| pool.stats.waiting)
                    .fold(0, u32::saturating_add),
                ..Stats::default()
            }
        }

        async fn clients(&self, scope: Scope) -> Result<Vec<ClientView>, AdminError> {
            let clients = self.clients_now(scope);
            if scope.is_cluster() && self.lock().unreachable {
                // Partial, not empty: an incomplete answer is still useful, and
                // an incomplete answer presented as complete is not.
                return Err(AdminError::Partial {
                    reason: format!("{} of the fleet did not answer", "one node"),
                });
            }
            Ok(clients)
        }

        async fn drain(&self, ttl: Duration) -> Result<Duration, AdminError> {
            // Clamped rather than refused, as a real implementation does: a
            // caller asking for a week wants the node drained, and refusing
            // during an incident helps nobody.
            let applied = ttl.min(Self::MAX_TTL);
            let mut state = self.lock();
            state.mode = NodeMode::Draining;
            state.drained_for = Some(applied);
            Ok(applied)
        }

        async fn undrain(&self) -> Result<(), AdminError> {
            let mut state = self.lock();
            state.mode = NodeMode::Active;
            state.drained_for = None;
            Ok(())
        }

        async fn reset_pool(&self, key: &PoolKey) -> Result<u32, AdminError> {
            let mut state = self.lock();
            let Some(pool) = state.pools.iter_mut().find(|pool| &pool.key == key) else {
                return Err(AdminError::NotFound {
                    kind: "pool",
                    name: key.to_string(),
                });
            };
            // Idle only. Connections in use are finishing real transactions.
            let closed = pool.stats.idle;
            pool.stats = PoolStats {
                idle: 0,
                ..pool.stats
            };
            Ok(closed)
        }
    }

    impl FakeObservatory {
        /// Clients, without the fan-out failure, for the sync callers.
        fn clients_now(&self, scope: Scope) -> Vec<ClientView> {
            let state = self.lock();
            self.narrow(scope, &state.clients, |client| client.node)
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    // Not imported at file scope: the trait and its DTOs have no use for it,
    // and importing it there left the default build warning while
    // `--all-features` did not, because the fake below does use it.
    use crate::cluster::NodeMode;
    use crate::ids::ConnId;

    fn node(n: u16) -> NodeId {
        NodeId::new(n)
    }

    fn key(user: &str) -> PoolKey {
        PoolKey::new(ServerId::new("db-1", 5432), "tenant_acme", user)
    }

    fn pool_view(node_id: u16, user: &str, idle: u32) -> PoolView {
        PoolView {
            node: node(node_id),
            key: key(user),
            stats: PoolStats {
                active: 1,
                idle,
                waiting: 0,
                limit: 10,
            },
        }
    }

    fn client_view(node_id: u16, tenant: &str) -> ClientView {
        ClientView {
            conn: ConnId::new(node(node_id), 1),
            tenant: TenantId::new(tenant),
            node: node(node_id),
            state: ClientState::Idle,
            since: Duration::from_secs(5),
            pinned: None,
        }
    }

    fn digest(n: u16, mode: NodeMode) -> ClusterDigest {
        ClusterDigest {
            node: node(n),
            mode,
            client_conns: 0,
            upstream_conns: Vec::new(),
            tenant_usage: Vec::new(),
        }
    }

    #[test]
    fn scope_defaults_to_the_whole_cluster() {
        // An operator asking a question almost never means "on whichever pod my
        // request happened to reach".
        assert_eq!(Scope::default(), Scope::Cluster);
        assert!(Scope::default().is_cluster());
        assert_eq!(Scope::parse(""), Some(Scope::Cluster));
        assert_eq!(Scope::parse("cluster"), Some(Scope::Cluster));
        assert_eq!(Scope::parse(" LOCAL "), Some(Scope::Local));
    }

    #[test]
    fn an_unrecognised_scope_is_reportable_rather_than_defaulted() {
        // Quietly answering for the cluster when someone asked for something
        // they spelled wrong is how an operator draws the wrong conclusion from
        // a real number.
        for bad in ["node", "loca", "everything", "1"] {
            assert_eq!(Scope::parse(bad), None, "{bad}");
        }
    }

    #[tokio::test]
    async fn the_fake_actually_honours_scope() {
        // A fake returning the same rows for both scopes would let the whole
        // cluster-scoped design go untested.
        let observatory = FakeObservatory::new(node(1));
        observatory.set_pools(vec![pool_view(1, "a", 2), pool_view(2, "b", 3)]);
        observatory.set_clients(vec![client_view(1, "acme"), client_view(2, "globex")]);

        assert_eq!(observatory.pools(Scope::Cluster).len(), 2);
        assert_eq!(observatory.pools(Scope::Local).len(), 1);
        assert_eq!(observatory.pools(Scope::Local)[0].node, node(1));

        assert_eq!(observatory.clients(Scope::Cluster).await.unwrap().len(), 2);
        assert_eq!(observatory.clients(Scope::Local).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn an_unreachable_peer_is_a_partial_answer_rather_than_a_short_one() {
        // An incomplete answer is still useful. An incomplete answer presented
        // as complete is how an operator concludes a tenant has no clients.
        let observatory = FakeObservatory::new(node(1));
        observatory.set_clients(vec![client_view(1, "acme")]);
        observatory.set_unreachable(true);

        let err = observatory.clients(Scope::Cluster).await.unwrap_err();
        assert!(matches!(err, AdminError::Partial { .. }), "{err:?}");

        // A local read needs no peers, so it still answers.
        assert_eq!(observatory.clients(Scope::Local).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn resetting_a_pool_closes_idle_connections_and_leaves_the_rest() {
        // An operator asking for a reset is not asking for in-flight
        // transactions to fail.
        let observatory = FakeObservatory::new(node(1));
        observatory.set_pools(vec![pool_view(1, "a", 4)]);

        assert_eq!(observatory.reset_pool(&key("a")).await.unwrap(), 4);
        let after = observatory.pools(Scope::Local);
        assert_eq!(after[0].stats.idle, 0);
        assert_eq!(after[0].stats.active, 1, "an in-use connection was closed");
    }

    #[tokio::test]
    async fn resetting_a_pool_that_does_not_exist_says_so() {
        let observatory = FakeObservatory::new(node(1));
        let err = observatory.reset_pool(&key("missing")).await.unwrap_err();
        assert!(matches!(err, AdminError::NotFound { kind: "pool", .. }));
        assert!(err.to_string().contains("missing"), "got {err}");
    }

    #[tokio::test]
    async fn a_drain_through_this_contract_always_expires() {
        // The expiry is not optional, which is what stops this API and the
        // config document meaning different things by the same absence. The
        // declarative form is the document; this one is the incident tool.
        let observatory = FakeObservatory::new(node(1));
        assert_eq!(observatory.mode(), (NodeMode::Active, None));

        let applied = observatory.drain(Duration::from_secs(600)).await.unwrap();
        assert_eq!(applied, Duration::from_secs(600));
        assert_eq!(
            observatory.mode(),
            (NodeMode::Draining, Some(Duration::from_secs(600)))
        );
    }

    #[tokio::test]
    async fn an_over_long_drain_is_shortened_and_says_so() {
        // A caller asking for a week wants the node drained, and refusing
        // during an incident helps nobody. Returning what was applied is how
        // they find out they got something shorter.
        let observatory = FakeObservatory::new(node(1));
        let applied = observatory
            .drain(Duration::from_secs(86_400 * 7))
            .await
            .unwrap();

        assert_eq!(applied, FakeObservatory::MAX_TTL);
        assert_eq!(observatory.mode().1, Some(FakeObservatory::MAX_TTL));
    }

    #[tokio::test]
    async fn undraining_returns_the_node_to_the_document() {
        let observatory = FakeObservatory::new(node(1));
        observatory.drain(Duration::from_secs(600)).await.unwrap();

        observatory.undrain().await.unwrap();
        assert_eq!(observatory.mode(), (NodeMode::Active, None));
    }

    #[test]
    fn the_fake_reports_the_configuration_it_was_given() {
        // Exercised through pgprox-admin's tests, which do not count toward
        // this crate's coverage, so it is covered here too.
        let observatory = FakeObservatory::new(node(1));
        assert_eq!(observatory.config().max_client_conns, 10_000);

        observatory.set_config(Config {
            max_client_conns: 250,
            ..Config::default()
        });
        assert_eq!(observatory.config().max_client_conns, 250);
    }

    #[test]
    fn the_cluster_view_carries_a_hash_that_two_nodes_can_compare() {
        // Split brain surfaces as two pods disagreeing about this number,
        // rather than being inferred from two lists that look similar.
        let a = FakeObservatory::new(node(1));
        let b = FakeObservatory::new(node(2));
        for observatory in [&a, &b] {
            observatory.set_digest(digest(1, NodeMode::Active));
            observatory.set_digest(digest(2, NodeMode::Active));
        }
        assert_eq!(a.cluster().view_hash, b.cluster().view_hash);

        b.set_digest(digest(3, NodeMode::Active));
        assert_ne!(
            a.cluster().view_hash,
            b.cluster().view_hash,
            "two pods with different membership reported the same view"
        );
    }

    #[test]
    fn the_cluster_view_reports_who_answered() {
        let observatory = FakeObservatory::new(node(2));
        observatory.set_digest(digest(1, NodeMode::Active));
        observatory.set_digest(digest(2, NodeMode::Draining));

        let view = observatory.cluster();
        assert_eq!(view.node, node(2));
        assert_eq!(view.digests.len(), 2);
        assert_eq!(view.membership.local(), node(2));
        assert_eq!(
            view.membership.active_count(),
            1,
            "a drainer counted as active"
        );
    }

    #[test]
    fn server_headroom_is_the_number_an_operator_wants() {
        let view = ServerView {
            server: ServerId::new("db-1", 5432),
            cap: 100,
            in_use: 60,
            guaranteed: 10,
            leased: 5,
        };
        assert_eq!(view.headroom(), 40);

        // A momentarily inconsistent set of digests must report no headroom
        // rather than wrapping to four billion and looking healthy.
        let over = ServerView {
            in_use: 200,
            ..view
        };
        assert_eq!(over.headroom(), 0);
    }

    #[test]
    fn a_local_scope_reports_only_what_this_node_may_hold() {
        let observatory = FakeObservatory::new(node(1));
        observatory.set_servers(vec![ServerView {
            server: ServerId::new("db-1", 5432),
            cap: 100,
            in_use: 60,
            guaranteed: 10,
            leased: 5,
        }]);

        assert_eq!(observatory.servers(Scope::Cluster)[0].in_use, 60);
        assert_eq!(observatory.servers(Scope::Local)[0].in_use, 15);
    }

    #[test]
    fn tenants_narrow_to_the_ones_this_node_homes() {
        let observatory = FakeObservatory::new(node(1));
        observatory.set_tenants(vec![
            TenantView {
                tenant: TenantId::new("acme"),
                home: Some(node(1)),
                client_conns: 3,
                upstream_conns: 1,
            },
            TenantView {
                tenant: TenantId::new("globex"),
                home: Some(node(2)),
                client_conns: 5,
                upstream_conns: 2,
            },
        ]);

        assert_eq!(observatory.tenants(Scope::Cluster).len(), 2);
        assert_eq!(observatory.tenants(Scope::Local).len(), 1);
        assert_eq!(
            observatory
                .tenant(&TenantId::new("globex"))
                .unwrap()
                .client_conns,
            5,
            "a named tenant is answerable whichever node homes it"
        );
        assert!(observatory.tenant(&TenantId::new("nobody")).is_none());
    }

    #[test]
    fn stats_add_up_across_the_scope_asked_for() {
        let observatory = FakeObservatory::new(node(1));
        observatory.set_pools(vec![pool_view(1, "a", 2), pool_view(2, "b", 3)]);
        observatory.set_clients(vec![client_view(1, "acme"), client_view(2, "globex")]);

        let cluster = observatory.stats(Scope::Cluster);
        assert_eq!(cluster.client_conns, 2);
        assert_eq!(cluster.upstream_conns, 7);

        let local = observatory.stats(Scope::Local);
        assert_eq!(local.client_conns, 1);
        assert_eq!(local.upstream_conns, 3);
    }

    #[test]
    fn client_states_use_pgbouncers_names() {
        // So an existing dashboard parsing SHOW CLIENTS keeps working.
        assert_eq!(ClientState::Idle.as_str(), "idle");
        assert_eq!(ClientState::Active.as_str(), "active");
        assert_eq!(ClientState::Waiting.as_str(), "waiting");
    }

    #[test]
    fn admin_errors_say_which_kind_of_incomplete_they_are() {
        // Whether the answer is wrong or merely partial decides whether an
        // operator should act on it.
        let not_found = AdminError::NotFound {
            kind: "tenant",
            name: "acme".into(),
        };
        assert!(not_found.to_string().contains("tenant"));

        let refused = AdminError::Refused {
            reason: "already draining".into(),
        };
        assert!(refused.to_string().contains("already draining"));

        let partial = AdminError::Partial {
            reason: "pgprox-3 timed out".into(),
        };
        assert!(partial.to_string().contains("pgprox-3"));
        assert_ne!(partial, refused);
    }

    #[tokio::test]
    async fn the_observatory_works_through_an_arc_dyn() {
        let fake = FakeObservatory::new(node(1));
        fake.set_pools(vec![pool_view(1, "a", 1)]);
        let observatory: Arc<dyn Observatory> = fake;

        assert_eq!(observatory.cluster().node, node(1));
        assert_eq!(observatory.pools(Scope::Cluster).len(), 1);
        assert_eq!(observatory.servers(Scope::Cluster).len(), 0);
        assert!(observatory.clients(Scope::Cluster).await.is_ok());
        assert_eq!(observatory.config().max_client_conns, 10_000);
        assert_eq!(observatory.stats(Scope::Cluster).client_conns, 0);
        assert!(observatory.drain(Duration::from_secs(60)).await.is_ok());
        assert!(observatory.undrain().await.is_ok());
        assert!(observatory.reset_pool(&key("a")).await.is_ok());
        assert!(observatory.tenant(&TenantId::new("nobody")).is_none());
        assert_eq!(observatory.tenants(Scope::Cluster).len(), 0);
    }

    /// The structural guarantee: no admin type can carry a credential.
    ///
    /// A compile-fail test rather than a convention, because the guarantee is
    /// only as good as the next person adding a field, and a reviewer reading a
    /// diff is exactly who misses it.
    ///
    /// ```compile_fail
    /// use pgprox_core::admin::TenantView;
    /// use pgprox_core::SecretString;
    ///
    /// // There is no field for one, and adding a struct-update over a secret
    /// // does not type-check.
    /// let _: SecretString = TenantView {
    ///     tenant: pgprox_core::ids::TenantId::new("acme"),
    ///     home: None,
    ///     client_conns: 0,
    ///     upstream_conns: 0,
    /// }.password;
    /// ```
    #[test]
    fn no_admin_type_holds_a_credential() {
        // The compile_fail doctest above proves the field does not exist. This
        // asserts the rendered forms cannot contain one either, which is what a
        // handler would actually leak.
        let observatory = FakeObservatory::new(node(1));
        observatory.set_pools(vec![pool_view(1, "acme_app", 1)]);
        observatory.set_tenants(vec![TenantView {
            tenant: TenantId::new("acme"),
            home: Some(node(1)),
            client_conns: 1,
            upstream_conns: 1,
        }]);

        let rendered = format!(
            "{:?}{:?}{:?}{:?}",
            observatory.pools(Scope::Cluster),
            observatory.tenants(Scope::Cluster),
            observatory.cluster(),
            observatory.stats(Scope::Cluster),
        );
        for forbidden in ["password", "secret", "token", "redacted"] {
            assert!(
                !rendered.to_lowercase().contains(forbidden),
                "an admin type rendered something that looks like a credential: {rendered}"
            );
        }
    }
}
