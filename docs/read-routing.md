---
title: Read routing
description: "How pgprox decides that a statement can go to a replica, how it learns where each replica has got to, and what it does when it cannot tell."
---

pgprox sends a statement to a replica only when it can show two things: that the
statement does not write, and that the replica has replayed far enough to answer
the session honestly. Neither is guessed. Where either is unknown, the statement
goes to the primary.

This page is the mechanism. [Features and limits](features.md) states the rules a
user needs, and this says how they are arrived at.

## The decision, end to end

Every statement takes the same path, and the first thing that answers wins.

1. **A session hint.** `SET pgprox.route = ...` is intercepted rather than
   forwarded and never reaches a server.
2. **An open transaction.** Its target was fixed at its first statement.
3. **A pinned session.** Pinned sessions hold one connection, and it is on the
   primary.
4. **Classification.** A lexical scan decides whether the statement writes.
5. **Eligibility.** Each replica's last known replay position is compared
   against the session's write watermark.
6. **Selection.** The first replica that passes serves it.

Steps 4 through 6 are the route decision, and it is a declared hot path: it runs
once per transaction on every connection, it allocates nothing, and it performs
no I/O at all. That last part is why replica state is polled in the background
rather than asked for on demand. A route decision that could block would make one
replica's latency into everyone's latency.

## Deciding whether a statement writes

A scan over tokens, not a SQL parser. Three tests, and a statement must pass all
three:

- Its **first word** is on a short allowlist: `SELECT`, `WITH`, `TABLE`,
  `VALUES`, `EXPLAIN`.
- **No word anywhere** in the statement is on a denylist of things that write or
  lock.
- **No call** names a function known to have side effects.

The denylist exists because the first word is not enough. `WITH x AS (INSERT ...
RETURNING *) SELECT * FROM x` opens with `WITH` and writes, `SELECT ... FOR
UPDATE` takes row locks, `SELECT ... INTO t` creates a table, and `EXPLAIN
ANALYZE` executes the plan for real rather than describing it. Each entry on the
list names the construct that put it there.

The function list is not every `VOLATILE` function. `random()` is volatile and
perfectly safe on a replica. These are the ones with side effects: `nextval` and
`setval`, the whole `pg_advisory_lock` family, and anything that assigns a real
transaction ID, which a replica cannot do. Bare names are matched, so
`pg_catalog.nextval` is caught alongside `nextval`.

**Every ambiguity resolves to the primary.** A false negative costs a little
throughput on one statement. A false positive returns stale data, which is a
correctness bug the client has no way to detect. The two are not comparable and
the classifier does not treat them as though they were.

A property test asserts the direction that matters: no statement carrying DML is
ever classified read-only. The classifier reads untrusted SQL, so it is also
fuzzed.

## Reading your own writes

Classification only says the statement is safe to run on a replica. It says
nothing about whether *this* replica can answer *this* session correctly.

Each session carries a watermark: the WAL position of its own last write, or
nothing if it has never written. A replica may serve the session only once it has
replayed at least that far.

```
session writes ──▶ watermark = LSN of that write
                      │
next read ────────────┤ replica.replayed >= watermark ?  ──▶ replica
                      └ otherwise ─────────────────────────▶ primary
```

Without the floor, a client inserts a row, reads it back from a replica that has
not caught up, and finds its own write missing. That failure is worse than the
latency the routing was meant to save, and preventing it is the reason the
watermark exists.

A session that has never written has no watermark, so any healthy replica will
do. A session that has just written reads from the primary until the replicas
catch up, which usually takes milliseconds and needs no configuration.

A statement is marked as writing at classification time, before the target is
chosen, because a write is a write whether or not it ends up where it was
expected to. The watermark itself is recorded afterwards, when the server has
said where the write landed. Between those two moments the session knows it has
written and does not yet know how far, and it routes to the primary rather than
guessing.

## Learning where each replica is

Replicas are not configured. They arrive from the token service in the grant,
alongside the credentials to reach them, so a node learns a replica set the first
time a session presents one.

A background loop asks each replica the same question every 250 ms, over a
connection it keeps rather than dialling per poll:

```sql
SELECT pg_last_wal_replay_lsn(), pg_is_in_recovery()
```

The second half is not decoration. A promoted replica keeps answering queries and
its replay position keeps looking reasonable, but it is no longer a replica: it
is a second primary, and routing reads to it is how a split brain starts serving
two versions of the truth.

The watch is keyed by the primary rather than by tenant, because a replica set is
a property of the database and not of whoever presented the grant. Two tenants on
one primary share a watch and a poll loop, which is what stops a thousand
sessions becoming a thousand `pg_last_wal_replay_lsn()` queries a second.

## When pgprox will not use a replica

Four states take a replica out of service, and all four look identical to the
route decision: unhealthy, and therefore not eligible.

| State | Why |
| --- | --- |
| Never polled | A node that has just learned a replica set has no readings yet, so every entry starts unhealthy and the first statements go to the primary |
| The probe failed | The reading is cleared rather than kept. A replica that stopped answering has an unknown position, and its last known one is the most misleading thing available |
| The reading is stale | A poll result is out of date the moment it is taken. Older than one second, four poll intervals, and the replica is set aside |
| Promoted | `pg_is_in_recovery()` returned false |

The freshness window covers the case nothing else does. If the poller itself dies
the readings simply stop arriving, and without a window every replica would keep
serving from whatever the fleet believed at the moment polling stopped. Instead
they age out and traffic returns to the primary, which is the safe direction.

One missed poll is not enough to matter: the window is four times the interval,
so it takes three consecutive misses.

## Which replica

The first eligible one, in the order the grant lists them.

There is no least-lag or round-robin selection, and saying so is more useful than
implying a policy that does not exist. Where reads need spreading across several
replicas today, that is a job for whatever fronts them, not for pgprox.

A replica that has never been polled stays in its slot as unhealthy rather than
being removed, so indices never shift under a session mid-decision.

## Asking for something different

Two forms, because they answer different questions. A whole connection dedicated
to reporting wants a session setting; one heavy statement in an otherwise normal
session wants a per-statement one.

| | |
| --- | --- |
| `SET pgprox.route = 'primary'` | Everything from this session goes to the primary |
| `SET pgprox.route = 'replica'` | Prefer a replica, where consistency still allows |
| `SET pgprox.route = 'auto'` | The default |
| `RESET pgprox.route` | Back to `auto`, and `RESET ALL` does it too |
| `/* pgprox:replica */ SELECT ...` | One statement, outranking the session setting |

**A hint asks; it does not assert.** `pgprox.route = 'replica'` on an `UPDATE`
still goes to the primary, and a replica behind the session's watermark is still
skipped. The hint chooses between destinations that are already correct. It
cannot make an incorrect one available, which is what stops a hint from becoming
a way to ask for stale data by accident.

`SET pgprox.route` is intercepted and answered by the proxy. Postgres would
happily accept it as a custom parameter and store it, and then the setting would
exist on one connection and mean nothing at all.

## Why a transaction cannot change its mind

The target is decided once, at the first statement, and every later statement in
that transaction follows it.

A transaction spanning two servers has no coherent semantics: no shared snapshot,
no shared locks, and no way to roll both halves back together. So the first
statement decides, and if it is a write the whole transaction is on the primary
whatever the rest of it does.

## Watching it

| Metric | What it tells you |
| --- | --- |
| `pgprox_route_total{route}` | Where statements went: `primary`, `replica`, or the query cache. The ratio is the answer to "is replica routing doing anything" |
| `pgprox_replica_lag_bytes{replica}` | How far each replica trails. Lag turns into primary load, because a session behind a lagging replica routes away from it |

Read them together. Reads collapsing onto the primary with lag climbing is a
replication problem showing up as a routing symptom. Reads collapsing onto the
primary with lag flat is more likely classification: something in the workload
stopped looking read-only, or a session set the hint and never reset it.

There is no `SHOW` command for replica state. The metrics above are how a node's
view of its replicas is read, and [Admin and management](admin.md) lists what the
admin surface does answer.

## The limit worth knowing

A lexical scan cannot know what a tenant's own functions do. A tenant calling a
write-performing function from inside a `SELECT` gets it routed as a read, and
pgprox has no way to see that from the text.

`SET pgprox.route = 'primary'` is the escape hatch for a session that does this,
and this is the honest limit of deciding from text rather than from a plan. The
alternative, asking the server to plan every statement before routing it, costs a
round trip on the hot path to answer a question that is rare.

See ADR
[0009](internal/product/decisions/0009-replica-routing-with-lsn-watermarks.md)
for the decision and what was weighed against it.
