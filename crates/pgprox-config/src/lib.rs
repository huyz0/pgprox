//! Configuration providers, schema validation and hot reload.
//!
//! Config is pulled, not pushed, and drain is desired state rather than a
//! command. A drained node stays drained across a restart, and the intent is
//! visible in whatever the config lives in rather than being a side effect
//! somebody ran once. See ADR 0006.
//!
//! [`document`] owns the file format, the provider owns where the file comes
//! from, and validation happens once in the shared path so every provider
//! behaves identically.

pub mod document;

pub use document::{ConfigDocument, parse};
