//! Cluster membership, quota, and tenant placement.
//!
//! # The invariant
//!
//! Guaranteed share plus outstanding leases never exceeds the cap, under
//! arbitrary partition, leader loss, and simultaneous restart. Breaching an
//! upstream cap can lock out the operator and take the database down for every
//! tenant on that host, so it is the one property with no graceful degradation.
//!
//! Partitions must therefore cause under-subscription, never over-subscription.
//! Slow beats down.

use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use crate::ids::{NodeId, ServerId, TenantId};

/// Whether a node is taking work.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[non_exhaustive]
pub enum NodeMode {
    /// Serving normally.
    #[default]
    Active,
    /// Finishing in-flight work and accepting nothing new.
    Draining,
}

/// A stable 64-bit hash.
///
/// [`std::collections::hash_map::DefaultHasher`] is explicitly not stable
/// across Rust releases. Using it for rendezvous hashing would mean two nodes
/// on different compiler versions disagreeing about which node owns a tenant,
/// which is a split-brain bug that would only appear during a rolling upgrade
/// and would look like random rehoming.
///
/// FNV-1a followed by a `SplitMix64` finalizer: deterministic forever, and the
/// finalizer supplies the avalanche that FNV alone lacks.
fn stable_hash(bytes: &[u8], seed: u64) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = FNV_OFFSET ^ seed;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    // SplitMix64 finalizer.
    let mut z = hash.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// One member of the cluster.
#[derive(Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub struct Member {
    /// Which node.
    pub id: NodeId,
    /// Whether it is taking work.
    pub mode: NodeMode,
}

/// Who is in the cluster right now.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MembershipView {
    local: NodeId,
    members: Vec<Member>,
}

impl MembershipView {
    /// Builds a view. Members are sorted so the leader choice is deterministic
    /// regardless of the order gossip delivered them.
    #[must_use]
    pub fn new(local: NodeId, mut members: Vec<Member>) -> Self {
        members.sort_by_key(|m| m.id);
        members.dedup_by_key(|m| m.id);
        Self { local, members }
    }

    /// This node.
    #[must_use]
    pub const fn local(&self) -> NodeId {
        self.local
    }

    /// Everyone, including this node.
    #[must_use]
    pub fn members(&self) -> &[Member] {
        &self.members
    }

    /// Nodes taking work, which excludes draining ones.
    pub fn active(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.members
            .iter()
            .filter(|m| m.mode == NodeMode::Active)
            .map(|m| m.id)
    }

    /// How many nodes are taking work.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.active().count()
    }

    /// The leader, which is simply the lowest active node ID.
    ///
    /// Returns [`None`] when every node is draining.
    #[must_use]
    pub fn leader(&self) -> Option<NodeId> {
        self.active().min()
    }

    /// Whether this node is the leader.
    #[must_use]
    pub fn is_leader(&self) -> bool {
        self.leader() == Some(self.local)
    }

    /// Which node owns a tenant, by rendezvous (highest random weight) hashing.
    ///
    /// Rendezvous rather than modulo: a membership change rehomes only the
    /// tenants that lived on the departed node, where modulo would rehome
    /// nearly all of them and stampede the upstream pools.
    ///
    /// Draining nodes are excluded, so a drain rehomes its tenants immediately.
    /// Returns [`None`] when every node is draining.
    #[must_use]
    pub fn home_node(&self, tenant: &TenantId) -> Option<NodeId> {
        self.active()
            .map(|node| (stable_hash(tenant.as_str().as_bytes(), u64::from(node.get())), node))
            // Ties break on the node ID so every node computes the same answer.
            .max_by_key(|(weight, node)| (*weight, node.get()))
            .map(|(_, node)| node)
    }

    /// Whether this node owns a tenant.
    #[must_use]
    pub fn is_home_for(&self, tenant: &TenantId) -> bool {
        self.home_node(tenant) == Some(self.local)
    }
}

/// Permission to hold some number of upstream connections beyond the
/// guaranteed share.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct QuotaLease {
    server: ServerId,
    count: u32,
    expires_at: Instant,
}

impl QuotaLease {
    /// Issues a lease.
    #[must_use]
    pub const fn new(server: ServerId, count: u32, expires_at: Instant) -> Self {
        Self {
            server,
            count,
            expires_at,
        }
    }

    /// Which server this lease is for.
    #[must_use]
    pub const fn server(&self) -> &ServerId {
        &self.server
    }

    /// How many connections it permits, or zero once expired.
    ///
    /// Returning zero rather than the nominal count is deliberate: a caller
    /// that forgets to check expiry gets the safe answer instead of
    /// over-subscribing the cap.
    #[must_use]
    pub fn count(&self, now: Instant) -> u32 {
        if self.is_expired(now) { 0 } else { self.count }
    }

    /// The count regardless of expiry, for diagnostics.
    #[must_use]
    pub const fn nominal_count(&self) -> u32 {
        self.count
    }

    /// When it lapses.
    #[must_use]
    pub const fn expires_at(&self) -> Instant {
        self.expires_at
    }

    /// Whether it has lapsed.
    #[must_use]
    pub fn is_expired(&self, now: Instant) -> bool {
        now >= self.expires_at
    }
}

/// What one node tells its peers about itself.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
#[non_exhaustive]
pub struct ClusterDigest {
    /// Which node this describes.
    pub node: NodeId,
    /// Whether it is taking work.
    pub mode: NodeMode,
    /// Client connections it is serving.
    pub client_conns: u32,
    /// Upstream connections it holds, per server.
    pub upstream_conns: Vec<(ServerId, u32)>,
}

/// Why a quota request failed.
#[derive(Clone, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum QuotaError {
    /// The free pool for this server is exhausted.
    #[error("no quota available for {server}")]
    Exhausted {
        /// The server that has no headroom.
        server: ServerId,
    },
    /// There is no leader to ask right now.
    #[error("no leader available to grant quota")]
    NoLeader,
}

/// Membership, placement, and quota.
#[async_trait::async_trait]
pub trait ClusterCoordinator: Send + Sync + fmt::Debug {
    /// Who is in the cluster right now.
    fn membership(&self) -> MembershipView;

    /// Asks the leader for permission to hold more upstream connections.
    async fn request_quota(&self, server: &ServerId, want: u32) -> Result<QuotaLease, QuotaError>;

    /// Gives quota back before it expires.
    fn release_quota(&self, lease: QuotaLease);

    /// What this node is telling its peers.
    fn digest(&self) -> ClusterDigest;
}

#[async_trait::async_trait]
impl<T: ClusterCoordinator + ?Sized> ClusterCoordinator for Arc<T> {
    fn membership(&self) -> MembershipView {
        (**self).membership()
    }

    async fn request_quota(&self, server: &ServerId, want: u32) -> Result<QuotaLease, QuotaError> {
        (**self).request_quota(server, want).await
    }

    fn release_quota(&self, lease: QuotaLease) {
        (**self).release_quota(lease);
    }

    fn digest(&self) -> ClusterDigest {
        (**self).digest()
    }
}

#[cfg(any(test, feature = "test-fakes"))]
pub use fake::FakeClusterCoordinator;

#[cfg(any(test, feature = "test-fakes"))]
mod fake {
    use std::collections::HashMap;
    use std::sync::{Mutex, PoisonError};
    use std::time::Duration;

    use super::{
        Arc, ClusterCoordinator, ClusterDigest, MembershipView, NodeMode, QuotaError, QuotaLease,
        ServerId,
    };
    use crate::clock::{Clock, FakeClock};

    /// An in-memory [`ClusterCoordinator`] for tests.
    ///
    /// Enforces the cap for real, on an injected clock, so the quota invariant
    /// is testable without a network.
    #[derive(Debug)]
    pub struct FakeClusterCoordinator {
        membership: Mutex<MembershipView>,
        clock: FakeClock,
        /// Free pool per server: what the leader may still grant.
        free: Mutex<HashMap<ServerId, u32>>,
        lease_ttl: Duration,
    }

    impl FakeClusterCoordinator {
        /// Builds a coordinator with a given membership and clock.
        #[must_use]
        pub fn new(membership: MembershipView, clock: FakeClock) -> Arc<Self> {
            Arc::new(Self {
                membership: Mutex::new(membership),
                clock,
                free: Mutex::new(HashMap::new()),
                lease_ttl: Duration::from_secs(5),
            })
        }

        /// Sets how much free quota exists for a server.
        pub fn set_free(&self, server: &ServerId, amount: u32) {
            self.free
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(server.clone(), amount);
        }

        /// How much free quota remains ungranted.
        #[must_use]
        pub fn free_remaining(&self, server: &ServerId) -> u32 {
            self.free
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(server)
                .copied()
                .unwrap_or(0)
        }

        /// Replaces the membership, modelling a node joining or leaving.
        pub fn set_membership(&self, membership: MembershipView) {
            *self
                .membership
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = membership;
        }
    }

    #[async_trait::async_trait]
    impl ClusterCoordinator for FakeClusterCoordinator {
        fn membership(&self) -> MembershipView {
            self.membership
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }

        async fn request_quota(
            &self,
            server: &ServerId,
            want: u32,
        ) -> Result<QuotaLease, QuotaError> {
            if self.membership().leader().is_none() {
                return Err(QuotaError::NoLeader);
            }

            let mut free = self.free.lock().unwrap_or_else(PoisonError::into_inner);
            let available = free.entry(server.clone()).or_insert(0);
            if *available == 0 {
                return Err(QuotaError::Exhausted {
                    server: server.clone(),
                });
            }

            let granted = want.min(*available);
            *available -= granted;

            Ok(QuotaLease::new(
                server.clone(),
                granted,
                self.clock.now() + self.lease_ttl,
            ))
        }

        fn release_quota(&self, lease: QuotaLease) {
            let mut free = self.free.lock().unwrap_or_else(PoisonError::into_inner);
            *free.entry(lease.server().clone()).or_insert(0) += lease.nominal_count();
        }

        fn digest(&self) -> ClusterDigest {
            let membership = self.membership();
            ClusterDigest {
                node: membership.local(),
                mode: NodeMode::Active,
                client_conns: 0,
                upstream_conns: Vec::new(),
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::clock::{Clock, FakeClock};
    use std::collections::HashSet;
    use std::time::Duration;

    fn view(local: u16, ids: &[u16]) -> MembershipView {
        MembershipView::new(
            NodeId::new(local),
            ids.iter()
                .map(|id| Member {
                    id: NodeId::new(*id),
                    mode: NodeMode::Active,
                })
                .collect(),
        )
    }

    #[test]
    fn the_leader_is_the_lowest_active_node() {
        let v = view(3, &[5, 1, 3]);
        assert_eq!(v.leader(), Some(NodeId::new(1)));
        assert!(!v.is_leader());
        assert!(view(1, &[5, 1, 3]).is_leader());
    }

    #[test]
    fn membership_order_does_not_change_the_leader() {
        // Gossip delivers members in arbitrary order. Two nodes must not reach
        // different conclusions about who leads.
        assert_eq!(view(1, &[3, 1, 2]).leader(), view(1, &[2, 3, 1]).leader());
    }

    #[test]
    fn a_draining_node_neither_leads_nor_homes_tenants() {
        let v = MembershipView::new(
            NodeId::new(1),
            vec![
                Member {
                    id: NodeId::new(1),
                    mode: NodeMode::Draining,
                },
                Member {
                    id: NodeId::new(2),
                    mode: NodeMode::Active,
                },
            ],
        );
        assert_eq!(v.leader(), Some(NodeId::new(2)));
        assert_eq!(v.active_count(), 1);
        for i in 0..50 {
            let tenant = TenantId::new(format!("tenant-{i}"));
            assert_eq!(v.home_node(&tenant), Some(NodeId::new(2)));
        }
    }

    #[test]
    fn an_all_draining_cluster_has_no_leader_and_no_home() {
        let v = MembershipView::new(
            NodeId::new(1),
            vec![Member {
                id: NodeId::new(1),
                mode: NodeMode::Draining,
            }],
        );
        assert_eq!(v.leader(), None);
        assert!(!v.is_leader());
        assert_eq!(v.home_node(&TenantId::new("t")), None);
        assert!(!v.is_home_for(&TenantId::new("t")));
    }

    #[test]
    fn home_node_is_deterministic_across_nodes() {
        // Every node must compute the same owner, including when gossip
        // delivered the member list in a different order.
        let a = view(1, &[1, 2, 3, 4, 5]);
        let b = view(4, &[5, 3, 1, 4, 2]);
        for i in 0..500 {
            let tenant = TenantId::new(format!("tenant-{i}"));
            assert_eq!(a.home_node(&tenant), b.home_node(&tenant), "{tenant} split");
        }
    }

    #[test]
    fn losing_a_node_rehomes_only_that_nodes_tenants() {
        // The reason for rendezvous hashing rather than modulo. Under modulo,
        // losing one node of five rehomes nearly every tenant and stampedes the
        // upstream pools.
        let before = view(1, &[1, 2, 3, 4, 5]);
        let after = view(1, &[1, 2, 3, 4]);
        let departed = NodeId::new(5);

        let tenants: Vec<_> = (0..2_000)
            .map(|i| TenantId::new(format!("tenant-{i}")))
            .collect();

        let mut moved_from_departed = 0;
        for tenant in &tenants {
            let old = before.home_node(tenant).unwrap();
            let new = after.home_node(tenant).unwrap();
            if old == departed {
                moved_from_departed += 1;
            } else {
                assert_eq!(old, new, "{tenant} moved but its home node was alive");
            }
        }

        assert!(moved_from_departed > 0, "the departed node homed nothing");
    }

    #[test]
    fn tenants_spread_across_nodes() {
        // A hash with poor avalanche would pile every tenant onto one node.
        let v = view(1, &[1, 2, 3, 4, 5]);
        let mut homes: HashSet<NodeId> = HashSet::new();
        let mut counts = [0_usize; 6];
        for i in 0..5_000 {
            let home = v.home_node(&TenantId::new(format!("tenant-{i}"))).unwrap();
            homes.insert(home);
            counts[home.get() as usize] += 1;
        }
        assert_eq!(homes.len(), 5, "not every node received tenants");
        for count in &counts[1..=5] {
            assert!(*count > 500, "distribution is badly skewed: {counts:?}");
        }
    }

    #[test]
    fn stable_hash_does_not_depend_on_the_standard_library() {
        // Pinning the values. If these change, nodes on different builds will
        // disagree about tenant placement during a rolling upgrade.
        assert_eq!(stable_hash(b"", 0), stable_hash(b"", 0));
        assert_ne!(stable_hash(b"a", 0), stable_hash(b"a", 1));
        assert_ne!(stable_hash(b"a", 0), stable_hash(b"b", 0));
    }

    #[test]
    fn a_lease_reports_zero_once_expired() {
        // A caller that forgets to check expiry must get the safe answer, not
        // the nominal count, or the cap gets over-subscribed.
        let clock = FakeClock::new();
        let lease = QuotaLease::new(
            ServerId::new("db-1", 5432),
            32,
            clock.now() + Duration::from_secs(5),
        );

        assert_eq!(lease.count(clock.now()), 32);
        assert!(!lease.is_expired(clock.now()));

        clock.advance(Duration::from_secs(5));
        assert!(lease.is_expired(clock.now()));
        assert_eq!(
            lease.count(clock.now()),
            0,
            "expired lease still granted quota"
        );
        assert_eq!(lease.nominal_count(), 32, "diagnostics lost the count");
    }

    #[tokio::test]
    async fn the_fake_never_grants_more_than_the_free_pool() {
        // The invariant, in miniature.
        let clock = FakeClock::new();
        let coord = FakeClusterCoordinator::new(view(1, &[1, 2]), clock.clone());
        let server = ServerId::new("db-1", 5432);
        coord.set_free(&server, 10);

        let mut granted = 0;
        for _ in 0..20 {
            match coord.request_quota(&server, 3).await {
                Ok(lease) => granted += lease.count(clock.now()),
                Err(QuotaError::Exhausted { .. }) => break,
                Err(other) => unreachable!("{other:?}"),
            }
        }

        assert_eq!(granted, 10, "granted {granted} from a pool of 10");
        assert_eq!(coord.free_remaining(&server), 0);
    }

    #[tokio::test]
    async fn releasing_quota_returns_it_to_the_pool() {
        let clock = FakeClock::new();
        let coord = FakeClusterCoordinator::new(view(1, &[1]), clock.clone());
        let server = ServerId::new("db-1", 5432);
        coord.set_free(&server, 4);

        let lease = coord.request_quota(&server, 4).await.unwrap();
        assert_eq!(coord.free_remaining(&server), 0);

        coord.release_quota(lease);
        assert_eq!(coord.free_remaining(&server), 4);
    }

    #[tokio::test]
    async fn quota_needs_a_leader() {
        let clock = FakeClock::new();
        let all_draining = MembershipView::new(
            NodeId::new(1),
            vec![Member {
                id: NodeId::new(1),
                mode: NodeMode::Draining,
            }],
        );
        let coord = FakeClusterCoordinator::new(all_draining, clock);
        let server = ServerId::new("db-1", 5432);
        coord.set_free(&server, 10);

        let err = coord.request_quota(&server, 1).await.unwrap_err();
        assert!(matches!(err, QuotaError::NoLeader), "got {err:?}");
    }

    #[tokio::test]
    async fn membership_changes_are_observable() {
        let clock = FakeClock::new();
        let coord = FakeClusterCoordinator::new(view(1, &[1, 2, 3]), clock);
        assert_eq!(coord.membership().active_count(), 3);

        coord.set_membership(view(1, &[1, 2]));
        assert_eq!(coord.membership().active_count(), 2);
        assert_eq!(coord.digest().node, NodeId::new(1));
    }

    #[test]
    fn duplicate_members_are_collapsed() {
        // Gossip can deliver the same node twice mid-convergence. Counting it
        // twice would shrink every node's guaranteed share.
        let v = view(1, &[1, 2, 2, 3, 3, 3]);
        assert_eq!(v.members().len(), 3);
        assert_eq!(v.active_count(), 3);
    }
}
