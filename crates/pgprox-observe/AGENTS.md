# pgprox-observe

Metrics, tracing, log initialization, health endpoints.

## Rules specific to this crate

- **Adding an unbounded metric label is a review blocker.** With 5,000 tenants a
  `tenant` label produces a series count that will take down a Prometheus.
  Per-tenant detail belongs in the admin API and `SHOW` output.
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
