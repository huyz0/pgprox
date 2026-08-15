//! Where statements went.
//!
//! Three counters, incremented once per statement as the relay decides. They
//! exist because the question "what share of reads did a replica serve" had no
//! answer: the pools show connections rather than statements, and a replica
//! pool at zero could mean the router never chose one or that it chose one and
//! the connection was already warm.
//!
//! The third is the query cache, which is not a `RouteTarget` and has its own
//! method rather than sharing [`RouteCounts::record`]. A hit never reached the
//! router, so there is nothing for the router to have chosen; putting it in
//! the same enum would describe a decision the routing layer does not make.
//! It belongs in the same *metric* because the question is where the
//! statements went, and a hit went here.
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
    cache: AtomicU64,
}

impl RouteCounts {
    /// A fresh pair of counters.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one statement's destination.
    pub fn record(&self, target: pgprox_core::route::RouteTarget) {
        use pgprox_core::route::RouteTarget;

        // Relaxed on purpose: these are counters read by a scrape, and no
        // decision anywhere depends on their ordering against other memory.
        //
        // `M88.18`. `RouteTarget` is `#[non_exhaustive]`, which forces a
        // trailing wildcard here regardless of whether every current variant
        // is already named - that is the whole point of the attribute, and
        // no rewrite of this match can turn a third variant added in
        // `pgprox-core` into a compile failure here. What this match can
        // control is what the wildcard arm does: naming `Primary` and
        // `Replica` explicitly, rather than folding `Primary` into the same
        // `_` that must also catch an unknown future variant, means a
        // variant this crate does not recognise is asserted against in debug
        // builds instead of silently landing in `primary` with nothing to
        // say so.
        let counter = match target {
            RouteTarget::Primary => &self.primary,
            RouteTarget::Replica(_) => &self.replica,
            _ => {
                debug_assert!(
                    false,
                    "RouteTarget gained a variant this match does not know about"
                );
                &self.primary
            }
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Statements sent to the primary.
    #[must_use]
    pub fn primary(&self) -> u64 {
        self.primary.load(Ordering::Relaxed)
    }

    /// Records one statement answered from the query cache.
    ///
    /// Apart from [`RouteCounts::record`] because a hit has no
    /// `RouteTarget`: it is a statement that went nowhere upstream, which is
    /// the whole point of it.
    pub fn record_cache_hit(&self) {
        self.cache.fetch_add(1, Ordering::Relaxed);
    }

    /// Statements sent to a replica.
    #[must_use]
    pub fn replica(&self) -> u64 {
        self.replica.load(Ordering::Relaxed)
    }

    /// Statements answered from the query cache.
    #[must_use]
    pub fn cache(&self) -> u64 {
        self.cache.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use pgprox_core::route::RouteTarget;

    #[test]
    fn record_names_every_current_variant_explicitly() {
        // `M88.18`. `RouteTarget` is `#[non_exhaustive]`, so no version of
        // this match can turn a future third variant into a compile failure
        // - the trailing wildcard is mandatory regardless of what else is
        // named, and no runtime test can construct a variant this crate
        // does not know about to exercise it. What changed, and what this
        // checks by reading the source rather than by calling anything, is
        // that `Primary` used to be folded into that same wildcard along
        // with anything unrecognised. Named on its own arm, the wildcard's
        // `debug_assert!` means "this crate has never seen this variant"
        // rather than "this crate did not bother naming Primary."
        //
        // Only the production half: the assertion below would otherwise find
        // its own string literal and pass regardless of what `record` does.
        let production = include_str!("routes.rs")
            .split_once("#[cfg(test)]")
            .expect("this module's own test marker")
            .0;
        assert!(
            production.contains("RouteTarget::Primary =>"),
            "record() folded Primary back into the wildcard arm"
        );
    }

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

        // And the cache, which is not a target. Recorded through its own
        // method, and it must not land in either of the other two: a hit
        // counted as a primary statement would say the proxy sent something
        // upstream that it did not.
        counts.record_cache_hit();
        assert_eq!(counts.cache(), 1);
        assert_eq!(counts.primary(), 1, "a cache hit was counted as primary");
        assert_eq!(counts.replica(), 2, "a cache hit was counted as a replica");
    }

    #[test]
    fn a_fresh_set_counts_nothing() {
        let counts = RouteCounts::default();
        assert_eq!(counts.primary(), 0);
        assert_eq!(counts.replica(), 0);
        assert_eq!(counts.cache(), 0);
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
