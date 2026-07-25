# pgprox-cluster

SWIM gossip, membership, quota leases, tenant reservations, shed decisions.

Needs no Postgres. Developed entirely against the deterministic simulation
harness, so it can start immediately after M0.

## The invariant

> Guaranteed share plus outstanding leases never exceeds the cap, under
> arbitrary partition, leader loss, and simultaneous restart.

Breaching an upstream cap can lock out the operator and take the database down
for every tenant on that host. It is the one property in the project with no
graceful degradation.

This is proven by property test over the simulation, not by integration test. It
is the class of bug that never reproduces in staging.

## Rules specific to this crate

- **Time is injected.** The simulation advances a 5s lease TTL in microseconds.
  Nothing calls `Instant::now()`.
- **Partitions must cause under-subscription, never over-subscription.** Slow
  beats down. Any change that could transiently over-grant is wrong.
- On leader change, the new leader waits one full lease TTL before granting from
  the free pool. Do not optimize this away.
- Gossip digests feed cluster-wide admin aggregates as well as control, so the
  digest schema needs the care of a public API.
- Any failing simulation seed is committed as a regression case.

See ADR [0004](../../product/decisions/0004-swim-gossip-with-leader-leases.md)
and [0005](../../product/decisions/0005-home-node-affinity-by-reservation.md).
