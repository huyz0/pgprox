# pgprox-cache

Query result cache. M9, which is in progress: M8 has closed and this is being
built now. The trait it implements has lived in `pgprox-core` since M0, which is
what let it be added without touching the session or pool layers.

## Rules specific to this crate

- **It promises bounded staleness and nothing stronger.** ADR
  [0021](../../product/decisions/0021-the-query-cache-promises-bounded-staleness.md)
  is the whole contract and this crate does not get to widen it. Off by
  default, opt-in per tenant, one node rather than the fleet, and the TTL is
  the guarantee. Invalidation on write is an improvement on that bound; nothing
  here, in a comment or in output a human reads, may call it read-your-writes.
- **Keyed by tenant, normalized SQL, parameter values, and `search_path`.**
  Omitting `search_path` is a correctness bug rather than a missed
  optimisation: the same SQL resolves to different tables under different
  paths, so two tenants running identical text would share an entry pointing at
  different data.
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
