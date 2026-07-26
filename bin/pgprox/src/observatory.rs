//! What the admin surfaces read.
//!
//! ADR 0018 says the HTTP API and the `SHOW` pseudo-database cannot drift into
//! different answers, because both read one contract. This is that contract's
//! only real implementation, and until now the only one was a fake.
//!
//! # Cluster-scoped by default
//!
//! Every aggregate answers from the local gossip digest, so hitting any pod
//! gives the fleet's answer with no fan-out. `?scope=local` narrows to the node
//! that answered. Only [`Observatory::clients`] fans out, because a node knows
//! only its own clients, and its signature says so.
//!
//! # What a live node knows that a fake cannot
//!
//! A pool exists because a client of that database connected, not because the
//! configuration mentions it. So `pools` reads the pool map rather than the
//! document, and a node serving nobody reports no pools rather than reporting
//! every pool it might one day hold.

use std::sync::Arc;
use std::time::Duration;

use pgprox_cluster::service::GossipCoordinator;
use pgprox_core::admin::{
    AdminError, ClientView, ClusterView, Observatory, PoolView, Scope, ServerView, Stats,
    TenantView,
};
use pgprox_core::clock::Clock;
use pgprox_core::cluster::{ClusterCoordinator, NodeMode};
use pgprox_core::config::{Config, ConfigSource};
use pgprox_core::ids::{NodeId, PoolKey, TenantId};
use pgprox_core::pool::PoolStats;
use pgprox_pool::reap::ReapConfig;

use crate::sessions::Sessions;
use crate::wiring::{NodePool, SharedDrain};

/// The live [`Observatory`].
#[derive(Debug)]
pub struct NodeObservatory {
    node: NodeId,
    clock: Arc<dyn Clock>,
    config: Arc<dyn ConfigSource>,
    cluster: Arc<GossipCoordinator>,
    pool: Arc<NodePool>,
    sessions: Arc<Sessions>,
    /// The imperative drain overlay, shared with the node that owns it.
    ///
    /// Shared rather than owned, because `/readyz` and this API have to be
    /// reading the same fact. They were two `DrainState`s until `M6.26`, which
    /// meant a drain posted here left the probe passing and the node kept
    /// taking traffic it had just been told to stop taking.
    ///
    /// A `std` mutex despite the trait's write methods being async: no guard
    /// here is held across an await, and a `tokio` mutex could not be read from
    /// the probe path, which is sync.
    drain: SharedDrain,
    /// Where the other nodes are, for the one read that fans out.
    ///
    /// Set after construction, because a peer table is a deployment fact and
    /// `App::build` opens no sockets. Empty until it is, which reads as a node
    /// that is alone: the same answer it gave before the fan-out existed.
    peers: std::sync::OnceLock<std::collections::BTreeMap<NodeId, String>>,
}

impl NodeObservatory {
    /// An observatory over one node's live components.
    #[must_use]
    pub fn new(
        node: NodeId,
        clock: Arc<dyn Clock>,
        config: Arc<dyn ConfigSource>,
        cluster: Arc<GossipCoordinator>,
        pool: Arc<NodePool>,
        sessions: Arc<Sessions>,
        drain: SharedDrain,
    ) -> Self {
        Self {
            node,
            clock,
            config,
            cluster,
            pool,
            sessions,
            drain,
            peers: std::sync::OnceLock::new(),
        }
    }

    /// Tells this observatory where its peers are.
    ///
    /// Once: a second call would mean two answers to "who is in the fleet",
    /// and the run loop is the only caller.
    pub fn set_peers(&self, peers: std::collections::BTreeMap<NodeId, String>) -> bool {
        self.peers.set(peers).is_ok()
    }

    /// The pools this node holds, as views.
    fn local_pools(&self) -> Vec<PoolView> {
        self.pool
            .all_stats()
            .into_iter()
            .map(|(key, stats)| PoolView {
                node: self.node,
                key,
                stats,
            })
            .collect()
    }
}

#[async_trait::async_trait]
impl Observatory for NodeObservatory {
    fn cluster(&self) -> ClusterView {
        ClusterView {
            node: self.node,
            membership: self.cluster.membership(),
            digests: self.cluster.digests(),
            view_hash: self.cluster.view_hash(),
        }
    }

    fn config(&self) -> Arc<Config> {
        // From the watch rather than from a copy taken at startup, so a
        // reloaded document is what an operator sees rather than what the node
        // booted with.
        self.config.watch().borrow().clone()
    }

    fn config_is_current(&self) -> bool {
        self.config.is_healthy()
    }

    fn pools(&self, scope: Scope) -> Vec<PoolView> {
        let mut pools = self.local_pools();
        if matches!(scope, Scope::Local) {
            return pools;
        }

        // Peers report totals per server rather than per pool, because
        // gossiping every pool would put one message per database on the wire
        // every second. So a cluster-scoped answer carries this node's pools in
        // full and one summarising row per peer, which is the honest shape of
        // what gossip actually knows.
        for digest in self.cluster.digests() {
            if digest.node == self.node {
                continue;
            }
            for (server, count) in digest.upstream_conns {
                pools.push(PoolView {
                    node: digest.node,
                    key: PoolKey::new(server, "*", "*"),
                    stats: PoolStats {
                        active: count,
                        ..PoolStats::default()
                    },
                });
            }
        }
        pools
    }

    fn servers(&self, scope: Scope) -> Vec<ServerView> {
        let config = self.config();
        config
            .servers
            .iter()
            .map(|server| {
                let allowance = self.cluster.allowance(&server.server);
                let in_use = if matches!(scope, Scope::Local) {
                    self.local_pools()
                        .iter()
                        .filter(|pool| pool.key.server == server.server)
                        .map(|pool| pool.stats.active + pool.stats.idle)
                        .sum()
                } else {
                    self.cluster.cluster_usage(&server.server)
                };

                ServerView {
                    server: server.server.clone(),
                    cap: server.max_connections,
                    in_use,
                    guaranteed: allowance.guaranteed,
                    leased: allowance.leased,
                }
            })
            .collect()
    }

    fn tenants(&self, scope: Scope) -> Vec<TenantView> {
        let membership = self.cluster.membership();
        let mut tenants: std::collections::BTreeMap<TenantId, TenantView> =
            std::collections::BTreeMap::new();

        for digest in self.cluster.digests() {
            if matches!(scope, Scope::Local) && digest.node != self.node {
                continue;
            }
            for (tenant, count) in digest.tenant_usage {
                let entry = tenants.entry(tenant.clone()).or_insert_with(|| TenantView {
                    home: membership.home_node(&tenant),
                    tenant,
                    client_conns: 0,
                    upstream_conns: 0,
                });
                entry.upstream_conns += count;
            }
        }

        for (tenant, clients) in self.sessions.per_tenant() {
            let entry = tenants.entry(tenant.clone()).or_insert_with(|| TenantView {
                home: membership.home_node(&tenant),
                tenant,
                client_conns: 0,
                upstream_conns: 0,
            });
            entry.client_conns += clients;
        }

        tenants.into_values().collect()
    }

    fn tenant(&self, tenant: &TenantId) -> Option<TenantView> {
        // Cluster scope, because a tenant an operator asks about by name is
        // rarely one they believe is on the pod they happened to reach.
        self.tenants(Scope::Cluster)
            .into_iter()
            .find(|view| &view.tenant == tenant)
    }

    fn stats(&self, scope: Scope) -> Stats {
        let pools = self.local_pools();
        let local_upstream: u32 = pools.iter().map(|p| p.stats.active + p.stats.idle).sum();

        Stats {
            client_conns: match scope {
                Scope::Local => self.sessions.len(),
                _ => self.cluster.cluster_clients(),
            },
            upstream_conns: match scope {
                Scope::Local => local_upstream,
                _ => self
                    .cluster
                    .digests()
                    .iter()
                    .flat_map(|digest| digest.upstream_conns.iter().map(|(_, count)| *count))
                    .sum(),
            },
            transactions: self.sessions.transactions(),
            pins: self.sessions.pins(),
            sheds: self.sessions.sheds(),
            waiting: pools.iter().map(|p| p.stats.waiting).sum(),
        }
    }

    async fn clients(&self, scope: Scope) -> Result<Vec<ClientView>, AdminError> {
        let mut clients = self.sessions.views(self.clock.now());
        if matches!(scope, Scope::Local) {
            return Ok(clients);
        }

        // The only read that costs a round trip, and its signature says so.
        // Aggregates answer from the digest every node already holds; a client
        // list is one row per connection, and gossiping those every second
        // would put a hundred thousand rows on the wire.
        let peers = self.peers.get().cloned().unwrap_or_default();
        let mut missed = Vec::new();
        for (node, address) in &peers {
            if *node == self.node {
                continue;
            }
            match crate::gossip::clients_of(address).await {
                Ok(theirs) => clients.extend(theirs),
                Err(reason) => missed.push(format!("{node}: {reason}")),
            }
        }

        clients.sort_by_key(|view| (view.node, view.conn));
        if missed.is_empty() {
            return Ok(clients);
        }

        // Partial rather than short. An operator seeing a list with a node
        // silently missing from it concludes that node has no clients, which
        // is the one failure ADR 0018 singles out.
        Err(AdminError::Partial {
            reason: format!(
                "{} peer(s) did not answer: {}",
                missed.len(),
                missed.join("; ")
            ),
        })
    }

    async fn drain(&self, ttl: Duration) -> Result<Duration, AdminError> {
        let now = self.clock.now();
        let mut drain = crate::wiring::lock(&self.drain);
        let expires_at = drain.set(NodeMode::Draining, Some(ttl), now);
        Ok(expires_at.saturating_duration_since(now))
    }

    async fn undrain(&self) -> Result<(), AdminError> {
        let now = self.clock.now();
        let config = self.config();
        let mut drain = crate::wiring::lock(&self.drain);

        // Refused rather than silently ineffective. The next config poll would
        // put the drain back, so the node would oscillate and the operator
        // would be told it had worked.
        if matches!(
            drain.mode(&config, now),
            (
                NodeMode::Draining,
                pgprox_config::drain::ModeSource::Document
            )
        ) {
            return Err(AdminError::Refused {
                reason: "this node is draining because the config document says so; \
                         change the document instead"
                    .to_owned(),
            });
        }

        drain.clear();
        Ok(())
    }

    async fn reset_pool(&self, key: &PoolKey) -> Result<u32, AdminError> {
        if !self.pool.all_stats().iter().any(|(held, _)| held == key) {
            return Err(AdminError::NotFound {
                kind: "pool",
                name: format!("{}/{}", key.database, key.user),
            });
        }

        // Idle only, which is what ReapConfig with a zero idle timeout does.
        // An operator asking for a reset is not asking for live transactions
        // to fail.
        let closed = self.pool.reap_idle(&ReapConfig {
            idle_timeout: Duration::ZERO,
            ..ReapConfig::default()
        });
        Ok(u32::try_from(closed).unwrap_or(u32::MAX))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use pgprox_cluster::coordinator::CoordinatorConfig;
    use pgprox_core::clock::FakeClock;
    use pgprox_core::cluster::ClusterDigest;
    use pgprox_core::config::{FakeConfigSource, NodeOverride, ServerConfig};
    use pgprox_core::ids::ServerId;
    use pgprox_core::pool::UpstreamPool;
    use pgprox_pool::live::LivePool;
    use pgprox_pool::pool::PoolConfig;
    use pgprox_session::connect::PgConnector;
    use std::time::Instant;

    use crate::dial::TcpUpstream;

    fn server() -> ServerId {
        ServerId::new("db-1", 5432)
    }

    fn config() -> Config {
        Config {
            servers: vec![ServerConfig {
                server: server(),
                max_connections: 100,
                guaranteed_fraction: 0.5,
            }],
            ..Config::default()
        }
    }

    fn digest(node: u16) -> ClusterDigest {
        ClusterDigest {
            node: NodeId::new(node),
            mode: NodeMode::Active,
            client_conns: u32::from(node),
            upstream_conns: vec![(server(), u32::from(node) * 2)],
            tenant_usage: vec![(TenantId::new("acme"), u32::from(node))],
        }
    }

    struct Fixture {
        observatory: NodeObservatory,
        sessions: Arc<Sessions>,
        pool: Arc<NodePool>,
        clock: FakeClock,
        source: Arc<FakeConfigSource>,
    }

    fn fixture(config: Config) -> Fixture {
        let clock = FakeClock::new();
        let shared: Arc<dyn Clock> = Arc::new(clock.clone());
        let source = FakeConfigSource::new(config).expect("the test config is valid");
        let cluster = GossipCoordinator::new(
            NodeId::new(1),
            CoordinatorConfig::default(),
            Arc::clone(&shared),
        );
        cluster.set_cap(server(), 100);
        for node in [1_u16, 2] {
            cluster.gossip(pgprox_cluster::digest::VersionedDigest {
                digest: digest(node),
                version: 1,
            });
        }
        cluster.tick();

        let tls = pgprox_tls::client_config(tokio_rustls::rustls::RootCertStore::empty()).unwrap();
        let pool = LivePool::new(
            Arc::new(PgConnector::new(TcpUpstream::new(tls))),
            Arc::clone(&shared),
            PoolConfig::default(),
        );
        let sessions = Sessions::new();
        let observatory = NodeObservatory::new(
            NodeId::new(1),
            Arc::clone(&shared),
            Arc::clone(&source) as Arc<dyn ConfigSource>,
            cluster,
            Arc::clone(&pool),
            Arc::clone(&sessions),
            Arc::new(std::sync::Mutex::new(
                pgprox_config::drain::DrainState::new(
                    "pgprox-1",
                    pgprox_config::drain::DrainConfig::default(),
                ),
            )),
        );

        Fixture {
            observatory,
            sessions,
            pool,
            clock,
            source,
        }
    }

    #[test]
    fn the_cluster_view_carries_every_nodes_digest_and_a_hash() {
        // Answered from gossip with no fan-out, which is what lets an operator
        // ask any pod. The hash is what makes two pods' answers comparable
        // without reading two lists side by side.
        let fixture = fixture(config());
        let view = fixture.observatory.cluster();

        assert_eq!(view.node, NodeId::new(1));
        assert_eq!(view.digests.len(), 2);
        assert_ne!(view.view_hash, 0);
    }

    #[test]
    fn the_configuration_reported_is_the_live_one() {
        // Not the copy taken at startup. An operator checking whether a reload
        // landed is asking exactly this.
        let fixture = fixture(config());
        assert_eq!(fixture.observatory.config().max_client_conns, 10_000);

        let mut reloaded = config();
        reloaded.max_client_conns = 500;
        fixture.source.publish(reloaded).unwrap();

        assert_eq!(fixture.observatory.config().max_client_conns, 500);
    }

    #[tokio::test]
    async fn a_node_serving_nobody_reports_no_pools() {
        // A pool exists because a client connected, not because the document
        // mentions a server. Reporting one per configured server would show an
        // operator pools that hold nothing.
        let fixture = fixture(config());
        assert!(fixture.observatory.pools(Scope::Local).is_empty());
    }

    #[tokio::test]
    async fn a_pool_appears_once_a_client_has_used_it() {
        let fixture = fixture(config());
        let key = PoolKey::new(server(), "acme", "acme_app");
        // Fails to dial, which is fine: the pool records the attempt, and what
        // is under test is that the observatory reads the pool map.
        let _ = fixture
            .pool
            .acquire(&key, fixture.clock.now() + Duration::from_millis(1))
            .await;

        let pools = fixture.observatory.pools(Scope::Local);
        assert_eq!(pools.len(), 1);
        assert_eq!(pools[0].key, key);
        assert_eq!(pools[0].node, NodeId::new(1));
    }

    #[test]
    fn a_cluster_scoped_pool_read_includes_the_peers_totals() {
        // Gossip carries totals per server rather than per pool, so a
        // cluster-scoped answer is this node's pools plus one summarising row
        // per peer. Pretending otherwise would invent pools nobody holds.
        let fixture = fixture(config());
        let pools = fixture.observatory.pools(Scope::Cluster);

        assert_eq!(pools.len(), 1, "the peer's total was not included");
        assert_eq!(pools[0].node, NodeId::new(2));
        assert_eq!(pools[0].stats.active, 4);
    }

    #[test]
    fn a_server_reports_the_fleets_usage_and_this_nodes_allowance() {
        let fixture = fixture(config());
        let servers = fixture.observatory.servers(Scope::Cluster);

        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].cap, 100);
        // Two nodes gossiping two and four.
        assert_eq!(servers[0].in_use, 6);
        assert_eq!(servers[0].headroom(), 94);
        assert!(servers[0].guaranteed > 0);
    }

    #[test]
    fn a_local_server_read_counts_only_what_this_node_holds() {
        let fixture = fixture(config());
        assert_eq!(fixture.observatory.servers(Scope::Local)[0].in_use, 0);
    }

    #[test]
    fn a_tenant_carries_its_client_and_upstream_counts_and_its_home() {
        let fixture = fixture(config());
        let now = Instant::now();
        let _held = fixture.sessions.register(
            pgprox_core::ids::ConnId::new(NodeId::new(1), 1),
            TenantId::new("acme"),
            NodeId::new(1),
            now,
            16,
            crate::run::Shutdown::new(),
        );

        let view = fixture.observatory.tenant(&TenantId::new("acme")).unwrap();
        assert_eq!(view.client_conns, 1);
        assert_eq!(view.upstream_conns, 3, "the peers' usage was not counted");
        assert!(view.home.is_some());
    }

    #[test]
    fn a_tenant_nobody_has_heard_of_is_not_invented() {
        let fixture = fixture(config());
        assert!(fixture.observatory.tenant(&TenantId::new("nope")).is_none());
    }

    #[test]
    fn stats_count_the_fleet_by_default_and_this_node_on_request() {
        let fixture = fixture(config());
        let now = Instant::now();
        let _held = fixture.sessions.register(
            pgprox_core::ids::ConnId::new(NodeId::new(1), 1),
            TenantId::new("acme"),
            NodeId::new(1),
            now,
            16,
            crate::run::Shutdown::new(),
        );

        assert_eq!(fixture.observatory.stats(Scope::Local).client_conns, 1);
        assert_eq!(
            fixture.observatory.stats(Scope::Cluster).client_conns,
            3,
            "the cluster count did not come from gossip"
        );
    }

    #[tokio::test]
    async fn a_local_client_read_answers_and_a_cluster_one_says_what_is_missing() {
        // The failure ADR 0018 singles out is an incomplete answer presented as
        // complete. The fan-out does not exist yet, so a cluster-scoped read
        // says so rather than reporting one node's clients as the fleet's.
        let fixture = fixture(config());
        let now = Instant::now();
        let _held = fixture.sessions.register(
            pgprox_core::ids::ConnId::new(NodeId::new(1), 1),
            TenantId::new("acme"),
            NodeId::new(1),
            now,
            16,
            crate::run::Shutdown::new(),
        );

        assert_eq!(
            fixture
                .observatory
                .clients(Scope::Local)
                .await
                .unwrap()
                .len(),
            1
        );
        // A node that knows of no peers is alone, and a cluster-scoped read
        // is its own clients. Being told "partial" by a node with nobody to
        // ask would send an operator looking for a peer that is not missing.
        assert_eq!(
            fixture
                .observatory
                .clients(Scope::Cluster)
                .await
                .unwrap()
                .len(),
            1
        );

        // With a peer it cannot reach, the answer is partial rather than
        // short: a list with a node silently absent reads as that node having
        // no clients.
        fixture
            .observatory
            .set_peers(std::collections::BTreeMap::from([(
                NodeId::new(2),
                // Reserved for documentation, so nothing answers and nothing on
                // the machine running this is disturbed by the attempt.
                "192.0.2.1:6433".to_owned(),
            )]));

        assert!(matches!(
            fixture.observatory.clients(Scope::Cluster).await,
            Err(AdminError::Partial { .. })
        ));
    }

    #[tokio::test]
    async fn a_drain_through_the_api_expires_on_its_own() {
        let fixture = fixture(config());
        let applied = fixture
            .observatory
            .drain(Duration::from_secs(60))
            .await
            .unwrap();

        assert_eq!(applied, Duration::from_secs(60));
        fixture.observatory.undrain().await.unwrap();
    }

    #[tokio::test]
    async fn undraining_a_node_the_document_drains_is_refused() {
        // Silently ineffective is the failure here: the next config poll would
        // put the drain back, so the node would oscillate while the operator
        // was told it had worked.
        let mut drained = config();
        drained.nodes.insert(
            "pgprox-1".to_owned(),
            NodeOverride {
                mode: NodeMode::Draining,
            },
        );
        let fixture = fixture(drained);

        let err = fixture.observatory.undrain().await.unwrap_err();
        assert!(matches!(err, AdminError::Refused { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn resetting_a_pool_that_does_not_exist_says_so() {
        let fixture = fixture(config());
        let err = fixture
            .observatory
            .reset_pool(&PoolKey::new(server(), "nope", "nobody"))
            .await
            .unwrap_err();

        assert!(matches!(err, AdminError::NotFound { kind: "pool", .. }));
    }

    #[tokio::test]
    async fn resetting_a_pool_that_exists_closes_its_idle_connections() {
        let fixture = fixture(config());
        let key = PoolKey::new(server(), "acme", "acme_app");
        let _ = fixture
            .pool
            .acquire(&key, fixture.clock.now() + Duration::from_millis(1))
            .await;

        // Nothing is idle, because nothing connected. What is asserted is that
        // a known pool is reset rather than reported missing, which is the
        // branch an operator hits.
        assert_eq!(fixture.observatory.reset_pool(&key).await.unwrap(), 0);
    }
}
