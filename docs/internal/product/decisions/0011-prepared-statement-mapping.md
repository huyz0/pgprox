# 0011. Protocol-level prepared statement mapping is MVP scope

Status: accepted

## Context

Transaction pooling returns an upstream connection to the pool at every
transaction boundary, so a client's next statement may execute on a different
connection.

Protocol-level prepared statements break under that. The client sends `Parse`
with a statement name on one connection, then `Bind` and `Execute` against that
name later, possibly on a connection that has never seen the `Parse`.

The reason this cannot be deferred: every mainstream driver uses named `Parse`
by default. pgx, asyncpg, JDBC, npgsql, and SQLAlchemy all do. Without mapping,
the pool must pin any session that prepares a statement, which is nearly all of
them, and transaction pooling silently degrades into session pooling. The
headline benefit of the product disappears without anything visibly failing.

## Decision

Each upstream connection keeps a map of `global_stmt_name -> sql_hash`.

The proxy rewrites the client's local statement name to a global name derived
from the SQL hash. On acquire, any `Parse` the target connection does not
already hold is replayed before the client's `Bind` reaches it.

Eviction is LRU with a configurable per-connection cap, since Postgres holds
prepared statements in backend memory and an unbounded map is a slow leak.

This ships in the MVP, in `pgprox-pool`, alongside the pool itself.

## Consequences

- Transaction pooling actually delivers its ratio with real drivers rather than
  only in benchmarks that use simple query protocol.
- The pool now carries per-connection state that must stay consistent with the
  server's actual prepared statement set. A desync produces confusing errors, so
  it is tested against all five drivers in the conformance suite rather than
  against one.
- Replay on acquire adds latency to the first statement after a pool switch.
  Bounded by the number of statements the session actually uses, and warm
  connections converge to holding the common set.
- Deriving the global name from the SQL hash means two clients preparing
  identical SQL share one server-side statement, which is a real memory saving
  at 5,000 tenants.
- The LRU cap is a correctness knob, not a performance one. Set too low it
  causes constant re-preparation; set too high it grows backend memory across
  thousands of connections.
- The mapping and the rewriting live in different crates, and neither depends on
  the other. `pgprox-pool` owns the mapping: SQL hash to global name, which
  connection holds which name, and the LRU. That is a data structure over
  strings and hashes with no protocol knowledge in it. `pgprox-proto` owns the
  rewriting, and already decodes `Parse` and `Bind` with their statement names.
  `pgprox-session` joins the two, which is what a composer is for.

  This paragraph originally said `pgprox-pool` would gain a dependency on
  `pgprox-proto` and that M0 had settled how. It was wrong twice: M0 settled
  nothing of the sort, and `scripts/check-layering.sh` forbids the dependency
  outright, since only `pgprox-session` and `bin/pgprox` may compose crates.
  Corrected in M5.1 before any code was written against it. The split above
  needs no contract change, which is the sign that the layering rule was right
  and the consequence was the thing at fault.

## Alternatives rejected

**Pin any session that uses named prepared statements.** Trivially correct and
what PgBouncer did before 1.21. Rejected because it pins essentially every real
session, which defeats the purpose.

**Force clients into simple query mode.** Would sidestep the problem entirely.
Rejected because it requires driver configuration changes on the tenant side and
loses the performance benefit prepared statements exist for.

**Deferring to a post-MVP phase.** Rejected because the MVP would measure well
and behave badly, which is the worst combination: the failure is invisible until
production load, and by then the pool layer has tests built on the wrong
assumption.
