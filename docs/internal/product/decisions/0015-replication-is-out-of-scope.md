# 0015. Replication is passed through, not understood

Status: accepted

## Context

M1F.17 gates the rest of Group D: whether this proxy should model the
replication protocol. pgdog carries a whole logical-decoding subtree, with
message types for begin, commit, insert, update, delete, truncate, relation and
tuple data.

pgdog needs that because it shards: it rewrites and routes individual changes.
This proxy pools connections. Nothing in its job requires knowing what a change
stream contains.

Replication also breaks every assumption pooling rests on. A replication
connection is opened with `replication=database` in the startup packet, enters
`CopyBoth` mode, and then streams indefinitely. It never reaches a transaction
boundary, so it can never be returned to a pool. It is a long-lived dedicated
connection that happens to speak Postgres.

## Decision

Replication connections are recognised, pinned for life, and relayed byte for
byte. No replication message type is decoded.

Recognition is by the `replication` startup parameter, which is present before
any replication message flows, so the decision is made once at connection time
rather than inferred later from traffic.

A pinned connection does not count toward the multiplexing ratio and is
accounted separately, because a hundred replication connections that look like a
hundred pooled ones would make the pool statistics lie.

## Consequences

- Logical decoding, physical replication, and any future replication message
  work without this proxy being taught about them, because it never looks
  inside. That is the same argument as never parsing `DataRow`.
- A replication connection consumes an upstream connection for its lifetime,
  and must be counted against the cap. Ten replication clients on a server with
  a cap of a hundred leave ninety for everyone else, and the operator needs to
  see that.
- `CopyBothResponse` already sets the session's COPY state, which holds the
  connection. The pin is the stronger statement: it survives the COPY ending.
- Group D's remaining tasks, standby status updates and keepalive passthrough,
  are unnecessary. Passthrough covers them by construction.
- If this proxy ever grows a reason to inspect a change stream, such as routing
  by table, this decision is where to start rather than a gap to discover.

## Alternatives rejected

**Model the logical decoding messages, as pgdog does.** Would allow routing or
filtering a change stream. Rejected because nothing in this proxy's stated
mission asks for it, the surface is large and grows with every Postgres release,
and passing bytes through is both simpler and strictly more compatible.

**Refuse replication connections.** Simplest, and defensible for a connection
pooler. Rejected because a tenant running a CDC pipeline would find the proxy
silently unusable for it, and the cost of passthrough is a startup-parameter
check.

**Treat replication as an ordinary session and let COPY state hold it.** Nearly
works, since `CopyBoth` already holds the connection. Rejected because the hold
ends when the COPY does, and a replication connection that briefly leaves
`CopyBoth` would become eligible for reuse while still being a replication
connection.
