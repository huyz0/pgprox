//! Where the retry backoff's randomness comes from.
//!
//! [`pgprox_core::retry::backoff`] is a pure function: it takes a `roll` and
//! does the arithmetic. Something has to draw the roll, and this is that
//! something, kept behind a trait for the same reason [`crate::live::Connector`]
//! is: the pool's own behaviour, including whether it stops retrying at the
//! right attempt, is testable against a fixed roll rather than a real one.
//!
//! # Why this is not `pgprox_session::cancel::Entropy`
//!
//! That trait exists for cancel keys, which are bearer tokens: its contract is
//! about unguessability, and it refuses outright rather than fall back to
//! anything predictable. A retry's jitter has no security property to defend.
//! Its only job is to keep two callers backing off together from staying
//! synchronised, which any source of variation does. Reusing `Entropy` would
//! also reach upward across a dependency this crate does not have:
//! `pgprox-session` depends on `pgprox-pool`, not the reverse.

use std::fmt;

/// A source of variation for retry backoff, in `[0, 1)`.
pub trait Jitter: Send + Sync + fmt::Debug {
    /// The next roll.
    fn roll(&self) -> f64;
}

impl<T: Jitter + ?Sized> Jitter for std::sync::Arc<T> {
    fn roll(&self) -> f64 {
        (**self).roll()
    }
}

/// A jitter source that always returns the same value, for a test that wants
/// to assert on an exact delay rather than only on a bound.
#[cfg(any(test, feature = "test-fakes"))]
#[derive(Debug, Clone, Copy)]
pub struct FixedJitter(pub f64);

#[cfg(any(test, feature = "test-fakes"))]
impl Jitter for FixedJitter {
    fn roll(&self) -> f64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn an_arc_forwards_rather_than_defaulting() {
        let jitter: Arc<dyn Jitter> = Arc::new(FixedJitter(0.42));
        assert!((jitter.roll() - 0.42).abs() < f64::EPSILON);
    }
}
