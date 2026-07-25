//! Upstream pools, lifecycle, idle reap, pinning and prepared statement
//! mapping.
//!
//! # What makes the ratio real
//!
//! A node holds tens of thousands of client connections against a few thousand
//! upstream ones. That works only because an upstream connection is borrowed
//! for a transaction rather than for a session, and it stops working the moment
//! a large fraction of sessions become unmovable.
//!
//! So the two halves of this crate pull in opposite directions and both matter:
//! [`pin`] decides which sessions genuinely cannot be moved, and [`statements`]
//! removes the largest reason they otherwise would be.

pub mod params;
pub mod pin;
pub mod statements;

pub use params::{ParamChange, SessionParams};
pub use pin::{PinReason, PinState, REPLAYABLE_PARAMETERS, pin_reason};
pub use statements::{
    ConnectionStatements, GlobalName, Preparation, SessionStatements, StatementConfig,
};
