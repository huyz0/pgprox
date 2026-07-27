//! Where statements went.
//!
//! Two counters, incremented once per statement as the relay decides. They
//! exist because the question "what share of reads did a replica serve" had no
//! answer: the pools show connections rather than statements, and a replica
//! pool at zero could mean the router never chose one or that it chose one and
//! the connection was already warm.
//!
//! Counters rather than a gauge, because the interesting quantity is a ratio
//! over a run and a gauge would only say what happened at the moment of the
//! scrape.

use std::sync::atomic::{AtomicU64, Ordering};

/// Statements routed, by where they went.
#[derive(Debug, Default)]
pub struct RouteCounts {
    primary: AtomicU64,
    replica: AtomicU64,
}

impl RouteCounts {
    /// A fresh pair of counters.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one statement's destination.
    pub fn record(&self, target: pgprox_core::route::RouteTarget) {
        // Relaxed on purpose: these are counters read by a scrape, and no
        // decision anywhere depends on their ordering against other memory.
        match target {
            pgprox_core::route::RouteTarget::Replica(_) => &self.replica,
            _ => &self.primary,
        }
        .fetch_add(1, Ordering::Relaxed);
    }

    /// Statements sent to the primary.
    #[must_use]
    pub fn primary(&self) -> u64 {
        self.primary.load(Ordering::Relaxed)
    }

    /// Statements sent to a replica.
    #[must_use]
    pub fn replica(&self) -> u64 {
        self.replica.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pgprox_core::route::RouteTarget;

    #[test]
    fn each_target_lands_in_its_own_counter() {
        let counts = RouteCounts::new();
        counts.record(RouteTarget::Primary);
        counts.record(RouteTarget::Replica(0));
        counts.record(RouteTarget::Replica(1));

        assert_eq!(counts.primary(), 1);
        assert_eq!(
            counts.replica(),
            2,
            "a replica index was counted as primary"
        );
    }

    #[test]
    fn a_fresh_pair_counts_nothing() {
        let counts = RouteCounts::default();
        assert_eq!(counts.primary(), 0);
        assert_eq!(counts.replica(), 0);
    }

    #[test]
    fn counting_is_shared_across_threads() {
        // One pair per node, and every session on it increments.
        use std::sync::Arc;

        let counts = Arc::new(RouteCounts::new());
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let counts = Arc::clone(&counts);
                scope.spawn(move || {
                    for _ in 0..1_000 {
                        counts.record(RouteTarget::Replica(0));
                    }
                });
            }
        });
        assert_eq!(counts.replica(), 8_000);
    }
}
