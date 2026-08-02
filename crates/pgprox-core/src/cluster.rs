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

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::watch;

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
///
/// Not `#[non_exhaustive]`: `pgprox-cluster` builds these from gossip, and the
/// attribute would make them unconstructable outside this crate. It belongs on
/// enums describing external state, not on a DTO a caller assembles. Same
/// correction as `Grant` and `PoolHints` in M0.
#[derive(Clone, PartialEq, Eq, Debug)]
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
///
/// Not `#[non_exhaustive]`: `pgprox-cluster` builds these from gossip. This is
/// the third DTO here to have carried that attribute wrongly, after `Grant` in
/// M0 and `Member` in M3, so `scripts/check-layering.sh` now rejects it on any
/// struct whose fields are all public.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ClusterDigest {
    /// Which node this describes.
    pub node: NodeId,
    /// Whether it is taking work.
    pub mode: NodeMode,
    /// Client connections it is serving.
    pub client_conns: u32,
    /// Upstream connections it holds, per server.
    pub upstream_conns: Vec<(ServerId, u32)>,
    /// Upstream connections it holds per tenant, for the tenants it homes.
    ///
    /// Only homed tenants, which is what bounds this. A node homes roughly
    /// `tenants / nodes` of the fleet by rendezvous hashing, and every other
    /// node needs exactly this to decide whether the home node is using the
    /// share it reserved. Gossiping every tenant a node touches would put 5,000
    /// entries in a message sent once a second.
    ///
    /// This is what feeds `Reservations::observe` and the `home_has_headroom`
    /// input to a shed decision. Without it both are correct functions with no
    /// source of truth.
    pub tenant_usage: Vec<(TenantId, u32)>,
}

/// Why a quota request failed.
///
/// `PartialEq` so a test can assert which failure happened. Every other error
/// type here has it, and its absence made `pgprox-cluster` compare strings.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
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

/// The peers a node gossips with, by node id, as `host:port`.
///
/// Never includes the local node. A node gossiping with itself is a peer that
/// can never be down, which would make quorum unfalsifiable.
pub type PeerTable = BTreeMap<NodeId, String>;

/// Where a node learns which peers to gossip with.
///
/// Discovery, and deliberately not liveness. A source may cause this node to
/// gossip with more peers, or to treat one as draining sooner than gossip
/// would. **It may never cause a node to be counted alive that gossip has not
/// heard from.**
///
/// That rule is the whole reason this is a separate thing from membership.
/// `pgprox_cluster::membership` counts a peer alive from digests that
/// *arrived*, which is what makes a one-way network failure safe: a node that
/// can still send but no longer receives ages its peers out and steps down. A
/// source backed by an external service is a third party, and a node
/// partitioned from its peers but still able to reach that service would be
/// told the fleet is healthy while the other side elected a replacement. That
/// is the two-leaders case ADR 0004's majority rule exists to prevent, and this
/// crate's invariant is that partitions cause under-subscription and never the
/// reverse.
///
/// Getting discovery wrong costs a failed dial, which the failure detector
/// already handles. Getting liveness wrong costs the one property with no
/// graceful degradation.
///
/// Shaped like [`crate::config::ConfigSource`] on purpose. Both answer "a thing
/// that changes while a node runs", and a second mechanism for that would be a
/// second set of mistakes.
///
/// See ADR 0004 and ADR 0023.
#[async_trait::async_trait]
pub trait PeerSource: Send + Sync + fmt::Debug {
    /// The peers this node should gossip with.
    fn peers(&self) -> Arc<PeerTable>;

    /// Observes changes. The receiver always holds the latest table.
    fn watch(&self) -> watch::Receiver<Arc<PeerTable>>;

    /// Whether the last attempt to read the peer table succeeded.
    ///
    /// Defaulted to true, because a source with no loop cannot fail between
    /// reads. A source that can go stale overrides it, for the reason
    /// [`crate::config::ConfigSource::is_healthy`] exists: a node gossiping
    /// with a table from twenty minutes ago looks exactly like one gossiping
    /// with the current table.
    fn is_healthy(&self) -> bool {
        true
    }

    /// Runs whatever loop this source needs to notice a change, until dropped.
    ///
    /// Defaulted to never returning, because the static source has no loop. It
    /// exists so the composition root can start the loop without knowing which
    /// source it holds.
    async fn run_loop(self: Arc<Self>) {
        std::future::pending::<()>().await;
    }
}

#[async_trait::async_trait]
impl<T: PeerSource + ?Sized> PeerSource for Arc<T> {
    // Forwarded rather than defaulted, and this is the one method where that
    // matters. An `Arc` around a source that can go stale can go stale, and
    // taking the default would report every wrapped source as healthy forever.
    // `M14.34` found both mutants of the identical `ConfigSource` method
    // surviving, which is why the test for this asserts the false case.
    fn is_healthy(&self) -> bool {
        (**self).is_healthy()
    }

    fn peers(&self) -> Arc<PeerTable> {
        (**self).peers()
    }

    fn watch(&self) -> watch::Receiver<Arc<PeerTable>> {
        (**self).watch()
    }
}

/// A fixed peer table, which is what `--peer` flags produce.
///
/// The default, and the behaviour the fleet has today. It has no loop, so it
/// takes both defaults above.
#[derive(Debug)]
pub struct StaticPeers {
    tx: watch::Sender<Arc<PeerTable>>,
}

impl StaticPeers {
    /// A source serving this table, forever.
    #[must_use]
    pub fn new(peers: PeerTable) -> Arc<Self> {
        let (tx, _) = watch::channel(Arc::new(peers));
        Arc::new(Self { tx })
    }
}

#[async_trait::async_trait]
impl PeerSource for StaticPeers {
    fn peers(&self) -> Arc<PeerTable> {
        Arc::clone(&self.tx.borrow())
    }

    fn watch(&self) -> watch::Receiver<Arc<PeerTable>> {
        self.tx.subscribe()
    }
}

#[cfg(any(test, feature = "test-fakes"))]
pub use fake::{FakeClusterCoordinator, FakePeerSource};

#[cfg(any(test, feature = "test-fakes"))]
mod fake {
    use std::collections::HashMap;
    use std::sync::{Mutex, PoisonError};
    use std::time::Duration;

    use std::sync::atomic::{AtomicBool, Ordering};

    use super::{
        Arc, ClusterCoordinator, ClusterDigest, MembershipView, NodeMode, QuotaError, QuotaLease,
        ServerId,
    };
    use super::{PeerSource, PeerTable, watch};
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
                tenant_usage: Vec::new(),
            }
        }
    }

    /// An in-memory [`PeerSource`] for tests.
    ///
    /// It can publish and it can go stale, and both matter. The whole point of
    /// the seam is a table that changes, so a fake that could not change would
    /// test only the case the static source already covers; and `is_healthy`
    /// needs a driver for its false branch, because that is the branch whose
    /// mutants survived on `ConfigSource` until `M14.34`.
    #[derive(Debug)]
    pub struct FakePeerSource {
        tx: watch::Sender<Arc<PeerTable>>,
        healthy: AtomicBool,
    }

    impl FakePeerSource {
        /// A source serving `initial`.
        #[must_use]
        pub fn new(initial: PeerTable) -> Arc<Self> {
            let (tx, _) = watch::channel(Arc::new(initial));
            Arc::new(Self {
                tx,
                healthy: AtomicBool::new(true),
            })
        }

        /// Publishes a new table to every watcher.
        pub fn publish(&self, next: PeerTable) {
            self.tx.send_replace(Arc::new(next));
        }

        /// Makes [`PeerSource::is_healthy`] report false from now on.
        pub fn go_stale(&self) {
            self.healthy.store(false, Ordering::SeqCst);
        }
    }

    #[async_trait::async_trait]
    impl PeerSource for FakePeerSource {
        fn peers(&self) -> Arc<PeerTable> {
            Arc::clone(&self.tx.borrow())
        }

        fn watch(&self) -> watch::Receiver<Arc<PeerTable>> {
            self.tx.subscribe()
        }

        fn is_healthy(&self) -> bool {
            self.healthy.load(Ordering::SeqCst)
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    fn table(ids: &[u16]) -> PeerTable {
        ids.iter()
            .map(|id| (NodeId::new(*id), format!("pgprox-{id}:6431")))
            .collect()
    }

    #[test]
    fn a_static_source_serves_what_it_was_built_with() {
        let source = StaticPeers::new(table(&[2, 3]));

        assert_eq!(source.peers().len(), 2);
        assert_eq!(
            source.peers().get(&NodeId::new(2)).map(String::as_str),
            Some("pgprox-2:6431")
        );
        // And the watch holds the same table, so a consumer that subscribes
        // rather than asks sees the same fleet.
        assert_eq!(*source.watch().borrow(), source.peers());
    }

    #[tokio::test]
    async fn a_published_table_reaches_a_receiver_taken_before_it() {
        // The whole point of the seam. A receiver taken at startup has to see a
        // table published later, or every consumer is back to a copy.
        let source = FakePeerSource::new(table(&[2]));
        let mut watcher = source.watch();

        source.publish(table(&[2, 3, 4]));

        assert!(watcher.changed().await.is_ok());
        assert_eq!(watcher.borrow_and_update().len(), 3);
        assert_eq!(source.peers().len(), 3);
    }

    #[test]
    fn a_source_that_has_gone_stale_says_so_through_an_arc() {
        // `M14.34` found both mutants of the identical `ConfigSource` method
        // surviving: `is_healthy` could be replaced by `true` *and* by `false`.
        // A method whose mutants both survive is a method nothing asks. The
        // false case is the one that matters, and it has to survive being
        // wrapped: an `Arc` around a source that can go stale can go stale, and
        // a forwarding impl that took the default would report every wrapped
        // source healthy forever.
        let source = FakePeerSource::new(table(&[2]));
        assert!(source.is_healthy());

        let wrapped: Arc<dyn PeerSource> = source.clone();
        assert!(wrapped.is_healthy());

        source.go_stale();
        assert!(!source.is_healthy());
        assert!(
            !wrapped.is_healthy(),
            "an Arc reported a stale source as healthy"
        );
    }

    #[test]
    fn a_static_source_is_healthy_and_has_no_loop_to_make_it_otherwise() {
        // It takes both defaults, and that is the argument for the defaults
        // existing: a source read from flags cannot fail between reads.
        let source = StaticPeers::new(table(&[2]));
        assert!(source.is_healthy());
    }

    #[test]
    fn the_default_loop_never_returns() {
        // The composition root starts this without knowing which source it
        // holds, so a default that returned immediately would look like a
        // source that had finished discovering and would never be restarted.
        //
        // Polled by hand rather than with a timeout, for the reason
        // `config.rs`'s identical test gives: this crate depends on tokio only
        // for `sync`, and pulling in the time driver to assert that a future is
        // pending would be a dependency added for one test.
        let mut loop_future = Box::pin(PeerSource::run_loop(StaticPeers::new(table(&[2]))));
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(
            std::future::Future::poll(loop_future.as_mut(), &mut cx).is_pending(),
            "the default run_loop completed; the composition root would treat that as the loop having ended"
        );
    }

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

    /// `M14.32`. Nine of the ten mutants in this file are in `stable_hash`'s
    /// `SplitMix64` finalizer: every `^` could become `&` or `|`, and every `>>`
    /// could become `<<`, with nothing noticing.
    ///
    /// The function's own doc comment is the reason that matters.
    /// `DefaultHasher` was rejected here because it is explicitly not stable
    /// across Rust releases, and two nodes on different compiler versions would
    /// then disagree about which node owns a tenant. Stability of the *value*
    /// is the contract, and the only thing that pins a value is the value.
    ///
    /// So: a golden vector, for the same reason `pgprox-auth` uses published
    /// vectors for SCRAM and `M14.15` used one for the simulator's generator.
    /// Properties like "different inputs differ" hold for almost any mixing
    /// function, including every mutant here.
    #[test]
    fn the_stable_hash_produces_its_documented_values() {
        assert_eq!(stable_hash(b"", 0), 0xc381_7c01_6ba4_ff30);
        assert_eq!(stable_hash(b"", 1), 0xadd5_9ec7_95ad_7f61);
        assert_eq!(stable_hash(b"tenant-acme", 0), 0x7f73_da9c_38d8_5a7f);
        assert_eq!(stable_hash(b"tenant-acme", 1), 0x8063_8787_7db3_ae63);
        assert_eq!(stable_hash(b"tenant-acme", 7), 0xbe1e_2cf2_b1a4_4e56);
        assert_eq!(stable_hash(b"a", 0), 0x5f29_c2aa_dd9b_8527);
        assert_eq!(stable_hash(b"b", 0), 0x56f6_a47e_3092_3664);
    }

    #[test]
    fn a_node_knows_whether_it_is_a_tenants_home() {
        // `is_home_for` could return `false` unconditionally. It is how a node
        // decides whether it owns a tenant, which drives reservations and
        // shedding, and every existing test asked `home_node` directly instead.
        let members = [1_u16, 2, 3];
        let tenant = (0..1_000)
            .map(|i| TenantId::new(format!("tenant-{i}")))
            .find(|t| view(1, &members).home_node(t) == Some(NodeId::new(2)))
            .unwrap();

        // Seen from the node that owns it, and from one that does not.
        assert!(view(2, &members).is_home_for(&tenant));
        assert!(!view(1, &members).is_home_for(&tenant));
        assert!(!view(3, &members).is_home_for(&tenant));

        // And it agrees with `home_node`, which is the invariant that makes it
        // safe for callers to use either.
        for local in members {
            let v = view(local, &members);
            assert_eq!(
                v.is_home_for(&tenant),
                v.home_node(&tenant) == Some(NodeId::new(local))
            );
        }
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
