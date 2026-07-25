# pgprox-admin

HTTP/JSON admin API and the `SHOW` pseudo-database.

## Rules specific to this crate

- **Cluster-scoped by default.** Hitting any pod gives the whole cluster's
  truth. Aggregates answer from the local gossip digest at no cost; only
  drill-downs like listing sessions fan out to peers. `?scope=local` and
  `SHOW LOCAL ...` give single-pod detail.
- The OpenAPI document is generated from the handlers, so tooling and agents get
  a typed contract rather than scraped text.
- No response ever contains a credential, and no response leaks upstream
  hostnames or internal topology to an untrusted caller.
- The `SHOW` subset that PgBouncer also has stays compatible with it, so existing
  dashboards and runbooks keep working.

See ADR [0007](../../product/decisions/0007-cluster-scoped-observability.md).
