# pgprox-cluster

How several proxy nodes hold one upstream connection cap between them without
sharing memory.

## The invariant

Guaranteed share plus outstanding leases never exceeds the cap, under arbitrary
partition, leader loss and simultaneous restart.

Breaching an upstream cap can lock the operator out and take the database down
for every tenant on that host. It is the one property here with no graceful
degradation, so partitions must cause under-subscription and never
over-subscription. Slow beats down.

## How the cap divides

Half the cap by default is split evenly across the configured fleet size and
handed to each node to use without asking. The rest is a free pool, leased by
the lowest-numbered node in the current view, on a five second TTL.

Three rules came out of a property test breaking the original design, and each
is in [ADR 0004](../../docs/internal/product/decisions/0004-pairwise-gossip-with-leader-leases.md)
with the reasoning that made the wrong version look right:

The divisor is the configured fleet size, not the live member count. A node cut
off from its peers would otherwise see one member and award itself everything.

The division remainder goes to nobody, because a free pool that grows when
membership shrinks outlives the view it was granted under.

A leader may grant only while it sees a strict majority, and waits
`lease TTL + doubt window` on taking office. Either alone leaves two leaders
granting from one pool.

## It needs nothing to develop against

No Postgres, no sidecar, no network. `sim` is a deterministic simulation with
an injectable network, and the invariant above is a property test over it
rather than an integration test, because this is the class of bug that never
reproduces in staging.

## Where it sits

Depends on `pgprox-core` and nothing else. Used only by `bin/pgprox`, which
owns the gossip transport; this crate decides, and the binary sends.

## Reading it

`membership` derives liveness from when digests arrive. `quota` splits a cap.
`lease` is the ledger. `coordinator` puts those three together and is what the
binary calls. `reservation` holds a tenant's budget on its home node and decays
it when unused. `shed` decides which clients belong somewhere else. `digest` is
what one round exchanges.
