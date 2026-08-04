//! Statement classification, replica eligibility and target selection.
//!
//! # The rule that matters
//!
//! When the classifier is not confident, route to the primary.
//!
//! A false negative costs a little throughput. A false positive is a stale
//! read, which is a data-correctness bug from the tenant's perspective and
//! worse than the slowness it was meant to fix.
//!
//! The routing decision itself lives in [`pgprox_core::route::decide`], not
//! here, so the real router and every fake share one implementation. This crate
//! supplies what that decision needs: what a statement does, and how far each
//! replica has replayed.
//!
//! # No unsafe, and not by the workspace's leave
//!
//! `#![forbid]` rather than the workspace's `deny`, so no `#[allow]` anywhere
//! in this crate can reach it. This crate classifies untrusted SQL, and a wrong answer here is a
//! stale read rather than a crash only because nothing in it can corrupt memory.
//!
//! `M27.1` opened the door elsewhere and left it shut here on purpose. See ADR
//! 0026 and `scripts/check-unsafe.sh`, which holds the list.

#![forbid(unsafe_code)]
pub mod classify;
pub mod hints;
pub mod poller;
pub mod replica;
pub mod router;

pub use classify::{begins_read_only_transaction, classify};
pub use hints::{RouteAssignment, parse_route_assignment, statement_hint};
pub use poller::{Probe, ReplicaProbe, ReplicaWatch};
pub use replica::{ReplicaConfig, Replicas, Watermark};
pub use router::{Routed, SessionRouter, StatelessRouter};
