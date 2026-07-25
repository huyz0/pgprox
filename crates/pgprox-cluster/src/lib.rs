//! Membership, quota leases, tenant placement and shed decisions.
//!
//! Needs no Postgres and no sidecar, so it develops entirely against the
//! deterministic simulation in [`sim`].
//!
//! # The invariant
//!
//! Guaranteed share plus outstanding leases never exceeds the cap, under
//! arbitrary partition, leader loss and simultaneous restart. Breaching an
//! upstream cap can lock out the operator and take the database down for every
//! tenant on that host, which makes it the one property here with no graceful
//! degradation.
//!
//! Partitions must therefore cause under-subscription, never over-subscription.
//! Slow beats down.

// Compiled during this crate's own tests so the coverage gate measures it, and
// available to downstream crates only when they ask. Same pattern as the fakes
// in pgprox-core: gating on the feature alone would leave the simulation
// invisible to coverage, which is the code most in need of it.
#[cfg(any(test, feature = "sim"))]
pub mod sim;
