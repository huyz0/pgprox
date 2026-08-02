# Tasks

Ordered. Each leaves the tree green on its own, which is what makes the order
matter: the trait cannot land without its fake and its implementations, and the
consumers cannot move before the trait exists.

Cross-referenced into `product/backlog.md` as `M19.*` when this is scheduled.
They are not filed there yet, because `M18.2` is the spec and filing tasks for
unscheduled work puts entries in a backlog nobody can start.

## The seam

**1. `PeerSource`, its static implementation, its fake, and the ADR.**

One commit, because non-negotiable 6 says so and
`scripts/check-core-contract.sh` will refuse the alternative. The ADR is the one
recording the discovery/liveness split; it is small, and it is the artifact that
makes the next person's version of this question cheap to answer.

Green on its own: nothing consumes it yet, so this is additive. The risk is that
it stays that way, which is what `scripts/check-wired.sh` exists to catch, so
`product/wired.txt` gains `PeerSource` in the same commit with `?` and this
task's successor as its owner.

Acceptance: the trait, `StaticPeers`, `FakePeerSource` with `publish` and
`go_stale`, the `Arc` forwarding impl with `is_healthy` forwarded rather than
defaulted, and an ADR. Tier 1 tests per `tests.md`.

**2. `run_with_peers` takes the source, and `entry.rs` builds a `StaticPeers`.**

The signature changes and the three consumers still receive a table read once,
at the top of the function. Nothing behaves differently.

This is separable from task 3 and worth separating: it is the change with the
widest diff and the least thinking, and reviewing it beside the semantic change
would hide the semantic change.

Acceptance: every existing test passes unchanged. `wired.txt`'s `?` marker for
`PeerSource` goes.

**3. The three consumers read the current table.**

`GossipTransport`, `NodeObservatory` and `Context`. The `OnceLock` on
`set_peers` goes, and its doc comment is replaced rather than deleted: it says a
second call would mean two answers to who is in the fleet, which was right when
the answer could not change and is the reasoning a future change will repeat.

Acceptance: the three tests in `tests.md` that publish after construction.

**4. The simulation gains a changing peer table.**

Acceptance: `pgprox_cluster::sim` can add and remove peers mid-run, and the cap
invariant still holds. This is the task that would catch a future change letting
discovery feed liveness.

## The Kubernetes source, once the seam exists

**5. A source backed by the Endpoints of the headless Service.**

Needs a client dependency, a `ServiceAccount`, a `Role` granting `get`, `list`
and `watch` on `endpoints`, and a decision on watch versus poll. `spec.md`
records the two open questions.

The safety rule is the acceptance: this source may add peers and may report a
node draining. It may not make a node count as alive. Assert it by pointing the
node at a fake API server reporting five Ready pods while gossip hears from one,
and checking quorum is not met.

**6. The chart offers it, and the StatefulSet stops rendering `--peer`.**

Values-gated and off by default, because the flags work and a deployment that
does not want an API dependency should not acquire one. The shell loop in the
template goes only when the values say so.

## What this list deliberately does not contain

**Moving to a Deployment.** The node id is the StatefulSet ordinal and is
encoded into every `ConnId` so a cancel landing on any pod routes to the owner.
That needs a different allocator and changes what clients see on the wire. It is
a separate spec and a larger one, and none of the tasks above depend on it or
bring it closer.
