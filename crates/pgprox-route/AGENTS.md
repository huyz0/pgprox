# pgprox-route

Target selection, statement classification, replica LSN watermarks.

## The rule that matters

**When the classifier is not confident, route to the primary.**

A false negative costs a little throughput. A false positive is a stale read,
which is a data-correctness bug from the tenant's perspective and worse than the
slowness it was meant to fix.

The property test that must always hold: no DML-bearing statement is ever
classified read-only. This crate parses untrusted SQL, so it is fuzzed.

## Rules specific to this crate

- Classification is a fast token-prefix scan, not a full SQL parser. It must
  handle `WITH` CTEs containing DML, `SELECT ... FOR UPDATE`/`FOR SHARE`,
  `EXPLAIN ANALYZE`, and volatile function calls.
- **Do not write another SQL scanner.** `pgprox_core::sql` decides which text is
  SQL and which is data. This crate and `pgprox-pool` once had one each, and
  they disagreed about where an `E'...'` string ends.
- Route target is decided once per transaction, at the first statement. A
  transaction spanning two servers has no coherent semantics.
- A replica is eligible only if its replayed LSN is at or past the session's
  write watermark.
- Pinned and session-mode connections always use the primary unless explicitly
  marked read-only.

The route decision is a declared hot path.

See ADR [0009](../../product/decisions/0009-replica-routing-with-lsn-watermarks.md).
