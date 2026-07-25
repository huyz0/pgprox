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
- The guaranteed share is divided by the **configured** fleet size, never by the
  live member count. A node that can see only itself must not conclude it is the
  whole cluster.
- A leader may grant only while it can see a strict majority of the fleet.
  Leading a partitioned minority is not leading.
- On taking office, a leader waits `ttl + suspect_after` before granting, and
  regaining a quorum counts as taking office. Do not shorten this to `ttl`: a
  failure detector reports the past, so a node can arm its clock on a quorum it
  has already lost while the other leader is still granting.
- These three came out of `guaranteed_plus_leased_never_exceeds_the_cap`, not out
  of reading the design. Every one of them looked unnecessary until the test
  produced the schedule that needed it. Treat a change that removes one as
  needing a new proof, not a new argument.
- `QuotaLease::count` already returns zero once expired. Rely on that rather
  than checking expiry separately, so a forgotten check cannot over-subscribe.
- A failing seed is committed as a regression case, named for what it broke.
- Gossip digests feed cluster-wide admin aggregates as well as control, so the
  digest schema needs the care of a public API.

See ADR [0004](../../product/decisions/0004-swim-gossip-with-leader-leases.md)
and [0005](../../product/decisions/0005-home-node-affinity-by-reservation.md).
