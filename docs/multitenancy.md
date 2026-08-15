---
title: Multitenancy
description: "How one process holding every tenant's credentials keeps them apart, and where the isolation boundary actually sits."
---

One upstream Postgres server can host thousands of tenant databases, each with
its own role and its own password. The proxy holds all of those credentials in
one process, and multiplexes every tenant's clients onto one capped set of
upstream connections. That is the whole reason it exists, and it is also the
thing that makes a mistake expensive.

This page is about the second half: what keeps tenants apart, and where the
boundary really is.

## A client does not name its own tenant

The client sends a database name, a user name and a JWT. None of those decides
who it is.

The proxy passes the token, the requested database, the requested user and the
peer address to your token service, and the service returns a grant: the tenant
identity, the real backend, the credentials to reach it, and the pool policy.
Everything downstream reads the tenant from that grant.

So a client cannot claim a tenant by asking for one. It can ask for a database
that its token does not map to, and the answer comes back from the service that
knows, not from the startup packet.

The proxy parses the token's claims for logging and policy, and verifies no
signature itself. Two validators that disagree about whether a token is valid is
a vulnerability rather than redundancy, so there is one, and it is yours.

## Where tenants meet

Every shared structure in the proxy is somewhere two tenants could touch. Each
one has a key, and the key is the isolation.

| Shared thing | What keeps tenants apart |
| --- | --- |
| Upstream connection pool | Keyed on server, database and role. A connection is never handed to a session that would connect as anything else |
| One connection over time | The release rule below |
| Grant cache | Keyed on `sha256(token)` plus the requested database and startup user, never the tenant |
| Query cache | Keyed on tenant, database, role, normalized SQL, bound parameters and `search_path` |
| Prepared statement map | Per connection, and a connection belongs to one pool key |
| Cancel keys | Issued from a CSPRNG, resolvable only while the connection is held |
| Metrics | A tenant becomes a label only from a bounded allowlist; everything else aggregates |
| Logs | Query text is debug level **and** opt-in per tenant, two switches |
| Admin surface | A static operator credential reaches no database at all |

Two of those are worth more than a table row.

**The grant cache is keyed by token hash, not by tenant.** Keying by tenant
would let a revoked token keep working for as long as some other valid token for
the same tenant sat in the cache. That is a revocation bypass wearing a cache
optimization's clothes. Hashing rather than storing the token means a memory
dump of the keys is not a dump of credentials.

**The query cache key carries the role.** Row-level security and column
privileges are properties of the role, so the same SQL under two roles is two
different answers, and sharing one entry between them publishes rows one of them
is not allowed to see. The database is in the key for the same reason: one
tenant reaching two databases gets two backends, and `SELECT * FROM t` names a
different table in each. All seven components are load-bearing, and
[Features](features.md#query-cache) lists them.

## The release rule

A connection goes back to the pool only when the server reports
`ReadyForQuery('I')`, no extended query sequence is outstanding, and the session
is unpinned. Never on SQL text, never on a guess about what a statement did.

Anything else is closed rather than returned. Handing a connection that is
sitting inside someone else's transaction to a second client gives them a
session already holding locks, part way through a unit of work they know nothing
about. Nothing about that looks like an error to either side, which is what
makes it the worst failure the pool can produce.

The guard that carries a checked-out connection defaults to discarding it, so a
guard dropped by a cancelled future, an early return or a panic closes its
connection. Reuse takes an explicit call at a point the caller has established
is safe. The safe direction is the one you get by doing nothing.

## Session state does not survive the handoff

A session that lands on a different upstream connection has to look the same to
its client. Two things are replayed and everything else pins.

Session parameters from a small replayable set are reissued on the new
connection, and only the ones that differ from what it already has. `SET LOCAL`
is never recorded, because it belongs to a transaction that has already ended by
the time the connection is released.

Prepared statements are rewritten. The client's chosen name is replaced with a
global name derived from a hash of the SQL, each connection tracks which global
names it holds, and any missing `Parse` is replayed before the client's `Bind`
arrives. Two clients preparing the same query share one server-side statement,
which at five thousand tenants running one application is most of them.

That sharing happens inside a pool, and a pool is one database and one role. It
is not sharing across a security boundary; it is two sessions that were already
going to connect as the same Postgres identity.

Anything outside those two sets pins the session instead. See
[Features](features.md#when-a-session-pins) for the full list.

## Capacity, and the noisy neighbour

Isolation of data is not the same as isolation of capacity. Three things keep
one tenant from consuming the fleet.

**A per-tenant cap, when your token service sets one.** The grant carries
`max_upstream`, and a tenant with one cannot exceed it regardless of how many
clients it opens.

**Home node reservation.** Each tenant gets a home node by rendezvous hashing,
and that node reserves most of the tenant's budget. Other nodes work from what
is left and multiplex harder rather than opening more. Reservations decay after
a few gossip rounds of non-use, so an idle tenant's home node does not hold
capacity hostage.

**The cap converts into waiting, not into breach.** Past the limit a client
waits, and past the fleet limit it is told `53300 too many connections`. Neither
path opens another upstream connection. A burst becomes latency for the tenant
causing it rather than a breach that takes the database down for everyone on it.

## Where the boundary actually is

The isolation boundary is the database credential, not the tenant name.

If your token service maps two tenants onto the same server, database and role,
they share a pool, they share prepared statements, and Postgres itself cannot
tell them apart. The proxy will not invent a boundary that the credentials do
not have. Tenants that must not see each other's rows need distinct roles, and
that is a decision made in the service that issues grants.

What the proxy does not isolate at all:

- **CPU and I/O on the database.** A tenant running expensive queries is
  competing with every other tenant on that server, as it would be without a
  proxy in the path.
- **Statement rate.** There is no per-tenant throttle. The upstream cap bounds
  concurrency, not the work each connection asks for.
- **The `SHOW` surface.** It is an operator view of the whole fleet, reached by
  a static credential. It is not a per-tenant view and is not meant to be
  exposed to tenants.

## Related

[Security](security.md) covers authentication, credential handling and the
threat model. [Architecture](architecture.md#tenant-affinity-without-moving-clients)
covers how the cap is divided across nodes.
