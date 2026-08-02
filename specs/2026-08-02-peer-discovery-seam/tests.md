# Test plan

Written before the code, so the plan is not a description of whatever gets
built.

## Tier 1: the unit tests carrying the coverage

**`pgprox-core`, the trait and the static source.**

- A static source returns what it was built with, and its `watch` receiver holds
  the same table.
- `is_healthy` is true for the static source, and the `Arc` forwarding impl
  returns the *wrapped* source's answer rather than the default. Assert both
  directions with a source that reports false: `M14.34` found both mutants of
  the identical `ConfigSource` method surviving, so the negative case is the one
  that matters.
- The fake publishes, and a receiver taken before the publish observes the new
  table.

**`bin/pgprox`, the three consumers.** Each is a test that the consumer reads
the *current* table rather than one copied at startup, and each is written by
publishing after the consumer was built:

- A cancel for a node added after `Context` was built is forwarded to it. This
  extends `a_cancel_for_a_peers_connection_is_forwarded_from_a_running_node`,
  which already exists and already proves the wiring; the new assertion is that
  it still works after a publish.
- The observatory's client fan-out reaches a peer added after construction. The
  `OnceLock` this replaces is why the current test cannot be written that way.
- A quota request goes to a leader whose address changed. The existing
  `a_lease_is_asked_for_over_a_real_socket` gives the shape.

**The rule itself, which is the assertion this whole spec exists for:**

- Given a peer source publishing a node id nothing has gossiped with, quorum is
  unchanged. `has_quorum` counts `liveness.alive_count`, so this should hold by
  construction; it is asserted because "holds by construction" is what every
  survivor in `product/mutants-baseline.txt` was assumed to be. If a future
  refactor sources `alive_count` from the peer table, this is the test that
  fails.

## Tier 2

`scripts/e2e.sh` needs no change: the compose stack passes `--peer` flags, which
become a `StaticPeers`, and the three properties it asserts are unaffected. Run
it anyway, before and after, because a seam that changes who a node talks to is
exactly the class of change whose damage does not show in a unit test.

`scripts/rolling-upgrade.sh` likewise. Its disruption case is the one that
exercises a node disappearing while the fleet is serving.

## Properties worth a `proptest`

The existing simulation in `pgprox_cluster::sim` already holds the invariant
that guaranteed plus outstanding leases never exceeds the cap under arbitrary
partition, leader loss and simultaneous restart. **It should gain one operation:
a peer table that changes mid-run.**

That is the property that would catch the mistake this spec is written to
prevent. If a future change lets discovery feed liveness, a table that grows
during a partition would let both sides reach quorum, and the invariant would
break in the simulation rather than in production.

## What is deliberately not tested, and why

- **That a Kubernetes source returns the right pods.** There is no Kubernetes
  source in this spec. When there is, its test is against a fake API server and
  belongs with it.
- **That the API server is reachable.** Out of scope in the same way the sidecar
  being reachable is: the failure is a source reporting unhealthy, and the
  behaviour under that is tested with the fake.
- **The `--peer` flag parsing.** `entry.rs` already tests it, including the
  malformed cases, and this work does not touch it.
