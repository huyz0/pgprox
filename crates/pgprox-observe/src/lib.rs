//! Metrics, tracing, log initialization and health endpoints.
//!
//! # The rule that shapes this crate
//!
//! Adding an unbounded metric label is a review blocker. At five thousand
//! tenants a `tenant` label is a series count that takes a Prometheus down, and
//! it does so at exactly the moment somebody is trying to work out why the
//! proxy is unhappy.
//!
//! [`metrics`] makes that checkable rather than reviewable: every metric is
//! declared in one place with the reason each of its labels is bounded, so
//! `no_metric_has_an_unbounded_label` is a test.
//!
//! Per-tenant detail is not lost. It lives in the admin API and `SHOW` output,
//! which are pull-based and cost nothing when nobody is looking. See ADR 0007.

pub mod metrics;
pub mod spans;

pub use metrics::{ALL, Kind, Label, MAX_LABEL_VALUES, Metric};
pub use spans::{REDACTED, Span, is_recordable, may_record_query, redact};
