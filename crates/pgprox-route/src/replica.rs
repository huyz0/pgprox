//! How far each replica has replayed, and how far each session needs it to.
//!
//! # Read-your-writes
//!
//! A session that has written records an LSN floor. A replica may serve that
//! session only once it has replayed at least that far. Without the floor, a
//! client writes a row, reads it back from a replica that has not caught up
//! yet, and sees its own write missing. That is the failure this whole module
//! exists to prevent, and it is worse than the latency it was meant to save.
//!
//! # Why the router never awaits
//!
//! A background poller asks each replica for `pg_last_wal_replay_lsn()` and
//! `pg_is_in_recovery()` every few hundred milliseconds and writes the answers
//! here. The router reads them with no lock held across an await and no I/O of
//! its own, because the route decision is a declared hot path taken once per
//! transaction on every connection.
//!
//! The I/O lives in the caller. This module is the state it writes into and the
//! rules for reading it, which is what makes both testable without a socket.
//!
//! # Staleness of the staleness data
//!
//! A poll result is itself out of date the moment it is taken, so a replica
//! that has not been heard from recently is treated as ineligible rather than
//! as last seen. Trusting an old reading is how a replica that fell over
//! quietly keeps serving reads.

use std::time::{Duration, Instant};

use pgprox_core::ids::Lsn;
use pgprox_core::route::ReplicaState;

/// How stale a poll result may be before its replica stops being eligible.
#[derive(Clone, Copy, Debug)]
pub struct ReplicaConfig {
    /// How long a poll result stays usable.
    ///
    /// Should be a small multiple of the poll interval, so a single missed poll
    /// does not take a replica out of service but a stopped poller does.
    pub freshness: Duration,
}

impl Default for ReplicaConfig {
    fn default() -> Self {
        // Four times the 250ms poll interval in ADR 0009: three consecutive
        // missed polls before a replica is set aside.
        Self {
            freshness: Duration::from_secs(1),
        }
    }
}

/// The last thing a poller learned about one replica.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Observation {
    replayed: Lsn,
    /// Whether it answered and is still in recovery.
    ///
    /// A replica that has been promoted is no longer a replica: it is a second
    /// primary, and sending reads to it is how a split brain starts serving
    /// two versions of the truth.
    in_recovery: bool,
    at: Instant,
}

/// What every replica has replayed, as the poller last saw it.
///
/// Positions are fixed: index `n` here is index `n` in the grant's replica
/// list, which is what [`pgprox_core::route::RouteTarget::Replica`] names. A
/// replica never polled yet occupies its slot as unhealthy rather than being
/// absent, so the indices cannot shift under a caller.
#[derive(Clone, Debug)]
pub struct Replicas {
    config: ReplicaConfig,
    observations: Vec<Option<Observation>>,
}

impl Replicas {
    /// Tracking for `count` replicas, none of them yet polled.
    #[must_use]
    pub fn new(count: usize, config: ReplicaConfig) -> Self {
        Self {
            config,
            observations: vec![None; count],
        }
    }

    /// How many replicas are tracked.
    #[must_use]
    pub fn len(&self) -> usize {
        self.observations.len()
    }

    /// Whether there are no replicas at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.observations.is_empty()
    }

    /// Records a poll result.
    ///
    /// Out-of-range indices are ignored rather than panicking: the replica list
    /// comes from a grant and can change under a poller mid-flight, and a
    /// racing poll must not take the process down.
    pub fn observe(&mut self, index: usize, replayed: Lsn, in_recovery: bool, at: Instant) {
        if let Some(slot) = self.observations.get_mut(index) {
            *slot = Some(Observation {
                replayed,
                in_recovery,
                at,
            });
        }
    }

    /// Records that a replica did not answer.
    ///
    /// Clears the reading rather than keeping the old one. A replica that has
    /// stopped answering has an unknown position, and the last known one is the
    /// most misleading thing available.
    pub fn observe_failure(&mut self, index: usize) {
        if let Some(slot) = self.observations.get_mut(index) {
            *slot = None;
        }
    }

    /// The state of every replica, in the order the router expects.
    ///
    /// Anything unpolled, stale, promoted or failed reports as unhealthy, so a
    /// caller cannot accidentally treat missing data as a healthy replica at
    /// LSN zero.
    ///
    /// Allocates. On the route decision, which is a declared hot path taken per
    /// statement for autocommit workloads, use [`Self::fill_states`] with a
    /// buffer the caller keeps.
    #[must_use]
    pub fn states(&self, now: Instant) -> Vec<ReplicaState> {
        let mut out = Vec::with_capacity(self.observations.len());
        self.fill_states(&mut out, now);
        out
    }

    /// Writes the state of every replica into `out`, replacing its contents.
    ///
    /// The allocation-free form. A router holding one buffer for the life of a
    /// session pays for it once rather than once per statement, which is what
    /// the hot-path budget for the route decision is about.
    pub fn fill_states(&self, out: &mut Vec<ReplicaState>, now: Instant) {
        out.clear();
        out.extend((0..self.observations.len()).map(|index| self.state(index, now)));
    }

    /// The state of one replica.
    #[must_use]
    pub fn state(&self, index: usize, now: Instant) -> ReplicaState {
        let unhealthy = ReplicaState {
            replayed: Lsn::new(0),
            healthy: false,
        };

        let Some(Some(observation)) = self.observations.get(index) else {
            return unhealthy;
        };
        if !observation.in_recovery {
            // Promoted, so no longer a replica.
            return unhealthy;
        }
        // `saturating_duration_since` because a reading stamped in the future
        // is fresh, not impossibly old.
        if now.saturating_duration_since(observation.at) > self.config.freshness {
            return unhealthy;
        }

        ReplicaState {
            replayed: observation.replayed,
            healthy: true,
        }
    }

    /// How far behind the furthest-ahead replica a replica is, for
    /// `pgprox_replica_lag_bytes`.
    ///
    /// [`None`] when either end is unknown, since a lag figure invented from
    /// missing data would read as healthy.
    #[must_use]
    pub fn lag_behind(&self, index: usize, primary: Lsn, now: Instant) -> Option<u64> {
        let state = self.state(index, now);
        state
            .healthy
            .then(|| primary.get().saturating_sub(state.replayed.get()))
    }
}

/// A session's read-your-writes floor.
///
/// Only ever moves forward. A watermark that went backwards would let a session
/// read from a replica it had already outrun, which is the bug this type exists
/// to make impossible rather than merely unlikely.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Watermark(Option<Lsn>);

impl Watermark {
    /// A session that has not written.
    #[must_use]
    pub const fn new() -> Self {
        Self(None)
    }

    /// The floor, or [`None`] if the session has never written.
    #[must_use]
    pub const fn get(self) -> Option<Lsn> {
        self.0
    }

    /// Whether this session has written at all.
    #[must_use]
    pub const fn is_set(self) -> bool {
        self.0.is_some()
    }

    /// Records the LSN a write committed at.
    ///
    /// Monotonic. An out-of-order or replayed report cannot lower the floor,
    /// which matters because the commit LSN arrives on a separate round trip
    /// and nothing guarantees the order two of them are processed in.
    pub fn advance(&mut self, lsn: Lsn) {
        self.0 = Some(match self.0 {
            Some(current) if current >= lsn => current,
            _ => lsn,
        });
    }

    /// Clears the floor, as ending a session does.
    ///
    /// Not called at the end of a transaction: the whole point is that it
    /// outlives one, so a session's later reads still see its earlier writes.
    pub const fn reset(&mut self) {
        self.0 = None;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use pgprox_core::route::{RouteCtx, RouteTarget, StmtClass, decide};

    fn config() -> ReplicaConfig {
        ReplicaConfig::default()
    }

    fn read_only(watermark: Option<Lsn>) -> RouteCtx {
        RouteCtx {
            class: StmtClass::ReadOnly,
            watermark,
            ..RouteCtx::default()
        }
    }

    #[test]
    fn an_unpolled_replica_is_unhealthy_rather_than_at_zero() {
        // The dangerous default. A replica reported healthy at LSN 0 would
        // serve every session that has never written.
        let replicas = Replicas::new(2, config());
        let now = Instant::now();
        assert_eq!(replicas.len(), 2);
        assert!(!replicas.is_empty());
        for state in replicas.states(now) {
            assert!(!state.healthy, "an unpolled replica reported healthy");
        }
        assert_eq!(
            decide(&read_only(None), &replicas.states(now)),
            RouteTarget::Primary
        );
    }

    #[test]
    fn no_replicas_at_all_is_not_an_error() {
        let replicas = Replicas::new(0, config());
        assert!(replicas.is_empty());
        assert!(replicas.states(Instant::now()).is_empty());
    }

    #[test]
    fn a_freshly_polled_replica_in_recovery_is_healthy() {
        let now = Instant::now();
        let mut replicas = Replicas::new(1, config());
        replicas.observe(0, Lsn::new(500), true, now);

        let state = replicas.state(0, now);
        assert!(state.healthy);
        assert_eq!(state.replayed, Lsn::new(500));
    }

    #[test]
    fn a_stale_reading_stops_being_trusted() {
        // A poll result is out of date the moment it is taken. Trusting an old
        // one is how a replica that fell over quietly keeps serving reads.
        let config = config();
        let start = Instant::now();
        let mut replicas = Replicas::new(1, config);
        replicas.observe(0, Lsn::new(500), true, start);

        assert!(replicas.state(0, start + config.freshness).healthy);
        assert!(
            !replicas
                .state(0, start + config.freshness + Duration::from_millis(1))
                .healthy,
            "a stale reading was still trusted"
        );
    }

    #[test]
    fn a_promoted_replica_stops_being_a_replica() {
        // It is a second primary now. Sending reads to it is how a split brain
        // starts serving two versions of the truth.
        let now = Instant::now();
        let mut replicas = Replicas::new(1, config());
        replicas.observe(0, Lsn::new(9_999), false, now);
        assert!(
            !replicas.state(0, now).healthy,
            "a promoted replica still took reads"
        );
    }

    #[test]
    fn a_failed_poll_clears_the_reading_rather_than_keeping_it() {
        // The last known position is the most misleading thing available once
        // a replica stops answering.
        let now = Instant::now();
        let mut replicas = Replicas::new(1, config());
        replicas.observe(0, Lsn::new(500), true, now);
        assert!(replicas.state(0, now).healthy);

        replicas.observe_failure(0);
        assert!(!replicas.state(0, now).healthy);
        assert_eq!(replicas.state(0, now).replayed, Lsn::new(0));
    }

    #[test]
    fn an_out_of_range_index_is_ignored_rather_than_panicking() {
        // The replica list comes from a grant and can change under a poller
        // mid-flight. A racing poll must not take the process down.
        let now = Instant::now();
        let mut replicas = Replicas::new(1, config());
        replicas.observe(5, Lsn::new(500), true, now);
        replicas.observe_failure(5);
        assert_eq!(replicas.len(), 1);
        assert!(!replicas.state(5, now).healthy);
    }

    #[test]
    fn positions_do_not_shift_when_a_replica_is_unhealthy() {
        // Index n here is index n in the grant's replica list, which is what
        // RouteTarget::Replica names. Compacting the list would silently
        // redirect reads to a different server.
        let now = Instant::now();
        let mut replicas = Replicas::new(3, config());
        replicas.observe(2, Lsn::new(500), true, now);

        let states = replicas.states(now);
        assert_eq!(states.len(), 3);
        assert!(!states[0].healthy);
        assert!(!states[1].healthy);
        assert!(states[2].healthy);
        assert_eq!(
            decide(&read_only(None), &states),
            RouteTarget::Replica(2),
            "the healthy replica was found at the wrong index"
        );
    }

    #[test]
    fn a_replica_behind_the_watermark_is_never_chosen() {
        // The property this module exists for. A session reads its own write
        // back, or it reads from the primary; it never reads a version of the
        // world from before its own commit.
        let now = Instant::now();
        let mut replicas = Replicas::new(1, config());
        let mut watermark = Watermark::new();
        watermark.advance(Lsn::new(500));

        replicas.observe(0, Lsn::new(499), true, now);
        assert_eq!(
            decide(&read_only(watermark.get()), &replicas.states(now)),
            RouteTarget::Primary,
            "a session read from a replica behind its own write"
        );

        replicas.observe(0, Lsn::new(500), true, now);
        assert_eq!(
            decide(&read_only(watermark.get()), &replicas.states(now)),
            RouteTarget::Replica(0),
            "a caught-up replica was refused"
        );
    }

    #[test]
    fn a_session_that_has_never_written_accepts_any_healthy_replica() {
        let now = Instant::now();
        let mut replicas = Replicas::new(1, config());
        replicas.observe(0, Lsn::new(0), true, now);

        let watermark = Watermark::new();
        assert!(!watermark.is_set());
        assert_eq!(watermark.get(), None);
        assert_eq!(
            decide(&read_only(watermark.get()), &replicas.states(now)),
            RouteTarget::Replica(0)
        );
    }

    #[test]
    fn a_watermark_only_moves_forward() {
        // The commit LSN arrives on a separate round trip and nothing
        // guarantees the order two of them are processed in. A watermark that
        // went backwards would let a session read from a replica it had
        // already outrun.
        let mut watermark = Watermark::new();
        watermark.advance(Lsn::new(500));
        watermark.advance(Lsn::new(400));
        assert_eq!(watermark.get(), Some(Lsn::new(500)), "the floor dropped");

        watermark.advance(Lsn::new(500));
        assert_eq!(watermark.get(), Some(Lsn::new(500)));

        watermark.advance(Lsn::new(600));
        assert_eq!(watermark.get(), Some(Lsn::new(600)));
    }

    #[test]
    fn a_watermark_outlives_a_transaction_and_ends_with_the_session() {
        // Clearing it per transaction would break read-your-writes for
        // everything after the transaction that wrote.
        let mut watermark = Watermark::new();
        watermark.advance(Lsn::new(500));
        assert!(watermark.is_set());

        watermark.reset();
        assert!(!watermark.is_set());
        assert_eq!(Watermark::default(), Watermark::new());
    }

    #[test]
    fn filling_a_buffer_matches_allocating_a_fresh_one() {
        // The allocation-free form is on the route decision's hot path, so it
        // must not be a second implementation that can drift from the first.
        let now = Instant::now();
        let mut replicas = Replicas::new(3, config());
        replicas.observe(0, Lsn::new(500), true, now);
        replicas.observe(2, Lsn::new(700), false, now);

        let mut buffer = vec![
            ReplicaState {
                replayed: Lsn::new(9_999),
                healthy: true,
            };
            9
        ];
        replicas.fill_states(&mut buffer, now);
        assert_eq!(
            buffer,
            replicas.states(now),
            "the buffered form disagreed with the allocating one"
        );
        assert_eq!(buffer.len(), 3, "the buffer kept its previous contents");
    }

    #[test]
    fn lag_is_reported_only_when_it_is_known() {
        // A lag figure invented from missing data would read as healthy.
        let now = Instant::now();
        let mut replicas = Replicas::new(2, config());
        replicas.observe(0, Lsn::new(900), true, now);

        assert_eq!(replicas.lag_behind(0, Lsn::new(1_000), now), Some(100));
        assert_eq!(
            replicas.lag_behind(1, Lsn::new(1_000), now),
            None,
            "lag was reported for a replica that has never answered"
        );
        assert_eq!(replicas.lag_behind(9, Lsn::new(1_000), now), None);
    }

    #[test]
    fn a_replica_ahead_of_the_primary_reading_reports_no_lag() {
        // The two readings are taken at different moments, so the replica's can
        // legitimately be the newer one. Saturating means that reads as caught
        // up rather than as an enormous negative lag.
        let now = Instant::now();
        let mut replicas = Replicas::new(1, config());
        replicas.observe(0, Lsn::new(1_100), true, now);
        assert_eq!(replicas.lag_behind(0, Lsn::new(1_000), now), Some(0));
    }

    #[test]
    fn a_reading_stamped_in_the_future_is_fresh_rather_than_impossibly_old() {
        let now = Instant::now();
        let mut replicas = Replicas::new(1, config());
        replicas.observe(0, Lsn::new(500), true, now + Duration::from_secs(60));
        assert!(replicas.state(0, now).healthy);
    }
}
