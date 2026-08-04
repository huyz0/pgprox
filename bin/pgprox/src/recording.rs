//! What happened to answers the cache was offered and never got.
//!
//! # Why this is not on the store
//!
//! An answer abandoned for being too big never reaches `QueryCache::put`, so
//! the store cannot count it: by the time the decision is made the recording is
//! already being dropped and nothing is handed anywhere. `CacheStats::rejected`
//! counts the check *inside* `put`, which is a different check with a different
//! fix.
//!
//! [`crate::routes::RouteCounts`] is the pattern and its module comment is the
//! argument: a count belongs where the decision is made, and in the metric that
//! answers the question an operator is asking.
//!
//! # The question it answers
//!
//! A tenant whose results all sit just over the per-answer cap sees a hit rate
//! of zero. Every lookup counted a miss, because `get` ran before anything knew
//! how big the answer would be, and `rejected` stayed at zero because `put` was
//! never called. The counters said the working set does not fit, which is the
//! remedy for a different problem: raising the budget does nothing here.
//!
//! `M25.1`.

use std::sync::atomic::{AtomicU64, Ordering};

/// Answers the recorder started and gave up on.
#[derive(Debug, Default)]
pub struct Recordings {
    abandoned: AtomicU64,
}

impl Recordings {
    /// A fresh counter.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records an answer given up on for exceeding the per-answer cap.
    pub fn abandon(&self) {
        // Relaxed, like `RouteCounts`: a counter read by a scrape, and nothing
        // decides anything on its ordering against other memory.
        self.abandoned.fetch_add(1, Ordering::Relaxed);
    }

    /// How many have been given up on.
    #[must_use]
    pub fn abandoned(&self) -> u64 {
        self.abandoned.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abandoning_moves_the_counter_and_nothing_else_does() {
        let recordings = Recordings::new();
        assert_eq!(recordings.abandoned(), 0);

        recordings.abandon();
        recordings.abandon();
        assert_eq!(recordings.abandoned(), 2);
    }

    #[test]
    fn the_counter_is_shared_across_sessions() {
        // Every session records into one of these, so it has to count across
        // them: a per-session counter would report the last connection's
        // experience rather than the node's.
        let recordings = std::sync::Arc::new(Recordings::new());
        let other = std::sync::Arc::clone(&recordings);

        recordings.abandon();
        other.abandon();

        assert_eq!(recordings.abandoned(), 2);
        assert_eq!(other.abandoned(), 2);
    }
}
