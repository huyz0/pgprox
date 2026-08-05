---
title: Request flow
description: "What happens inside pgprox from the moment a client connects to the moment a connection goes back to the pool, and which component does each part."
---

What happens between a client connecting and its answer arriving, and which
crate does each part. Useful when a metric moves and you want to know what
touched it, or when you are about to change something and want to know what
else is on that path.

Two phases: a connection is established once, then a loop runs per frame until
the client goes away.

## Establishing a connection

```
  client
    │  TCP
    ▼
  listener ─────────▶ admission gate        bin/pgprox
    │                 refuse past the cap, do not drop
    ▼
  startup negotiation                       pgprox-session::shell
    │  SSLRequest? ──▶ TLS accept           pgprox-tls
    │  CancelRequest? ──▶ look up, cancel, done
    ▼
  authentication                            pgprox-session::auth
    │  JWT in the password field
    │  algorithm allowlist                  pgprox-auth::jwt
    ▼
  resolve the token                         pgprox-auth::client
    │  grant cache, keyed sha256(token)+db  pgprox-auth::cache
    │  miss ──▶ gRPC over a Unix socket ──▶ your token service
    ▼
  register                                  bin/pgprox::sessions, ::cancel
    │  cancel key from a CSPRNG
    ▼
  the relay loop
```

**Admission comes before anything else.** A node past its client cap refuses
with a message the driver can read rather than dropping the socket, because a
dropped socket reads as a network fault and sends the operator to the wrong
place.

**The handshake has one deadline, not one per read.** A client sending a byte a
second would pass every per-read timeout and hold its slot indefinitely. The
deadline covers what the client owes the proxy and stops there; what the proxy
then does on the client's behalf is its own latency.

**Nothing is decoded before it is framed.** The startup packet is read against a
32 KiB limit rather than the gigabyte a `DataRow` may need, because an
unauthenticated peer should not be able to make a node grow a buffer to whatever
it felt like sending.

**Token resolution is cached and collapsed.** The key is a hash of the token
plus the requested database, never the tenant, so a revoked token cannot keep
working off another valid token for the same tenant. Concurrent resolves for the
same token become one call, so a reconnect storm produces one request rather
than thousands.

At this point the session holds a grant, a connection id, a cancel key, and no
upstream connection at all.

## The relay loop

Once per client frame:

```
   ┌─▶ read the header                      pgprox-session::shell
   │     5 bytes: tag and length
   │        │
   │        ▼
   │   read as much body as policy wants    pgprox-proto::frame
   │     Query, Parse: all of it
   │     DataRow, CopyData: a prefix, or none
   │        │
   │        ▼
   │   decode                               pgprox-proto::frontend
   │        │
   │        ▼
   │   classify and route                   pgprox-route
   │     lexical scan ──▶ read-only?
   │     watermark vs each replica's LSN
   │        │
   │        ▼
   │   cache?                    ┌──hit──▶ answer, no connection touched
   │     pgprox-cache           │           pgprox-cache::store
   │        │ miss/ineligible ◀─┘
   │        ▼
   │   acquire, if not holding one          pgprox-pool::live
   │     quota check across the fleet       pgprox-cluster
   │     replay session parameters          pgprox-pool::params
   │     replay missing Parse               pgprox-pool::statements
   │        │
   │        ▼
   │   forward, then stream the answer back pgprox-proto::relay
   │     header first, body only if wanted
   │        │
   │        ▼
   │   ReadyForQuery('I'), unpinned,        pgprox-pool
   │   no sequence outstanding?
   │        ├── yes ──▶ release the connection, return buffers
   └────────┴── no ───▶ keep holding it
```

### Reading

The header is five bytes and is read on its own. What happens to the body then
depends on the tag, because reading every body in full would mean holding a 16
MiB `DataRow` twice: once in a buffer and once again in the write buffer it is
copied into.

A `Query` or a `Parse` is read whole, because the SQL decides everything
downstream. A `DataRow` or a `CopyData` is streamed: the header goes out, the
body follows as it arrives, and nothing lands. That is every row of every
uncached statement.

Buffers come from a pool when a socket has something to say and go back when it
goes quiet. An idle connection holds no buffer, which is what makes 100,000 of
them affordable.

### Deciding

The classifier reads the statement's text and answers read-only, write, or
unknown. Unknown routes like a write, so a construct nobody has taught it yet is
safe by default.

The router then compares the session's write watermark against each replica's
replayed position, and takes the first that has caught up. Four conditions send
the statement to the primary regardless: pinned session, open transaction,
primary hint, or no replica caught up. See
[Features](features.md#replicas-and-lsn-watermarks).

The same scan decides whether the session must pin, and a separate pass decides
whether a `SET` can be replayed or has to pin instead.

### The cache, when a tenant opted in

Checked before a connection is acquired, which is the point: a hit answers
without touching the pool at all, so a cached read costs no upstream connection
and no database work.

A hit is served from the store. A miss records the answer as it streams past, up
to the per-answer cap, and stores it if the statement was eligible.

For extended-protocol queries the sequence is withheld from the upstream until
the client ends it, and a hit is assembled from the sequence the client actually
sent. Withholding starts only from an idle session with nothing held open.

### Acquiring

Only when the session does not already hold a connection. Most frames in a
multi-statement transaction skip this entirely.

The pool checks its own cap, then the cluster layer checks the fleet's: a node
uses its guaranteed share without asking and leases from the leader beyond it.
Past the cap a client waits rather than the node opening another connection,
which turns a burst into latency instead of a breach.

Once a connection is in hand, two replays run before the client's frame reaches
it. Session parameters that differ from what that connection already has, and
any prepared statement the session holds that this connection does not. Both are
no-ops on a warm connection serving the same tenant, which is the common case.

### Releasing

The connection goes back when the server says `ReadyForQuery('I')`, no extended
sequence is outstanding, and the session is unpinned. Never on SQL text.

A connection released mid-transaction is closed rather than returned. That is
the cancellation-safety case that matters most: a client that disappears
mid-transaction must not hand the next session a connection with an open
transaction on it.

Buffers go back to the pool at the same time, and an idle connection is left
holding a socket and a small struct.

## What runs alongside

Three things are not in the request path and change what it does.

**Replica polling.** A prober asks each replica for its replayed position on an
interval. The router reads what the prober last saw rather than asking during a
decision, so a routing decision costs no network call.

**Gossip.** Nodes exchange usage and membership pairwise, and the leader hands
out leases against the free pool. A node that cannot reach its peers falls back
to its guaranteed share.

**The reaper.** Idle upstream connections are closed after a threshold, and
`min_pool` is zero, so a node that served a tenant an hour ago is not still
holding connections for it. That is what stops a fleet's total upstream count
growing with the number of tenants it has ever seen.

## Where to look when something is wrong

| Symptom | The step above | What to read |
| --- | --- | --- |
| Clients waiting | acquire | `pgprox_wait_seconds`, `SHOW POOLS`, `SHOW QUOTA` |
| Multiplexing ratio near 1 | release | `pgprox_pin_total{reason}` |
| Reads not reaching replicas | decide | `pgprox_route_total{route}`, `pgprox_replica_lag_bytes` |
| Cache doing nothing | cache | `SHOW CACHE`, and whether the tenant opted in |
| Connections refused at connect | admission | `pgprox_client_conns` against `max_client_conns` |
| Auth failures under load | resolve | `pgprox_auth_cache_total`, and your token service |

[Operations](operations.md#diagnosing) has the longer version of each.
