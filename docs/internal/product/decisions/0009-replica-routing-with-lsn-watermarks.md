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
`pg_is_in_recovery()` from each replica every `replica_poll_interval` (default
250ms) into a lock-free cell. A replica is eligible for a session only if its
replayed LSN is at or past that session's watermark.

Tenants preferring throughput over strict read-your-writes can opt into bounded
staleness, where eligibility is `lag < max_replica_lag` and no watermark is
tracked.

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
