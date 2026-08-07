//! How a failed connection attempt may be retried, and the pure arithmetic
//! behind it.
//!
//! # What this is for
//!
//! Not every failure on the connect path is the same kind of failure. A
//! statement mid-flight cannot be retried without knowing whether the server
//! already acted on it, which is the reason `pgprox` retries nothing on the
//! relay path. Failing to *open* a connection at all is a different case:
//! nothing has been sent to any server, so trying again costs nothing and
//! risks nothing. See ADR 0029.
//!
//! # Why the arithmetic is here and the randomness is not
//!
//! [`backoff`] is a pure function: given a policy, an attempt number and a
//! caller-supplied `roll` in `[0, 1)`, it returns how long to wait, or `None`
//! when the policy says stop. It does not draw the roll itself, for the same
//! reason nothing in this crate reads the real clock: a function that is
//! sometimes deterministic and sometimes is not, depending on who calls it, is
//! a function nobody can write a reliable test against. The caller supplies
//! the randomness; this crate only does the arithmetic on it.

use std::time::Duration;

/// How a failed connection attempt is retried.
///
/// The default is off: `attempts: 0`. A retry envelope is a policy about how
/// hard to try again, and this crate does not decide that on an operator's
/// behalf. See [`docs/configuration.md`](../../../docs/configuration.md) for
/// the document field this is read from.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct RetryConfig {
    /// How many times to retry after the first failure. Zero disables retry
    /// entirely: the first failure is the only one, and it is reported as it
    /// always was.
    pub attempts: u32,
    /// The delay before the first retry, before backoff or jitter are
    /// applied.
    pub base: Duration,
    /// The delay no retry waits longer than, however many attempts have
    /// already failed.
    pub max: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            attempts: 0,
            base: Duration::from_millis(20),
            max: Duration::from_secs(2),
        }
    }
}

/// How long to wait before attempt number `attempt` (zero-indexed, counting
/// from the first retry), or `None` if the policy has been exhausted.
///
/// Full jitter: the delay is drawn uniformly between zero and
/// `min(max, base * 2^attempt)`, using `roll` as the draw. That is the
/// simplest of the backoff-with-jitter families and the one most resistant to
/// a thundering herd, because it does not merely vary each retry's delay, it
/// can make it arbitrarily short — two callers retrying together are not
/// still synchronised at a smaller amplitude.
///
/// `roll` is clamped to `[0, 1)` rather than trusted, because a caller drawing
/// from a real source can hand back exactly `1.0` at the boundary and this
/// must not then wait longer than `max`.
#[must_use]
pub fn backoff(config: &RetryConfig, attempt: u32, roll: f64) -> Option<Duration> {
    if attempt >= config.attempts {
        return None;
    }
    let roll = roll.clamp(0.0, 1.0);

    let scale = 1_u32.checked_shl(attempt).unwrap_or(u32::MAX);
    let capped = config.base.saturating_mul(scale).min(config.max);
    Some(capped.mul_f64(roll))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;

    fn config() -> RetryConfig {
        RetryConfig {
            attempts: 3,
            base: Duration::from_millis(10),
            max: Duration::from_millis(100),
        }
    }

    #[test]
    fn zero_attempts_never_retries() {
        assert_eq!(backoff(&RetryConfig::default(), 0, 0.5), None);
    }

    #[test]
    fn an_attempt_at_or_past_the_limit_stops() {
        let config = config();
        assert!(
            backoff(&config, 2, 0.5).is_some(),
            "the last allowed attempt refused"
        );
        assert_eq!(
            backoff(&config, 3, 0.5),
            None,
            "attempts is a count, not an index"
        );
        assert_eq!(backoff(&config, 4, 0.5), None);
    }

    #[test]
    fn the_delay_never_exceeds_the_cap_even_at_a_full_roll() {
        let config = RetryConfig {
            attempts: 10,
            base: Duration::from_millis(10),
            max: Duration::from_millis(50),
        };
        for attempt in 0..10 {
            let delay = backoff(&config, attempt, 1.0).unwrap();
            assert!(
                delay <= config.max,
                "attempt {attempt} waited {delay:?}, past the cap of {:?}",
                config.max
            );
        }
    }

    #[test]
    fn a_roll_of_zero_never_waits() {
        let config = config();
        assert_eq!(backoff(&config, 0, 0.0), Some(Duration::ZERO));
    }

    #[test]
    fn the_uncapped_ceiling_doubles_each_attempt() {
        // Before the cap engages, so this is the raw exponential growth
        // rather than the clamp hiding it.
        let config = RetryConfig {
            attempts: 5,
            base: Duration::from_millis(10),
            max: Duration::from_secs(100),
        };
        assert_eq!(backoff(&config, 0, 1.0).unwrap(), Duration::from_millis(10));
        assert_eq!(backoff(&config, 1, 1.0).unwrap(), Duration::from_millis(20));
        assert_eq!(backoff(&config, 2, 1.0).unwrap(), Duration::from_millis(40));
        assert_eq!(backoff(&config, 3, 1.0).unwrap(), Duration::from_millis(80));
    }

    #[test]
    fn an_out_of_range_roll_is_clamped_rather_than_trusted() {
        let config = config();
        let low = backoff(&config, 0, -1.0).unwrap();
        assert_eq!(low, Duration::ZERO);

        let high = backoff(&config, 0, 2.0).unwrap();
        assert_eq!(
            high, config.base,
            "a roll above 1.0 was not clamped to the uncapped ceiling"
        );
    }

    #[test]
    fn an_attempt_count_that_would_overflow_the_shift_still_respects_the_cap() {
        // `1u32 << 32` is undefined behaviour in debug builds without the
        // checked form, and an operator is free to configure a large attempt
        // count. This is the property that matters regardless: the cap holds.
        let config = RetryConfig {
            attempts: 64,
            base: Duration::from_millis(1),
            max: Duration::from_secs(1),
        };
        let delay = backoff(&config, 40, 1.0).unwrap();
        assert_eq!(delay, config.max);
    }
}
