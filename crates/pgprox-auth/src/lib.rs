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

pub mod cache;
pub mod client;
pub mod jwt;

pub use cache::{CacheConfig, CachingResolver};
pub use client::{SidecarConfig, SidecarResolver};
pub use jwt::{ALLOWED_ALGORITHMS, check_algorithm};
