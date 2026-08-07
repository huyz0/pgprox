# 0028. Topology refresh is a second RPC, and it carries no authorization

Status: accepted

## Context

`M71.0` (ADR 0027) gave new clients fast relief from a demoted primary: the
grant cache invalidates the moment a probe sees `pg_is_in_recovery()` turn
true. It could not do anything for a session already connected, because that
session holds a `Grant`, not a token, from the moment `authenticate_token`
returns. There is nothing left in the process to call `Resolve` with.

An established session still needs to learn the new primary, or it goes on
sending writes to a host that will refuse every one of them until it
disconnects and reconnects, if it ever does.

## Decision

A second RPC on `pgprox.auth.v1.CredentialResolver`, additive to the frozen
v1 contract (ADR 0017): `RefreshTopology(RefreshTopologyRequest) returns
(RefreshTopologyResponse)`. Keyed by the primary's host and port, not by
tenant or token, because a primary can host thousands of tenant databases and
a failover is one fact about all of them at once.

**The response carries no TTL, no pool hints, and no claims.** That is the
core of this decision and it is deliberate rather than an oversight. `Resolve`
answers "who may use this database and for how long"; `RefreshTopology`
answers "where is it". A session's authorization was already granted once, at
connect time, by a token this process can no longer even reach; nothing about
learning a new primary should be able to extend that grant's life or change
what it permits. Reusing `ResolveResponse` and leaving the extra fields at
their zero values was rejected for exactly the reason `Topology` (the type
`refresh_topology` returns) is not a `Grant`: a caller reading a default TTL
of zero cannot tell "expires now" from "field not sent", and a type that can
express both invites the bug.

**A new `pgprox_core::auth::TopologyRefresh` trait, not a new method on
`CredentialResolver`.** The two answer different questions with different
inputs — one needs a token and a client address, the other needs neither —
and a caller holding only a `Grant` should not be offered a method it cannot
call meaningfully. `SidecarResolver` implements both, over the same
connection, because in this process they are the same relationship with the
same sidecar; nothing requires that of an implementor.

**Where the answer is applied is `PrimaryWatches`, not the grant cache.** The
watch that detects demotion (`M71.0`) is what asks the question, on the same
edge that triggers cache invalidation. A successful refresh is stored keyed by
the *original* primary's `ServerId`, and `backend_for` — the one place a
`RouteTarget` becomes a `Backend` to connect to — checks that store before
falling back to the grant's own value. A session's next connection *acquire*
therefore sees the corrected primary without needing a new grant at all. This
is deliberately at acquire time, once per transaction, not mid-statement:
`pgprox-pool`'s whole design is that a connection returns to the pool at a
transaction boundary and is re-acquired for the next one, and acquisition is
already the one place per transaction where "which backend" is decided.

**Best-effort, and never the only thing that happens.** If the refresh RPC
fails, `PrimaryWatches` still invalidates the grant cache exactly as `M71.0`
does; a new client's exposure is unaffected by whether this succeeded. An
established session gets no relief in that case and keeps failing writes
until it reconnects, which is the same outcome as before this ADR. Nothing
here is worse than the state it replaces.

## What was rejected

**Pushing the refresh from the sidecar instead of pulling it.** A push model
needs the sidecar to hold a connection or a subscription per pod and needs
`auth.v2`, since a streaming RPC is not additive to a unary-only v1 service in
the way a second unary RPC is. Pull, on the same poll the demotion probe
already runs, costs one more RPC per demotion event rather than a standing
connection per pod, and the frozen v1 contract needed no breaking change.

**Answering `RefreshTopology` with the replica list only, leaving the primary
implicit.** The primary is the thing that changed; a response that named only
replicas would leave the caller inferring the primary from what is absent,
which is the shape of bug ADR 0017's freezing rules exist to prevent — a field
whose meaning depends on what else was sent.

**Swapping the backend inside `Grant` itself, in place.** A session's `Grant`
is read from several places without a lock today, on the assumption that it
does not change under a session once authentication finishes. Mutating it
would make every reader of `grant.primary` a place that needs to reconsider
that assumption. Keying the override by the *original* primary's `ServerId`
in a side table, consulted only at the one call site that turns a
`RouteTarget` into a `Backend`, keeps the assumption true everywhere else.

## Consequences

- `pgprox_core::auth::TopologyRefresh` is a new trait, with `FakeTopologyRefresh`
  behind `test-fakes` per this crate's standing rule.
- `pgprox-auth`'s `SidecarResolver` implements it over the existing gRPC
  channel; the mock sidecar implements the server side, env-driven like the
  rest of it.
- `bin/pgprox`'s `PrimaryWatches` gains an optional handle to a
  `TopologyRefresh` implementor and a small table of accepted overrides,
  consulted by `backend_for` at connection-acquire time.
- An established session's exposure to a demoted primary is bounded by how
  soon it next acquires a connection — every transaction boundary for a
  well-behaved client — rather than by how soon it reconnects.
