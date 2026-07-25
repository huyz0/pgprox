//! HTTP/JSON admin API and the `SHOW` pseudo-database.
//!
//! # Cluster-scoped by default
//!
//! Hitting any pod gives the whole cluster's truth, so there is no wrong pod to
//! ask. Aggregates answer from the local gossip digest at no cost; only
//! drill-downs fan out, and those are the ones that can come back partial.
//! `?scope=local` and `SHOW LOCAL ...` narrow a read to the node that answered.
//!
//! # Where the data comes from
//!
//! [`pgprox_core::admin::Observatory`], implemented by the composition root.
//! This crate depends only on `pgprox-core`, like every other, and the fan-in
//! across pools, sessions and cluster state happens once rather than in every
//! handler. That also means the HTTP surface and the `SHOW` surface read the
//! same data by construction, so they cannot drift into giving different
//! answers to the same question. See ADR 0018.

pub mod api;
pub mod openapi;

pub use api::{ApiError, Shared, read_routes, routes, write_routes};
pub use openapi::{ApiDoc, document};
