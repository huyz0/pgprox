//! The composition root: the one place concrete types meet.
//!
//! Everything else in this workspace is written against a trait and tested
//! against a fake. Here the fakes are replaced, once, and the result is handed
//! to a run loop. That is the whole job.
//!
//! # Why the wiring is a library
//!
//! `main.rs` is the only file excluded from coverage, so anything in it is
//! untested by construction. Keeping it to argument parsing and one call means
//! the exclusion buys nothing: the wiring below is called by a test with fakes
//! in place of sockets, and `scripts/gates/m6-complete.sh` fails if `main.rs` grows
//! past a handful of lines.
//!
//! # What is deliberately not here
//!
//! Any decision. This module builds objects and connects them. Every rule
//! about what the proxy does lives in the crate that owns it, and a rule that
//! appeared here would be one no other crate's tests could reach.

#![warn(missing_docs)]

pub mod admin;
pub mod dial;
pub mod drain;
pub mod entropy;
pub mod entry;
/// A Postgres server that answers well enough to be connected to, for tests.
///
/// Test-only, and shared: `serve.rs` drives whole sessions through it and
/// `observatory.rs` needs a pool that holds a real upstream connection. It was
/// inside `serve.rs`'s own tests until `M17.4`, which is the reason a mutant in
/// `reset_pool` survived every run.
#[cfg(test)]
pub mod fakepg;
pub mod gossip;
pub mod http;
pub mod logging;
pub mod metrics;
pub mod observatory;
pub mod primary_watch;
pub mod recording;
pub mod replicas;
pub mod routes;
pub mod run;
pub mod serve;
pub mod sessions;
pub mod wiring;

pub use dial::{ClientScram, Stream, TcpUpstream};
pub use drain::{Drain, Step};
pub use entropy::SystemEntropy;
pub use entry::{Options, run_with, start, start_with};
pub use http::{Probes, probe_routes, router};
pub use observatory::NodeObservatory;
pub use run::{Addrs, Listeners, Shutdown, run};
pub use serve::{Admitted, Context, Gate};
pub use sessions::{Registration, Sessions};
pub use wiring::{App, Deps, SharedDrain, SharedHealth, StartupError};
