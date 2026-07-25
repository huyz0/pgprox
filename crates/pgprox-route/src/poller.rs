//! Keeping [`Replicas`] up to date.
//!
//! # Where the I/O is
//!
//! Behind [`ReplicaProbe`]. Asking a replica how far it has replayed means
//! running `pg_last_wal_replay_lsn()` and `pg_is_in_recovery()` on it, which
//! needs a connection this crate has no business owning. The trait keeps the
//! question here and the socket in the composition root, and lets every rule
//! below be tested against a probe that fails, lies or hangs on demand.
//!
//! # The router never waits for this
//!
//! Polling happens on its own schedule and writes into shared state. The route
//! decision reads that state with no await and no I/O, because it is taken once
//! per transaction on every connection and a route decision that could block
//! would make every replica's latency everyone's latency.
//!
//! # Failure is information
//!
//! A probe that fails clears the replica's reading rather than leaving the last
//! one in place. A replica that has stopped answering has an unknown position,
//! and its last known one is the most misleading thing available. Silence is
//! handled the same way by the freshness window in [`Replicas`], so a poller
//! that dies entirely takes every replica out of service rather than freezing
//! the fleet's view at the moment it stopped.

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use pgprox_core::clock::Clock;
use pgprox_core::ids::Lsn;
use pgprox_core::route::ReplicaState;

use crate::replica::{ReplicaConfig, Replicas};

/// What a replica reported about itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Probe {
    /// How far it has replayed, from `pg_last_wal_replay_lsn()`.
    pub replayed: Lsn,
    /// Whether it is still a replica, from `pg_is_in_recovery()`.
    pub in_recovery: bool,
}

/// Asks a replica where it has got to.
#[async_trait::async_trait]
pub trait ReplicaProbe: Send + Sync + fmt::Debug {
    /// Probes the replica at `index` in the grant's replica list.
    ///
    /// # Errors
    ///
    /// Fails when the replica is unreachable, refuses, or does not answer in
    /// time. The caller treats every failure the same way, so the error type is
    /// only for the operator.
    async fn probe(&self, index: usize) -> Result<Probe, String>;
}

/// Shared replica state, written by the poller and read by the router.
#[derive(Debug)]
pub struct ReplicaWatch {
    replicas: Mutex<Replicas>,
    clock: Arc<dyn Clock>,
}

impl ReplicaWatch {
    /// A watch over `count` replicas, none of them polled yet.
    #[must_use]
    pub fn new(count: usize, config: ReplicaConfig, clock: Arc<dyn Clock>) -> Arc<Self> {
        Arc::new(Self {
            replicas: Mutex::new(Replicas::new(count, config)),
            clock,
        })
    }

    /// How many replicas are watched.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Whether there are no replicas.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// The current state of every replica, for the router.
    ///
    /// Takes the lock and releases it. Never held across an await, and never
    /// held while the router is deciding.
    #[must_use]
    pub fn states(&self) -> Vec<ReplicaState> {
        let now = self.clock.now();
        self.lock().states(now)
    }

    /// A snapshot of the tracker itself, for a caller that wants to route
    /// against a fixed view rather than re-reading per statement.
    #[must_use]
    pub fn snapshot(&self) -> Replicas {
        self.lock().clone()
    }

    /// Lag behind a known primary position, for `pgprox_replica_lag_bytes`.
    #[must_use]
    pub fn lag_behind(&self, index: usize, primary: Lsn) -> Option<u64> {
        let now = self.clock.now();
        self.lock().lag_behind(index, primary, now)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Replicas> {
        self.replicas.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records one probe result.
    fn record(&self, index: usize, result: &Result<Probe, String>, at: Instant) {
        let mut replicas = self.lock();
        match result {
            Ok(probe) => replicas.observe(index, probe.replayed, probe.in_recovery, at),
            // Cleared rather than left in place: a replica that has stopped
            // answering has an unknown position, and its last known one is the
            // most misleading thing available.
            Err(_) => replicas.observe_failure(index),
        }
    }

    /// Probes every replica once and records the answers.
    ///
    /// Concurrently, because one unreachable replica must not delay the
    /// readings for the healthy ones. A replica that hangs until its connect
    /// timeout would otherwise stall the whole round, and at a freshness window
    /// of a second that takes every replica out of service, turning one sick
    /// replica into no replicas at all.
    ///
    /// Each result is stamped when it arrives rather than when the round began,
    /// so a slow answer is correctly seen as the older reading.
    ///
    /// Returns how many replicas answered.
    pub async fn poll_once<P>(self: &Arc<Self>, probe: &Arc<P>) -> usize
    where
        P: ReplicaProbe + 'static,
    {
        let handles: Vec<_> = (0..self.len())
            .map(|index| {
                let probe = Arc::clone(probe);
                tokio::spawn(async move { (index, probe.probe(index).await) })
            })
            .collect();

        let mut answered = 0;
        for handle in handles {
            // A probe that panicked is a probe that did not answer. Treating it
            // as a failure keeps one broken implementation from taking the
            // poller down with it.
            let Ok((index, result)) = handle.await else {
                continue;
            };
            if result.is_ok() {
                answered += 1;
            }
            self.record(index, &result, self.clock.now());
        }
        answered
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use pgprox_core::clock::FakeClock;
    use pgprox_core::route::{RouteCtx, RouteTarget, StmtClass, decide};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    /// A probe that answers from a script and counts calls.
    #[derive(Debug)]
    struct FakeProbe {
        /// Per replica: replayed LSN, or `None` to fail.
        answers: Mutex<Vec<Option<(u64, bool)>>>,
        calls: AtomicU32,
    }

    impl FakeProbe {
        fn new(answers: Vec<Option<(u64, bool)>>) -> Self {
            Self {
                answers: Mutex::new(answers),
                calls: AtomicU32::new(0),
            }
        }

        fn set(&self, index: usize, answer: Option<(u64, bool)>) {
            self.answers.lock().unwrap()[index] = answer;
        }

        fn calls(&self) -> u32 {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl ReplicaProbe for FakeProbe {
        async fn probe(&self, index: usize) -> Result<Probe, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.answers.lock().unwrap().get(index).copied().flatten() {
                Some((replayed, in_recovery)) => Ok(Probe {
                    replayed: Lsn::new(replayed),
                    in_recovery,
                }),
                None => Err("unreachable".to_owned()),
            }
        }
    }

    fn watch(count: usize) -> (Arc<ReplicaWatch>, FakeClock) {
        let clock = FakeClock::new();
        let watch = ReplicaWatch::new(count, ReplicaConfig::default(), Arc::new(clock.clone()));
        (watch, clock)
    }

    fn read_only(watermark: Option<Lsn>) -> RouteCtx {
        RouteCtx {
            class: StmtClass::ReadOnly,
            watermark,
            ..RouteCtx::default()
        }
    }

    #[tokio::test]
    async fn a_poll_records_what_each_replica_reported() {
        let (watch, _clock) = watch(2);
        let probe = Arc::new(FakeProbe::new(vec![Some((500, true)), Some((400, true))]));

        assert_eq!(watch.poll_once(&probe).await, 2);
        let states = watch.states();
        assert!(states[0].healthy);
        assert_eq!(states[0].replayed, Lsn::new(500));
        assert_eq!(states[1].replayed, Lsn::new(400));
    }

    #[tokio::test]
    async fn every_replica_is_probed_once_per_round() {
        let (watch, _clock) = watch(3);
        let probe = Arc::new(FakeProbe::new(vec![Some((1, true)); 3]));

        watch.poll_once(&probe).await;
        assert_eq!(probe.calls(), 3);
        watch.poll_once(&probe).await;
        assert_eq!(probe.calls(), 6);
    }

    #[tokio::test]
    async fn a_failing_replica_does_not_stop_the_others_being_read() {
        // One unreachable replica must not cost the healthy ones their
        // readings, which is why the round probes concurrently rather than in
        // sequence.
        let (watch, _clock) = watch(3);
        let probe = Arc::new(FakeProbe::new(vec![
            Some((500, true)),
            None,
            Some((600, true)),
        ]));

        assert_eq!(watch.poll_once(&probe).await, 2);
        let states = watch.states();
        assert!(states[0].healthy);
        assert!(!states[1].healthy);
        assert!(states[2].healthy);
    }

    #[tokio::test]
    async fn a_failed_probe_clears_the_reading_rather_than_keeping_it() {
        // Once a replica stops answering its last known position is the most
        // misleading thing available.
        let (watch, _clock) = watch(1);
        let probe = Arc::new(FakeProbe::new(vec![Some((500, true))]));
        watch.poll_once(&probe).await;
        assert!(watch.states()[0].healthy);

        probe.set(0, None);
        watch.poll_once(&probe).await;
        assert!(!watch.states()[0].healthy);
        assert_eq!(
            watch.states()[0].replayed,
            Lsn::new(0),
            "a failed probe left a stale position behind"
        );
    }

    #[tokio::test]
    async fn a_promoted_replica_stops_taking_reads() {
        let (watch, _clock) = watch(1);
        let probe = Arc::new(FakeProbe::new(vec![Some((500, true))]));
        watch.poll_once(&probe).await;
        assert!(watch.states()[0].healthy);

        // pg_is_in_recovery() goes false: it is a primary now.
        probe.set(0, Some((500, false)));
        watch.poll_once(&probe).await;
        assert!(
            !watch.states()[0].healthy,
            "a promoted replica kept serving reads"
        );
    }

    #[tokio::test]
    async fn a_poller_that_stops_takes_the_replicas_out_of_service() {
        // Rather than freezing the fleet's view at the moment it died, which
        // would keep routing reads at a position nobody is confirming.
        let config = ReplicaConfig::default();
        let (watch, clock) = watch(1);
        let probe = Arc::new(FakeProbe::new(vec![Some((500, true))]));
        watch.poll_once(&probe).await;
        assert!(watch.states()[0].healthy);

        clock.advance(config.freshness + Duration::from_millis(1));
        assert!(
            !watch.states()[0].healthy,
            "a dead poller left its last readings serving traffic"
        );
    }

    #[tokio::test]
    async fn the_router_reads_the_watch_without_waiting() {
        // The whole point of the split. A route decision is taken once per
        // transaction on every connection, and one that could block would make
        // every replica's latency everyone's latency.
        let (watch, _clock) = watch(1);
        let probe = Arc::new(FakeProbe::new(vec![Some((500, true))]));
        watch.poll_once(&probe).await;

        assert_eq!(
            decide(&read_only(Some(Lsn::new(500))), &watch.states()),
            RouteTarget::Replica(0)
        );
        assert_eq!(
            decide(&read_only(Some(Lsn::new(501))), &watch.states()),
            RouteTarget::Primary,
            "a session read from a replica behind its own write"
        );
    }

    #[tokio::test]
    async fn a_watch_with_no_replicas_polls_nothing() {
        let (watch, _clock) = watch(0);
        let probe = Arc::new(FakeProbe::new(Vec::new()));

        assert!(watch.is_empty());
        assert_eq!(watch.poll_once(&probe).await, 0);
        assert_eq!(probe.calls(), 0);
        assert!(watch.states().is_empty());
    }

    #[tokio::test]
    async fn a_snapshot_routes_against_a_fixed_view() {
        // For a caller that wants every statement of a transaction judged
        // against the same readings rather than a moving target.
        let (watch, _clock) = watch(1);
        let probe = Arc::new(FakeProbe::new(vec![Some((500, true))]));
        watch.poll_once(&probe).await;

        let snapshot = watch.snapshot();
        assert_eq!(watch.len(), 1);

        probe.set(0, None);
        watch.poll_once(&probe).await;
        assert!(!watch.states()[0].healthy, "the live view did not move");
        assert!(
            snapshot.state(0, Instant::now()).healthy,
            "the snapshot moved under its holder"
        );
    }

    #[tokio::test]
    async fn lag_is_reported_from_the_watch() {
        let (watch, _clock) = watch(2);
        let probe = Arc::new(FakeProbe::new(vec![Some((900, true)), None]));
        watch.poll_once(&probe).await;

        assert_eq!(watch.lag_behind(0, Lsn::new(1_000)), Some(100));
        assert_eq!(
            watch.lag_behind(1, Lsn::new(1_000)),
            None,
            "lag was invented for a replica that never answered"
        );
    }
}
