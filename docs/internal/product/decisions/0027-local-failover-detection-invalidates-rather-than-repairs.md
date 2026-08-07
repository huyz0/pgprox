# 0027. Local failover detection invalidates the cache; it does not repair it

Status: accepted

## Context

`features.md` decides against automatic failover of a primary: "an upstream
that goes away is reported to the client rather than silently retried against
something else." That stands, and it is not what this addresses.

What it does not say is that the grant cache can serve a client a primary that
has *already* been demoted, for as long as `grant_ttl_cap`, 300 seconds by
default. Nothing in the process learns that a primary stopped being one until
the cached grant expires, however that primary changed. Every new client
presenting an already-resolved token spends up to five minutes being routed to
a host that will refuse every write it sends with Postgres's own "cannot
execute ... in a read-only transaction".

A demoted primary is directly observable. `pg_is_in_recovery()` answers `true`
for a server that used to be a primary and no longer is, and the replica
poller `M5.18` built already asks every replica the same question every 250 ms.
Asking it of the primary too is the same probe, the same interval, and the
same connection-holding shape.

## Decision

Every session's primary is probed on the poller's cadence, in `bin/pgprox`.
The question is `pg_is_in_recovery()`, reused from
[`REPLICA_QUERY`](../../../../crates/pgprox-session/src/probe.rs), against the
same `SqlReplicaProbe` machinery the replica poller uses, constructed with one
backend instead of a list.

**On the transition into recovery, the process invalidates the cache. It does
not discover a replacement.** A new `pgprox_core::auth::GrantInvalidation`
trait carries one method, `invalidate_primary(&self, server) -> usize`,
implemented by `pgprox-auth`'s `CachingResolver`. On the edge into demotion,
every cached grant naming that primary is dropped. Nothing here or in the
sidecar contract names the new primary; only the control plane does, so the
one correct action available locally is to stop serving the stale answer and
force the next lookup to ask again.

**Edge-triggered, once.** A demoted primary keeps answering `true` on every
following poll. Firing the invalidation again on each one would mean a primary
that stays demoted for an hour asks the sidecar's cache to re-evict an already-
empty entry 14,400 times. An `AtomicBool::swap` makes the check-and-set one
step, so two polls racing a slow tick cannot both fire.

**A failed probe is not a demotion.** The most common cause of a failed probe
is the host being briefly unreachable, and invalidating on every network blip
would turn a poll interval into a resolve storm on the sidecar for a primary
that never actually changed. This has no equivalent to the replica poller's
freshness window, because nothing routes on this value the way eligibility
does; a probe that starts succeeding again finds the flag exactly where it
left it.

**A session already connected to the demoted host is unaffected by this.** It
holds a `Grant`, not a token, from the moment `authenticate_token` returns
(`serve.rs`), so it has nothing left to re-resolve with even in principle. Its
next write fails with Postgres's own read-only-transaction error, which is a
transient error a retry policy can act on. This ADR is the detection half;
what a session already connected does about the failure is a separate
decision.

**No eviction of a primary watch.** `crate::replicas::ReplicaSets` keys on the
primary and the ordered replica list together (`M69.0`) and therefore mints a
generation on every topology change, which makes eviction load-bearing.
`PrimaryWatches` keys on the primary alone: one `ServerId` per upstream
database in the fleet, bounded by the operator's own topology rather than by
session count or by how often a replica set is reshuffled. If that stops being
true for some deployment, eviction belongs here rather than being paid by
every session today's shape does not need it for.

## What was rejected

**Discovering the new primary from `pg_stat_replication`.** The primary
already knows its own standbys, and `client_addr` there is often the same
number the replica poller wants. It is an observation, not a routable address:
behind NAT or a separate replication network it is not where a client should
connect, and the view carries no port, database, role or password to build a
`Backend` from. Using it as a change detector that triggers a re-resolve
through the sidecar remains open; using it as a replacement source of
addresses was rejected, because the control plane is the one place that
correctly knows what a proxy should connect to.

**A second trait method on `CredentialResolver` instead of a new trait.**
Eviction is a property of the cache wrapping a resolver, not of resolving
itself. A raw resolver with nothing cached has nothing to evict, and giving it
a method that can only no-op is an API that lies about what it does.

**Repairing the entry instead of dropping it.** This process has no way to
know the new primary; only overwriting the cached grant with a *correct* one
would avoid a round trip, and there is nothing here qualified to construct
one.

**Reusing `pgprox_route::replica::Replicas`'s freshness window for demotion.**
That structure answers "is this replica eligible right now" and ages a stale
reading out because a stale reading must not look healthy on the route
decision path. Demotion has no route decision reading it and no window that
would make an old `true` reading less true; a plain flag says everything a
probe needs to say.

## Consequences

- A new client's exposure to an already-demoted primary shrinks from up to
  `grant_ttl_cap` to one poll interval, without a control-plane push.
- A session already connected when its primary demotes is not moved by this;
  see the retry ADR this one is written beside.
- `GrantInvalidation` is a new `pgprox-core` trait with a fake behind
  `test-fakes`, per this crate's standing rule that a trait ships with one.
- `bin/pgprox` gains `primary_watch.rs`, probing every session's primary on
  the replica poller's cadence and independent of whether that grant has any
  replicas at all.
