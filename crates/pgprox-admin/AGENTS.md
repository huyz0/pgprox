# pgprox-admin

HTTP/JSON admin API and the `SHOW` pseudo-database.

This crate depends only on `pgprox-core`, like every other. The data it reports
lives in three other crates, so it reads through `pgprox_core::admin`, which the
composition root implements by fanning in. See ADR
[0018](../../product/decisions/0018-admin-reads-through-a-core-contract.md).

## Rules specific to this crate

- **Do not reach for the crates that hold the data.** If something is missing
  from the `Observatory` contract, add it there. Making this crate a composer
  would put an HTTP handler inside three subsystems, which is the coupling the
  layering rule exists to prevent.
- **Cluster-scoped by default.** Hitting any pod gives the whole cluster's
  truth. Aggregates answer from the local gossip digest at no cost; only
  drill-downs like listing sessions fan out to peers. `?scope=local` and
  `SHOW LOCAL ...` give single-pod detail.
- The OpenAPI document is generated from the handlers, so tooling and agents get
  a typed contract rather than scraped text.
- No response ever contains a credential. The `Observatory` DTOs have no field
  for one, so this holds structurally rather than by review. No response leaks
  upstream hostnames or internal topology to an untrusted caller.
- An incomplete answer says so. A fan-out that loses a peer returns
  `AdminError::Partial`, never a short list, because a short list presented as
  complete is how an operator concludes a tenant has no clients.
- The `SHOW` subset that PgBouncer also has stays compatible with it, so existing
  dashboards and runbooks keep working.

See ADR [0007](../../product/decisions/0007-cluster-scoped-observability.md).
