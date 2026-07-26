//! The reference workload, its sampler, and the run report.
//!
//! # Why the load client is not just pgbench
//!
//! pgbench opens one connection per client and one thread per few clients, so
//! 100k connections is not a matter of raising a flag. It also cannot express
//! the two things this proxy is judged on: a tenant mix where most connections
//! are idle most of the time, and connection churn. So the workload lives in a
//! committed document, this crate turns it into a deterministic stream of work,
//! and `bin/pgload` is the thin part that puts that stream on a socket.
//!
//! Nothing here performs I/O. The sampling, the distribution and the report are
//! pure functions of a workload and a seed, which is what makes a run
//! reproducible and this crate testable without a database.

pub mod report;
pub mod sampler;
pub mod workload;

pub use report::{Histogram, Latency, Report};
pub use sampler::{Planned, Sampler, Transaction};
pub use workload::{Churn, Kind, Statement, TenantGroup, TransactionSize, Workload, WorkloadError};
