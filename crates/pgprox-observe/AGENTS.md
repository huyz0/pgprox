# pgprox-observe

Metrics, tracing, log initialization, health endpoints.

## Rules specific to this crate

- **Adding an unbounded metric label is a review blocker**, and now also a test
  failure. With 5,000 tenants a `tenant` label produces a series count that will
  take down a Prometheus. Every metric is declared in `metrics.rs` with the
  reason each label is bounded, and `no_metric_has_an_unbounded_label` walks the
  list. Per-tenant detail belongs in the admin API and `SHOW` output.
- Labels multiply. Each one being individually bounded is not enough, so there
  is a ceiling on the series one metric can produce as well as on any single
  label.
- Declare metrics in `metrics.rs`, never at the call site. A metric declared
  where it is incremented cannot be enumerated, so nobody can answer what the
  proxy exports and the cardinality rule cannot be checked at all.
- **Build the exporter from the registry**, using `describe_all`. Typing a
  metric name at the exporter as well as here makes the registry a description
  of what somebody intended rather than of what is exported, and the two drift
  the first time a name changes.
- Every metric carries a `node` label. `pgprox_cluster_view_hash` exists so a
  mismatch across pods surfaces split brain directly.
- **Nothing may make `/readyz` fail transiently** except drain. A flapping
  readiness probe under load causes exactly the connection storm the design
  exists to prevent.
- Credentials never reach a log line, span attribute, or metric label. Query text
  is `debug` only and opt-in per tenant, because SQL routinely carries customer
  data in literals.
- Span names are stable and low cardinality. The tenant goes in an attribute.

See ADR [0007](../../product/decisions/0007-cluster-scoped-observability.md).
