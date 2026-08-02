# A seam for peer discovery, and why membership is not one

Status: **proposed, not started.** `standards/contracts.md` requires stopping
before a change that crosses tracks. This exists to make the blast radius and
the safety argument visible before anyone edits.

Filed as `M18.2`.

## Problem

A node learns its peers from `--peer <id>=<host>:<port>` flags, rendered by a
shell loop in the StatefulSet template against `replicaCount`. The table is a
`BTreeMap<NodeId, String>` built once in `run_with_peers` and handed to three
places that never see it change:

- `GossipTransport`, which addresses the leader for a quota lease
- `NodeObservatory::set_peers`, which is a `OnceLock` and refuses a second call
- `Context.peers`, which is where a cancel for another node's connection goes

So a deployment that wants Kubernetes to supply peers has nowhere to put it,
and scaling the fleet means changing `replicaCount` and restarting every pod so
each one re-reads its flags.

The question that raised this was whether the Kubernetes API could supply
membership instead of gossip. It cannot, and the reason is the whole design of
this spec.

## Membership is two things and only one of them is discoverable

A gossip round exchanges a `ClusterDigest`: mode, client connections, upstream
connections per server, and per-tenant usage for homed tenants. Liveness is
derived from *when* those arrive; there is no separate heartbeat. One message
does two jobs.

The API server can say which pods exist and are Ready. It cannot say that pod 3
is holding seventeen upstream connections against `db-1:5432` right now, and
that number is what `apply_quota`, `shed_pass` and `Reservations::observe` run
on. **Whatever supplies peers, the digest exchange stays.**

And liveness must stay first-party. `membership.rs` counts a peer alive from
digests that *arrived*, which is what makes a one-way network failure safe: a
node that can still send but no longer receives ages its peers out and steps
down. An API server is a third party. A pod partitioned from its peers but still
able to reach the control plane would be told the fleet is healthy and would go
on granting from the free pool while the other side elected a replacement. That
is the two-leaders case ADR 0004's majority rule and `ttl + suspect_after`
takeover wait exist to prevent, and `pgprox-cluster`'s stated invariant is that
partitions cause under-subscription and never over-subscription.

**So the seam is discovery, and the rule that keeps it safe is one sentence:**

> An external source may cause this node to gossip with more peers, or to treat
> a node as draining sooner than gossip would. It may never cause a node to be
> counted alive that gossip has not heard from.

Getting discovery wrong then costs a failed dial, which the existing failure
detector already handles. Getting liveness wrong costs the one property with no
graceful degradation.

## Scope

In:

- A `PeerSource` trait in `pgprox-core`, modelled on `ConfigSource`, with a
  `watch` receiver so the table can change while a node runs.
- A static implementation carrying today's `--peer` flags, which is the default
  and keeps the current behaviour exactly.
- A fake, per non-negotiable 6.
- The three consumers reading from the watch rather than from a value copied at
  startup. `NodeObservatory::set_peers` becomes a subscription, which means its
  `OnceLock` goes.
- An ADR recording the discovery/liveness split, because the next person to ask
  this question deserves the argument rather than this file.

Out, and each for a stated reason:

- **A Kubernetes implementation.** It needs a client dependency, an RBAC story
  and a `ServiceAccount`, and it is worthless until the seam exists. Filed as a
  follow-on in `tasks.md`.
- **Sourcing liveness from anywhere but gossip.** See above. This is the point
  of the spec, not an omission from it.
- **`fleet_size`.** The guaranteed share is divided by a *configured* fleet
  size, not a live count, and ADR 0004 records that as the first correction its
  property test forced: a node cut off from its peers would otherwise see
  `N = 1` and award itself the whole guaranteed total. A live peer table must
  not become a live divisor.
- **Node identity.** The node id is the StatefulSet ordinal and is encoded into
  every `ConnId`, so a cancel landing on any pod can be routed to the owner.
  Moving to a Deployment needs a different allocator and changes what clients
  see on the wire. It is the real blocker on "pods and a Service" and it is a
  bigger question than this one.

## Acceptance criteria

> Given a node built with the static source carrying three peers
> When it runs a gossip round
> Then it dials exactly those three, which is what it does today

> Given a node whose peer source publishes a fourth peer while it is running
> When the next tick fires
> Then the round dials four, and no restart was required

> Given a node whose peer source drops a peer it had been gossiping with
> When that peer's silence passes `dead_after`
> Then it leaves the membership view by the existing liveness path, and nothing
> in the peer source removed it directly

> Given a peer source that publishes a node this fleet has never heard from
> When quorum is evaluated
> Then that node counts toward neither `alive_count` nor the quorum, because
> alive is counted from digests received

> Given a cancel for a connection owned by a node added after startup
> When it arrives on this node
> Then it is forwarded, because `Context` read the current table rather than a
> copy taken before that node existed

## Open questions

None blocking. Two worth a decision when the Kubernetes source is built:

1. **Watch or poll.** `FileSource` polls, and the reason is written down: a
   `ConfigMap` update swaps a symlink. An Endpoints watch is a long-lived
   connection to the API server and fails differently. Polling is the more
   conservative default and matches what exists.
2. **What a source does when it cannot read.** `ConfigSource::is_healthy`
   exists so a node serving a stale document says so. A peer source needs the
   same, and the answer is almost certainly "keep the last good table", since
   an empty one would silently stop all gossip.
