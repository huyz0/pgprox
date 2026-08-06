# 0001. Transaction pooling with automatic pinning

Status: accepted

## Context

Downstream connections are cheap and numerous, target 50k to 100k per node.
Upstream connections are scarce, around 5k per Postgres host shared across up to
5,000 tenant databases. The proxy exists to absorb that ratio.

Session pooling, where one client owns one upstream connection for its lifetime,
gives almost no absorption: upstream connections track active downstream ones.
Transaction pooling, where an upstream connection is returned to the pool at
every transaction boundary, gives one to two orders of magnitude.

The cost is that session-scoped Postgres features stop working, because the
client's next transaction may land on a different upstream connection.

## Decision

Transaction-level multiplexing by default, with automatic pinning when a session
uses a feature that requires connection affinity.

The release signal is the transaction status byte in `ReadyForQuery`: release
only on `I`, with no extended-query sequence outstanding, and only when unpinned.
Not the SQL text, not a heuristic.

Pin triggers: `LISTEN`/`UNLISTEN`, session-scoped advisory locks, temp tables,
`WITH HOLD` cursors, SQL-level `PREPARE`, and `SET` outside the replayable
allowlist.

`COPY` is not among them. A pin never clears, and a COPY stream ends, so a
session that once ran one would hold its connection for life. It is a hold
while it runs, which is what the release signal above already covers.

Session parameters inside the allowlist are recorded and replayed on acquire
rather than pinning.

The allowlist is a type, `pgprox_pool::Replayable`, and not a `&[&str]`. Two
different things consult it: one decides whether a `SET` pins the session, the
other decides whether the same `SET` is recorded for replay. Given different
lists they disagree without saying so, and that bug is a session recorded as
movable whose settings are never replayed, so a client's `search_path` quietly
reverts between statements and nothing errors. A caller can obtain one only
from `Replayable::DEFAULT`, `Replayable::NONE`, or `from_names`.

## Consequences

- Upstream connection count tracks concurrent *transactions*, not connections,
  which is what makes the cap reachable at all.
- The session state machine becomes the most correctness-critical code in the
  project. It gets property tests and mutation testing.
- Protocol-level prepared statement mapping becomes mandatory rather than an
  optimization. Every modern driver uses named `Parse`, and without mapping the
  pool pins nearly every session and collapses back to session pooling. See
  [0011](0011-prepared-statement-mapping.md).
- A rising `pgprox_pin_total` is an early warning that multiplexing is degrading.
  It is instrumented by reason so the cause is visible.
- Clients relying heavily on `LISTEN`/`NOTIFY` get little benefit. If a large
  fraction of tenants do, the pool sizing model needs revisiting.
  Measured by `M11.7`, on one node's worth of clients rather than the tenant
  population the plan describes, and the sizing answer is simpler than "needs
  revisiting" suggested. The cost is linear with no threshold:
  `upstream = c0 + (1 - 1/r0) * pins`, where `c0` and `r0` are the unpinned
  arm's connection count and its clients per connection. At 40 clients that is
  `14 + 0.650 * pins`, fitting the three measured shares to R^2 = 0.994 with no
  free parameters. So a pinned session costs about two thirds of a connection
  from the very first one, there is no safe fraction below which sizing is
  unaffected, and sizing can simply carry the term.
  The collapse this ADR names is exact rather than figurative: with every
  session pinned the fleet held one upstream connection per client. What it does
  *not* cost, while the pool has headroom, is throughput, which stayed flat
  within 1.6% across the curve.
  See [run-2026-07-31-pinning-curve.md](../perf/run-2026-07-31-pinning-curve.md).

## Alternatives rejected

**Session pooling only.** Fully transparent, no broken features, much simpler.
Rejected because the cap benefit reduces to idle-client reaping, which does not
solve the stated problem.

**Transaction pooling with session features hard-rejected.** Simplest state
machine, maximum sharing. Rejected because it breaks working applications with
an error rather than a slowdown, and tenants cannot always change their code.

**Statement-level pooling.** Maximum sharing. Rejected because it breaks
multi-statement transactions, which is not a tradeoff any tenant would accept.
