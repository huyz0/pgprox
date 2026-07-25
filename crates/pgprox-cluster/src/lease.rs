//! The leader's lease ledger.
//!
//! # Why a leader at all
//!
//! The guaranteed share needs no coordination: every node computes the same
//! split and stays inside it. The free pool does, because two nodes each taking
//! "what is left" would take it twice.
//!
//! The leader is simply the lowest live node ID. That is not a consensus
//! protocol and does not need to be, because the ledger is not the source of
//! truth about capacity: expiry is. A wrongly elected second leader can grant
//! at most what it believes the pool holds, and both leaders' grants expire.
//!
//! # The rule that makes failover safe
//!
//! A new leader waits one full lease TTL before granting from the free pool.
//!
//! Its predecessor may have granted leases it never learned about, and gossip
//! carries usage rather than a ledger. Waiting one TTL guarantees every lease
//! the old leader issued has either been renewed through the new leader, and so
//! is known, or has expired, and so is gone. Without the wait, a failover is
//! the one moment the cap can be breached.

use std::collections::HashMap;
use std::time::Duration;

use pgprox_core::cluster::{MembershipView, QuotaError, QuotaLease};
use pgprox_core::ids::{NodeId, ServerId};
use std::time::Instant;

/// How the ledger is tuned.
#[derive(Clone, Copy, Debug)]
pub struct LeaseConfig {
    /// How long a lease lives without renewal.
    pub ttl: Duration,
    /// How long after taking office a leader refuses to grant.
    ///
    /// Must be at least `ttl`, or a lease the previous leader issued could
    /// still be live and unknown while this one grants against the same
    /// capacity.
    pub takeover_wait: Duration,
}

impl Default for LeaseConfig {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(5),
            takeover_wait: Duration::from_secs(5),
        }
    }
}

impl LeaseConfig {
    /// Whether this configuration can breach the cap across a failover.
    ///
    /// Exposed so configuration validation can refuse it rather than discovering
    /// it during an incident.
    #[must_use]
    pub fn is_safe(&self) -> bool {
        self.takeover_wait >= self.ttl
    }
}

/// One outstanding grant.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Grant {
    holder: NodeId,
    count: u32,
    expires_at: Instant,
}

/// The leader's view of what it has handed out for one server.
#[derive(Debug)]
pub struct LeaseLedger {
    config: LeaseConfig,
    /// Total the pool may hold out at once.
    pool: u32,
    grants: HashMap<NodeId, Grant>,
    /// When this node took office, or `None` if it is not the leader.
    took_office: Option<Instant>,
}

impl LeaseLedger {
    /// A ledger for a pool of `pool` connections.
    #[must_use]
    pub fn new(pool: u32, config: LeaseConfig) -> Self {
        Self {
            config,
            pool,
            grants: HashMap::new(),
            took_office: None,
        }
    }

    /// Records that this node has become leader.
    ///
    /// Starts the takeover wait. Called on every membership change so a node
    /// that was already leader is not reset: re-arming the wait on every gossip
    /// round would stall granting indefinitely in a churning cluster.
    pub fn observe_membership(&mut self, view: &MembershipView, now: Instant) {
        let is_leader = view.is_leader();
        match (is_leader, self.took_office) {
            (true, None) => self.took_office = Some(now),
            (false, Some(_)) => {
                // Lost office. The ledger is discarded rather than kept, because
                // its contents describe grants this node can no longer renew,
                // and a stale ledger is worse than an empty one.
                self.took_office = None;
                self.grants.clear();
            }
            _ => {}
        }
    }

    /// Whether this node is currently able to grant.
    #[must_use]
    pub fn can_grant(&self, now: Instant) -> bool {
        self.took_office
            .is_some_and(|at| now >= at + self.config.takeover_wait)
    }

    /// Connections currently leased out, ignoring anything expired.
    #[must_use]
    pub fn outstanding(&self, now: Instant) -> u32 {
        self.grants
            .values()
            .filter(|g| g.expires_at > now)
            .map(|g| g.count)
            .sum()
    }

    /// What remains grantable.
    #[must_use]
    pub fn available(&self, now: Instant) -> u32 {
        self.pool.saturating_sub(self.outstanding(now))
    }

    /// Drops expired grants. Purely housekeeping: [`Self::outstanding`] already
    /// ignores them, so forgetting to call this cannot over-subscribe.
    pub fn reap(&mut self, now: Instant) {
        self.grants.retain(|_, g| g.expires_at > now);
    }

    /// Grants up to `want` connections to `holder`.
    ///
    /// A renewal replaces the holder's existing grant rather than adding to it,
    /// so a node renewing in a loop cannot accumulate.
    ///
    /// # Errors
    ///
    /// Fails when this node cannot grant yet, or the pool has nothing left.
    pub fn grant(
        &mut self,
        server: &ServerId,
        holder: NodeId,
        want: u32,
        now: Instant,
    ) -> Result<QuotaLease, QuotaError> {
        if !self.can_grant(now) {
            return Err(QuotaError::NoLeader);
        }

        // The holder's own live grant is not competing with its renewal.
        let held_by_others: u32 = self
            .grants
            .iter()
            .filter(|(node, g)| **node != holder && g.expires_at > now)
            .map(|(_, g)| g.count)
            .sum();

        let available = self.pool.saturating_sub(held_by_others);
        if available == 0 {
            return Err(QuotaError::Exhausted {
                server: server.clone(),
            });
        }

        let granted = want.min(available);
        let expires_at = now + self.config.ttl;
        self.grants.insert(
            holder,
            Grant {
                holder,
                count: granted,
                expires_at,
            },
        );

        Ok(QuotaLease::new(server.clone(), granted, expires_at))
    }

    /// Returns a holder's grant early.
    pub fn release(&mut self, holder: NodeId) {
        self.grants.remove(&holder);
    }

    /// How many nodes hold live grants.
    #[must_use]
    pub fn holders(&self, now: Instant) -> usize {
        self.grants.values().filter(|g| g.expires_at > now).count()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use pgprox_core::cluster::{Member, NodeMode};

    fn node(n: u16) -> NodeId {
        NodeId::new(n)
    }

    fn server() -> ServerId {
        ServerId::new("db-1", 5432)
    }

    fn view(local: u16, ids: &[u16]) -> MembershipView {
        MembershipView::new(
            node(local),
            ids.iter()
                .map(|id| Member {
                    id: node(*id),
                    mode: NodeMode::Active,
                })
                .collect(),
        )
    }

    /// A ledger whose takeover wait has already elapsed.
    ///
    /// Time comes from a base instant advanced by arithmetic rather than from
    /// the system clock, so nothing here sleeps and a run is reproducible.
    fn serving(pool: u32) -> (LeaseLedger, Instant) {
        let config = LeaseConfig::default();
        let mut ledger = LeaseLedger::new(pool, config);
        let start = Instant::now();
        ledger.observe_membership(&view(1, &[1, 2, 3]), start);
        (ledger, start + config.takeover_wait)
    }

    #[test]
    fn a_new_leader_refuses_to_grant_until_a_full_ttl_has_passed() {
        // The rule that makes failover safe. Its predecessor may have granted
        // leases it never learned about; waiting one TTL guarantees those have
        // either been renewed through this leader or expired.
        let config = LeaseConfig::default();
        let mut ledger = LeaseLedger::new(100, config);
        let start = Instant::now();
        ledger.observe_membership(&view(1, &[1, 2, 3]), start);

        assert!(!ledger.can_grant(start));
        assert_eq!(
            ledger.grant(&server(), node(2), 10, start).unwrap_err(),
            QuotaError::NoLeader
        );

        // One millisecond short.
        let almost = (start + config.takeover_wait)
            .checked_sub(Duration::from_millis(1))
            .unwrap();
        assert!(!ledger.can_grant(almost), "granted before the wait elapsed");

        let ready = start + config.takeover_wait;
        assert!(ledger.can_grant(ready));
        assert!(ledger.grant(&server(), node(2), 10, ready).is_ok());
    }

    #[test]
    fn a_non_leader_never_grants() {
        let mut ledger = LeaseLedger::new(100, LeaseConfig::default());
        let start = Instant::now();
        // Node 3 is not the lowest ID, so it is not the leader.
        ledger.observe_membership(&view(3, &[1, 2, 3]), start);

        let much_later = start + Duration::from_secs(100);
        assert!(!ledger.can_grant(much_later));
        assert_eq!(
            ledger.grant(&server(), node(2), 1, much_later).unwrap_err(),
            QuotaError::NoLeader
        );
    }

    #[test]
    fn staying_leader_does_not_re_arm_the_wait() {
        // Membership churns constantly. Re-arming on every gossip round would
        // stall granting indefinitely.
        let config = LeaseConfig::default();
        let mut ledger = LeaseLedger::new(100, config);
        let start = Instant::now();
        ledger.observe_membership(&view(1, &[1, 2, 3]), start);

        let ready = start + config.takeover_wait;
        for tick in 1..20 {
            ledger.observe_membership(
                &view(1, &[1, 2, 3]),
                start + Duration::from_millis(tick * 100),
            );
        }
        assert!(ledger.can_grant(ready), "the wait was re-armed by churn");
    }

    #[test]
    fn losing_office_discards_the_ledger() {
        // Its contents describe grants this node can no longer renew, and a
        // stale ledger is worse than an empty one: it would make a returning
        // leader believe capacity is taken when it has expired.
        let (mut ledger, now) = serving(100);
        ledger.grant(&server(), node(2), 40, now).unwrap();
        assert_eq!(ledger.outstanding(now), 40);

        // Node 0 appears and takes the leadership.
        ledger.observe_membership(&view(1, &[0, 1, 2, 3]), now);
        assert!(!ledger.can_grant(now));
        assert_eq!(ledger.outstanding(now), 0, "a stale ledger survived");
    }

    #[test]
    fn the_pool_is_never_over_granted() {
        let (mut ledger, now) = serving(10);

        let first = ledger.grant(&server(), node(2), 7, now).unwrap();
        assert_eq!(first.nominal_count(), 7);

        // The second holder gets what is left, not what it asked for.
        let second = ledger.grant(&server(), node(3), 7, now).unwrap();
        assert_eq!(second.nominal_count(), 3, "the pool was over-granted");
        assert_eq!(ledger.outstanding(now), 10);

        assert_eq!(
            ledger.grant(&server(), node(4), 1, now).unwrap_err(),
            QuotaError::Exhausted { server: server() }
        );
    }

    #[test]
    fn a_renewal_replaces_rather_than_accumulates() {
        // A node renewing in a loop must not creep upward.
        let (mut ledger, now) = serving(10);

        for _ in 0..20 {
            ledger.grant(&server(), node(2), 5, now).unwrap();
        }
        assert_eq!(ledger.outstanding(now), 5, "renewals accumulated");
        assert_eq!(ledger.holders(now), 1);
    }

    #[test]
    fn a_renewal_can_take_capacity_freed_by_others() {
        let (mut ledger, now) = serving(10);
        ledger.grant(&server(), node(2), 6, now).unwrap();
        ledger.grant(&server(), node(3), 4, now).unwrap();

        ledger.release(node(3));
        let renewed = ledger.grant(&server(), node(2), 10, now).unwrap();
        assert_eq!(renewed.nominal_count(), 10);
    }

    #[test]
    fn an_unreachable_holders_grant_expires_without_anyone_acting() {
        // Nobody tells the leader a node died. Capacity comes back on its own,
        // which is what makes the whole scheme partition-tolerant.
        let config = LeaseConfig::default();
        let (mut ledger, now) = serving(10);
        ledger.grant(&server(), node(2), 10, now).unwrap();
        assert_eq!(ledger.available(now), 0);

        let after = now + config.ttl + Duration::from_millis(1);
        assert_eq!(ledger.available(after), 10, "capacity never returned");
        assert_eq!(ledger.holders(after), 0);
    }

    #[test]
    fn an_expired_grant_is_not_counted_even_before_reaping() {
        // Reaping is housekeeping. Relying on it would mean a forgotten call
        // over-subscribes the cap.
        let config = LeaseConfig::default();
        let (mut ledger, now) = serving(10);
        ledger.grant(&server(), node(2), 10, now).unwrap();

        let after = now + config.ttl + Duration::from_millis(1);
        assert_eq!(
            ledger.outstanding(after),
            0,
            "an expired grant still counted"
        );

        ledger.reap(after);
        assert_eq!(ledger.outstanding(after), 0);
    }

    #[test]
    fn a_grant_expiring_exactly_now_is_already_gone() {
        // The boundary. Counting it as live for one more instant is how an
        // off-by-one becomes a cap breach.
        let config = LeaseConfig::default();
        let (mut ledger, now) = serving(10);
        ledger.grant(&server(), node(2), 10, now).unwrap();

        let exactly = now + config.ttl;
        assert_eq!(ledger.outstanding(exactly), 0);
    }

    #[test]
    fn a_configuration_whose_wait_is_shorter_than_the_ttl_is_unsafe() {
        // Exposed so config validation refuses it rather than an incident
        // discovering it.
        assert!(LeaseConfig::default().is_safe());
        assert!(
            !LeaseConfig {
                ttl: Duration::from_secs(10),
                takeover_wait: Duration::from_secs(5),
            }
            .is_safe(),
            "a wait shorter than the TTL was reported safe"
        );
        assert!(
            LeaseConfig {
                ttl: Duration::from_secs(5),
                takeover_wait: Duration::from_secs(10),
            }
            .is_safe()
        );
    }

    #[test]
    fn releasing_returns_capacity_immediately() {
        let (mut ledger, now) = serving(10);
        ledger.grant(&server(), node(2), 10, now).unwrap();
        assert_eq!(ledger.available(now), 0);

        ledger.release(node(2));
        assert_eq!(ledger.available(now), 10);
    }

    #[test]
    fn releasing_a_node_that_holds_nothing_is_harmless() {
        let (mut ledger, now) = serving(10);
        ledger.release(node(9));
        assert_eq!(ledger.available(now), 10);
    }

    #[test]
    fn an_empty_pool_grants_nothing() {
        let (mut ledger, now) = serving(0);
        assert_eq!(
            ledger.grant(&server(), node(2), 1, now).unwrap_err(),
            QuotaError::Exhausted { server: server() }
        );
    }
}
