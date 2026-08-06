# 0023. Peer discovery is pluggable, membership is not

Status: accepted

## Context

A node learns its peers from `--peer <id>=<host>:<port>` flags, rendered by a
shell loop in the StatefulSet template. The table is built once at startup and
handed to three consumers that never see it change: the quota transport, the
observatory's client fan-out, and the cancel router.

The question that forced a decision was whether the Kubernetes API could supply
membership instead of gossip, since the API server already knows which pods
exist and is more available than any peer.

## Decision

**Peer discovery becomes a trait. Membership does not.**

`PeerSource` answers one question: which peers should this node gossip with, and
at what addresses. It is shaped like `ConfigSource`, with a watch receiver, an
`is_healthy` and a `run_loop`, because both answer "a thing that changes while a
node runs" and a second mechanism for that would be a second set of mistakes.

Liveness stays where it is, derived from gossip arrivals in
`pgprox_cluster::membership`.

The rule that makes the split safe, and it is in the trait's own doc comment
rather than only here:

> A source may cause this node to gossip with more peers, or to treat one as
> draining sooner than gossip would. It may never cause a node to be counted
> alive that gossip has not heard from.

## Why membership cannot be sourced externally

**One message does two jobs.** A gossip round exchanges a `ClusterDigest`: mode,
client connections, upstream connections per server, and per-tenant usage for
homed tenants. Liveness is derived from when those arrive; there is no separate
heartbeat. An external service can say which pods are Ready. It cannot say that
pod 3 is holding seventeen upstream connections against `db-1:5432`, and that
number is what `apply_quota`, `shed_pass` and `Reservations::observe` run on. So
the digest exchange stays whatever supplies peers, and replacing membership
would replace half a message.

**A third party can lie by being right.** `membership.rs` counts a peer alive
from digests that *arrived*, and its module comment says why: a node that can
still send but no longer receives ages its peers out and steps down, exactly as
one cut off in both directions does. Counting anything else leaves it convinced
it still leads while the other side elects a replacement.

An API server is not lying when it reports five Ready pods to a node that cannot
reach any of them. It is answering a different question. But that node would
keep granting from the free pool while its replacement granted from the same
pool, which is the two-leaders case ADR 0004 added the majority requirement and
the `ttl + suspect_after` takeover wait to prevent. `pgprox-cluster`'s invariant
is that partitions cause under-subscription and never over-subscription, and
this is the one way to break it from outside.

**The asymmetry is the whole design.** Getting discovery wrong costs a failed
dial, which the failure detector already handles: a peer that is not there is a
peer nothing is heard from. Getting liveness wrong costs the one property with
no graceful degradation.

## Consequences

- A Kubernetes source is possible and is not in this ADR. It watches or polls
  the Endpoints of the headless Service, needs a `ServiceAccount` and a `Role`,
  and is bound by the rule above.
- `fleet_size` is untouched. The guaranteed share is divided by a *configured*
  fleet size, not a live count, and ADR 0004 records that as the first
  correction its property test forced: a node cut off from its peers would
  otherwise see `N = 1` and award itself the whole guaranteed total. A live peer
  table must not become a live divisor.
- Adding peers cannot inflate quorum. `has_quorum` counts nodes heard from
  against the configured fleet size, so a source that published nodes nobody has
  gossiped with moves nothing. A source that *removes* peers makes quorum
  harder, which is the safe direction.
- `NodeObservatory::set_peers` was a `OnceLock` whose doc said a second call
  would mean two answers to who is in the fleet. That was right when the answer
  could not change. It becomes wrong the moment it can, and it goes.
- The node id is unaffected and remains the StatefulSet ordinal, encoded into
  every `ConnId` so a cancel landing on any pod routes to the owner. Moving to a
  Deployment needs a different allocator and changes what clients see on the
  wire. That is a separate decision and a larger one.

## Alternatives rejected

**Sourcing membership from the Kubernetes API.** The question that started this.
Rejected for the reason above: it replaces half of what the message carries and
puts a third party in the one position that can break the cap invariant.

**Making `MembershipConfig`'s windows configurable per source.** A softer
version of the same mistake. The three and ten second windows are derived from
the one-second gossip period, and a source that could widen them could make a
partitioned node believe it still leads for longer.

**Leaving discovery static and telling deployments to restart.** What happens
today. It works, and it means scaling the fleet is a rolling restart of every
pod so each re-reads its flags, on a system whose whole purpose is to hold
client connections open.
