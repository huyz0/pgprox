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

pub mod coordinator;
pub mod digest;
pub mod lease;
pub mod membership;
pub mod quota;
pub mod reservation;
pub mod service;
pub mod shed;

pub use coordinator::{CoordinatorConfig, NodeCoordinator};
pub use digest::{DigestStore, MergeOutcome, VersionedDigest};
pub use lease::{LeaseConfig, LeaseLedger};
pub use membership::{Membership, MembershipConfig, NodeState};
pub use quota::{NodeAllowance, QuotaSplit, split};
pub use reservation::{ReservationConfig, Reservations, TenantEntitlement};
pub use service::GossipCoordinator;
pub use shed::{ShedConfig, ShedCtx, ShedDecision, ShedRefusal};

// Compiled during this crate's own tests so the coverage gate measures it, and
// available to downstream crates only when they ask. Same pattern as the fakes
// in pgprox-core: gating on the feature alone would leave the simulation
// invisible to coverage, which is the code most in need of it.
#[cfg(any(test, feature = "sim"))]
pub mod sim;
