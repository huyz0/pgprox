# 0021. The query cache promises bounded staleness and nothing stronger

Status: accepted

## Context

`pgprox-core` has carried a `QueryCache` trait since M0 with no implementation
behind it. M9 adds one, for a reason that changed while the milestone waited:
the plan filed it as throughput work, and M7.56 then measured 45% of the
proxy's CPU in the upstream pool's lock, with the cost landing per connection
because contention tracks how many connections are queued. A cache hit is a
statement that never acquires a connection, so it neither queues nor contends.

That makes the cache attractive. It does not make it safe, and the hazard is
the one ADR 0009 spends its length on, arriving from a different direction.

A replica can be behind the write a session just made. ADR 0009 refuses to
guess about that: the session records an LSN floor after a write, a poller reads
each replica's replayed LSN, and a replica serves a session only once it has
caught up. Read-your-writes holds by construction.

A cache cannot do the same thing, and the reason is worth stating precisely
rather than hand-waving. A replica's staleness is *measurable*: `SELECT
pg_last_wal_replay_lsn()` returns exactly how far behind it is, on demand, from
the replica itself. A cache entry has no equivalent. It is a copy of bytes the
server produced at some past moment, carrying no version of the rows behind
them and no way to ask whether they have changed since. There is no
`pg_last_wal_replay_lsn()` for a `SELECT` result.

What the proxy can observe is its own traffic, and only some of it:

| Write | Visible to this node |
| --- | --- |
| Through this node | Yes |
| Through another node in the fleet | Only through gossip |
| From outside the proxy entirely | Never |

The third row is not an implementation gap. A migration, a nightly batch job, a
logical replication subscriber, an operator with `psql`, a trigger firing inside
the database: none of these pass through the proxy, and no amount of work here
makes them visible. Any design that needs them to be visible is a design that
cannot be built.

## Decision

**The cache promises bounded staleness. The TTL is the bound, and it is the
only guarantee.**

This is the mode ADR 0009 already offers tenants who prefer throughput to
read-your-writes, and it is offered on the same terms: opt-in, per tenant, with
the bound stated as a number the tenant chose.

Four consequences of that, each a rule rather than a preference:

**Off by default.** A config document with no `query_cache` section produces a
proxy that caches nothing. A tenant that has not opted in is never served from
the cache, whatever the SQL.

**One node, not the fleet.** Each node caches for itself and invalidates for
itself. Cluster-wide invalidation over gossip is not part of this, because a
partitioned node would keep serving entries whose invalidation it never
received, and the TTL is the only thing that bounds staleness under partition
anyway. A fleet of five nodes with a 5s TTL is a 5s bound; adding gossip makes
the common case fresher and the bound no tighter.

**Invalidation is an improvement on the bound, not a promise.** A write through
this node drops that tenant's entries here. That is worth doing because it
makes the common case, a tenant whose traffic all flows through the proxy,
behave much better than the TTL alone would. It must not be described anywhere
as read-your-writes, because a tenant that believed it would be wrong the first
time a batch job ran.

**Where the cache and read routing disagree, routing wins.** A session with a
watermark is a session that has written, and a session that has written is not
served from the cache at all until its transaction ends. The two mechanisms
protect the same property and the stricter one governs.

## Consequences

- A tenant opting in is making a statement about its own workload: that data
  this old is acceptable for these reads. That is a decision only the tenant can
  make, which is why it is per-tenant configuration and not a global switch.
- The documentation, the config comment and the `SHOW CACHE` output all have to
  say "bounded staleness" rather than anything warmer. A guarantee people infer
  is a guarantee they rely on.
- The TTL is doing real safety work, so it is bounded from above by
  configuration the operator controls, the way `grant_ttl_cap` bounds a
  sidecar's TTL. A tenant cannot ask for a day.
- Table-dependency tracking from the parse tree, which the plan mentions as a
  later refinement, would tighten invalidation and would not change any of
  this: it still only sees writes that pass through the proxy. It is a better
  improvement on the same bound.
- Because staleness is bounded by time rather than by correctness, the failure
  mode is visible and dull: a tenant sees data up to its TTL old. Compare the
  failure mode of guessing, which is a write that appears to vanish.

## Alternatives rejected

**Invalidate on write and call it read-your-writes.** The tempting one, because
for a tenant whose traffic all goes through one proxy node it is even true. It
is rejected because the conditions under which it holds are invisible to the
person relying on it: nothing warns a tenant that the nightly job connecting
directly has just made their cache wrong. A guarantee that holds until it
silently does not is worse than a weaker one that always holds.

**Cluster-wide invalidation over gossip, as the guarantee.** Rejected as a
guarantee for the partition reason above, and not implemented as an optimisation
because it adds a distributed failure mode to buy a shorter window in the
common case, when the common case is already handled by local invalidation.
Worth revisiting if measurement shows cross-node writes are frequent.

**Versioning entries against something from the database.** There is no cheap
server-side thing to version a `SELECT` result against. `pg_stat_all_tables`
change counters and `xmin` horizons are per-table and would need a query per
lookup, which costs the round trip the cache exists to avoid. Rejected as
self-defeating rather than as wrong.

**Caching everything and letting the TTL sort it out.** Rejected because some
results must never be cached at any TTL: anything from a session that has
written, anything volatile, anything on a pinned session. That is a
cacheability rule rather than a TTL question, and it is where a cache goes
wrong in ways a TTL cannot fix.
