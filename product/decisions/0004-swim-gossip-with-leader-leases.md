# 0004. SWIM gossip for membership, leader leases for quota

Status: accepted

## Context

The proxy runs as 3 to 5 pods. Each holds its own upstream pools, and together
they must never exceed a Postgres server's connection cap. Breaching that cap
can lock out the operator and take the database down for every tenant on it, so
this is the one property with no graceful degradation.

Holding a global cap from processes that do not share memory, under partition
and restart, is the hard part of the whole design.

## Decision

SWIM gossip over UDP using `foca`, seeded from headless Service DNS. One-second
protocol period, sub-second failure detection. Each message piggybacks a compact
per-node digest: upstream counts per server, client counts, per-tenant usage for
homed tenants, lease state, and drain mode.

Quota for a server with cap `C` across `N` live members:

- Guaranteed share `G = floor(C * guaranteed_fraction / N)`, default fraction
  0.5. A node may open up to `G` with no coordination at all.
- The remaining `C - N*G` is a free pool leased by the leader, which is the
  lowest node ID in the current stable membership view. Leases carry a 5s TTL
  and are renewed at 2s.
- A node that becomes unreachable has its leases expire, returning capacity
  within one TTL with no explicit action.
- On leader change, the new leader rebuilds lease state from gossip digests and
  waits one full lease TTL before granting from the free pool.

## Consequences

- The invariant to prove is that guaranteed plus outstanding leases never
  exceeds `C`, under arbitrary partition, leader loss, and simultaneous restart.
  This is a property test over a deterministic simulation with an injectable
  network, not an integration test, because it is the class of bug that never
  reproduces in staging.
- Partitions cause under-subscription, never over-subscription. That is the
  correct direction to fail: slow beats down.
- No external dependency on the connection path. The cluster layer has no
  Postgres and no etcd to be unavailable.
- `pgprox-cluster` is developed entirely against the simulation harness, so it
  needs nothing from the other tracks and can start immediately after M0.
- The one-TTL wait after leader change costs a few seconds of reduced headroom
  during failover. Accepted, because the alternative is over-granting.
- Gossip digests double as the source for cluster-wide admin aggregates, so
  `SHOW POOLS` answers locally with no fan-out.

## Alternatives rejected

**Static fair share from k8s Endpoints.** Zero peer traffic, dead simple.
Rejected because it wastes capacity whenever tenant load is skewed across nodes,
which it always is.

**etcd leases.** Strong consistency and a familiar operator story. Rejected
because it puts a hard external dependency in the connection path and adds a
failure domain to the one property that must not fail.

**Gossip with no leases, each node computing its own limit.** Simplest peer
protocol. Rejected because it can transiently over-subscribe during
convergence, which is the one outcome that is unacceptable.
