//! Where a running node's randomness comes from.
//!
//! One implementation, of one trait, over the crypto provider this workspace
//! already builds against. It is here rather than in `pgprox-session` for the
//! same reason every other concrete choice is here: the crate that defines
//! [`Entropy`] must be testable without one.
//!
//! # Why this matters more than it looks
//!
//! A cancel key is a bearer token. Anything holding one can cancel the query it
//! names, and the 3.0 protocol gives it 32 bits of secret and no
//! authentication. `M1F.36` recorded the requirement: the part of a `ConnId`
//! that is not the node number must be unguessable, because a counter lets one
//! tenant cancel another's queries by trying numbers near its own.

use pgprox_session::cancel::Entropy;

/// The system entropy source, through `aws-lc-rs`.
///
/// The same provider rustls and the SCRAM implementation use, so a FIPS build
/// has one validated module rather than one plus whatever else got linked in.
#[derive(Debug, Default)]
pub struct SystemEntropy;

impl Entropy for SystemEntropy {
    fn next(&self) -> Option<u64> {
        use aws_lc_rs::rand::{SecureRandom as _, SystemRandom};

        let rng = SystemRandom::new();
        let mut bytes = [0_u8; 8];

        // Retried, because a transient failure of the system source is the only
        // kind worth surviving. A persistent one means the machine has no
        // entropy, and the answer to that is `None`: the connection is refused
        // with an internal error rather than handed a guessable cancel key.
        for _ in 0..3 {
            if rng.fill(&mut bytes).is_ok() {
                return Some(u64::from_be_bytes(bytes));
            }
        }
        None
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_source_produces_something() {
        assert!(
            SystemEntropy.next().is_some(),
            "the system entropy source produced nothing"
        );
    }

    #[test]
    fn two_draws_differ() {
        // The property M1F.36 is about: a sequence number is not a secret.
        let entropy = SystemEntropy;
        let drawn: HashSet<Option<u64>> = (0..64).map(|_| entropy.next()).collect();

        assert!(
            drawn.len() > 60,
            "the entropy source repeated itself {} times in 64 draws",
            64 - drawn.len()
        );
    }

    #[test]
    fn the_whole_width_is_used() {
        // A source that filled only the low bits would leave a cancel key with
        // far less than the 48 bits the design assumes.
        let entropy = SystemEntropy;
        let seen = (0..64).fold(0_u64, |bits, _| bits | entropy.next().unwrap_or(0));

        assert_eq!(
            seen.count_ones(),
            64,
            "some bit was never set in 64 draws: {seen:#x}"
        );
    }
}
