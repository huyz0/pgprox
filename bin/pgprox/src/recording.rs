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
//!
//! # The bound lives here too
//!
//! Because the bound and the count of hitting it are one subject, and because
//! the bound has to be settable while the node runs. The tick loop pushes it
//! from the live document the way it pushes `max_client_conns` into the gate:
//! `Context` is built once and a configuration that only reached it at startup
//! would be a configuration an operator has to restart a pod to change.
//!
//! `M25.2`.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// The bound on one recorded answer, and how often it has been hit.
#[derive(Debug)]
pub struct Recordings {
    abandoned: AtomicU64,
    max_bytes: AtomicUsize,
}

impl Default for Recordings {
    fn default() -> Self {
        Self {
            abandoned: AtomicU64::new(0),
            // The document's default, so a node with no `query_cache` section
            // behaves the way it did before the key existed. Read from the
            // configuration type rather than restated, because a default
            // written twice is a default that disagrees with itself.
            max_bytes: AtomicUsize::new(
                pgprox_core::config::QueryCacheConfig::default().max_entry_bytes,
            ),
        }
    }
}

impl Recordings {
    /// A fresh counter, bounded at the configured default.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The largest answer the recorder will hold.
    #[must_use]
    pub fn max_bytes(&self) -> usize {
        self.max_bytes.load(Ordering::Relaxed)
    }

    /// Applies a new bound, as the tick loop reads one from the document.
    pub fn set_max_bytes(&self, bytes: usize) {
        self.max_bytes.store(bytes, Ordering::Relaxed);
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
    fn the_bound_starts_at_the_documents_default_and_moves() {
        // `M25.2`. It was a `const` in `serve.rs` while `max_bytes`, the budget
        // it interacts with, was configuration that reloads live. An operator
        // who raised the budget to a gigabyte still could not cache a five
        // megabyte result, and nothing they could read said why.
        let recordings = Recordings::new();
        assert_eq!(
            recordings.max_bytes(),
            pgprox_core::config::QueryCacheConfig::default().max_entry_bytes,
            "a node with no query_cache section changed behaviour"
        );

        recordings.set_max_bytes(4 * 1024 * 1024);
        assert_eq!(recordings.max_bytes(), 4 * 1024 * 1024);

        // Down as well as up, since an operator narrowing it is the case that
        // protects a node under memory pressure.
        recordings.set_max_bytes(4096);
        assert_eq!(recordings.max_bytes(), 4096);
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
