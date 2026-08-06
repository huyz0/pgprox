---
title: Features and limits
description: "What pgprox does with pooling, replicas, LSN watermarks and caching, and the things it deliberately does not do."
---

What pgprox does, what it refuses to do, and what it has not been built to do
yet. The last two are different and the difference is kept.

## Pooling

**Transaction pooling** is the default and the point. An upstream connection is
borrowed when a client needs one and returned when the server reports it idle,
with no extended-query sequence outstanding and the session unpinned. Never on
SQL text or a guess about what a statement did.

**Session pooling** is available per tenant, through the pool policy the token
service returns. One upstream connection per client for its lifetime. Fully
transparent and it gives up almost all of the multiplexing.

**Statement pooling is not supported.** pgbouncer offers it; pgprox does not.
It breaks any multi-statement transaction, and a proxy that resolves credentials
per tenant has no way to warn a tenant that its own transactions will not work.

### When a session pins

Pinning attaches a session to one upstream connection for the rest of its life.
There is no unpinning: `UNLISTEN *` looks like it should undo a `LISTEN`, but a
notification may already be queued.

| Trigger | Why the connection cannot move |
| --- | --- |
| `LISTEN`, or a notification arriving | Notifications reach only the backend that registered the interest |
| Session-scoped advisory lock | Held until unlocked or the session ends. The `_xact_` variants release at commit and do not pin |
| Temp table, or anything in the temp schema | Lives in one backend's temp schema and is invisible from any other |
| `DECLARE ... WITH HOLD` | Outlives its transaction on purpose, so it outlives the release point |
| SQL-level `PREPARE` | Named at the SQL level, so the proxy cannot rewrite it, and the name lives on one backend |
| A `SET` outside the replayable set | The setting cannot be reproduced on another connection |
| `SET pgprox.pin` | The client asked |

Every pin is counted in `pgprox_pin_total{reason}`. A rising rate is
multiplexing degrading toward session pooling, and the label says which
application feature is doing it. What that costs is measured in
[Performance](performance.md#what-pinning-costs).

### Prepared statements

Protocol-level prepared statements do **not** pin, which is what makes the rest
affordable. Every mainstream driver sends a named `Parse` by default, so pinning
on one would pin nearly every real session.

The client's chosen name is rewritten to a global name derived from a hash of
the SQL. Each upstream connection tracks which global names it holds, and any
`Parse` the target does not have is replayed before the client's `Bind` reaches
it. Two tenants running the same application query share one server-side
statement.

The map is bounded per connection and evicted LRU, because Postgres holds
prepared statements in backend memory and an unbounded map is a slow leak
multiplied by every connection.

### Session settings

A small set of parameters is recorded and reissued when the session lands on a
new connection, and only the ones that differ from what that connection already
has. Everything outside the set pins instead.

`SET LOCAL` is never recorded. It is scoped to the transaction, so by the time
the connection is released it has already been undone, and replaying it would
apply a deliberately temporary setting to a connection where its transaction no
longer exists.

## Replicas and LSN watermarks

pgprox can send reads to replicas rather than to the primary, deciding statement
by statement, with nothing to change in the application.

The difficulty is lag. A replica replays the primary's write-ahead log and is
always some distance behind, so a read sent to one that has not caught up can
return data older than what the same session just wrote. The watermark below is
what stops that.

A statement reaches a replica only when both halves hold.

**The statement must classify read-only.** A lexical scan, not a parser. The
first word must be on a short allowlist (`SELECT`, `WITH`, `TABLE`, `VALUES`,
`EXPLAIN`), no word anywhere may be on a denylist of things that write or lock,
and no call may name a function known to have side effects. Every ambiguity
resolves to the primary.

**The replica must have caught up to the session.** Each session carries a
watermark: the write position of its own last write, or nothing if it has never
written. A replica serves only if it is healthy and its replayed position is at
or past that watermark.

```
session writes ──▶ watermark = LSN of that write
                      │
next read ────────────┤ replica.replayed >= watermark ?  ──▶ replica
                      └ otherwise ─────────────────────────▶ primary
```

A session that has never written has no watermark and any healthy replica will
do. A session that has just written reads its own writes, because no replica
behind that write can serve it.

**Its own writes, and no one else's.** The watermark is per session, so a write
by another session, another pgprox node or a client connected straight to
Postgres does not move it, and a read can still be older than that write. That is
what asynchronous replication is rather than a routing error, and
[Read routing](read-routing.md#whose-writes-the-watermark-covers) covers the
scope and the one case where a reconnect loses the floor.

Four things send a statement to the primary regardless of what it does: the
session is pinned, a transaction is open, the hint says primary, or no replica
has caught up.

[Read routing](read-routing.md) is the mechanism behind all of this: how a node
learns where each replica has got to, the four states that take one out of
service, and which replica gets picked when several qualify.

### Overriding the decision

| | |
| --- | --- |
| `SET pgprox.route = 'primary'` | Everything from this session goes to the primary |
| `SET pgprox.route = 'replica'` | Prefer a replica, where consistency still allows |
| `SET pgprox.route = 'auto'` | The default |
| `/* pgprox:replica */ SELECT ...` | One statement, outranking the session setting |

A hint asks; it does not assert. `pgprox.route = 'replica'` on a write still
goes to the primary, and a replica behind the watermark is still skipped.

### The limit worth knowing

A lexical scan cannot know what a tenant's own functions do. A tenant calling a
write-performing function from a `SELECT` gets it routed as a read.
`SET pgprox.route = 'primary'` is the escape hatch, and it is the honest limit
of deciding from text rather than from a plan.

## Query cache

pgprox can answer a repeated read out of its own memory without going to a
server at all. The node keeps the result frames of a statement and replays them
to the next session that asks the identical question.

Off by default, and off for every tenant that has not opted in. A tenant opting
in is stating that reads this stale are acceptable for its own workload, which
nobody else can decide for it.

**It promises bounded staleness and nothing stronger.** The TTL is the bound and
it is the only guarantee. Writes seen by the same node invalidate the tenant's
entries, which improves on the bound and does not change it: another node's
writes are not seen, so this is not read-your-writes and nothing in the product
calls it that.

An entry is keyed on the tenant, the database, the role, the normalized SQL, the
parameter values and the `search_path`. All six. The same SQL under a different
role is a different answer, because row-level security and column privileges
belong to the role.

### What is never cached

| Reason | |
| --- | --- |
| Not read-only | The classifier's verdict, including anything it could not place |
| The session has written | Its reads can see rows nobody else can |
| A transaction is open | Same visibility problem, plus a stored answer carries a transaction status that would lie to the client |
| The session is pinned | It is not sharing connections, so an entry from it is not a shared answer |
| Multiple statements in one message | The answer is a sequence, not one result |

Bounds are bytes rather than entries, because nothing bounds the size of one
result. There are two budgets: `max_bytes` for the store, and `max_entry_bytes`
spent per session while an answer is in flight and being considered.

Extended-protocol queries are cached by withholding the sequence from the
upstream until the client ends it, then assembling a hit from what the client
actually sent. Withholding starts only from an idle session with no connection
held and no transaction open.

What the cache is worth is in
[Performance](performance.md#what-the-query-cache-is-worth).

## Protocol support

What a driver can expect to work when it points at pgprox instead of at Postgres.

| | |
| --- | --- |
| Protocol 3.0 | Yes |
| Protocol 3.2 | Negotiated down to 3.0 |
| Simple query | Yes |
| Extended query (`Parse`/`Bind`/`Execute`) | Yes, with statement rewriting |
| `COPY` in and out | Yes, streamed |
| `FunctionCall` | Relayed opaque, never parsed, always routed to the primary |
| Cancellation | Yes, with keys this proxy issues from a CSPRNG |
| Replication connections | Recognised, pinned for life, relayed byte for byte |

Protocol 3.2 is deferred rather than rejected. It gets implemented when a
mainstream driver refuses to negotiate down, when Postgres deprecates 3.0, or
when a 3.2 feature is needed. The design work is written down so it is not
re-derived when one of those fires.

Replication is out of scope in the sense that matters: no replication message
type is decoded. A replication connection is recognised from its startup
parameter, before any replication message flows, then pinned and passed
through. It is accounted separately so a hundred replication connections do not
make the pool statistics lie.

## Authentication

Clients authenticate with a JWT in the password field. The proxy checks the
header's algorithm against an allowlist, rejecting `none` and the `HS*` family,
then asks your token service to validate it and say which backend it maps to.
The proxy verifies no signature itself.

A static role may authenticate with **SCRAM-SHA-256** instead, for admin tooling
and monitoring that cannot carry a token.

**Not supported:** `md5`, which Postgres deprecated in 14; and
`SCRAM-SHA-256-PLUS`, because channel binding would tie the exchange to the
proxy's own TLS session rather than the database's, which states a guarantee
that is not being made.

[Security](security.md) has the rest: what a grant authorizes, how credentials
are kept out of logs, and what happens to bytes a client chose.
[Multitenancy](multitenancy.md) covers what keeps one tenant's data and capacity
away from another's.

## Clustering

Several nodes hold one upstream cap between them without sharing memory. A
guaranteed fraction is divided evenly as a floor each node uses without asking,
and the rest is leased from an elected leader.

Each tenant gets a home node by rendezvous hashing, which reserves most of that
tenant's budget so the tenant's connections concentrate rather than spreading a
pool across every pod. Other nodes queue against what is left rather than
opening more.

A node that cannot reach its peers keeps its guaranteed share and stops leasing.
A partition makes it serve less, never more.

## Not supported

Things that are decided against rather than unfinished:

**Sharding.** pgcat routes by sharding key; pgprox does not, and has no design
for it.

**Statement pooling.** See above.

**md5 authentication and `SCRAM-SHA-256-PLUS`.** See above.

**Decoding replication.** Recognised and relayed, never parsed.

**Read-your-writes from the cache.** The bound is the TTL. Where the cache and
read routing disagree, routing wins.

**A cache shared across nodes.** Each node's cache is its own, which is why the
staleness bound is per node and why invalidation only sees writes through the
same node.

**Automatic failover of a primary.** An upstream that goes away is reported to
the client rather than silently retried against something else.

## Not built yet

Things with no decision against them, which have simply not been done:

- A 100,000-connection run that serves rather than only holds. See
  [Performance](performance.md#what-has-not-been-measured).
- Protocol 3.2, on the triggers above.
- Table-dependency tracking for cache invalidation, which would improve the
  bound without changing the contract.
