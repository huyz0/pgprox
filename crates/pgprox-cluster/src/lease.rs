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

use pgprox_core::cluster::{QuotaError, QuotaLease};
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

    /// Moves the ceiling this ledger grants against.
    ///
    /// Called every `observe` round with the freshly computed split, not only
    /// at construction: a cap change, or a membership change that moves the
    /// free pool, must reach a ledger already handing out leases against the
    /// old value, or a node whose cap grew stays capped below its own share
    /// and a node whose cap shrank keeps leasing above the new one.
    ///
    /// Safe to move in either direction with nothing outstanding to reconcile.
    /// [`Self::available`] and [`Self::grant`] both compute headroom as
    /// `pool.saturating_sub(outstanding)`, so a pool lowered below what is
    /// already leased out reads as no headroom rather than underflowing, and
    /// nothing already granted is revoked; it simply is not renewed past the
    /// new ceiling.
    pub fn set_pool(&mut self, pool: u32) {
        self.pool = pool;
    }

    /// Records whether this node is currently able to lead.
    ///
    /// Takes a decision rather than a view, because being the lowest active ID
    /// is not sufficient on its own: the caller must also have established that
    /// it can see enough of the fleet to act. A node that was leading a
    /// partitioned minority has not been leading in any sense that matters, and
    /// passing `true` throughout would let it grant the instant the partition
    /// heals, against capacity the majority's leader had already handed out.
    /// Passing `false` while it cannot act makes regaining contact a fresh
    /// takeover, which serves the full wait.
    ///
    /// Starts the takeover wait on a false-to-true transition. Called on every
    /// gossip round, so a node that was already leading is deliberately not
    /// reset: re-arming every round would stall granting forever in a churning
    /// cluster.
    pub fn observe_leadership(&mut self, leading: bool, now: Instant) {
        match (leading, self.took_office) {
            (true, None) => self.took_office = Some(now),
            (false, Some(_)) => {
                // Lost office, or lost the ability to act on it. The ledger is
                // discarded rather than kept, because its contents describe
                // grants this node can no longer renew, and a stale ledger is
                // worse than an empty one.
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

    /// Entries the ledger is still carrying, live or expired.
    ///
    /// Test-only, and it exists because of a surviving mutant. `reap` is
    /// housekeeping: every other reader filters expired grants out, so
    /// replacing its body with `()` changes no answer this type gives and no
    /// test could tell. What it does change is that a long-lived leader carries
    /// every grant it ever made. That is a real property with no observer, so
    /// `M14.1` added one here rather than accepting the mutant as equivalent.
    #[cfg(test)]
    fn tracked(&self) -> usize {
        self.grants.len()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn node(n: u16) -> NodeId {
        NodeId::new(n)
    }

    fn server() -> ServerId {
        ServerId::new("db-1", 5432)
    }

    /// A ledger whose takeover wait has already elapsed.
    ///
    /// Time comes from a base instant advanced by arithmetic rather than from
    /// the system clock, so nothing here sleeps and a run is reproducible.
    fn serving(pool: u32) -> (LeaseLedger, Instant) {
        let config = LeaseConfig::default();
        let mut ledger = LeaseLedger::new(pool, config);
        let start = Instant::now();
        ledger.observe_leadership(true, start);
        (ledger, start + config.takeover_wait)
    }

    /// The four mutants `M14.1` found in this file, and one shape behind three
    /// of them: expiry is exclusive. A grant is live while `expires_at > now`,
    /// so at exactly `expires_at` it is already gone. Every reader was written
    /// that way and nothing pinned it, so `>` could become `>=` in `grant`,
    /// `holders` and `reap` with all 156 tests in this crate still passing.
    ///
    /// The instant matters because it is reachable. Grants expire at
    /// `now + ttl`, and a caller that computes its next deadline the same way
    /// lands exactly on it rather than near it.
    #[test]
    fn a_grant_is_gone_at_the_instant_it_expires_and_not_a_tick_later() {
        let (mut ledger, now) = serving(100);
        let ttl = LeaseConfig::default().ttl;

        let lease = ledger.grant(&server(), node(2), 40, now).unwrap();
        let expiry = now + ttl;
        assert_eq!(lease.expires_at(), expiry);

        // One tick before: still held, by every reader.
        let before = expiry.checked_sub(Duration::from_nanos(1)).unwrap();
        assert_eq!(ledger.holders(before), 1);
        assert_eq!(ledger.outstanding(before), 40);

        // At the instant itself: gone, by every reader. `>=` in any of them
        // would keep it alive here.
        assert_eq!(ledger.holders(expiry), 0);
        assert_eq!(ledger.outstanding(expiry), 0);
        assert_eq!(ledger.available(expiry), 100);
    }

    #[test]
    fn set_pool_moves_the_ceiling_in_either_direction() {
        let (mut ledger, now) = serving(100);
        assert_eq!(ledger.available(now), 100);

        ledger.set_pool(200);
        assert_eq!(
            ledger.available(now),
            200,
            "a raised pool did not reach a ledger already serving"
        );

        ledger.set_pool(50);
        assert_eq!(
            ledger.available(now),
            50,
            "a lowered pool did not reach a ledger already serving"
        );
    }

    #[test]
    fn set_pool_below_what_is_already_leased_reads_as_no_headroom_rather_than_underflowing() {
        // `available` and `grant` both compute `pool.saturating_sub(...)`.
        // Moving the ceiling below outstanding grants must read as zero, not
        // wrap around a `u32` subtraction into something enormous.
        let (mut ledger, now) = serving(100);
        ledger.grant(&server(), node(2), 80, now).unwrap();

        ledger.set_pool(50);
        assert_eq!(
            ledger.available(now),
            0,
            "a pool lowered below what is outstanding underflowed instead of reading empty"
        );
        assert_eq!(
            ledger.grant(&server(), node(3), 10, now).unwrap_err(),
            QuotaError::Exhausted { server: server() }
        );

        // The holder already granted is not revoked; it simply is not renewed
        // past the new ceiling once it expires.
        assert_eq!(ledger.outstanding(now), 80);
    }

    #[test]
    fn a_holder_at_its_expiry_instant_no_longer_competes_for_the_pool() {
        // The same boundary seen from `grant`, which sums what others hold to
        // decide what is left. A grant expiring exactly now must not count, or
        // the pool looks smaller than it is and a node is refused capacity that
        // is free.
        let (mut ledger, now) = serving(100);
        let ttl = LeaseConfig::default().ttl;

        ledger.grant(&server(), node(2), 100, now).unwrap();
        let expiry = now + ttl;

        // The whole pool is held right up to the instant.
        let before = expiry.checked_sub(Duration::from_nanos(1)).unwrap();
        assert_eq!(
            ledger.grant(&server(), node(3), 10, before).unwrap_err(),
            QuotaError::Exhausted { server: server() }
        );

        // And free at it.
        let lease = ledger.grant(&server(), node(3), 100, expiry).unwrap();
        assert_eq!(lease.count(expiry), 100);
    }

    #[test]
    fn reap_drops_what_it_is_meant_to_and_keeps_what_it_is_not() {
        // `reap` is the one function here whose effect no other reader exposes,
        // which is why two of its mutants survived: replacing its body with
        // `()`, and moving its boundary to `>=`. Both leave every answer this
        // type gives unchanged and let the map grow instead.
        let (mut ledger, now) = serving(100);
        let ttl = LeaseConfig::default().ttl;

        ledger.grant(&server(), node(2), 10, now).unwrap();
        ledger.grant(&server(), node(3), 10, now).unwrap();
        assert_eq!(ledger.tracked(), 2);

        // A third, granted a tick later, so it expires a tick later too.
        let stagger = Duration::from_nanos(1);
        ledger.grant(&server(), node(4), 10, now + stagger).unwrap();
        assert_eq!(ledger.tracked(), 3);

        // At the first two's expiry instant they are gone and the third is not.
        // `()` keeps all three; `>=` keeps the two that expire exactly now.
        let expiry = now + ttl;
        ledger.reap(expiry);
        assert_eq!(ledger.tracked(), 1);
        assert_eq!(ledger.holders(expiry), 1);

        // And once the last one goes, nothing is carried at all.
        ledger.reap(expiry + stagger);
        assert_eq!(ledger.tracked(), 0);
    }

    #[test]
    fn a_new_leader_refuses_to_grant_until_a_full_ttl_has_passed() {
        // The rule that makes failover safe. Its predecessor may have granted
        // leases it never learned about; waiting one TTL guarantees those have
        // either been renewed through this leader or expired.
        let config = LeaseConfig::default();
        let mut ledger = LeaseLedger::new(100, config);
        let start = Instant::now();
        ledger.observe_leadership(true, start);

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
        ledger.observe_leadership(false, start);

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
        ledger.observe_leadership(true, start);

        let ready = start + config.takeover_wait;
        for tick in 1..20 {
            ledger.observe_leadership(true, start + Duration::from_millis(tick * 100));
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
        ledger.observe_leadership(false, now);
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
