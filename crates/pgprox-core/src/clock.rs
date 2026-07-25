//! Time, injected.
//!
//! Nothing in the workspace calls [`std::time::Instant::now`] directly. Taking
//! a [`Clock`] instead is what lets the cluster simulation advance a five
//! second lease TTL in microseconds, and what stops a two minute test suite
//! becoming a twenty minute one, one sleeping test at a time.
//!
//! Two readings, because they answer different questions. [`Clock::now`] is
//! monotonic and is for deadlines and elapsed time. [`Clock::wall`] is the
//! system clock and is only for comparing against a token's expiry claim,
//! which is stated in wall time by whoever issued it.

use std::fmt;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

#[cfg(any(test, feature = "test-fakes"))]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(any(test, feature = "test-fakes"))]
use std::time::Duration;

/// A source of time.
///
/// Object safe, so it can be held as `Arc<dyn Clock>`.
pub trait Clock: Send + Sync + fmt::Debug {
    /// A monotonic instant. Use for deadlines, timeouts, and elapsed time.
    fn now(&self) -> Instant;

    /// Wall clock time. Use only for comparing against externally issued
    /// timestamps such as a JWT `exp` claim. It can jump backwards.
    fn wall(&self) -> SystemTime;
}

impl<T: Clock + ?Sized> Clock for Arc<T> {
    fn now(&self) -> Instant {
        (**self).now()
    }

    fn wall(&self) -> SystemTime {
        (**self).wall()
    }
}

/// The real clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn wall(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// A clock that moves only when told to.
///
/// Shared handles observe the same time, so a test can hold one and hand
/// another to the code under test.
#[cfg(any(test, feature = "test-fakes"))]
#[derive(Clone, Debug)]
pub struct FakeClock {
    base: Instant,
    wall_base: SystemTime,
    /// Nanoseconds elapsed since construction. Shared across clones.
    offset_nanos: Arc<AtomicU64>,
}

#[cfg(any(test, feature = "test-fakes"))]
impl FakeClock {
    /// A clock starting now and frozen there.
    #[must_use]
    pub fn new() -> Self {
        Self {
            base: Instant::now(),
            wall_base: SystemTime::now(),
            offset_nanos: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Moves time forward.
    ///
    /// Saturates rather than wrapping, so an absurd advance in a property test
    /// cannot silently send the clock backwards.
    pub fn advance(&self, by: Duration) {
        let add = u64::try_from(by.as_nanos()).unwrap_or(u64::MAX);
        self.offset_nanos
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                Some(current.saturating_add(add))
            })
            .ok();
    }

    /// How far this clock has been advanced since construction.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        Duration::from_nanos(self.offset_nanos.load(Ordering::SeqCst))
    }
}

#[cfg(any(test, feature = "test-fakes"))]
impl Default for FakeClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(test, feature = "test-fakes"))]
impl Clock for FakeClock {
    fn now(&self) -> Instant {
        self.base.checked_add(self.elapsed()).unwrap_or(self.base)
    }

    fn wall(&self) -> SystemTime {
        self.wall_base
            .checked_add(self.elapsed())
            .unwrap_or(self.wall_base)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn fake_clock_does_not_move_on_its_own() {
        let clock = FakeClock::new();
        let first = clock.now();
        // Real work happens here in a real test. The clock must not notice.
        let mut spin = 0_u64;
        for i in 0..100_000 {
            spin = spin.wrapping_add(i);
        }
        assert!(spin > 0);
        assert_eq!(clock.now(), first, "clock moved without being told");
        assert_eq!(clock.elapsed(), Duration::ZERO);
    }

    #[test]
    fn fake_clock_moves_exactly_as_far_as_told() {
        let clock = FakeClock::new();
        let start = clock.now();
        clock.advance(Duration::from_secs(5));
        assert_eq!(clock.now().duration_since(start), Duration::from_secs(5));
        clock.advance(Duration::from_millis(500));
        assert_eq!(clock.elapsed(), Duration::from_millis(5_500));
    }

    #[test]
    fn a_lease_ttl_expires_without_the_test_sleeping() {
        // The property the cluster simulation depends on: five seconds of lease
        // TTL costs no wall-clock time at all.
        let clock = FakeClock::new();
        let issued = clock.now();
        let ttl = Duration::from_secs(5);

        let real_start = Instant::now();
        clock.advance(ttl + Duration::from_millis(1));
        let expired = clock.now().duration_since(issued) > ttl;
        let real_elapsed = real_start.elapsed();

        assert!(expired, "lease should have expired on the fake clock");
        assert!(
            real_elapsed < Duration::from_millis(100),
            "test actually slept for {real_elapsed:?}"
        );
    }

    #[test]
    fn clones_share_one_timeline() {
        // A test holds one handle and gives another to the code under test.
        let clock = FakeClock::new();
        let handed_out = clock.clone();
        clock.advance(Duration::from_secs(1));
        assert_eq!(handed_out.elapsed(), Duration::from_secs(1));
        assert_eq!(handed_out.now(), clock.now());
    }

    #[test]
    fn wall_clock_advances_with_the_monotonic_one() {
        let clock = FakeClock::new();
        let before = clock.wall();
        clock.advance(Duration::from_secs(60));
        let after = clock.wall();
        let moved = after
            .duration_since(before)
            .expect("wall clock must not go backwards");
        assert_eq!(moved, Duration::from_secs(60));
    }

    #[test]
    fn advancing_saturates_instead_of_wrapping() {
        // An absurd advance in a property test must not send time backwards.
        let clock = FakeClock::new();
        clock.advance(Duration::from_secs(u64::MAX));
        let huge = clock.elapsed();
        clock.advance(Duration::from_secs(u64::MAX));
        assert!(
            clock.elapsed() >= huge,
            "time went backwards: {huge:?} then {:?}",
            clock.elapsed()
        );
    }

    #[test]
    fn system_clock_moves_forward() {
        let clock = SystemClock;
        let first = clock.now();
        let second = clock.now();
        assert!(second >= first, "monotonic clock went backwards");
        assert!(clock.wall() >= std::time::UNIX_EPOCH);
    }

    #[test]
    fn works_behind_a_trait_object() {
        // The shape every consumer uses.
        fn deadline_passed(clock: &dyn Clock, start: Instant, budget: Duration) -> bool {
            clock.now().duration_since(start) > budget
        }

        let clock = FakeClock::new();
        let start = clock.now();
        assert!(!deadline_passed(&clock, start, Duration::from_secs(1)));
        clock.advance(Duration::from_secs(2));
        assert!(deadline_passed(&clock, start, Duration::from_secs(1)));
    }

    #[test]
    fn works_through_an_arc() {
        let clock: Arc<dyn Clock> = Arc::new(FakeClock::new());
        let first = clock.now();
        assert_eq!(clock.now(), first);
        assert!(clock.wall() >= std::time::UNIX_EPOCH);
    }

    #[test]
    fn fake_clock_default_starts_frozen() {
        assert_eq!(FakeClock::default().elapsed(), Duration::ZERO);
    }
}
