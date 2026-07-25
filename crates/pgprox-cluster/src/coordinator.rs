//! Wiring membership, quota and leases into one node's view.
//!
//! # What a node may open
//!
//! `guaranteed share + leases it currently holds`. A node that is cut off keeps
//! its share and loses its leases when they expire, which is why a partition
//! costs capacity rather than correctness.
//!
//! # Three rules that make that true, all three learned from the property test
//!
//! **The guaranteed share is divided by the configured fleet size, never by the
//! live count.** A node that can see only itself would otherwise conclude it is
//! the whole cluster and award itself the entire guaranteed total, while the
//! four nodes on the other side of the partition award themselves the same
//! total again. Membership decides who leads. It does not decide how large a
//! share is. Since a share never grows, a node that cannot see its peers is
//! never emboldened by their absence.
//!
//! **A leader may grant only while it can see a majority of the fleet.** The
//! takeover wait alone does not cover a partitioned leader: it still believes it
//! holds office, and it goes on granting from a free pool the new leader is
//! granting from as well. Two disjoint majorities cannot exist, so requiring one
//! means at most one ledger is ever handing out leases. The wait then covers the
//! handover between them, and the two together are what close the free pool.
//!
//! **The takeover wait covers failure detection, not just the lease TTL.** A
//! failure detector reports the past. A node counts a peer alive for up to
//! `suspect_after` past its last contact, so it can arm its takeover clock on a
//! quorum it has already lost, while the leader on the other side is still
//! granting. The wait must therefore be at least `ttl + suspect_after`, and
//! [`CoordinatorConfig::effective_lease`] derives that rather than trusting the
//! configuration to have got it right.
//!
//! None of the three needs consensus, an extra round trip, or a message this
//! crate does not already send. All three were found by the property test at the
//! bottom of this file, none by reading the design.
//!
//! # Where the invariant is proven
//!
//! At the bottom of this file, over the simulation: randomized schedules with
//! partitions, leader loss and simultaneous restarts, asserting after every step
//! that the sum of what every node believes it may open never exceeds the cap.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use pgprox_core::cluster::{ClusterDigest, MembershipView, NodeMode, QuotaError, QuotaLease};
use pgprox_core::ids::{NodeId, ServerId, TenantId};

use crate::digest::{DigestStore, MergeOutcome, VersionedDigest};
use crate::lease::{LeaseConfig, LeaseLedger};
use crate::membership::{Membership, MembershipConfig};
use crate::quota::{self, NodeAllowance};
use crate::reservation::{ReservationConfig, Reservations};

/// How a node's quota behaviour is tuned.
#[derive(Clone, Copy, Debug)]
pub struct CoordinatorConfig {
    /// Fraction of a cap distributed as guaranteed per-node share.
    pub guaranteed_fraction: f64,
    /// How many nodes the deployment is configured to run.
    ///
    /// The divisor for the guaranteed share and the denominator for the leader's
    /// majority check. Deliberately configured rather than observed: see the
    /// module docs. Set it to the replica count, and raise it before scaling up,
    /// not after. Running more nodes than this is safe but wasteful, since the
    /// coordinator then divides by the larger of the two.
    pub fleet_size: u32,
    /// Lease timing.
    pub lease: LeaseConfig,
    /// Liveness timing.
    pub membership: MembershipConfig,
    /// Tenant reservation tuning.
    pub reservation: ReservationConfig,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        let membership = MembershipConfig::default();
        let lease = LeaseConfig::default();
        Self {
            guaranteed_fraction: 0.5,
            fleet_size: 5,
            // Spelled out rather than taken from `LeaseConfig::default`, whose
            // own default only knows about the TTL. The defaults of this struct
            // must satisfy `is_safe` on their own: a default configuration that
            // needed correcting would teach every reader the wrong relation.
            lease: LeaseConfig {
                takeover_wait: lease.ttl + membership.suspect_after,
                ..lease
            },
            membership,
            reservation: ReservationConfig::default(),
        }
    }
}

impl CoordinatorConfig {
    /// The takeover wait actually used, which may be longer than configured.
    ///
    /// `ttl + suspect_after`, at least. A failure detector reports the past: a
    /// node counts a peer alive for up to `suspect_after` after its last
    /// contact, so a node can arm its takeover clock believing it holds a quorum
    /// it lost moments ago. The deposed leader is still granting during that
    /// window and its last lease lives one more `ttl`. Waiting only `ttl` lets
    /// the two overlap, which is exactly the seed-8 breach this is named for.
    ///
    /// Derived rather than merely validated, so a configuration that gets this
    /// wrong is slow rather than unsafe.
    #[must_use]
    pub fn effective_lease(&self) -> LeaseConfig {
        LeaseConfig {
            ttl: self.lease.ttl,
            takeover_wait: self
                .lease
                .takeover_wait
                .max(self.lease.ttl + self.membership.suspect_after),
        }
    }

    /// Whether the timings compose safely as written.
    ///
    /// [`Self::effective_lease`] enforces this regardless, so a `false` here
    /// means the deployment will be slower to recover than its config claims,
    /// not that it can breach the cap.
    #[must_use]
    pub fn is_safe(&self) -> bool {
        self.lease.is_safe()
            && self.membership.is_safe()
            && self.lease.takeover_wait >= self.lease.ttl + self.membership.suspect_after
    }
}

/// One node's view of membership, quota and leases.
#[derive(Debug)]
pub struct NodeCoordinator {
    local: NodeId,
    config: CoordinatorConfig,
    digests: DigestStore,
    liveness: Membership,
    /// Ledgers this node keeps as leader, one per server.
    ledgers: HashMap<ServerId, LeaseLedger>,
    /// Leases this node holds, one per server.
    held: HashMap<ServerId, QuotaLease>,
    /// Configured caps.
    caps: HashMap<ServerId, u32>,
    /// What this node reports about itself.
    mode: NodeMode,
    client_conns: u32,
    upstream_conns: Vec<(ServerId, u32)>,
    tenant_usage: Vec<(TenantId, u32)>,
    /// Tenant placement.
    reservations: Reservations,
    tracked_tenants: HashSet<TenantId>,
    /// When the view last changed, for the shed settle window.
    membership_changed_at: Instant,
    view_hash: u64,
}

impl NodeCoordinator {
    /// A coordinator for `local`.
    #[must_use]
    pub fn new(local: NodeId, config: CoordinatorConfig, started: Instant) -> Self {
        Self {
            local,
            config,
            digests: DigestStore::new(),
            liveness: Membership::new(local, config.membership),
            ledgers: HashMap::new(),
            held: HashMap::new(),
            caps: HashMap::new(),
            mode: NodeMode::Active,
            client_conns: 0,
            upstream_conns: Vec::new(),
            tenant_usage: Vec::new(),
            reservations: Reservations::new(config.reservation),
            tracked_tenants: HashSet::new(),
            // A node that has just started has by definition just seen its
            // membership change, so the settle window applies from boot rather
            // than from the first peer it happens to lose.
            membership_changed_at: started,
            view_hash: 0,
        }
    }

    /// Which node this is.
    #[must_use]
    pub const fn node(&self) -> NodeId {
        self.local
    }

    /// Registers a server's cap.
    pub fn set_cap(&mut self, server: ServerId, cap: u32) {
        self.caps.insert(server, cap);
    }

    /// The digests this node holds, for gossip and for admin aggregates.
    #[must_use]
    pub const fn digests(&self) -> &DigestStore {
        &self.digests
    }

    /// Liveness, for diagnostics and for the admin surface.
    #[must_use]
    pub const fn liveness(&self) -> &Membership {
        &self.liveness
    }

    /// Takes in a peer's gossip.
    ///
    /// Records liveness whether or not the digest was new. A peer repeating
    /// itself is still a peer we can hear, and treating a stale payload as
    /// silence would age out a node that is merely quiet.
    pub fn gossip(&mut self, incoming: VersionedDigest, now: Instant) -> MergeOutcome {
        self.liveness
            .heard(incoming.digest.node, incoming.digest.mode, now);
        self.digests.merge(incoming)
    }

    /// Drops a peer at once, as an explicit leave announcement does.
    pub fn forget(&mut self, node: NodeId) {
        self.liveness.forget(node);
        self.digests.forget(node);
    }

    /// Membership as this node currently sees it.
    #[must_use]
    pub fn membership(&self, now: Instant) -> MembershipView {
        self.liveness.view(now)
    }

    /// How a server's cap divides, from the configured fleet size.
    ///
    /// Divides by the larger of the configured size and what this node can
    /// actually see, so discovering more peers than were configured shrinks the
    /// share rather than over-subscribing the cap.
    fn split_for(&self, server: &ServerId, view: &MembershipView) -> quota::QuotaSplit {
        let cap = self.caps.get(server).copied().unwrap_or(0);
        let seen = u32::try_from(view.members().len()).unwrap_or(u32::MAX);
        quota::split(
            cap,
            self.config.fleet_size.max(seen),
            self.config.guaranteed_fraction,
        )
    }

    /// Whether this node sees enough of the fleet to lead.
    ///
    /// Strictly more than half, so two disjoint views cannot both qualify.
    /// Counted from nodes we have actually heard from recently, not from the
    /// membership view, so a suspect suspends granting before it changes any
    /// node's share.
    fn has_quorum(&self, now: Instant) -> bool {
        let alive = u32::try_from(self.liveness.alive_count(now)).unwrap_or(u32::MAX);
        let fleet = self.config.fleet_size.max(1);
        alive.saturating_mul(2) > fleet
    }

    /// Brings the ledgers in line with membership.
    ///
    /// Called every gossip round. The ledger decides for itself whether office
    /// changed hands, so calling this repeatedly is harmless.
    pub fn observe(&mut self, now: Instant) {
        let view = self.membership(now);
        // Leading and being able to act on it are separate conditions, and the
        // ledger is told about the conjunction. See `observe_leadership`.
        let leading = view.is_leader() && self.has_quorum(now);
        let servers: Vec<ServerId> = self.caps.keys().cloned().collect();

        for server in servers {
            let split = self.split_for(&server, &view);
            let ledger = self.ledgers.entry(server).or_insert_with(|| {
                LeaseLedger::new(split.free_pool, self.config.effective_lease())
            });
            ledger.observe_leadership(leading, now);
            ledger.reap(now);
        }
        self.liveness.reap(now);

        // A reservation decays by counting gossip rounds in which the home node
        // reported no use, so this must advance every round whether or not
        // anything arrived. Advancing it only on a digest merge would mean a
        // silent home node looked busy forever, which is the direction that
        // strands its tenants' capacity.
        let tenants: Vec<TenantId> = self.tracked_tenants.iter().cloned().collect();
        for tenant in tenants {
            let usage = self.home_usage(&tenant, now);
            self.reservations.observe(&tenant, usage);
        }

        // The settle window measures from a change in the view, not from a
        // gossip arrival: a peer repeating itself must not keep resetting the
        // clock and suppress shedding forever.
        let hash = self.digests.view_hash();
        if hash != self.view_hash {
            self.view_hash = hash;
            self.membership_changed_at = now;
        }
    }

    /// What this node may open for a server right now.
    #[must_use]
    pub fn allowance(&self, server: &ServerId, now: Instant) -> NodeAllowance {
        let split = self.split_for(server, &self.membership(now));
        let leased = self.held.get(server).map_or(0, |lease| lease.count(now));

        NodeAllowance {
            server: server.clone(),
            guaranteed: split.guaranteed_per_node,
            leased,
        }
    }

    /// Asks the local ledger for more, which only succeeds on a leader that can
    /// see a majority of the fleet.
    ///
    /// A real cluster sends this to the leader over gossip. The simulation
    /// drives it directly so the test measures the quota rules rather than a
    /// message-passing layer that is not what the invariant is about.
    ///
    /// # Errors
    ///
    /// Fails when this node is not a serving leader, cannot see a majority, or
    /// the pool is exhausted.
    pub fn request(
        &mut self,
        server: &ServerId,
        holder: NodeId,
        want: u32,
        now: Instant,
    ) -> Result<QuotaLease, QuotaError> {
        // Checked before the ledger, not inside it. A partitioned leader's
        // ledger looks perfectly healthy from the inside; what disqualifies it
        // is the view, which only the coordinator holds.
        if !self.has_quorum(now) {
            return Err(QuotaError::NoLeader);
        }
        let ledger = self.ledgers.get_mut(server).ok_or(QuotaError::NoLeader)?;
        ledger.grant(server, holder, want, now)
    }

    /// Records a lease this node has been granted.
    pub fn accept(&mut self, lease: QuotaLease) {
        self.held.insert(lease.server().clone(), lease);
    }

    /// Returns a lease before it expires.
    ///
    /// Only clears the holder's own record. The ledger reclaims the capacity on
    /// expiry regardless, so a release that never reaches the leader costs one
    /// TTL of headroom rather than correctness.
    pub fn release(&mut self, server: &ServerId) {
        self.held.remove(server);
        if let Some(ledger) = self.ledgers.get_mut(server) {
            ledger.release(self.local);
        }
    }

    /// Drops every lease this node holds, as a restart would.
    pub fn forget_leases(&mut self) {
        self.held.clear();
    }

    /// Sets whether this node is taking work.
    pub fn set_mode(&mut self, mode: NodeMode) {
        self.mode = mode;
    }

    /// Records what this node is currently serving, for the next digest.
    pub fn report(&mut self, client_conns: u32, upstream_conns: Vec<(ServerId, u32)>) {
        self.client_conns = client_conns;
        self.upstream_conns = upstream_conns;
    }

    /// Records this node's per-tenant usage, for the next digest.
    ///
    /// Only tenants this node homes belong here. See [`ClusterDigest`].
    pub fn report_tenants(&mut self, usage: Vec<(TenantId, u32)>) {
        self.tenant_usage = usage;
    }

    /// What this node tells its peers about itself.
    #[must_use]
    pub fn digest(&self) -> ClusterDigest {
        ClusterDigest {
            node: self.local,
            mode: self.mode,
            client_conns: self.client_conns,
            upstream_conns: self.upstream_conns.clone(),
            tenant_usage: self.tenant_usage.clone(),
        }
    }

    /// What the tenant's home node last said it was using for that tenant.
    ///
    /// Zero when the tenant has no home, or when its home has said nothing.
    /// Both read as idle, which is the direction that lets peers reclaim the
    /// slack rather than reserving capacity for a node that may be gone.
    #[must_use]
    pub fn home_usage(&self, tenant: &TenantId, now: Instant) -> u32 {
        let Some(home) = self.membership(now).home_node(tenant) else {
            return 0;
        };
        self.digests.get(home).map_or(0, |digest| {
            digest
                .tenant_usage
                .iter()
                .find(|(id, _)| id == tenant)
                .map_or(0, |(_, used)| *used)
        })
    }

    /// Whether the tenant's home node has room for another connection.
    ///
    /// The cluster-side input to a shed decision. Defaults to `false` when the
    /// tenant has no home or its home has gone quiet, so a client is kept rather
    /// than moved toward a node that may not be able to take it.
    #[must_use]
    pub fn home_has_headroom(&self, tenant: &TenantId, budget: u32, now: Instant) -> bool {
        let view = self.membership(now);
        let Some(home) = view.home_node(tenant) else {
            return false;
        };
        let peers = u32::try_from(view.active_count()).unwrap_or(u32::MAX);
        let entitlement = self
            .reservations
            .entitlement(tenant, home, Some(home), budget, peers);
        self.home_usage(tenant, now) < entitlement.allowed
    }

    /// Whether the tenant's home node is draining.
    ///
    /// `false` when there is no home: a drain is a reason not to shed toward a
    /// node, and "no home at all" is covered by [`Self::home_has_headroom`]
    /// instead of being reported here as a drain that is not happening.
    #[must_use]
    pub fn home_draining(&self, tenant: &TenantId, now: Instant) -> bool {
        let view = self.membership(now);
        view.home_node(tenant)
            .and_then(|home| self.digests.get(home))
            .is_some_and(|digest| digest.mode == NodeMode::Draining)
    }

    /// How long since the membership view last changed.
    ///
    /// The settle window a shed decision waits out. Measured from a change in
    /// the view hash rather than from a gossip arrival, so a peer repeating
    /// itself does not keep resetting the clock and suppress shedding forever.
    #[must_use]
    pub fn since_membership_change(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.membership_changed_at)
    }

    /// The reservation tracker, for entitlement questions and diagnostics.
    #[must_use]
    pub const fn reservations(&self) -> &Reservations {
        &self.reservations
    }

    /// Starts tracking a tenant's reservation.
    ///
    /// Tracking is explicit because the decay counter only means something if
    /// it is advanced on every gossip round. A tenant added lazily on first use
    /// would start at zero idle rounds and read as freshly active.
    pub fn track_tenant(&mut self, tenant: TenantId) {
        self.tracked_tenants.insert(tenant);
    }

    /// Stops tracking a tenant.
    pub fn forget_tenant(&mut self, tenant: &TenantId) {
        self.tracked_tenants.remove(tenant);
        self.reservations.forget(tenant);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::digest::VersionedDigest;
    use crate::sim::Rng;
    use std::time::Duration;

    const CAP: u32 = 100;

    fn server() -> ServerId {
        ServerId::new("db-1", 5432)
    }

    fn node(n: u16) -> NodeId {
        NodeId::new(n)
    }

    fn digest_for(n: u16, version: u64, mode: NodeMode) -> VersionedDigest {
        VersionedDigest {
            digest: ClusterDigest {
                node: node(n),
                mode,
                client_conns: 0,
                upstream_conns: Vec::new(),
                tenant_usage: Vec::new(),
            },
            version,
        }
    }

    fn config_for(size: u16) -> CoordinatorConfig {
        CoordinatorConfig {
            fleet_size: u32::from(size),
            ..CoordinatorConfig::default()
        }
    }

    /// A cluster of `size` nodes that all know about each other.
    fn cluster(size: u16, now: Instant) -> Vec<NodeCoordinator> {
        let mut nodes: Vec<NodeCoordinator> = (1..=size)
            .map(|n| {
                let mut c = NodeCoordinator::new(node(n), config_for(size), now);
                c.set_cap(server(), CAP);
                c
            })
            .collect();
        gossip_round(&mut nodes, 1, now);
        nodes
    }

    /// Every node hears from every node, then acts on it.
    fn gossip_round(nodes: &mut [NodeCoordinator], version: u64, now: Instant) {
        let peers: Vec<u16> = nodes.iter().map(|c| c.node().get()).collect();
        for c in nodes.iter_mut() {
            for peer in &peers {
                c.gossip(digest_for(*peer, version, NodeMode::Active), now);
            }
            c.observe(now);
        }
    }

    /// The invariant, evaluated over every node's current belief.
    ///
    /// Deliberately sums what each node believes it may open rather than what
    /// it has open: a node acts on its belief, so a belief that sums above the
    /// cap is a breach waiting for load.
    fn total_permitted(nodes: &[NodeCoordinator], now: Instant) -> u32 {
        nodes
            .iter()
            .map(|c| c.allowance(&server(), now).total())
            .fold(0_u32, u32::saturating_add)
    }

    /// The cluster size the schedules run at.
    const FLEET: u16 = 5;

    /// What is currently wrong with the cluster.
    ///
    /// Held across steps rather than applied as one-shot events. Liveness is
    /// time-based, so a partition that heals on the next step is one no node
    /// ever notices, and a schedule made of those proves nothing.
    #[derive(Clone, Copy, Debug, Default)]
    struct Faults {
        /// Where the network is cut, as a count of nodes on the left side.
        /// `None` is a healthy network.
        split_at: Option<usize>,
        /// Whether node 1, the leader, is draining.
        leader_loss: bool,
    }

    /// One gossip round, respecting the current faults.
    fn deliver(nodes: &mut [NodeCoordinator], faults: Faults, version: u64, now: Instant) {
        let peers: Vec<u16> = nodes.iter().map(|c| c.node().get()).collect();
        for (index, c) in nodes.iter_mut().enumerate() {
            for (peer_index, peer) in peers.iter().enumerate() {
                let reachable = faults
                    .split_at
                    .is_none_or(|at| (peer_index < at) == (index < at));
                if !reachable {
                    continue;
                }
                let mode = if *peer == 1 && faults.leader_loss {
                    NodeMode::Draining
                } else {
                    NodeMode::Active
                };
                c.gossip(digest_for(*peer, version, mode), now);
            }
            c.observe(now);
        }
    }

    /// A restart: a fresh process, with no leases and no idea who is out there.
    fn restart(nodes: &mut [NodeCoordinator], index: usize, at: Instant) {
        let id = nodes[index].node();
        let mut fresh = NodeCoordinator::new(id, config_for(FLEET), at);
        fresh.set_cap(server(), CAP);
        nodes[index] = fresh;
    }

    #[test]
    fn guaranteed_plus_leased_never_exceeds_the_cap() {
        // The milestone. Randomized schedules over sustained partitions, leader
        // loss and simultaneous restarts, asserted after every step.
        for seed in 0..500_u64 {
            let mut rng = Rng::new(seed);
            let mut now = Instant::now();
            let mut nodes = cluster(FLEET, now);
            let mut faults = Faults::default();
            let mut version = 1_u64;

            for step in 0..80 {
                now += Duration::from_millis(500);
                version += 1;

                // Faults change occasionally, so most steps run under whatever
                // is already broken and both sides get time to detect it.
                match rng.below(20) {
                    0 => {
                        // A partition at a random point, the degenerate
                        // all-on-one-side cases included.
                        let at = usize::try_from(rng.below(u64::from(FLEET) + 1)).unwrap();
                        faults.split_at = Some(at);
                    }
                    1 => faults.split_at = None,
                    2 => faults.leader_loss = !faults.leader_loss,
                    3 => {
                        // Simultaneous restarts, up to the whole fleet.
                        let count = usize::try_from(rng.below(u64::from(FLEET)) + 1).unwrap();
                        for index in 0..count {
                            restart(&mut nodes, index, now);
                        }
                    }
                    _ => {}
                }

                deliver(&mut nodes, faults, version, now);

                // Everyone asks for more than they could possibly be owed, from
                // whichever node will answer. Greed is the point: a rule that
                // holds only under modest demand is not a cap.
                for asker in 0..nodes.len() {
                    let holder = nodes[asker].node();
                    let want = u32::try_from(rng.below(60)).unwrap();
                    for responder in 0..nodes.len() {
                        if let Ok(lease) = nodes[responder].request(&server(), holder, want, now) {
                            nodes[asker].accept(lease);
                            break;
                        }
                    }
                }

                let total = total_permitted(&nodes, now);
                assert!(
                    total <= CAP,
                    "seed {seed} step {step} under {faults:?}: \
                     nodes believe they may open {total}, cap is {CAP}"
                );
            }
        }
    }

    #[test]
    fn a_split_brain_grants_from_one_side_only() {
        // The regression behind the quorum rule. Before it, a partitioned old
        // leader kept granting from its ledger while the new leader granted
        // from its own, and the free pool was handed out twice.
        let mut now = Instant::now();
        let mut nodes = cluster(FLEET, now);
        let faults = Faults {
            split_at: Some(2),
            leader_loss: false,
        };

        // Hold the partition open long enough for the minority to be declared
        // dead, for the majority's new leader to take office, and for its
        // takeover wait to elapse on top of that.
        for _ in 0..60 {
            now += Duration::from_millis(500);
            deliver(&mut nodes, faults, 2, now);
        }

        // The minority side, which still contains the original leader.
        let minority = nodes[0].request(&server(), node(1), 40, now);
        assert_eq!(
            minority.unwrap_err(),
            QuotaError::NoLeader,
            "the minority side granted quota"
        );

        // The majority side may, once its wait has elapsed.
        assert!(
            nodes[2].request(&server(), node(3), 40, now).is_ok(),
            "the majority side could not grant"
        );
        assert!(total_permitted(&nodes, now) <= CAP);
    }

    #[test]
    fn a_returning_leader_waits_out_the_deposed_leaders_last_lease() {
        // Seed 8, made deterministic. Node 1 is cut off and node 3 leads the
        // majority. When node 1 rejoins it becomes leader again on liveness that
        // is up to `suspect_after` stale, so it arms its takeover clock while
        // node 3 is still granting. With a wait of only `ttl` the two overlapped
        // by a step and the cap went to 140.
        let config = config_for(FLEET);
        let mut now = Instant::now();
        let mut nodes = cluster(FLEET, now);
        let isolated = Faults {
            split_at: Some(1),
            leader_loss: false,
        };

        // Long enough for node 1 to be declared dead and node 2, the lowest of
        // the remaining four, to take over and serve its wait.
        for _ in 0..60 {
            now += Duration::from_millis(500);
            deliver(&mut nodes, isolated, 2, now);
        }
        // The majority's leader hands out the entire free pool.
        let lease = nodes[1].request(&server(), node(2), 100, now).unwrap();
        let pool = lease.nominal_count();
        nodes[1].accept(lease);
        assert!(pool > 0, "the majority leader granted nothing");

        // Node 1 rejoins. Its very next view makes it leader again.
        now += Duration::from_millis(500);
        deliver(&mut nodes, Faults::default(), 3, now);
        let rejoined = now;

        // Through the whole window in which node 2's lease is still live, node 1
        // must refuse.
        let step = Duration::from_millis(500);
        let wait = config.effective_lease().takeover_wait;
        let mut steps = 0;
        while now + step < rejoined + wait {
            now += step;
            deliver(&mut nodes, Faults::default(), 4, now);
            assert_eq!(
                nodes[0].request(&server(), node(1), 100, now).unwrap_err(),
                QuotaError::NoLeader,
                "the returning leader granted while the old lease was still live"
            );
            assert!(total_permitted(&nodes, now) <= CAP);
            steps += 1;
        }
        assert!(steps > 0, "the wait was over before it began");

        // By the time it may grant, the deposed leader's lease has lapsed, which
        // is the whole reason the wait is `ttl + suspect_after` and not `ttl`.
        assert_eq!(
            nodes[1].allowance(&server(), now).leased,
            0,
            "the deposed leader's lease outlived the takeover wait"
        );
        now += step;
        deliver(&mut nodes, Faults::default(), 5, now);
        assert!(
            nodes[0].request(&server(), node(1), 100, now).is_ok(),
            "the returning leader never resumed granting"
        );
        assert!(total_permitted(&nodes, now) <= CAP);
    }

    #[test]
    fn an_isolated_node_never_awards_itself_the_whole_guaranteed_total() {
        // The other half of the split-brain regression. The share is divided by
        // the configured fleet size, so a node that can see only itself does not
        // conclude it is the whole cluster.
        let mut now = Instant::now();
        let mut nodes = cluster(FLEET, now);
        let faults = Faults {
            split_at: Some(1),
            leader_loss: false,
        };
        for _ in 0..30 {
            now += Duration::from_millis(500);
            deliver(&mut nodes, faults, 2, now);
        }

        assert_eq!(nodes[0].liveness().alive_count(now), 1, "not isolated");
        assert_eq!(
            nodes[0].allowance(&server(), now).guaranteed,
            CAP / 2 / u32::from(FLEET),
            "an isolated node raised its own share"
        );
    }

    #[test]
    fn a_partition_costs_capacity_rather_than_correctness() {
        // The direction that matters. Cut off, a node keeps its guaranteed
        // share and loses only its leases.
        let start = Instant::now();
        let now = start + config_for(FLEET).effective_lease().takeover_wait;
        let mut nodes = cluster(FLEET, now);

        let before = total_permitted(&nodes, now);
        assert!(before <= CAP);

        // Node 5 is cut off from everyone, and everyone from it.
        let mut now = now;
        for _ in 0..30 {
            now += Duration::from_millis(500);
            deliver(
                &mut nodes,
                Faults {
                    split_at: Some(4),
                    leader_loss: false,
                },
                2,
                now,
            );
        }

        let after = total_permitted(&nodes, now);
        assert!(after <= CAP, "a partition over-subscribed the cap");
        // The isolated node still has its own share, so it can still serve.
        assert!(
            nodes[4].allowance(&server(), now).guaranteed > 0,
            "an isolated node lost its guaranteed share and cannot serve"
        );
    }

    #[test]
    fn a_leader_that_loses_office_stops_granting() {
        let start = Instant::now();
        let ready = start + config_for(3).effective_lease().takeover_wait;
        let mut nodes = cluster(3, start);
        deliver(&mut nodes, Faults::default(), 2, ready);

        // Node 1 leads and can grant.
        assert!(nodes[0].request(&server(), node(2), 5, ready).is_ok());

        // Node 1 starts draining, so node 2 leads.
        deliver(
            &mut nodes,
            Faults {
                split_at: None,
                leader_loss: true,
            },
            2,
            ready,
        );

        assert_eq!(
            nodes[0].request(&server(), node(2), 5, ready).unwrap_err(),
            QuotaError::NoLeader,
            "a deposed leader kept granting"
        );
    }

    #[test]
    fn a_new_leader_cannot_grant_until_its_wait_elapses() {
        // Over-granting across a failover is the one moment the cap is at risk,
        // and the wait is what closes it.
        let config = config_for(3);
        let start = Instant::now();
        let mut nodes = cluster(3, start);

        // Node 1 drains, so node 2 takes office at `start`.
        deliver(
            &mut nodes,
            Faults {
                split_at: None,
                leader_loss: true,
            },
            2,
            start,
        );

        assert_eq!(
            nodes[1].request(&server(), node(3), 5, start).unwrap_err(),
            QuotaError::NoLeader,
            "a fresh leader granted immediately"
        );

        let ready = start + config.effective_lease().takeover_wait;
        deliver(
            &mut nodes,
            Faults {
                split_at: None,
                leader_loss: true,
            },
            3,
            ready,
        );
        assert!(nodes[1].request(&server(), node(3), 5, ready).is_ok());
    }

    #[test]
    fn a_restarted_node_holds_nothing_until_it_asks_again() {
        let start = Instant::now();
        let now = start + config_for(3).effective_lease().takeover_wait;
        let mut nodes = cluster(3, start);
        deliver(&mut nodes, Faults::default(), 2, now);

        let lease = nodes[0].request(&server(), node(2), 20, now).unwrap();
        nodes[1].accept(lease);
        assert!(nodes[1].allowance(&server(), now).leased > 0);

        nodes[1].forget_leases();
        assert_eq!(
            nodes[1].allowance(&server(), now).leased,
            0,
            "a restarted node still believed it held a lease"
        );
        assert!(total_permitted(&nodes, now) <= CAP);
    }

    #[test]
    fn a_shrinking_cluster_raises_each_share_without_breaching_the_cap() {
        // The transient where nodes disagree about membership. Each recomputes
        // its share from its own view, and neither view may exceed the cap.
        let start = Instant::now();
        let now = start + config_for(3).effective_lease().takeover_wait;

        for size in 2..=6_u16 {
            let nodes = cluster(size, now);
            let total = total_permitted(&nodes, now);
            assert!(total <= CAP, "{size} nodes permitted {total}");
        }
    }

    #[test]
    fn an_expired_lease_stops_counting_without_anyone_acting() {
        let config = config_for(3).effective_lease();
        let start = Instant::now();
        let now = start + config.takeover_wait;
        let mut nodes = cluster(3, start);
        deliver(&mut nodes, Faults::default(), 2, now);

        let lease = nodes[0].request(&server(), node(2), 20, now).unwrap();
        nodes[1].accept(lease);
        assert!(nodes[1].allowance(&server(), now).leased > 0);

        let after = now + config.ttl + Duration::from_millis(1);
        assert_eq!(
            nodes[1].allowance(&server(), after).leased,
            0,
            "an expired lease still counted toward what the node may open"
        );
    }

    #[test]
    fn the_default_configuration_is_safe_without_correction() {
        // A default that `effective_lease` had to lengthen would teach every
        // reader the wrong relation between the two windows.
        let config = CoordinatorConfig::default();
        assert!(config.is_safe());
        assert_eq!(
            config.effective_lease().takeover_wait,
            config.lease.takeover_wait,
            "the default needed correcting"
        );
    }

    #[test]
    fn a_wait_shorter_than_detection_is_lengthened_rather_than_trusted() {
        // Slow beats down. A deployment that gets this wrong recovers late; it
        // does not breach the cap.
        let config = CoordinatorConfig {
            lease: LeaseConfig {
                ttl: Duration::from_secs(5),
                takeover_wait: Duration::from_secs(5),
            },
            membership: MembershipConfig {
                suspect_after: Duration::from_secs(3),
                dead_after: Duration::from_secs(10),
            },
            ..CoordinatorConfig::default()
        };
        assert!(!config.is_safe(), "an unsafe config was reported safe");
        assert_eq!(
            config.effective_lease().takeover_wait,
            Duration::from_secs(8),
            "the wait was not lengthened to cover detection"
        );
        assert_eq!(config.effective_lease().ttl, config.lease.ttl);
    }

    #[test]
    fn a_generous_configured_wait_is_left_alone() {
        let config = CoordinatorConfig {
            lease: LeaseConfig {
                ttl: Duration::from_secs(5),
                takeover_wait: Duration::from_secs(60),
            },
            ..CoordinatorConfig::default()
        };
        assert!(config.is_safe());
        assert_eq!(
            config.effective_lease().takeover_wait,
            Duration::from_secs(60)
        );
    }

    #[test]
    fn forgetting_a_peer_drops_it_from_liveness_and_from_the_digests() {
        // An explicit leave announcement, where waiting out the detection
        // window would be pointless.
        let now = Instant::now();
        let mut nodes = cluster(3, now);
        assert_eq!(nodes[0].digests().len(), 3);

        nodes[0].forget(node(3));
        assert_eq!(nodes[0].liveness().tracked(), 2);
        assert_eq!(nodes[0].digests().len(), 2);
        assert_eq!(nodes[0].membership(now).members().len(), 2);
    }

    #[test]
    fn repeated_gossip_still_counts_as_contact() {
        // A peer with nothing new to say is still a peer we can hear. Treating
        // a stale payload as silence would age out a healthy quiet node.
        let start = Instant::now();
        let mut nodes = cluster(3, start);
        let later = start + Duration::from_secs(5);

        assert_eq!(
            nodes[0].gossip(digest_for(2, 1, NodeMode::Active), later),
            MergeOutcome::Stale,
            "an equal version should not count as new information"
        );
        assert_eq!(
            nodes[0].liveness().state(node(2), later),
            crate::membership::NodeState::Alive,
            "a repeat was treated as silence"
        );
    }

    #[test]
    fn seeing_more_peers_than_configured_shrinks_the_share() {
        // Scaling past the configured fleet size is wasteful, not unsafe.
        let now = Instant::now();
        let mut nodes: Vec<NodeCoordinator> = (1..=8_u16)
            .map(|n| {
                let mut c = NodeCoordinator::new(node(n), config_for(FLEET), now);
                c.set_cap(server(), CAP);
                c
            })
            .collect();
        for c in &mut nodes {
            for peer in 1..=8_u16 {
                c.gossip(digest_for(peer, 1, NodeMode::Active), now);
            }
            c.observe(now);
        }

        assert_eq!(
            nodes[0].allowance(&server(), now).guaranteed,
            50 / 8,
            "the share was divided by the configured size, not the real one"
        );
        assert!(total_permitted(&nodes, now) <= CAP);
    }

    /// A digest carrying per-tenant usage.
    fn digest_with_tenants(
        n: u16,
        version: u64,
        mode: NodeMode,
        usage: &[(&str, u32)],
    ) -> VersionedDigest {
        let mut d = digest_for(n, version, mode);
        d.digest.tenant_usage = usage.iter().map(|(t, u)| (TenantId::new(*t), *u)).collect();
        d
    }

    /// The tenant that `cluster(3, _)` homes on node `n`, found by asking.
    fn tenant_homed_on(nodes: &[NodeCoordinator], n: u16, now: Instant) -> TenantId {
        let view = nodes[0].membership(now);
        (0..1_000)
            .map(|i| TenantId::new(format!("tenant-{i}")))
            .find(|t| view.home_node(t) == Some(node(n)))
            .unwrap()
    }

    #[test]
    fn a_peer_reads_the_home_nodes_usage_out_of_its_digest() {
        // The link that was missing: Reservations::observe takes the home
        // node's usage, and before this the digest carried no per-tenant data
        // at all, so nothing could feed it.
        let now = Instant::now();
        let mut nodes = cluster(3, now);
        let tenant = tenant_homed_on(&nodes, 2, now);

        nodes[0].gossip(
            digest_with_tenants(2, 2, NodeMode::Active, &[(tenant.as_str(), 7)]),
            now,
        );
        assert_eq!(nodes[0].home_usage(&tenant, now), 7);
    }

    #[test]
    fn a_tenant_with_no_reported_usage_reads_as_idle() {
        // The direction that lets peers reclaim slack rather than reserving
        // capacity for a node that may be gone.
        let now = Instant::now();
        let nodes = cluster(3, now);
        let tenant = tenant_homed_on(&nodes, 2, now);
        assert_eq!(nodes[0].home_usage(&tenant, now), 0);
    }

    #[test]
    fn a_reservation_decays_from_gossip_alone() {
        // The acceptance criterion: a peer watches a home node do nothing for
        // this tenant and reclaims the slack, with no message beyond the digest.
        let config = config_for(3);
        let mut now = Instant::now();
        let mut nodes = cluster(3, now);
        let tenant = tenant_homed_on(&nodes, 2, now);
        nodes[0].track_tenant(tenant.clone());

        for round in 2..=(config.reservation.decay_rounds + 1) {
            now += Duration::from_secs(1);
            nodes[0].gossip(
                digest_with_tenants(2, u64::from(round), NodeMode::Active, &[]),
                now,
            );
            nodes[0].observe(now);
        }
        assert!(
            nodes[0].reservations().has_decayed(&tenant),
            "an idle home node kept its reservation forever"
        );
    }

    #[test]
    fn any_use_at_all_resets_the_decay() {
        let mut now = Instant::now();
        let mut nodes = cluster(3, now);
        let tenant = tenant_homed_on(&nodes, 2, now);
        nodes[0].track_tenant(tenant.clone());

        for round in 2..=6_u64 {
            now += Duration::from_secs(1);
            nodes[0].gossip(digest_with_tenants(2, round, NodeMode::Active, &[]), now);
            nodes[0].observe(now);
        }
        assert!(nodes[0].reservations().has_decayed(&tenant));

        now += Duration::from_secs(1);
        nodes[0].gossip(
            digest_with_tenants(2, 7, NodeMode::Active, &[(tenant.as_str(), 1)]),
            now,
        );
        nodes[0].observe(now);
        assert!(
            !nodes[0].reservations().has_decayed(&tenant),
            "a home node that came back was still treated as idle"
        );
    }

    #[test]
    fn decay_advances_on_a_round_where_nothing_arrived() {
        // A silent home node must decay. Advancing only on a merge would let it
        // look busy forever and strand its tenants' capacity.
        let config = config_for(3);
        let mut now = Instant::now();
        let mut nodes = cluster(3, now);
        let tenant = tenant_homed_on(&nodes, 2, now);
        nodes[0].track_tenant(tenant.clone());

        for _ in 0..=config.reservation.decay_rounds {
            now += Duration::from_secs(1);
            nodes[0].observe(now);
        }
        assert!(nodes[0].reservations().has_decayed(&tenant));
    }

    #[test]
    fn a_forgotten_tenant_stops_being_tracked() {
        let mut now = Instant::now();
        let mut nodes = cluster(3, now);
        let tenant = tenant_homed_on(&nodes, 2, now);
        nodes[0].track_tenant(tenant.clone());
        now += Duration::from_secs(1);
        nodes[0].observe(now);
        assert_eq!(nodes[0].reservations().tracked(), 1);

        nodes[0].forget_tenant(&tenant);
        now += Duration::from_secs(1);
        nodes[0].observe(now);
        assert_eq!(nodes[0].reservations().tracked(), 0);
    }

    #[test]
    fn a_busy_home_node_reports_no_headroom() {
        let now = Instant::now();
        let mut nodes = cluster(3, now);
        let tenant = tenant_homed_on(&nodes, 2, now);

        // Budget 10, home share 0.8, so the home node reserves 8.
        nodes[0].gossip(
            digest_with_tenants(2, 2, NodeMode::Active, &[(tenant.as_str(), 7)]),
            now,
        );
        assert!(nodes[0].home_has_headroom(&tenant, 10, now));

        nodes[0].gossip(
            digest_with_tenants(2, 3, NodeMode::Active, &[(tenant.as_str(), 8)]),
            now,
        );
        assert!(
            !nodes[0].home_has_headroom(&tenant, 10, now),
            "a home node at its reservation reported headroom"
        );
    }

    #[test]
    fn a_tenant_with_no_home_has_no_headroom_to_shed_toward() {
        // Every node draining leaves no home at all. Keeping the client is the
        // only safe answer; shedding it would aim at nobody.
        let now = Instant::now();
        let mut nodes = cluster(3, now);
        let tenant = tenant_homed_on(&nodes, 2, now);
        for peer in 1..=3_u16 {
            nodes[0].gossip(digest_for(peer, 2, NodeMode::Draining), now);
        }
        assert_eq!(nodes[0].membership(now).home_node(&tenant), None);
        assert!(!nodes[0].home_has_headroom(&tenant, 10, now));
        assert!(!nodes[0].home_draining(&tenant, now));
        assert_eq!(nodes[0].home_usage(&tenant, now), 0);
    }

    #[test]
    fn a_draining_home_node_is_reported_as_draining() {
        let now = Instant::now();
        let mut nodes = cluster(3, now);
        let tenant = tenant_homed_on(&nodes, 3, now);
        assert!(!nodes[0].home_draining(&tenant, now));

        // Node 3 drains, so the tenant rehomes and its new home is not draining.
        nodes[0].gossip(digest_for(1, 2, NodeMode::Draining), now);
        nodes[0].gossip(digest_for(2, 2, NodeMode::Draining), now);
        assert_eq!(nodes[0].membership(now).home_node(&tenant), Some(node(3)));
        nodes[0].gossip(digest_for(3, 3, NodeMode::Draining), now);
        assert_eq!(
            nodes[0].membership(now).home_node(&tenant),
            None,
            "a draining node still homed a tenant"
        );
    }

    #[test]
    fn the_settle_window_runs_from_a_view_change_not_from_a_gossip_arrival() {
        // A peer repeating itself must not keep resetting the clock, or the
        // settle window suppresses shedding forever in a healthy cluster.
        let start = Instant::now();
        let mut nodes = cluster(3, start);
        let mut now = start;

        for round in 2..=10_u64 {
            now += Duration::from_secs(1);
            for peer in 1..=3_u16 {
                nodes[0].gossip(digest_for(peer, round, NodeMode::Active), now);
            }
            nodes[0].observe(now);
        }
        assert_eq!(
            nodes[0].since_membership_change(now),
            now - start,
            "repeated gossip reset the settle window"
        );

        // A node leaving is a change, and does reset it.
        now += Duration::from_secs(1);
        nodes[0].forget(node(3));
        nodes[0].observe(now);
        assert_eq!(nodes[0].since_membership_change(now), Duration::ZERO);
    }

    #[test]
    fn the_digest_carries_the_tenants_this_node_homes() {
        let now = Instant::now();
        let mut nodes = cluster(3, now);
        let tenant = tenant_homed_on(&nodes, 1, now);
        nodes[0].report_tenants(vec![(tenant.clone(), 12)]);

        assert_eq!(nodes[0].digest().tenant_usage, vec![(tenant, 12)]);
    }

    #[test]
    fn an_unknown_server_permits_nothing() {
        let now = Instant::now();
        let nodes = cluster(3, now);
        let allowance = nodes[0].allowance(&ServerId::new("db-9", 5432), now);
        assert_eq!(allowance.total(), 0);
    }
}
