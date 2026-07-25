# Observability

The proxy runs as several pods and sits between a customer and their database.
When something is wrong, the question is almost always "which tenant, which
upstream, which pod", and the system should answer it without a person guessing
which pod to ask.

## Cluster-scoped by default

Admin reads and `SHOW` commands answer for the whole cluster, not the local pod.
Aggregates come from the gossip digest every node already carries, so they cost
nothing; only drill-downs like listing sessions fan out to peers. `?scope=local`
and `SHOW LOCAL ...` exist for single-pod detail.

Hitting any pod gives the same answer. That property is worth protecting.

## Metrics

The `metrics` facade with `metrics-exporter-prometheus`. Names are
`pgprox_<subsystem>_<thing>_<unit>`, and every metric carries a `node` label.

| Metric | Why it matters |
| --- | --- |
| `pgprox_wait_seconds` | Time blocked acquiring an upstream connection. The single most important latency signal in the system. |
| `pgprox_upstream_conns{server,state}` | Against the cap. If this exceeds it, the quota layer has a bug. |
| `pgprox_quota_leased{server}` | Leader lease state, for diagnosing over- and under-subscription. |
| `pgprox_client_conns{state}` | Idle versus active, drives shed and evict decisions. |
| `pgprox_pin_total{reason}` | Rising pin rate means falling multiplexing. Watch the reason label. |
| `pgprox_shed_total{reason}` | Shedding should be rare. A spike means the rebalance logic is thrashing. |
| `pgprox_replica_lag_bytes{replica}` | Feeds routing eligibility, so it is correctness-relevant. |
| `pgprox_cluster_members` | Disagreement across pods means split brain. |
| `pgprox_cluster_view_hash` | Same view hash on every pod, or membership has diverged. |

**Cardinality.** With 5,000 tenants a `tenant` label would blow up the series
count. Per-tenant detail lives in the admin API and `SHOW` output. Prometheus
gets per-node and per-server aggregates plus a configurable allowlist for the
handful of tenants worth a series. Adding an unbounded label is a review
blocker.

## Tracing

OpenTelemetry spans for connection lifecycle and per transaction, carrying
`tenant_id`, `node_id`, `pool_key`, and route target. Sample at 1% by default
and always sample on error, because the 1% that fails is the 1% worth having.

Span names are stable and low cardinality. The tenant goes in an attribute,
never in the span name.

## Logs

JSON, through `tracing-subscriber`. Every line carries `conn_id`, which is
`base32(node_id || counter)`, so a log line identifies its pod without a lookup
and a single connection can be followed across the auth, route, and upstream
stages.

Log at the boundary that decides what to do about a problem, not where it is
detected. Detecting code returns the error; see
[error-handling.md](error-handling.md).

## What must never be logged

Passwords, JWTs, any field of a `Backend`, and query parameter values. Query
text is logged only at `debug` and only when explicitly enabled per tenant,
because SQL routinely carries customer data in literals.

`SecretString` redacts in `Debug` and `Display`, which makes the safe path the
default one. Bypassing it to get a real value into a format string is a review
blocker. See [security.md](security.md).

## Health

`/livez` is process liveness. `/readyz` reports whether this node should receive
new connections, and it fails during drain so kubernetes removes the pod from
Service endpoints. Nothing else is allowed to make `/readyz` fail transiently;
a flapping readiness probe under load causes exactly the connection storm the
whole design exists to avoid.
