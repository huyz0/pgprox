---
title: Clustering and deployment
description: "How several pgprox nodes hold one upstream cap between them, how they find each other, and what a deployment looks like."
---

pgprox runs as several pods. They share no memory, they each hold their own
pools, and together they must never exceed a Postgres server's connection cap.
Breaching that cap can lock the operator out and take the database down for
every tenant on it, so it is the one property with no graceful degradation.

Everything on this page follows from holding a global cap from processes that
cannot see each other's memory.

## What a node can do on its own

A server's cap `C` is split in two. Half by default is divided evenly across the
fleet as a **guaranteed share**, which a node opens without asking anyone:

```
G = floor(C * guaranteed_fraction / N)
```

`N` is the **configured** fleet size, taken from the number of entries in the
document's `nodes:` map, not from how many peers a node can currently see. That
matters more than it looks. A node cut off from its peers would otherwise see
`N = 1`, award itself the entire guaranteed total, and so would every node on
the other side of the partition.

The one place the live count enters is as a floor: the divisor is the larger of
the configured size and the number of members actually seen. Discovering more
peers than were configured shrinks each share rather than over-subscribing the
cap.

The remainder of that division, at most `N-1` connections, is given to nobody.
It looks like waste and it is not: the free pool is leased, leases outlive the
membership view they were granted under, and a remainder that moved with the
view once produced 102 outstanding connections against a cap of 100.

### What a node does with its allowance

A node's allowance covers every pool it holds for that server, and a pool is one
`(server, database, user)` triple, so a node serving many tenant databases holds
many pools against one number. The allowance is spread across them rather than
divided into them: each pool gets the floor, and whatever the floor leaves over
goes one connection at a time to the pools with the most demand behind them.

Where a node holds more pools than it has connections allowed, some pools get
zero and their clients wait, exactly as they would at a cap. It is not a
permanent condition. The split is recomputed every tick, waiting clients count
as demand, and demand is what the leftovers follow.

The alternative, giving every pool a floor of one, is what this used to do, and
it is a cap breach with extra steps: a node allowed a hundred connections and
holding three hundred pools opens three hundred.

## The gossip round

Once a second, every node opens a connection to every peer, sends its own
digest and reads the peer's back. TCP, one newline-delimited JSON object per
message, two second timeout per peer, one mebibyte cap on what a single
connection may deliver.

The digest carries the node's mode, its client connection count, its upstream
connection count per server, and per-tenant usage for the tenants it homes.
Only homed tenants, which is what bounds the message: a node homes roughly
`tenants / nodes` of the fleet, and that is exactly the set peers need to judge
whether it is using what it reserved. Gossiping every tenant a node has touched
would put five thousand entries in a message sent every second.

**Liveness is derived from when digests arrive.** There is no separate
heartbeat: the message that carries load is also the failure detector. A peer is
doubted after three seconds of silence and dropped after ten, and those are the
numbers quorum and leadership turn on.

This is all-to-all, not SWIM. One message per peer per round is O(N) per node
and O(N²) across the fleet, which at three to five pods is a handful of small
messages a second. A fleet an order of magnitude larger would need something
else.

## Leases, for the rest of the cap

What is not guaranteed is a free pool, leased out by a leader: the lowest node
id in the current view. Leases carry a five second TTL and are renewed on the
gossip round, so a node that becomes unreachable has its capacity returned
within one TTL with nothing having to be done about it.

Two rules stop two leaders granting from the same pool at once.

**A leader may grant only while it can see a strict majority of the fleet.** Two
disjoint views cannot both be a majority, so at most one ledger is ever granting.

**On taking office, a leader waits `lease TTL + doubt window` before granting.**
Eight seconds at the defaults. The wait alone is not enough and the majority
alone is not enough: a failure detector reports the past, so a node can arm its
takeover clock on a quorum it has already lost while the old leader is still
granting. The two together close it.

Lease state is deliberately not gossiped. The takeover wait already guarantees
that every lease the previous leader issued has either been renewed through the
new one or expired.

## What a partition does

It under-subscribes. Always, by construction.

A fleet split two against three leaves the two-node side unable to lease at all,
holding its guaranteed shares and serving at reduced capacity until it rejoins.
Clients wait, and `pgprox_wait_seconds` says so. That is the correct direction
to fail: slow beats down.

There is no external dependency on this path. No etcd, no Kubernetes API, no
Postgres to be unavailable in the middle of a decision. The one thing that must
not fail has no third party in it.

## Tenant affinity

Spreading a tenant's connections evenly across every node would mean each node
holding a small pool for it, and the fleet holding several partly-idle pools
where one busy pool would do.

So each tenant has a **home node**, chosen by rendezvous hashing, which reserves
most of that tenant's budget. Other nodes work from what is left and multiplex
harder rather than opening more.

Reservations decay after a few gossip rounds of non-use. A home node that never
uses its reservation would otherwise hold capacity hostage forever, whether
because the tenant is idle or because the load balancer never sends it there.
Decay is what makes a reservation a hint rather than a lock.

Nodes also **shed**: a client whose tenant belongs elsewhere is moved toward its
home node at a transaction boundary, counted in `pgprox_shed_total`. A draining
node does not shed, because moving work twice is worse than moving it once.

## What else crosses nodes, and what does not

**Cancellation.** A `CancelRequest` arrives on a fresh connection carrying
nothing but a key, and the load balancer will not send it to the node holding
the query. The node id is encoded into every connection id, so any node can
route a cancel to the owner. This is why the node id has to be stable and
numeric.

**Admin reads.** Aggregates answer from the local gossip digest, so hitting any
pod gives the fleet's numbers at no cost. Only drill-downs fan out, and one that
lost a peer answers 206 rather than pretending. See
[Admin and management](admin.md#ask-any-node).

**The query cache does not.** Each node's cache is its own. Entries are not
shared, invalidation only sees writes through the same node, and the staleness
bound is therefore per node. A summed hit rate across the fleet would describe
nothing that happened anywhere.

**Sessions do not.** A client belongs to the node it connected to until that
node sheds it or drains, and shedding is a reconnect rather than a migration.

## Finding each other

Peers come from `--peer <id>=<host>:<port>` flags, built once at startup. A node
learns of no node it was not told about.

That is a deliberate line rather than an unfinished feature. Discovery is
pluggable behind a trait shaped like the configuration source, and a Kubernetes
source that watched the headless Service's endpoints is possible. **Membership
is not pluggable**, and cannot be, for two reasons.

One message does two jobs. An external service can say which pods are Ready. It
cannot say that pod 3 is holding seventeen upstream connections against
`db-1:5432`, and that number is what the quota, shedding and reservation logic
run on.

And a third party can lie by being right. An API server reporting five Ready
pods to a node that cannot reach any of them is not wrong, it is answering a
different question. But a node that believed it would keep granting from the
free pool while its replacement granted from the same pool, which is exactly the
two-leaders case the majority rule exists to prevent.

The asymmetry is the design: getting discovery wrong costs a failed dial, which
the failure detector already handles. Getting liveness wrong costs the one
property that has no safe failure.

The honest cost of static discovery is that scaling the fleet means changing the
peer table, which means a rolling restart of every pod, on a system whose whole
purpose is holding client connections open.

## The deployment

A StatefulSet, a headless Service, one ConfigMap, and a PodDisruptionBudget. The
chart is in `deploy/helm`.

**A StatefulSet rather than a Deployment**, for one reason: gossip addresses a
peer by name and expects that name to mean the same node after a restart. A
Deployment's pods get a new random name every time, so each restart would look
to the fleet like a node leaving and a stranger arriving, and the quota that node
had reserved would churn with it. The ordinal is also where the node id comes
from, and that id is on the wire in every connection id.

**One ConfigMap, mounted on every pod.** Its checksum is a pod annotation, so
editing the document rolls the pods. Without that, a ConfigMap edit reaches the
mount and whether it takes effect depends on the file watch, which is a thing to
test rather than a thing to bet an upgrade on.

**`OrderedReady` pod management.** One node at a time, and the next does not
start until the previous is ready. Parallel would drain the whole fleet at once,
which is an outage rather than an upgrade.

**A PodDisruptionBudget**, because leases, rendezvous hashing and cross-node
cancellation all assume most of the fleet is up. An eviction taking two of three
nodes at once would rehome every tenant twice and shed clients that had nowhere
better to go.

**A preStop hook that drains and waits.** It posts a drain with an explicit TTL
and then sleeps out the grace period. The TTL matters: a drain written with no
expiry is a node that comes back from a restart still draining, which is
indistinguishable from one somebody meant to drain. `terminationGracePeriod` is
longer than that wait, because the kubelet starts counting when it starts the
hook rather than when the hook returns.

Two things the chart cannot do for you. File descriptor limits have to clear the
client cap: the default 1024 means a node refusing its thousandth client for a
reason that looks nothing like the truth, and a node aiming at 100,000
connections needs 262,144. And `net.core.somaxconn` is left unset, because the
kubelet refuses that sysctl unless it was started with
`--allowed-unsafe-sysctls`, and a pod asking for it on a default cluster does
not fail to tune, it fails to start.

## Scaling the fleet

1. Add the node to the document's `nodes:` map and let the config reload.
2. Raise `replicaCount` and roll, so every pod re-reads its peer flags.

In that order. The `nodes:` map is the divisor for every guaranteed share, and
raising it first means shares shrink before the new node arrives to use one. A
value larger than the running fleet only costs headroom.

Removing a node is the same in reverse: drain it, remove it, then lower the
count.

## Defaults worth knowing

| | |
| --- | --- |
| Gossip period | 1s |
| Peer timeout | 2s |
| Peer doubted | 3s of silence |
| Peer dropped | 10s of silence |
| Lease TTL | 5s, renewed each round |
| Leader takeover wait | lease TTL plus the doubt window, 8s |
| `guaranteed_fraction` | 0.5 |
| Home node share of a tenant's budget | 0.8 |
| Reservation decay | 3 gossip rounds of non-use |
| Fleet size for quota | entries in the document's `nodes:` map |

[Configuration](configuration.md) has where each is set.
[Operations](operations.md#deploying-a-fleet) has the deployment checklist.
