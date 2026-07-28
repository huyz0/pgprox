//! Query result cache with bounded staleness.
//!
//! ADR 0021 is the contract and this crate does not widen it: off by default,
//! opt-in per tenant, one node rather than the fleet, and the TTL on each
//! entry is the guarantee. Invalidation on write is an improvement on that
//! bound. Nothing here calls it read-your-writes.
//!
//! # What this crate does and does not decide
//!
//! It stores and expires. It does not decide what may be stored: that is
//! the cacheability rule's job in a later task, and until then a caller handing
//! this the result of a write would get exactly what it asked for. The split is
//! deliberate, because the two are wrong in different ways. A store that
//! expires badly serves stale data, which the TTL bounds. A store handed
//! something uncacheable serves wrong data, which nothing bounds.

#![forbid(unsafe_code)]

pub mod normalize;
pub mod store;

pub use normalize::normalize;
pub use store::{CacheStats, Store};
