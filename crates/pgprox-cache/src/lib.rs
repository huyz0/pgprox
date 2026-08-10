//! Query result cache with bounded staleness.
//!
//! ADR 0021 is the contract and this crate does not widen it: off by default,
//! opt-in per tenant, one node rather than the fleet, and the TTL on each
//! entry is the guarantee. Invalidation on write is an improvement on that
//! bound. Nothing here calls it read-your-writes.
//!
//! # What this crate does and does not decide
//!
//! [`mod@store`] stores and expires. [`mod@cacheable`] decides what may be
//! stored at all, and [`mod@normalize`] decides what counts as the same
//! question. The split is deliberate, because they are wrong in different
//! ways: a store that
//! expires badly serves stale data, which the TTL bounds, while a store handed
//! something uncacheable serves wrong data, which nothing bounds.
//!
//! # Who it serves is configuration, and configuration moves
//!
//! [`Store`] holds no settings of its own. The byte budget, the TTL cap and
//! the tenant list all come from `pgprox_core::config::QueryCacheConfig`
//! through [`Store::reconfigure`], which the node's tick loop calls with
//! whatever the document currently says. A store built and then never
//! reconfigured serves nobody, which is the right thing for a node that has
//! not been told otherwise.

#![forbid(unsafe_code)]

pub mod cacheable;
pub mod normalize;
pub mod settings;
pub mod store;

pub use cacheable::{NotCacheable, SessionFacts, cacheable};
pub use normalize::normalize;
pub use settings::fingerprint as settings_fingerprint;
pub use store::{CacheStats, Store};
