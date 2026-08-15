# 0009. Read replica routing gated on LSN watermarks

Status: accepted

## Context

Read traffic dominates most workloads, and replicas are idle capacity. Routing
reads to them is the largest available throughput win after pooling itself.

The hazard is stale reads. An application that writes a row and immediately
reads it back will see the write vanish if the read lands on a lagging replica.
That is a data-correctness bug from the tenant's perspective, and it is worse
than the slowness it was meant to fix.

## Decision

Route targets are decided once per transaction, at the first statement.

**Watermarks.** After a write transaction commits on the primary, the session
records an LSN floor obtained by appending `SELECT pg_current_wal_lsn()` to the
commit round trip. A background poller reads `pg_last_wal_replay_lsn()` and
`pg_is_in_recovery()` from each replica every 250ms (`POLL_INTERVAL`, a
constant rather than a configured value — see Outstanding) into a lock-free
cell. A replica is eligible for a session only if its replayed LSN is at or
past that session's watermark.

Tenants preferring throughput over strict read-your-writes could opt into
bounded staleness instead, where eligibility is `lag < max_replica_lag` and no
watermark is tracked — see Outstanding, this mode is not built.

**Classification.** A fast token-prefix classifier, not a full SQL parser.
Anything it cannot classify confidently goes to the primary. It must correctly
handle `WITH` CTEs containing DML, `SELECT ... FOR UPDATE`/`FOR SHARE`,
`EXPLAIN ANALYZE`, and volatile function calls.

Explicit overrides: `SET pgprox.route = 'replica' | 'primary' | 'auto'`, and a
leading `/* pgprox:replica */` comment for a single statement. `BEGIN READ ONLY`
marks a transaction replica-eligible.

## Consequences

- Read-your-writes holds by construction rather than by hoping lag is small.
- Cost is one extra statement per write transaction for replica-eligible
  sessions, on the same round trip, and it is opt-in per tenant so workloads
  with no replicas pay nothing.
- The classifier parses untrusted SQL, so it is fuzzed and property-tested. The
  property that matters: no DML-bearing statement is ever classified read-only.
  A false negative costs a little throughput; a false positive is a correctness
  bug.
- Conservative-by-default means throughput gains are lower than a naive
  classifier would show. That is the intended trade.
- Replica lag becomes correctness-relevant, so `pgprox_replica_lag_bytes` is an
  alerting signal rather than a curiosity.
- Pinned and session-mode connections always use the primary unless explicitly
  marked read-only.

## Alternatives rejected

**Time-based staleness bounds only.** Much simpler, no per-session state.
Rejected as the default because "lag under 100ms" says nothing about whether
*this* session's write has arrived. It remains available as an opt-in mode for
tenants who prefer it.

**Route by statement, not by transaction.** Higher replica utilization.
Rejected because a transaction spanning two servers has no coherent semantics.

**Defer replica routing entirely to a later phase.** Rejected on the grounds
that the session and pool layers would need reworking to add watermarks
afterward, and that rework touches the most correctness-critical code in the
project.

## Outstanding

`M90.6`. Two claims above describe more than the code delivers.

`replica_poll_interval` reads as a configured setting with a stated default.
It is `POLL_INTERVAL`, a compile-time constant in `bin/pgprox/src/replicas.rs`
(and, deliberately at the same value and cross-referenced in its own doc
comment, in `primary_watch.rs`) — there is no `config.yaml` field or
command-line flag for it, and changing the cadence means changing the
constant and rebuilding.

The bounded-staleness opt-in this ADR describes as an alternative to strict
watermark gating — `lag < max_replica_lag`, no watermark tracked — is not
implemented. `pgprox-route` has no `max_replica_lag`-driven eligibility check
and no per-tenant mode to opt into one; every session that routes to a
replica at all does so under the strict watermark rule this ADR's "Decision"
section describes, with no time-based alternative. "Bounded staleness"
appears elsewhere in this codebase for a different mechanism entirely — the
query cache's TTL, per ADR 0021 — which resembles this one in vocabulary but
not in code; neither implements the other.
