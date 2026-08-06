# 0007. Observability is cluster-scoped by default

Status: accepted

## Context

The proxy runs as several pods. When something is wrong the question is almost
always "which tenant, which upstream, which pod", and the default experience of
a multi-pod system is that you have to guess which pod to ask, then repeat the
query five times and merge the answers by hand.

There is also a cardinality trap. With 5,000 tenants, a `tenant` metric label
produces a series count that will take down a Prometheus.

## Decision

Admin API reads and `SHOW` commands answer for the whole cluster by default.
Aggregates come from the gossip digest every node already carries, so they cost
nothing extra; only drill-downs like listing sessions fan out to peers.
`?scope=local` and `SHOW LOCAL ...` give single-pod detail.

Three surfaces, all fed from the same data:

- Prometheus metrics for per-node and per-server aggregates, every metric
  labelled with `node`
- An HTTP/JSON admin API with a generated OpenAPI document
- A `SHOW` pseudo-database reachable with `psql`, PgBouncer-compatible where the
  command exists there

Per-tenant detail lives in the admin API and `SHOW` output, never in Prometheus
labels, except for a configurable allowlist of tenants worth a series.

`pgprox_cluster_view_hash` is exported so a mismatch across pods surfaces split
brain directly rather than being inferred.

## Consequences

- An operator or an agent hits any pod and gets the whole truth. This is the
  property worth protecting, and it is why aggregates must stay answerable from
  the local digest.
- The gossip digest becomes an observability dependency as well as a control
  one, so its schema needs the same care as a public API.
- Adding an unbounded metric label is a review blocker, not a preference.
- The machine-readable admin API means an agent can operate the fleet without
  scraping text. An MCP server wrapping it can be added later with no proxy
  changes.
- `SHOW` costs a small amount of protocol work to implement, repaid by existing
  PgBouncer dashboards and runbooks continuing to work.

## Alternatives rejected

**Per-pod endpoints, aggregate externally.** Standard practice, and the
aggregation burden lands on whoever is debugging at the time. Rejected because
it is precisely the friction we can remove for free, given gossip already
carries the data.

**Prometheus with a tenant label.** Would answer per-tenant questions with
existing tooling. Rejected on cardinality: 5,000 tenants times the metric set is
not survivable.

**MCP server in the MVP.** Attractive for agent operability. Deferred because
the JSON API with an OpenAPI document is already machine-readable, and MCP can
wrap it later without touching the proxy.
