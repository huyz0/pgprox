---
title: Architecture
description: "How transaction pooling works here, why sessions pin, and how the upstream cap holds across nodes that share no memory."
---

Why pgprox is built the way it is, and what each design choice costs.

## The problem

Downstream connections are cheap and numerous. Upstream connections are scarce
and capped. One tenant's application fleet can open thousands; the Postgres
behind it has room for a few hundred across every tenant it hosts.

pgbouncer solves that for one database. Three things make this harder:

Credentials differ per tenant, so the proxy cannot hold one connection string.
They arrive as a JWT, so the proxy does not know the real password until a
client has connected. And the cap has to hold across several proxy pods that
share no memory, because breaching it can lock an operator out of the database
for every tenant on it.

## How a connection is served

```
client ──startup+JWT──▶ pgprox
                          │ 1. resolve the token
                          ├──────────────────▶ token service (Unix socket)
                          │    ◀── tenant, backend, credentials, pool policy
                          │
                          │ 2. borrow an upstream connection for a transaction
                          ├──────────────────▶ pool
                          │
                          │ 3. relay frames, return the connection at COMMIT
                          └──────────────────▶ Postgres
```

The connection comes back to the pool at every transaction boundary, so the
next statement from that client may run on a different one. That is what makes
the ratio work and it is also what everything below is about defending.

## The token service

pgprox does not validate JWTs. It sends the token, the database and user the
client asked for, and the client address to a service on a Unix socket, and
receives:

- which tenant the token belongs to
- the primary's host, database, role, password and TLS mode
- the replicas, for read routing
- per-tenant pool policy: connection cap, pooling mode, statement timeout
- how long the answer may be cached

**You implement this service.** pgprox ships a mock for testing and defines the
contract; the real one is yours, because it is where your control plane lives.

Validation is entirely the service's. The proxy enforces an algorithm allowlist
on the JWT header as defence in depth, rejecting `none` and the `HS*` family
before it makes the call, and verifies no signature itself. Two validators that
disagree about whether a token is valid is a vulnerability rather than
redundancy.

Answers are cached, keyed on a hash of the token and the requested database
rather than on the tenant. Keying by tenant would let a revoked token keep
working off another valid token for the same tenant. Concurrent resolves for
the same token collapse into one call, so a reconnect storm produces one
request rather than thousands.

## Transaction pooling, and what breaks under it

A connection returns to the pool when the server reports it is idle, with no
extended-query sequence outstanding and the session unpinned. Never on SQL text
or a guess.

Some features attach state to the connection itself, and from the moment they
are used no boundary is safe. `LISTEN` registers interest on one backend, so a
session that moved would silently stop receiving notifications. A temp table
lives in one backend's temp schema. A `WITH HOLD` cursor outlives its
transaction on purpose.

pgprox detects these and pins the session to its connection for the rest of its
life. There is no unpinning: `UNLISTEN *` looks like it should undo a `LISTEN`,
but a notification may already be queued.

Pinning is expensive, which is why every pin is counted by reason. A rising pin
rate is transaction pooling degrading back toward session pooling, and the
label says which application feature is doing it.

### Prepared statements are the exception that makes the rest affordable

Every mainstream driver prepares by default. pgx, asyncpg, JDBC, npgsql and
SQLAlchemy all send a named `Parse`, so a proxy that pinned on one would pin
nearly every real session and transaction pooling would quietly become session
pooling with nothing visibly failing.

So pgprox rewrites instead. The client's chosen name is mapped to a global name
derived from a hash of the SQL, each upstream connection tracks which global
names it holds, and any `Parse` the target does not have is replayed before the
client's `Bind` reaches it.

Deriving the name from the SQL means two tenants running the same application
query share one server-side statement, which at 5,000 tenants is most of them.

### Session settings are replayed, not pinned

`SET search_path = tenant_acme` is among the most common things an application
does on connect, and pinning on it would cost most of the ratio. A small set of
parameters is recorded and reissued on acquire, and only the ones that differ
from the target connection's current values. Everything outside that set pins.

`SET LOCAL` is not recorded: it is scoped to the transaction, so by release
time it has already been undone.

## Holding the cap across nodes

Several pods, no shared memory, one cap that must not be breached.

Each upstream server's cap is split. A `guaranteed_fraction` is divided evenly
across the configured fleet size as a floor each node may use without asking.
The remainder is a free pool that nodes lease from an elected leader.

Nodes exchange state by pairwise gossip. A node that cannot reach its peers
keeps its guaranteed share and stops leasing, so a partition makes it serve
less and never more.

The fleet size is configured rather than discovered, because a node that
discovered its own fleet would be deciding what fraction its guaranteed share
is of.

### Tenant affinity without moving clients

The Postgres protocol has no redirect, so a proxy cannot hand a client to a
peer. With a normal load balancer, one tenant's clients land on every pod and
each pod opens its own upstream pool for that tenant. That is not a correctness
problem and it is up to a five-fold waste of upstream pools.

Each tenant gets a home node by rendezvous hashing over live membership, so a
membership change rehomes only the tenants that lived on the departed node. The
home node reserves most of that tenant's budget; other nodes share the rest and,
on hitting it, queue for an existing connection rather than opening a new one.

Reservations are use-it-or-lose-it. If a home node's gossiped usage stays below
its reservation, peers claim the slack.

A node may also shed: a client that has been idle at a transaction boundary
past a threshold, whose tenant's home node has room, is closed with `57P01` so
the driver reconnects and gets another roll of the load balancer. Never a
pinned session, never one mid-transaction, never toward a draining node.

## Replica routing without stale reads

A statement goes to a replica only if a lexical scan classifies it read-only
and every replica considered has replayed past the session's own write
position.

The classifier is a scan, not a parser. It requires the first word to be on a
short allowlist, no word anywhere on a denylist of things that write or lock,
and no call to a function known to have side effects. Every ambiguity resolves
to the primary, because a false negative costs a little throughput and a false
positive returns stale data.

The honest limit is that a lexical scan cannot know what a tenant's own
functions do. A tenant calling a write-performing function from a `SELECT` gets
it routed as a read. `SET pgprox.route = 'primary'` is the escape hatch, and a
`/* pgprox:replica */` comment is the per-statement override.

## Memory at 100,000 connections

An idle connection holds a socket and a small amount of state. Read and write
buffers are borrowed from a pool when a socket has something to say and returned
when it goes quiet, so 100,000 idle connections do not hold 100,000 buffers.

When the pool is empty a connection waits rather than allocating, which turns a
synchronised burst into latency instead of a memory spike. That is the correct
direction to fail and it is the whole point of the bound.

What this costs in practice is in [Performance](performance.md).

## Crate layout

Business logic is sans-I/O: it takes bytes and returns decisions, and can be
tested without a socket. Two crates compose the rest.

| Crate | |
| --- | --- |
| `pgprox-core` | Contracts, ID types, `SecretString`, the buffer pool, the SQL lexer |
| `pgprox-proto` | Wire codec, frame relay, extended-protocol state |
| `pgprox-auth` | JWT header checks, token service client, grant cache, SCRAM |
| `pgprox-pool` | Pool lifecycle, idle reap, pinning, statement mapping |
| `pgprox-route` | Statement classification, replica watermarks |
| `pgprox-cluster` | Gossip, leases, leader election, reservations |
| `pgprox-cache` | Query result cache |
| `pgprox-session` | The per-client state machine and the relay loop |
| `bin/pgprox` | The composition root |

Rules that hold across all of them are in `standards/`, and the decisions with
their reasoning are in [`product/decisions/`](../product/decisions/).
