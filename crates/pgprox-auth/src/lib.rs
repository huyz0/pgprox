//! Turning a client's token into the credentials for its database.
//!
//! The sidecar validates the token and answers with a grant. This crate asks
//! it, caches the answer, and collapses concurrent lookups so a reconnect storm
//! produces one call rather than thousands.
//!
//! # What this crate does not do
//!
//! It does not verify signatures. The sidecar owns validation, and two
//! validators that disagree about whether a token is valid is a vulnerability
//! rather than redundancy. See ADR 0003.
//!
//! # No unsafe, and not by the workspace's leave
//!
//! `#![forbid]` rather than the workspace's `deny`, so no `#[allow]` anywhere
//! in this crate can reach it. This crate parses a JWT header and runs a SCRAM exchange against bytes a
//! peer chose.
//!
//! `M27.1` opened the door elsewhere and left it shut here on purpose. See ADR
//! 0026 and `scripts/check-unsafe.sh`, which holds the list.

#![forbid(unsafe_code)]
pub mod cache;
pub mod client;
pub mod jwt;
pub mod scram;

pub use cache::{CacheConfig, CachingResolver};
pub use client::{SidecarConfig, SidecarResolver};
pub use jwt::{ALLOWED_ALGORITHMS, check_algorithm};
pub use scram::{ClientExchange, SCRAM_SHA_256, ScramError, ScramKeys};
