# pgprox-cache

Query result cache. Post-MVP (M9). In the MVP this is a trait stub only.

Do not implement this before M8 closes. It is listed here so the trait exists in
`pgprox-core` from M0 and the cache can be added later without touching the
session or pool layers.

## Design notes for when it lands

- Keyed by tenant, normalized SQL, parameter values, and `search_path`. Omitting
  `search_path` from the key is a correctness bug: the same SQL resolves to
  different tables under different paths.
- Invalidation by TTL first. Table-dependency tracking from the parse tree later,
  and only if measurement justifies it.
- A cache that can return a stale read violates the same property replica routing
  protects. Treat staleness with the same seriousness as ADR 0009 does.
