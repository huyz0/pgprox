# 0005. Tenant affinity by quota reservation, not by moving clients

Status: accepted

## Context

The Postgres protocol has no redirect message, so a proxy cannot hand a client
to a peer node. With a normal kubernetes Service spreading clients randomly, one
tenant's clients land on all five pods and each pod opens its own upstream pool
for that tenant.

Worth being precise about what this costs. It is not a correctness problem: the
sidecar returns the same host and database for a tenant regardless of which pod
serves it, so "same tenant, same database" already holds. And it is not a safety
problem: the quota layer holds the cap regardless of fan-out. It is purely an
efficiency cost, up to 5x the upstream pools for one tenant.

## Decision

Each tenant gets a home node by rendezvous (highest random weight) hashing over
live membership, so a membership change rehomes only the tenants that lived on
the departed node.

The home node reserves up to `tenant_home_share` (default 0.8) of that tenant's
upstream budget. Other nodes share the remainder and, on hitting it, queue for
an existing upstream connection rather than opening a new one, meaning they
multiplex harder rather than failing.

Reservations are use-it-or-lose-it: if the home node's gossiped usage stays below
its reservation for `reservation_decay_rounds` (default 3), peers claim the
slack.

Opportunistic shedding supplements this. On a non-home node, a client idle at
`ReadyForQuery('I')` past `shed_idle_threshold` (default 30s), whose tenant's
home node reports headroom, may be closed with SQLSTATE `57P01` so the driver
reconnects cleanly and gets another roll of the load balancer.

Shedding guard rails, all configurable with a global kill switch: per-tenant
rate limit, a cap on the fraction of a tenant's clients shed per minute, never
within `settle_window` of a membership change, never toward a draining node,
never a pinned or in-transaction session.

## Consequences

- No client is ever moved for placement reasons alone, and no extra network hop
  is added. Latency is unaffected.
- Fan-out is bounded by node count and collapses on its own, because `min_pool`
  is 0 and idle upstream connections are reaped.
- The shed path shares its mechanism with drain and with socket-pressure
  eviction, so there is one well-tested way to close a client cleanly.
- `57P01` is load-bearing: every mainstream driver treats it as a clean
  server-initiated close and reconnects. A driver that does not would see
  errors, so the cipher of which drivers are verified belongs in the test matrix.
- Shedding is probabilistic. A shed client has a 1-in-N chance of landing on its
  home node, so convergence is gradual. The rate limits exist to stop this
  becoming churn.
- `pgprox_shed_total` spiking means the rebalance logic is thrashing and is an
  alerting signal, not just a counter.

## Alternatives rejected

**Strict owner with cross-node forwarding.** Guarantees exactly one upstream
pool per tenant. Rejected because it adds a network hop to every query for
non-owner clients and makes owner-node failure a client-visible event.

**No affinity at all.** Simplest. Rejected as a default because 5x fan-out on
hot tenants wastes real upstream capacity, though it remains the fallback if
reservations prove troublesome.

**External load balancer affinity by client IP.** No proxy complexity. Rejected
because it is best-effort and breaks entirely behind NAT, which is common.

**Per-tenant hostnames resolving to the home pod.** Real affinity with no LB
smarts, and viable since the control plane already issues connection details.
Rejected for MVP because node loss then requires a DNS update to recover, which
makes failover slower and more fragile than the thing it optimizes.
