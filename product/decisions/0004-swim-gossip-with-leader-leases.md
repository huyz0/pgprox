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

Quota for a server with cap `C` across a fleet configured to run `N` nodes:

- Guaranteed share `G = floor(C * guaranteed_fraction / N)`, default fraction
  0.5. A node may open up to `G` with no coordination at all.
- `N` is the **configured** fleet size, not the live member count. Membership
  decides who leads; it does not decide how large a share is.
- The free pool is `C - floor(C * guaranteed_fraction)`, leased by the leader,
  which is the lowest node ID in the current view. Leases carry a 5s TTL and are
  renewed at 2s. The division remainder, at most `N-1` connections, is given to
  nobody.
- A node that becomes unreachable has its leases expire, returning capacity
  within one TTL with no explicit action.
- The leader may grant only while it can see a strict majority of the fleet.
- On taking office, a leader waits `ttl + suspect_after` before granting from
  the free pool. Regaining a quorum counts as taking office.

The last three points were added during M3 after the property test broke the
original design. Each is recorded below, because the reasoning that made the
original look correct is the reasoning a future change will repeat.

**The share cannot be divided by the live count.** A node cut off from its peers
sees `N = 1` and awards itself the whole guaranteed total, while the nodes on the
other side award themselves the same total again. Dividing by a configured
constant means a node is never emboldened by its peers' absence.

**The free pool cannot absorb the division remainder.** `C - N*G` grows when
membership shrinks. Leases outlive the view they were granted under, so a pool of
52 granted at three nodes was still outstanding when membership returned to five
and the guaranteed total rose to 50: 102 against a cap of 100.

**A majority is required to grant, and the takeover wait must cover detection.**
The takeover wait alone does not stop a partitioned leader, which still believes
it holds office and grants from the same pool as its replacement. Requiring a
strict majority means at most one ledger is ever granting. But a failure detector
reports the past: a node counts a peer alive for up to `suspect_after` after last
contact, so it can arm its takeover clock on a quorum it has already lost while
the other leader is still granting. Waiting `ttl` alone leaves the two
overlapping; waiting `ttl + suspect_after` does not. The implementation derives
this rather than validating it, so a misconfiguration is slow, not unsafe.

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
- The takeover wait costs `ttl + suspect_after` of reduced headroom after a
  failover, eight seconds at the defaults. Accepted, because the alternative is
  over-granting.
- The majority requirement means a minority partition cannot lease at all. It
  keeps its guaranteed shares and serves at reduced capacity, which is the
  under-subscription this design prefers. A fleet split two-three leaves the
  two-node side on `2G` until it rejoins.
- `fleet_size` becomes a configuration value that must track the deployment.
  Setting it too low over-subscribes nothing but wastes the free pool's headroom;
  setting it too high shrinks every share. Scaling up means raising it first.
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
