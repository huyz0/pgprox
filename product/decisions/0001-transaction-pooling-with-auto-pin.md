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
`WITH HOLD` cursors, SQL-level `PREPARE`, `SET` outside the replayable
allowlist, and `COPY` in progress.

Session parameters inside the allowlist are recorded and replayed on acquire
rather than pinning.

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
  fraction of tenants do, the pool sizing model needs revisiting. This is an
  open question in the plan.

## Alternatives rejected

**Session pooling only.** Fully transparent, no broken features, much simpler.
Rejected because the cap benefit reduces to idle-client reaping, which does not
solve the stated problem.

**Transaction pooling with session features hard-rejected.** Simplest state
machine, maximum sharing. Rejected because it breaks working applications with
an error rather than a slowdown, and tenants cannot always change their code.

**Statement-level pooling.** Maximum sharing. Rejected because it breaks
multi-statement transactions, which is not a tradeoff any tenant would accept.
