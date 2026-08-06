# pgprox-admin

HTTP/JSON admin API and the `SHOW` pseudo-database.

This crate depends only on `pgprox-core`, like every other. The data it reports
lives in three other crates, so it reads through `pgprox_core::admin`, which the
composition root implements by fanning in. See ADR
[0018](../../docs/internal/product/decisions/0018-admin-reads-through-a-core-contract.md).

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
  dashboards and runbooks keep working. Columns come from
  `reference/pgbouncer/src/admin.c`, names and order both, because most
  dashboards read by position.
- **`SHOW SERVERS` is not `GET /v1/servers`.** The first is PgBouncer's
  per-connection socket view and cannot change; the second is the capacity
  view, whose `SHOW` form is `SHOW QUOTA`. `tests/surfaces_agree.rs` pins which
  pairs actually correspond.
- The two surfaces reading one `Observatory` is an architectural claim, and
  `tests/surfaces_agree.rs` is what makes it true rather than hoped for. Add a
  case there when adding a question either surface can answer.

See ADR [0007](../../docs/internal/product/decisions/0007-cluster-scoped-observability.md).
