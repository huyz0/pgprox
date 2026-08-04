# pgprox-cache

Query result cache, built in M9 and closed. The trait it implements has lived
in `pgprox-core` since M0, which is what let it be added without touching the
session or pool layers.

It is worth about 7% of median latency and of CPU per statement on the
reference workload, and it does not move the pool lock that `M7.56` found half
this proxy's CPU in. See `product/perf/run-2026-07-29-cache.md` before assuming
a change here is worth making: the ceiling on this workload is the extended
protocol, which is all miss until `M9.12`, and the write rate, which empties a
tenant's entries roughly every other lookup.

## Rules specific to this crate

- **It promises bounded staleness and nothing stronger.** ADR
  [0021](../../product/decisions/0021-the-query-cache-promises-bounded-staleness.md)
  is the whole contract and this crate does not get to widen it. Off by
  default, opt-in per tenant, one node rather than the fleet, and the TTL is
  the guarantee. Invalidation on write is an improvement on that bound; nothing
  here, in a comment or in output a human reads, may call it read-your-writes.
- **Keyed by tenant, the database and role the grant resolved to, normalized
  SQL, parameter values, and `search_path`.** Omitting any of them is a
  correctness bug rather than a missed optimisation: the same SQL resolves to
  different tables under different paths and in different databases, and to
  different rows under different roles. The last two were absent until `M24.4`,
  which is ADR [0024](../../product/decisions/0024-a-cache-key-names-the-connection-that-would-have-answered.md).
- **Bounded by bytes, not by entries.** A cache holding ten thousand entries
  holds an unbounded amount of memory, and this runs on a node whose whole
  design is about what a connection costs.
- **What may be cached is decided before anything is stored.** A cache that is
  fast and occasionally wrong is worse than no cache. The class of the
  statement arrives as an argument, the way the pin allowlist does, because
  this crate may depend on `pgprox-core` and nothing else in the workspace.
- Table-dependency tracking from the parse tree is a later refinement and does
  not change the contract: it still only sees writes that pass through the
  proxy. It is a better improvement on the same bound.
