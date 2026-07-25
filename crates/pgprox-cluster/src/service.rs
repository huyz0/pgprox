//! The [`ClusterCoordinator`] implementation the rest of the proxy sees.
//!
//! [`crate::coordinator::NodeCoordinator`] is sans-I/O: every method takes the
//! time it should reason about and needs `&mut self`. The trait in
//! `pgprox-core` is shared, `&self`, and async. This is the adapter between
//! them, and it holds nothing but a lock and a clock.
//!
//! # Where the peer hop is
//!
//! It is not here. `request_quota` answers from the local ledger, which succeeds
//! only when this node is the serving leader. A node that is not the leader gets
//! [`QuotaError::NoLeader`] rather than a lease, and falls back to its
//! guaranteed share, which is exactly the safe direction.
//!
//! Forwarding the request to the leader needs the gossip transport, which is
//! deliberately outside this crate: `pgprox-cluster` needs no socket to be
//! tested, and the invariant is a property of the quota rules rather than of a
//! message-passing layer. See `M3.12` in the backlog.

use std::sync::{Arc, Mutex, PoisonError};

use pgprox_core::clock::Clock;
use pgprox_core::cluster::{
    ClusterCoordinator, ClusterDigest, MembershipView, NodeMode, QuotaError, QuotaLease,
};
use pgprox_core::ids::{NodeId, ServerId, TenantId};

use crate::coordinator::{CoordinatorConfig, NodeCoordinator};
use crate::digest::{MergeOutcome, VersionedDigest};
use crate::quota::NodeAllowance;

/// What the cluster layer knows about where a tenant belongs.
///
/// The cluster-side half of a [`crate::shed::ShedCtx`]. Kept separate because
/// the other half is session state this crate cannot see, and a type that
/// carried both would have to invent the fields it does not own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TenantPlacement {
    /// Whether this node is the tenant's home.
    pub on_home_node: bool,
    /// Whether the tenant's home node has room for another connection.
    pub home_has_headroom: bool,
    /// Whether the home node is draining.
    pub home_draining: bool,
    /// How long since the membership view last changed.
    pub since_membership_change: std::time::Duration,
}

/// A [`ClusterCoordinator`] over a gossiping [`NodeCoordinator`].
#[derive(Debug)]
pub struct GossipCoordinator {
    inner: Mutex<NodeCoordinator>,
    clock: Arc<dyn Clock>,
    local: NodeId,
}

impl GossipCoordinator {
    /// Wraps a coordinator for `local`.
    #[must_use]
    pub fn new(local: NodeId, config: CoordinatorConfig, clock: Arc<dyn Clock>) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(NodeCoordinator::new(local, config, clock.now())),
            clock,
            local,
        })
    }

    /// Runs `f` against the inner coordinator.
    ///
    /// The lock is poisoned only by a panic while held, and every method here is
    /// short and allocation-light. Recovering the guard rather than propagating
    /// keeps a panic in one connection's accounting from taking the cluster
    /// layer down for every other connection on the node.
    fn with<T>(&self, f: impl FnOnce(&mut NodeCoordinator) -> T) -> T {
        let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut guard)
    }

    /// Registers a server's cap.
    pub fn set_cap(&self, server: ServerId, cap: u32) {
        self.with(|c| c.set_cap(server, cap));
    }

    /// Takes in a peer's gossip and brings the ledgers up to date.
    pub fn gossip(&self, incoming: VersionedDigest) -> MergeOutcome {
        let now = self.clock.now();
        self.with(|c| {
            let outcome = c.gossip(incoming, now);
            c.observe(now);
            outcome
        })
    }

    /// One tick of the gossip loop with nothing new to merge.
    ///
    /// Still necessary: liveness, the takeover wait and lease expiry are all
    /// functions of time, so a quiet cluster must keep observing or a departed
    /// node is never noticed.
    pub fn tick(&self) {
        let now = self.clock.now();
        self.with(|c| c.observe(now));
    }

    /// Sets whether this node is taking work, which is how a drain is announced.
    pub fn set_mode(&self, mode: NodeMode) {
        self.with(|c| c.set_mode(mode));
    }

    /// Records what this node is serving, for the next digest.
    pub fn report(&self, client_conns: u32, upstream_conns: Vec<(ServerId, u32)>) {
        self.with(|c| c.report(client_conns, upstream_conns));
    }

    /// Records this node's per-tenant usage, for the next digest.
    pub fn report_tenants(&self, usage: Vec<(TenantId, u32)>) {
        self.with(|c| c.report_tenants(usage));
    }

    /// Starts tracking a tenant's reservation.
    pub fn track_tenant(&self, tenant: TenantId) {
        self.with(|c| c.track_tenant(tenant));
    }

    /// Stops tracking a tenant.
    pub fn forget_tenant(&self, tenant: &TenantId) {
        self.with(|c| c.forget_tenant(tenant));
    }

    /// The cluster-side inputs to a shed decision for one tenant.
    ///
    /// Returns only what this crate knows. The session-scoped fields, whether
    /// the client is idle, pinned or mid-transaction, belong to the caller, so
    /// this deliberately does not return a `ShedCtx`: assembling one here would
    /// mean inventing values for state this crate cannot see.
    #[must_use]
    pub fn placement(&self, tenant: &TenantId, budget: u32) -> TenantPlacement {
        let now = self.clock.now();
        self.with(|c| TenantPlacement {
            on_home_node: c.membership(now).is_home_for(tenant),
            home_has_headroom: c.home_has_headroom(tenant, budget, now),
            home_draining: c.home_draining(tenant, now),
            since_membership_change: c.since_membership_change(now),
        })
    }

    /// What this node may open for a server right now.
    #[must_use]
    pub fn allowance(&self, server: &ServerId) -> NodeAllowance {
        let now = self.clock.now();
        self.with(|c| c.allowance(server, now))
    }
}

#[async_trait::async_trait]
impl ClusterCoordinator for GossipCoordinator {
    fn membership(&self) -> MembershipView {
        let now = self.clock.now();
        self.with(|c| c.membership(now))
    }

    async fn request_quota(&self, server: &ServerId, want: u32) -> Result<QuotaLease, QuotaError> {
        let now = self.clock.now();
        // No await inside: the lock is a `std::sync::Mutex` and must not be held
        // across one. The method is async because the trait is, and because
        // forwarding to the leader will be.
        self.with(|c| {
            let lease = c.request(server, self.local, want, now)?;
            c.accept(lease.clone());
            Ok(lease)
        })
    }

    fn release_quota(&self, lease: QuotaLease) {
        self.with(|c| c.release(lease.server()));
    }

    fn digest(&self) -> ClusterDigest {
        self.with(|c| c.digest())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use pgprox_core::clock::FakeClock;
    use std::time::Duration;

    fn server() -> ServerId {
        ServerId::new("db-1", 5432)
    }

    fn node(n: u16) -> NodeId {
        NodeId::new(n)
    }

    fn digest_for(n: u16, version: u64) -> VersionedDigest {
        VersionedDigest {
            digest: ClusterDigest {
                node: node(n),
                mode: NodeMode::Active,
                client_conns: 0,
                upstream_conns: Vec::new(),
                tenant_usage: Vec::new(),
            },
            version,
        }
    }

    /// A leading coordinator that has served its takeover wait.
    fn serving() -> (Arc<GossipCoordinator>, FakeClock) {
        let clock = FakeClock::new();
        let config = CoordinatorConfig {
            fleet_size: 3,
            ..CoordinatorConfig::default()
        };
        let coordinator = GossipCoordinator::new(node(1), config, Arc::new(clock.clone()));
        coordinator.set_cap(server(), 100);
        // Gossip every second while the wait elapses, as the real loop does.
        // Advancing in one jump would let every peer go suspect and cost the
        // node its quorum, which is the correct behaviour and the wrong setup.
        for round in 1..=12 {
            coordinator.gossip(digest_for(1, round));
            coordinator.gossip(digest_for(2, round));
            coordinator.gossip(digest_for(3, round));
            clock.advance(Duration::from_secs(1));
        }
        coordinator.tick();
        (coordinator, clock)
    }

    #[tokio::test]
    async fn a_leader_grants_and_records_what_it_granted() {
        let (coordinator, _clock) = serving();
        let lease = coordinator.request_quota(&server(), 20).await.unwrap();
        assert_eq!(lease.nominal_count(), 20);
        assert_eq!(
            coordinator.allowance(&server()).leased,
            20,
            "the lease was granted but not recorded against this node"
        );
    }

    #[tokio::test]
    async fn a_node_that_is_not_the_leader_falls_back_to_its_share() {
        // The honest shape of the missing peer hop: no lease, and the guaranteed
        // share still available. Refusing to serve would be the wrong direction.
        let clock = FakeClock::new();
        let config = CoordinatorConfig {
            fleet_size: 3,
            ..CoordinatorConfig::default()
        };
        let coordinator = GossipCoordinator::new(node(2), config, Arc::new(clock.clone()));
        coordinator.set_cap(server(), 100);
        for round in 1..=12 {
            coordinator.gossip(digest_for(1, round));
            coordinator.gossip(digest_for(2, round));
            coordinator.gossip(digest_for(3, round));
            clock.advance(Duration::from_secs(1));
        }
        coordinator.tick();

        assert_eq!(
            coordinator.request_quota(&server(), 20).await.unwrap_err(),
            QuotaError::NoLeader
        );
        assert!(
            coordinator.allowance(&server()).guaranteed > 0,
            "a non-leader lost its guaranteed share"
        );
    }

    #[tokio::test]
    async fn releasing_returns_the_capacity_before_it_expires() {
        let (coordinator, _clock) = serving();
        let lease = coordinator.request_quota(&server(), 50).await.unwrap();
        assert_eq!(coordinator.allowance(&server()).leased, 50);

        coordinator.release_quota(lease);
        assert_eq!(
            coordinator.allowance(&server()).leased,
            0,
            "a released lease still counted"
        );
    }

    #[tokio::test]
    async fn a_lease_lapses_on_its_own_without_a_release() {
        // The property the whole design rests on: capacity comes back whether or
        // not anyone remembers to return it.
        let (coordinator, clock) = serving();
        coordinator.request_quota(&server(), 30).await.unwrap();
        assert_eq!(coordinator.allowance(&server()).leased, 30);

        clock.advance(CoordinatorConfig::default().lease.ttl + Duration::from_millis(1));
        assert_eq!(coordinator.allowance(&server()).leased, 0);
    }

    #[test]
    fn the_digest_reports_what_this_node_was_told_to_report() {
        let (coordinator, _clock) = serving();
        coordinator.report(1_234, vec![(server(), 17)]);

        let digest = coordinator.digest();
        assert_eq!(digest.node, node(1));
        assert_eq!(digest.mode, NodeMode::Active);
        assert_eq!(digest.client_conns, 1_234);
        assert_eq!(digest.upstream_conns, vec![(server(), 17)]);
    }

    #[test]
    fn draining_shows_up_in_the_digest_and_in_the_view() {
        // How a drain is announced. Peers exclude a draining node from
        // leadership and from rendezvous hashing on the strength of this.
        let (coordinator, _clock) = serving();
        coordinator.set_mode(NodeMode::Draining);
        assert_eq!(coordinator.digest().mode, NodeMode::Draining);

        coordinator.gossip(VersionedDigest {
            digest: coordinator.digest(),
            version: 13,
        });
        assert_eq!(coordinator.membership().leader(), Some(node(2)));
    }

    #[tokio::test]
    async fn placement_reports_only_what_the_cluster_layer_knows() {
        // Deliberately not a ShedCtx: the session-scoped half belongs to the
        // caller, and assembling one here would mean inventing those fields.
        let (coordinator, _clock) = serving();
        let view = coordinator.membership();
        let tenant = (0..1_000)
            .map(|i| TenantId::new(format!("tenant-{i}")))
            .find(|t| view.home_node(t) == Some(node(1)))
            .unwrap();

        let placement = coordinator.placement(&tenant, 10);
        assert!(placement.on_home_node);
        assert!(
            placement.home_has_headroom,
            "an unused home had no headroom"
        );
        assert!(!placement.home_draining);
        assert!(placement.since_membership_change > Duration::ZERO);
    }

    #[tokio::test]
    async fn a_tracked_tenant_decays_and_a_forgotten_one_stops() {
        let (coordinator, clock) = serving();
        let tenant = TenantId::new("tenant-1");
        coordinator.track_tenant(tenant.clone());
        for _ in 0..5 {
            clock.advance(Duration::from_secs(1));
            coordinator.tick();
        }
        coordinator.forget_tenant(&tenant);
        clock.advance(Duration::from_secs(1));
        coordinator.tick();

        // Nothing to assert beyond it not panicking and the digest surviving:
        // the decay arithmetic is covered in the coordinator's own tests.
        assert_eq!(coordinator.digest().node, node(1));
    }

    #[test]
    fn the_digest_carries_per_tenant_usage() {
        let (coordinator, _clock) = serving();
        let tenant = TenantId::new("tenant-1");
        coordinator.report_tenants(vec![(tenant.clone(), 9)]);
        assert_eq!(coordinator.digest().tenant_usage, vec![(tenant, 9)]);
    }

    #[test]
    fn membership_reflects_the_gossip_it_has_taken_in() {
        let (coordinator, _clock) = serving();
        let view = coordinator.membership();
        assert_eq!(view.members().len(), 3);
        assert_eq!(view.local(), node(1));
        assert!(view.is_leader());
    }

    #[test]
    fn a_repeated_digest_is_reported_as_stale() {
        let (coordinator, _clock) = serving();
        assert_eq!(coordinator.gossip(digest_for(2, 12)), MergeOutcome::Stale);
        assert_eq!(coordinator.gossip(digest_for(2, 13)), MergeOutcome::Updated);
    }

    #[test]
    #[allow(clippy::panic, clippy::expect_used)]
    fn a_poisoned_lock_does_not_take_the_node_down() {
        // One connection's panic must not stop every other connection on the
        // node from accounting for its quota.
        let (coordinator, _clock) = serving();
        let poisoner = Arc::clone(&coordinator);
        std::thread::spawn(move || {
            poisoner.with(|_| panic!("while holding the lock"));
        })
        .join()
        .expect_err("the thread should have panicked");

        assert_eq!(coordinator.membership().members().len(), 3);
        assert_eq!(coordinator.digest().node, node(1));
    }

    #[tokio::test]
    async fn the_trait_object_behaves_the_same_as_the_concrete_type() {
        // The rest of the proxy sees only the trait, so this is the shape that
        // actually gets exercised in M6.
        let (coordinator, _clock) = serving();
        let dynamic: Arc<dyn ClusterCoordinator> = coordinator;
        assert!(dynamic.membership().is_leader());
        assert!(dynamic.request_quota(&server(), 10).await.is_ok());
        assert_eq!(dynamic.digest().node, node(1));
    }
}
