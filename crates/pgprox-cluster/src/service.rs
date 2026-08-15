//! The [`ClusterCoordinator`] implementation the rest of the proxy sees.
//!
//! [`crate::coordinator::NodeCoordinator`] is sans-I/O: every method takes the
//! time it should reason about and needs `&mut self`. The trait in
//! `pgprox-core` is shared, `&self`, and async. This is the adapter between
//! them, and it holds nothing but a lock and a clock.
//!
//! # Where the peer hop is
//!
//! Behind [`QuotaTransport`]. `request_quota` answers from the local ledger
//! when this node is the serving leader, and forwards to whoever is otherwise.
//! The socket that carries it stays outside this crate, because
//! `pgprox-cluster` needs no socket to be tested and the quota invariant is a
//! property of the rules rather than of a message-passing layer.
//!
//! A node with no transport, or one that cannot see a leader, gets
//! [`QuotaError::NoLeader`] and falls back to its guaranteed share. That is the
//! safe direction: the guaranteed share needs no coordination and cannot
//! breach the cap.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use pgprox_core::clock::Clock;
use pgprox_core::cluster::{
    ClusterCoordinator, ClusterDigest, MembershipView, NodeMode, QuotaError, QuotaLease,
};
use pgprox_core::ids::{NodeId, ServerId, TenantId};

use crate::coordinator::{CoordinatorConfig, NodeCoordinator, ServerQuota};
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

/// Carries a quota request to the leader.
///
/// Implemented over the gossip transport by the composition root. A trait
/// here, rather than a socket, because every rule this crate owns has to be
/// testable without one.
#[async_trait::async_trait]
pub trait QuotaTransport: Send + Sync + fmt::Debug {
    /// Asks `leader` for a lease on `holder`'s behalf.
    ///
    /// # Errors
    ///
    /// Fails when the leader refuses, or when it cannot be reached, which the
    /// caller treats identically: both mean falling back to the guaranteed
    /// share.
    async fn request(
        &self,
        leader: NodeId,
        server: &ServerId,
        holder: NodeId,
        want: u32,
    ) -> Result<QuotaLease, QuotaError>;
}

/// A [`ClusterCoordinator`] over a gossiping [`NodeCoordinator`].
#[derive(Debug)]
pub struct GossipCoordinator {
    inner: Mutex<NodeCoordinator>,
    clock: Arc<dyn Clock>,
    local: NodeId,
    /// Set once, after construction.
    ///
    /// The transport needs a coordinator to answer requests it receives, and
    /// the coordinator needs a transport to send them, so one of the two has to
    /// be filled in afterwards. A `OnceLock` makes that a single assignment
    /// rather than a mutable field anything could reassign later.
    transport: OnceLock<Arc<dyn QuotaTransport>>,
    /// Orders this node's own digests against each other.
    ///
    /// Per node rather than shared, which is what lets a fleet gossip with no
    /// common clock: a peer compares versions only against the ones it already
    /// holds from the same node.
    ///
    /// Seeded from wall time at construction rather than from zero. A process
    /// restarting inside `dead_after` (10s by default) is not reaped from a
    /// peer's `DigestStore` first, so its first post-restart digest is
    /// compared against the version its previous incarnation left behind
    /// there — and `merge` treats a lower version as stale, permanently,
    /// with no notion that a lower version could mean a fresher process
    /// rather than a reordered message. Two counters starting at zero every
    /// time collide on exactly the schedule a crash loop produces. Seeding
    /// from milliseconds since the epoch instead makes each restart's first
    /// version larger than any version the same counter could have reached
    /// by counting rounds alone: even a node gossiping once a second for a
    /// full year reaches barely thirty million, four orders of magnitude
    /// below where the next millisecond-based boot begins. `M90`, cycle 5.
    version: AtomicU64,
}

impl GossipCoordinator {
    /// Wraps a coordinator for `local`.
    #[must_use]
    pub fn new(local: NodeId, config: CoordinatorConfig, clock: Arc<dyn Clock>) -> Arc<Self> {
        let version = version_floor(clock.wall());
        Arc::new(Self {
            inner: Mutex::new(NodeCoordinator::new(local, config, clock.now())),
            clock,
            local,
            transport: OnceLock::new(),
            version: AtomicU64::new(version),
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

    /// Registers a server's cap and how it splits.
    pub fn set_cap(&self, server: ServerId, quota: ServerQuota) {
        self.with(|c| c.set_cap(server, quota));
    }

    /// What this node would tell a peer right now.
    ///
    /// Each call takes the next version, so two calls produce two messages a
    /// peer can order. The counter is this node's own and is never compared
    /// against another node's, which is what removes the need for a shared
    /// clock. See [`DigestStore::merge`](crate::digest::DigestStore::merge).
    ///
    /// A node's own digest deliberately does not enter its own store: the store
    /// is what peers said, and mixing the two would make a node's report of
    /// itself indistinguishable from a peer's report of it.
    pub fn outgoing(&self) -> VersionedDigest {
        VersionedDigest {
            digest: self.with(|c| c.digest()),
            version: self.version.fetch_add(1, Ordering::Relaxed),
        }
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
        self.with(|c| {
            // This node's own liveness, before anything reads the view. A node
            // that stops ticking ages out of its own membership, which is what
            // makes a wedged node stop leading rather than lead forever.
            c.heartbeat(now);
            c.observe(now);
        });
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

    /// Supplies the transport that carries quota requests to the leader.
    ///
    /// Returns whether it was set, which is false if one already was. A second
    /// transport would mean two paths to the leader and no way to say which
    /// one a lease came from.
    pub fn set_transport(&self, transport: Arc<dyn QuotaTransport>) -> bool {
        self.transport.set(transport).is_ok()
    }

    /// Serves a quota request that arrived from a peer.
    ///
    /// The leader end of [`QuotaTransport`]. Grants against this node's ledger
    /// and attributes the lease to `holder`, so the capacity is accounted to
    /// the node that will actually open the connections.
    ///
    /// # Errors
    ///
    /// Fails when this node is not a serving leader, or the free pool is
    /// exhausted.
    pub fn serve_request(
        &self,
        server: &ServerId,
        holder: NodeId,
        want: u32,
    ) -> Result<QuotaLease, QuotaError> {
        let now = self.clock.now();
        // Deliberately no `accept`: this node is granting, not holding.
        // Accepting here would count the lease twice, once on the leader and
        // once on the node that asked, and the second count is the real one.
        self.with(|c| c.request(server, holder, want, now))
    }

    /// Every node's last digest, this one included.
    ///
    /// What lets any pod answer a cluster-wide question locally, which is the
    /// property ADR 0007 is about: an operator asking a question should not
    /// have to know which pod to ask.
    #[must_use]
    pub fn digests(&self) -> Vec<ClusterDigest> {
        self.with(|c| c.digests().all())
    }

    /// The hash of the current membership view.
    ///
    /// Two pods reporting different hashes is split brain, stated directly
    /// rather than inferred from two lists that look similar.
    #[must_use]
    pub fn view_hash(&self) -> u64 {
        self.with(|c| c.digests().view_hash())
    }

    /// Upstream connections every known node reports holding for a server.
    #[must_use]
    pub fn cluster_usage(&self, server: &ServerId) -> u32 {
        self.with(|c| c.digests().cluster_usage(server))
    }

    /// Client connections every known node reports.
    #[must_use]
    pub fn cluster_clients(&self) -> u32 {
        self.with(|c| c.digests().cluster_clients())
    }

    /// How many tenants this node is tracking. Test-only; see `M14.12`.
    #[cfg(test)]
    fn tracked_tenant_count(&self) -> usize {
        self.with(|c| c.tracked_tenant_count())
    }

    /// What this node may open for a server right now.
    #[must_use]
    pub fn allowance(&self, server: &ServerId) -> NodeAllowance {
        let now = self.clock.now();
        self.with(|c| c.allowance(server, now))
    }
}

/// The first value the outgoing digest counter takes.
///
/// Milliseconds since the epoch, so a restarted process starts ahead of
/// anything its previous incarnation could have reached by counting gossip
/// rounds — see the doc comment on `version` for why that has to hold.
/// `wall` comes from the injected [`Clock`], never read directly, so this
/// stays deterministic under the simulation: a fixed clock reproduces a
/// fixed floor, the same as every other value this crate derives from time.
fn version_floor(wall: std::time::SystemTime) -> u64 {
    wall.duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since_epoch| {
            u64::try_from(since_epoch.as_millis()).unwrap_or(u64::MAX)
        })
}

#[async_trait::async_trait]
impl ClusterCoordinator for GossipCoordinator {
    fn membership(&self) -> MembershipView {
        let now = self.clock.now();
        self.with(|c| c.membership(now))
    }

    async fn request_quota(&self, server: &ServerId, want: u32) -> Result<QuotaLease, QuotaError> {
        let now = self.clock.now();
        // The lock is a std::sync::Mutex and is never held across the await
        // below. Every use of it here is a separate short critical section.
        let local = self.with(|c| {
            let lease = c.request(server, self.local, want, now)?;
            c.accept(lease.clone());
            Ok::<_, QuotaError>(lease)
        });

        match local {
            Ok(lease) => return Ok(lease),
            // Anything else, exhaustion in particular, is the leader's real
            // answer and forwarding it elsewhere would be asking a second time
            // for capacity that does not exist.
            Err(err) if !matches!(err, QuotaError::NoLeader) => return Err(err),
            Err(_) => {}
        }

        let leader = self.with(|c| c.membership(now).leader());
        let (Some(leader), Some(transport)) = (leader, self.transport.get()) else {
            return Err(QuotaError::NoLeader);
        };
        if leader == self.local {
            // This node is the leader by view and could not grant: it is
            // inside its takeover wait, or it has lost quorum. Asking itself
            // over a socket would get the same answer more slowly.
            return Err(QuotaError::NoLeader);
        }

        let lease = transport.request(leader, server, self.local, want).await?;
        if lease.server() != server {
            // A leader that answered about a different server would have this
            // node open connections against one cap while the lease was
            // counted against another.
            return Err(QuotaError::NoLeader);
        }

        self.with(|c| c.accept(lease.clone()));
        Ok(lease)
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

    /// A quota for a test, at the documented default fraction.
    fn test_quota(cap: u32) -> ServerQuota {
        ServerQuota {
            cap,
            guaranteed_fraction: 0.5,
        }
    }

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

    #[test]
    fn a_ticking_node_leads_and_can_grant_itself_a_lease() {
        // The seam that was broken, tested at the seam. The coordinator could
        // always heartbeat; nothing in the loop called it, so no node was in
        // its own membership view, `leader()` never named the local node on
        // any node in the fleet, no node took office, and every request for a
        // lease came back "no leader available to grant quota" for the life of
        // the deployment. Every node sat on its guaranteed share, which for
        // three nodes at a fraction of 0.5 is a sixth of the cap, while
        // clients queued behind it.
        let clock = FakeClock::new();
        let coordinator = GossipCoordinator::new(
            node(1),
            CoordinatorConfig {
                fleet_size: 1,
                ..CoordinatorConfig::default()
            },
            Arc::new(clock.clone()) as Arc<dyn pgprox_core::clock::Clock>,
        );
        coordinator.set_cap(server(), test_quota(60));

        coordinator.tick();
        assert!(
            coordinator.membership().is_leader(),
            "a node that ticked does not consider itself the leader"
        );

        // Past the takeover wait, which is what a leader observes before it
        // may grant. Nothing else moves in this test.
        clock.advance(
            CoordinatorConfig::default().effective_lease().takeover_wait + Duration::from_secs(1),
        );
        coordinator.tick();

        let lease = futures_lite_block_on(ClusterCoordinator::request_quota(
            &coordinator,
            &server(),
            5,
        ));
        assert!(
            lease.is_ok(),
            "the leader could not grant itself a lease: {lease:?}"
        );
        assert!(coordinator.allowance(&server()).leased >= 5);
    }

    /// The one await in these tests, without pulling in a runtime for it.
    fn futures_lite_block_on<F: std::future::Future>(future: F) -> F::Output {
        // A single future with no I/O and no timers in it: polling it to
        // completion on this thread is the whole of what a runtime would do.
        let mut future = Box::pin(future);
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        loop {
            if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut cx) {
                return value;
            }
        }
    }

    #[test]
    fn what_a_node_tells_a_peer_carries_what_it_reported() {
        // `report` was write-only until the transport existed: nothing could
        // read a node's own digest, so a node's contribution to the fleet's
        // totals was invisible even to itself.
        let coordinator = GossipCoordinator::new(
            node(1),
            CoordinatorConfig::default(),
            Arc::new(FakeClock::new()),
        );
        coordinator.report(7, vec![(server(), 3)]);
        coordinator.set_mode(NodeMode::Draining);

        let outgoing = coordinator.outgoing();
        assert_eq!(outgoing.digest.node, node(1));
        assert_eq!(outgoing.digest.client_conns, 7);
        assert_eq!(outgoing.digest.upstream_conns, vec![(server(), 3)]);
        assert_eq!(
            outgoing.digest.mode,
            NodeMode::Draining,
            "a draining node did not announce it"
        );
    }

    #[test]
    fn each_outgoing_digest_is_newer_than_the_last() {
        // A peer accepts a digest only if it is newer than what it holds, so a
        // version that did not advance would make every update after the first
        // one stale on arrival.
        let coordinator = GossipCoordinator::new(
            node(1),
            CoordinatorConfig::default(),
            Arc::new(FakeClock::new()),
        );

        let first = coordinator.outgoing().version;
        let second = coordinator.outgoing().version;
        assert!(second > first, "{second} did not follow {first}");
    }

    /// A [`Clock`] whose wall time is fixed by the test rather than read from
    /// the real one, so `version_floor` can be exercised at chosen instants
    /// without depending on how fast the test happens to run. `now()` still
    /// needs an answer — `NodeCoordinator::new` calls it — but nothing this
    /// clock is used for reads it, so any monotonic instant will do.
    #[derive(Debug, Clone, Copy)]
    struct WallOnly(std::time::SystemTime);

    impl Clock for WallOnly {
        fn now(&self) -> std::time::Instant {
            std::time::Instant::now()
        }

        fn wall(&self) -> std::time::SystemTime {
            self.0
        }
    }

    #[test]
    fn version_floor_reads_milliseconds_since_the_epoch() {
        let at = std::time::UNIX_EPOCH + Duration::from_millis(1_700_000_000_123);
        assert_eq!(version_floor(at), 1_700_000_000_123);
    }

    #[test]
    fn version_floor_does_not_panic_before_the_epoch() {
        // `duration_since` errs rather than going negative. The fallback of 0
        // is exactly today's unseeded behaviour, so a clock this wrong is no
        // worse off than before this fix.
        let before = std::time::UNIX_EPOCH - Duration::from_secs(1);
        assert_eq!(version_floor(before), 0);
    }

    #[test]
    fn a_process_that_restarts_inside_dead_after_is_not_rejected_as_stale() {
        // The M90 cycle-5 finding: a node killed and restarted faster than
        // `dead_after` (10s by default) is never reaped from a peer's
        // `DigestStore`, so its first digest after restart is compared
        // against whatever version its previous incarnation reached. A
        // counter that always starts at zero loses that comparison every
        // time and is rejected as `Stale` forever — the peer keeps whatever
        // stale state (client counts, a stuck `Draining` mode) it held
        // before the restart.
        let old_wall = std::time::UNIX_EPOCH + Duration::from_millis(1_000_000);
        let old = GossipCoordinator::new(
            node(1),
            CoordinatorConfig::default(),
            Arc::new(WallOnly(old_wall)),
        );

        // The old incarnation ran a few rounds, so the peer's held version is
        // ahead of a freshly-seeded 0 or 1 by more than one.
        let mut last = old.outgoing();
        for _ in 0..4 {
            last = old.outgoing();
        }

        let peer = GossipCoordinator::new(
            node(2),
            CoordinatorConfig::default(),
            Arc::new(FakeClock::new()),
        );
        assert_eq!(peer.gossip(last.clone()), MergeOutcome::Added);

        // The process restarts one second later — comfortably inside
        // `dead_after`'s default ten, so the peer never reaped node 1's
        // entry and this is exactly the comparison that has to still work.
        let new_wall = old_wall + Duration::from_secs(1);
        let restarted = GossipCoordinator::new(
            node(1),
            CoordinatorConfig::default(),
            Arc::new(WallOnly(new_wall)),
        );

        let outcome = peer.gossip(restarted.outgoing());
        assert_ne!(
            outcome,
            MergeOutcome::Stale,
            "the peer rejected the restarted node's first digest as stale"
        );
    }

    #[test]
    fn a_node_s_own_digest_does_not_enter_its_own_store() {
        // The store is what peers said. A node that merged its own report
        // could not tell its view of itself from a peer's view of it, which is
        // exactly the comparison split brain is found by.
        let coordinator = GossipCoordinator::new(
            node(1),
            CoordinatorConfig::default(),
            Arc::new(FakeClock::new()),
        );
        let _ = coordinator.outgoing();

        assert!(
            coordinator.digests().is_empty(),
            "a node gossiped to itself"
        );
    }

    /// A leading coordinator that has served its takeover wait.
    fn serving() -> (Arc<GossipCoordinator>, FakeClock) {
        let clock = FakeClock::new();
        let config = CoordinatorConfig {
            fleet_size: 3,
            ..CoordinatorConfig::default()
        };
        let coordinator = GossipCoordinator::new(node(1), config, Arc::new(clock.clone()));
        coordinator.set_cap(server(), test_quota(100));
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
        coordinator.set_cap(server(), test_quota(100));
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

    /// `M14.12`. Six mutants survived in this file and all six are the same
    /// gap: every one of these methods is a one-line delegation to the inner
    /// coordinator, the inner coordinator is thoroughly tested, and nothing
    /// went through the façade. So `track_tenant` and `forget_tenant` could
    /// become `()`, `view_hash` could return `1`, and `cluster_usage` and
    /// `cluster_clients` could return `0`, with 156 tests passing.
    ///
    /// That matters more here than the line count suggests. `/v1/servers` and
    /// `/v1/stats` are built on these three readers, and `M11.9` was a bug in
    /// exactly this accounting: a dead node's last reading stayed in the total
    /// forever. A mutant that makes them answer zero is the same class of
    /// defect, and nothing would have caught it.
    #[test]
    fn the_cluster_wide_readers_report_what_the_fleet_gossiped() {
        let (coordinator, _clock) = serving();

        // Three nodes, each holding a different number of connections, so a
        // constant answer cannot pass for the sum.
        for (n, clients, upstream) in [(1_u16, 10_u32, 3_u32), (2, 20, 5), (3, 30, 7)] {
            coordinator.gossip(VersionedDigest {
                digest: ClusterDigest {
                    node: node(n),
                    mode: NodeMode::Active,
                    client_conns: clients,
                    upstream_conns: vec![(server(), upstream)],
                    tenant_usage: Vec::new(),
                },
                version: 100 + u64::from(n),
            });
        }

        assert_eq!(coordinator.cluster_clients(), 60, "10 + 20 + 30");
        assert_eq!(coordinator.cluster_usage(&server()), 15, "3 + 5 + 7");

        // And a server nobody reports is zero rather than a sum of everything.
        assert_eq!(coordinator.cluster_usage(&ServerId::new("db-2", 5432)), 0);
    }

    #[test]
    fn the_view_hash_changes_when_the_view_does_and_not_otherwise() {
        // Two pods reporting different hashes is split brain, which is only a
        // usable signal if the hash actually depends on the view. A constant
        // makes every node agree forever, including through a partition.
        let (coordinator, _clock) = serving();

        let before = coordinator.view_hash();

        // Re-gossiping the same thing must not move it, or the hash is noise
        // and two healthy pods disagree on every round.
        coordinator.gossip(digest_for(2, 200));
        assert_eq!(coordinator.view_hash(), before);

        // A node going into drain is a different view.
        coordinator.gossip(VersionedDigest {
            digest: ClusterDigest {
                node: node(3),
                mode: NodeMode::Draining,
                client_conns: 0,
                upstream_conns: Vec::new(),
                tenant_usage: Vec::new(),
            },
            version: 300,
        });
        assert_ne!(
            coordinator.view_hash(),
            before,
            "a node entering drain did not change the view hash"
        );
    }

    #[test]
    fn tracking_a_tenant_through_the_facade_reaches_the_coordinator() {
        // `track_tenant` and `forget_tenant` are the two writers here with no
        // return value, which is why both could become `()`.
        //
        // The first version of this test asserted through `report_tenants` and
        // was wrong: that setter replaces the reported usage outright and does
        // not consult the tracked set, so forgetting a tenant does not stop it
        // being reported. What tracking decides is whether `observe` walks the
        // tenant each round and lets its reservation decay, which is several
        // rounds away from anything the façade returns. Hence the observer.
        let (coordinator, _clock) = serving();
        let acme = TenantId::new("acme");
        let globex = TenantId::new("globex");

        assert_eq!(coordinator.tracked_tenant_count(), 0);

        coordinator.track_tenant(acme.clone());
        coordinator.track_tenant(globex.clone());
        assert_eq!(coordinator.tracked_tenant_count(), 2);

        // Tracking the same tenant twice is not two tenants.
        coordinator.track_tenant(acme.clone());
        assert_eq!(coordinator.tracked_tenant_count(), 2);

        coordinator.forget_tenant(&acme);
        assert_eq!(coordinator.tracked_tenant_count(), 1);

        // Forgetting one that was never tracked is not an error and not a
        // change, so a mutant cannot pass by making forget a no-op either way.
        coordinator.forget_tenant(&TenantId::new("never-seen"));
        assert_eq!(coordinator.tracked_tenant_count(), 1);

        coordinator.forget_tenant(&globex);
        assert_eq!(coordinator.tracked_tenant_count(), 0);
    }

    #[tokio::test]
    async fn an_exhausted_pool_is_the_leaders_answer_and_is_not_asked_twice() {
        // The match guard. `!matches!(err, QuotaError::NoLeader)` returning the
        // error is what stops a local exhaustion being forwarded to the leader,
        // and the comment beside it says why: asking again is asking for
        // capacity that does not exist. Replacing the guard with `false` sends
        // every error down the forwarding path instead, and no test noticed.
        let (coordinator, _clock) = serving();

        // Another node takes everything leasable, through the leader path this
        // node is serving. It has to be another node: a holder's renewal
        // replaces its own grant rather than adding to it, so this node asking
        // twice can never exhaust anything, which is the second thing the first
        // version of this test got wrong.
        //
        // And "everything" is half the cap, not the cap. `guaranteed_fraction`
        // is 0.5, so half the 100 is held back as guaranteed shares and only
        // the rest is ever leased. That was the first thing it got wrong.
        let held = coordinator.serve_request(&server(), node(2), 100).unwrap();
        assert_eq!(
            held.nominal_count(),
            50,
            "the leasable half of a cap of 100"
        );

        // This node is the leader and has nothing left. The answer must be
        // Exhausted, not NoLeader: with the guard mutated it falls through to
        // the leader lookup and comes back with the wrong error, because there
        // is no transport configured in this fixture.
        let err = coordinator.request_quota(&server(), 10).await.unwrap_err();
        assert_eq!(
            err,
            QuotaError::Exhausted { server: server() },
            "exhaustion was forwarded rather than returned"
        );
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
    async fn a_caller_can_build_a_shed_ctx_without_inventing_a_field() {
        // The acceptance criterion for the digest change. Every cluster-side
        // field has a source; the rest is session state the caller already has.
        // If this stops compiling, an input was added to ShedCtx that nothing
        // can supply, which is the state the digest change was fixing.
        use crate::shed::{ShedConfig, ShedCtx, ShedDecision, decide};

        let (coordinator, clock) = serving();
        // A tenant this node does not home, so there is somewhere to send it.
        let view = coordinator.membership();
        let tenant = (0..1_000)
            .map(|i| TenantId::new(format!("tenant-{i}")))
            .find(|t| view.home_node(t) == Some(node(3)))
            .unwrap();

        // Keep gossiping while the settle window elapses. Letting time pass in
        // silence would age every peer out and leave the tenant with no home,
        // which is correct behaviour and the wrong setup.
        for round in 13..=90_u64 {
            for peer in 1..=3 {
                coordinator.gossip(digest_for(peer, round));
            }
            clock.advance(Duration::from_secs(1));
        }
        coordinator.tick();
        let placement = coordinator.placement(&tenant, 10);

        let ctx = ShedCtx {
            // From the cluster layer.
            on_home_node: placement.on_home_node,
            home_has_headroom: placement.home_has_headroom,
            home_draining: placement.home_draining,
            since_membership_change: placement.since_membership_change,
            // From the session, which this crate cannot and should not see.
            idle_for: Duration::from_secs(60),
            pinned: false,
            in_transaction: false,
            recent_sheds: 0,
        };

        assert!(!ctx.on_home_node);
        assert!(ctx.home_has_headroom, "an idle home reported no headroom");
        assert_eq!(
            decide(&ShedConfig::default(), &ctx),
            ShedDecision::Shed,
            "an idle client away from its home was not shed"
        );
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

    /// A follower that has seen the same gossip, and leads nothing.
    fn following(clock: &FakeClock) -> Arc<GossipCoordinator> {
        let config = CoordinatorConfig {
            fleet_size: 3,
            ..CoordinatorConfig::default()
        };
        let follower = GossipCoordinator::new(node(2), config, Arc::new(clock.clone()));
        follower.set_cap(server(), test_quota(100));
        for round in 1..=12 {
            follower.gossip(digest_for(1, round));
            follower.gossip(digest_for(2, round));
            follower.gossip(digest_for(3, round));
        }
        follower.tick();
        follower
    }

    /// A transport that hands the request straight to the leader's own
    /// coordinator.
    ///
    /// Not a mock: it calls the same entry point a real socket would land on,
    /// so what is being tested is the leader's accounting rather than a
    /// message format.
    #[derive(Debug)]
    struct DirectTransport {
        leader: Arc<GossipCoordinator>,
        seen: std::sync::atomic::AtomicU32,
    }

    #[async_trait::async_trait]
    impl QuotaTransport for DirectTransport {
        async fn request(
            &self,
            leader: NodeId,
            server: &ServerId,
            holder: NodeId,
            want: u32,
        ) -> Result<QuotaLease, QuotaError> {
            self.seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            assert_eq!(leader, node(1), "the request went to the wrong node");
            self.leader.serve_request(server, holder, want)
        }
    }

    /// A transport to a leader that has gone.
    #[derive(Debug)]
    struct Unreachable;

    #[async_trait::async_trait]
    impl QuotaTransport for Unreachable {
        async fn request(
            &self,
            _leader: NodeId,
            _server: &ServerId,
            _holder: NodeId,
            _want: u32,
        ) -> Result<QuotaLease, QuotaError> {
            Err(QuotaError::NoLeader)
        }
    }

    #[tokio::test]
    async fn a_non_leader_obtains_a_lease_through_the_leader() {
        // M3.12, deferred from M3 because the transport did not exist. Without
        // it every node but one is capped at its guaranteed share, and the free
        // pool, which is half the cap by default, is unreachable.
        let (leader, clock) = serving();
        let follower = following(&clock);
        let transport = Arc::new(DirectTransport {
            leader: Arc::clone(&leader),
            seen: std::sync::atomic::AtomicU32::new(0),
        });
        assert!(follower.set_transport(Arc::clone(&transport) as Arc<dyn QuotaTransport>));

        let lease = follower.request_quota(&server(), 5).await.unwrap();

        assert_eq!(lease.count(clock.now()), 5);
        assert_eq!(
            transport.seen.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "the follower answered locally instead of asking the leader"
        );
    }

    #[tokio::test]
    async fn a_forwarded_lease_is_counted_once_and_on_the_leader() {
        // The failure that would breach the cap: both ends recording the same
        // lease, so the fleet believes it has twice the capacity it has.
        let (leader, clock) = serving();
        let follower = following(&clock);
        follower.set_transport(Arc::new(DirectTransport {
            leader: Arc::clone(&leader),
            seen: std::sync::atomic::AtomicU32::new(0),
        }));

        follower.request_quota(&server(), 5).await.unwrap();

        assert_eq!(
            follower.allowance(&server()).leased,
            5,
            "the node that will open the connections does not hold the lease"
        );
        assert_eq!(
            leader.allowance(&server()).leased,
            0,
            "the leader counted a lease it granted to somebody else as its own"
        );
    }

    #[tokio::test]
    async fn a_leader_that_cannot_be_reached_leaves_the_guaranteed_share() {
        // The safe direction. A node that cannot get a lease still opens up to
        // its guaranteed share, which needs no coordination and cannot breach
        // the cap.
        let (_leader, clock) = serving();
        let follower = following(&clock);
        follower.set_transport(Arc::new(Unreachable));

        assert!(matches!(
            follower.request_quota(&server(), 5).await,
            Err(QuotaError::NoLeader)
        ));
        assert!(follower.allowance(&server()).guaranteed > 0);
    }

    #[tokio::test]
    async fn a_node_with_no_transport_asks_nobody() {
        let (_leader, clock) = serving();
        let follower = following(&clock);

        assert!(matches!(
            follower.request_quota(&server(), 5).await,
            Err(QuotaError::NoLeader)
        ));
    }

    #[tokio::test]
    async fn a_leader_inside_its_takeover_wait_does_not_ask_itself() {
        // It is the leader by view and cannot grant yet. Sending itself a
        // request over a socket would get the same answer, slower.
        let clock = FakeClock::new();
        let config = CoordinatorConfig {
            fleet_size: 3,
            ..CoordinatorConfig::default()
        };
        let fresh = GossipCoordinator::new(node(1), config, Arc::new(clock.clone()));
        fresh.set_cap(server(), test_quota(100));
        fresh.gossip(digest_for(1, 1));
        fresh.gossip(digest_for(2, 1));
        fresh.gossip(digest_for(3, 1));
        fresh.tick();

        let transport = Arc::new(DirectTransport {
            leader: Arc::clone(&fresh),
            seen: std::sync::atomic::AtomicU32::new(0),
        });
        fresh.set_transport(Arc::clone(&transport) as Arc<dyn QuotaTransport>);

        assert!(fresh.request_quota(&server(), 5).await.is_err());
        assert_eq!(
            transport.seen.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the leader asked itself over the transport"
        );
    }

    /// A leader that answers about a server nobody asked about.
    #[derive(Debug)]
    struct WrongServer;

    #[async_trait::async_trait]
    impl QuotaTransport for WrongServer {
        async fn request(
            &self,
            _leader: NodeId,
            _server: &ServerId,
            _holder: NodeId,
            want: u32,
        ) -> Result<QuotaLease, QuotaError> {
            Ok(QuotaLease::new(
                ServerId::new("db-9", 5432),
                want,
                std::time::Instant::now() + Duration::from_secs(5),
            ))
        }
    }

    #[tokio::test]
    async fn a_lease_for_the_wrong_server_is_refused() {
        // Accepting it would have this node open connections against one cap
        // while its lease was counted against another, which is how a cap gets
        // breached without any single ledger being wrong.
        let (_leader, clock) = serving();
        let follower = following(&clock);
        follower.set_transport(Arc::new(WrongServer));

        assert!(matches!(
            follower.request_quota(&server(), 5).await,
            Err(QuotaError::NoLeader)
        ));
    }

    #[tokio::test]
    async fn a_transport_is_set_once() {
        // Two paths to the leader would mean no way to say which one a lease
        // came from.
        let (leader, clock) = serving();
        let follower = following(&clock);

        assert!(follower.set_transport(Arc::new(Unreachable)));
        assert!(
            !follower.set_transport(Arc::new(DirectTransport {
                leader,
                seen: std::sync::atomic::AtomicU32::new(0),
            })),
            "a second transport replaced the first"
        );
    }

    #[test]
    fn the_aggregate_reads_answer_from_gossip_without_a_fan_out() {
        // ADR 0007's property: an operator asking any pod gets the fleet's
        // answer, so they never have to know which pod to ask.
        let (coordinator, _clock) = serving();

        assert_eq!(coordinator.digests().len(), 3);
        assert_eq!(coordinator.cluster_clients(), 0);
        assert_eq!(coordinator.cluster_usage(&server()), 0);
        assert_ne!(coordinator.view_hash(), 0);
    }

    #[test]
    fn two_nodes_seeing_the_same_fleet_report_the_same_view_hash() {
        // The whole point of exporting it. Two pods that disagree are split
        // brain, and comparing two lists by eye is how that goes unnoticed.
        let clock = FakeClock::new();
        let (first, _) = serving();
        let second = following(&clock);
        for round in 1..=12 {
            second.gossip(digest_for(1, round));
        }

        assert_eq!(first.view_hash(), second.view_hash());
    }
}
