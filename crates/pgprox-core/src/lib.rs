//! Contracts shared by every `pgprox` crate.
//!
//! This crate holds traits, DTOs, error types, and ID newtypes, plus a working
//! in-memory fake for every trait. It performs no I/O and depends on no other
//! workspace crate.
//!
//! That constraint is what lets several tracks develop in parallel: a track
//! codes against the traits and tests against the fakes, never against another
//! track's half-finished crate.
//!
//! # Changing anything here
//!
//! A contract change is one atomic commit covering the trait, every fake, every
//! implementation, every call site, and an ADR. If it touches more than one
//! track, stop and escalate first. See `standards/contracts.md`.

pub mod clock;
pub mod error;
pub mod ids;
pub mod secret;

pub use clock::{Clock, SystemClock};
pub use error::{AuthRejection, ClientError, SqlState};
pub use ids::{ConnId, Lsn, NodeId, PoolKey, ServerId, TenantId};
pub use secret::SecretString;
