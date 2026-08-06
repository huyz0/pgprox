# pgprox-cache

A query result cache with a bounded staleness guarantee and nothing stronger.

Off by default, and off for every tenant that has not opted in. A tenant opting
in is stating that reads this stale are acceptable for its own workload, which
nobody else can decide on its behalf.

[ADR 0021](../../docs/internal/product/decisions/0021-the-query-cache-promises-bounded-staleness.md)
is the contract and this crate does not widen it. The TTL on each entry is the
guarantee. Invalidation on write improves on that bound without changing it,
because another node's writes are not seen. Nothing here calls it
read-your-writes.

## Three modules, wrong in different ways

`store` stores and expires. `cacheable` decides what may be stored at all.
`normalize` decides what counts as the same question.

The split is deliberate. A store that expires badly serves stale data, which
the TTL bounds. A store handed something uncacheable serves wrong data, which
nothing bounds.

## The key has six parts

Tenant, database, role, normalized SQL, bound parameter values, and
`search_path`. All six are load-bearing.

The role is there because row-level security and column privileges belong to
the role, so the same SQL under two roles is two different answers and sharing
an entry between them publishes rows one of them cannot see. The database is
there because one tenant reaching two of them gets two backends, and the same
table name means different tables.

Parameters are kept in their wire form, length-prefixed. A SQL `NULL` and a
zero-length value are different questions, and a `Vec<Vec<u8>>` cannot tell
them apart.

## Bounds are bytes, not entries

Nothing bounds the size of one result. There are two budgets: `max_bytes` for
the store, and `max_entry_bytes` spent per session while an answer is in flight
and still being considered.

## Where it sits

Depends on `pgprox-core` and nothing else. Used only by `bin/pgprox`.

`Store` holds no settings of its own. The byte budget, the TTL cap and the
tenant list all arrive through `reconfigure` from whatever the config document
currently says, so a store that has never been reconfigured serves nobody.
