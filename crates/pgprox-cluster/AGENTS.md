# pgprox-cluster

SWIM gossip, membership, quota leases, tenant reservations, shed decisions.

Needs no Postgres and no sidecar, so it develops entirely against the
deterministic simulation in `src/sim.rs`.

## The invariant

> Guaranteed share plus outstanding leases never exceeds the cap, under
> arbitrary partition, leader loss, and simultaneous restart.

Breaching an upstream cap can lock out the operator and take the database down
for every tenant on that host. It is the one property in the project with no
graceful degradation.

Proven by property test over the simulation, not by integration test. It is the
class of bug that never reproduces in staging, so it has to be caused on
purpose, thousands of times, in milliseconds.

## Rules specific to this crate

- **Partitions must cause under-subscription, never over-subscription.** Slow
  beats down. Any change that could transiently over-grant is wrong even if it
  converges.
- **Determinism is not negotiable.** Nothing uses the system clock or the
  system RNG. A failing seed must replay exactly, or a property test is an
  anecdote rather than evidence.
- On leader change, the new leader waits one full lease TTL before granting from
  the free pool. Do not optimize this away: it is what makes over-granting
  impossible across a failover.
- `QuotaLease::count` already returns zero once expired. Rely on that rather
  than checking expiry separately, so a forgotten check cannot over-subscribe.
- A failing seed is committed as a regression case, named for what it broke.
- Gossip digests feed cluster-wide admin aggregates as well as control, so the
  digest schema needs the care of a public API.

See ADR [0004](../../product/decisions/0004-swim-gossip-with-leader-leases.md)
and [0005](../../product/decisions/0005-home-node-affinity-by-reservation.md).
