---
title: Operations
description: "Deploy a fleet, drain a node for upgrade, alert on the right metrics, and diagnose what a node is doing."
---

How to deploy a fleet, take a node out for an upgrade, and find out what one is
doing when something looks wrong.

## Deploying a fleet

A fleet is several pgprox pods sharing one configuration document and knowing
each other's addresses. The Helm chart in `deploy/helm` sets this up; the parts
that matter are:

- The same `config.yaml` mounted on every pod, from one ConfigMap.
- A distinct `--node` number and `--node-name` per pod, with the name matching
  a key in the document's `nodes` map.
- Every pod's `--peer` list naming the others.
- A file descriptor limit above the client cap. The default 1024 means a node
  refusing its thousandth client for a reason that looks nothing like the truth
  in a log. A node aiming at 100,000 connections needs 262,144.

Kernel socket memory does not go away because the proxy pools its own buffers.
At 100,000 sockets, `tcp_rmem` and `tcp_wmem` minimums of 4 KiB each are 800 MB
of kernel memory that no proxy design avoids. Size the node for it.

## Draining a node for upgrade

Set the node to draining in the document and reload:

```yaml
nodes:
  pgprox-2: { mode: draining }
```

The node stops accepting new clients and closes existing ones at their next
transaction boundary, telling each with SQLSTATE `57P01`, which every mainstream
driver reconnects from. After `drain_grace` it force-closes whatever is left.

Nothing is dropped mid-transaction. That is a property the end-to-end suite
asserts rather than an intention.

Watch it finish with `pgprox_client_conns` on that node, or `SHOW CLIENTS`.

## Metrics

Served at `--admin` on `/metrics`, in Prometheus text format.

The ones worth alerting on:

| Metric | Why |
| --- | --- |
| `pgprox_upstream_conns` | Against the configured cap. Approaching it means clients are about to wait. |
| `pgprox_pin_total{reason}` | Rising pin rate is multiplexing degrading. The label says which feature is doing it. |
| `pgprox_wait_seconds` | Time clients spend waiting for an upstream connection. The cap converting into latency, which is the intended failure direction. |
| `pgprox_shed_total` | Clients moved toward their home node. Sustained shedding means the fleet is unbalanced. |
| `pgprox_cluster_members` | Fleet size as this node sees it. Disagreement between nodes means a partition. |
| `pgprox_replica_lag_bytes` | How far each replica trails. Reads route away from a replica behind the session's write position, so lag turns into primary load. |

Others exist for diagnosis rather than alerting: `pgprox_client_conns`,
`pgprox_query_duration_seconds{route}`, `pgprox_route_total{route}`,
`pgprox_cache_total{result}`, `pgprox_quota_leased`, `pgprox_buffer_slab`,
`pgprox_auth_cache_total`, `pgprox_config_reload_total`.

### Reading a rising pin rate

A pinned session holds one upstream connection for its whole life, which is
what transaction pooling exists to avoid. `pgprox_pin_total{reason}` labels
each by cause:

| Reason | What the client did |
| --- | --- |
| `listen` | `LISTEN`, or a notification arrived. Notifications only reach the backend that registered. |
| `advisory_lock` | A session-scoped advisory lock. The `_xact_` variants do not pin. |
| `temp_table` | Created something in the temp schema, which lives in one backend. |
| `with_hold` | Declared a cursor `WITH HOLD`, which outlives its transaction on purpose. |
| `prepare` | SQL-level `PREPARE`, which the proxy cannot rewrite the way it rewrites protocol-level prepares. |
| `unreplayable_set` | A `SET` outside the small replayable set. |
| `requested` | The client asked, via the pin parameter. |

If one reason dominates, that is the feature to go and look at in the
application. Protocol-level prepared statements are deliberately absent from
this list; they are handled by rewriting rather than pinning, which is what
keeps the ratio real with drivers that prepare by default.

## Asking a node what it is doing

pgprox answers `SHOW` on the client port, matching pgbouncer's columns and
order where the command exists there:

| Command | |
| --- | --- |
| `SHOW POOLS` | Per pool: clients, upstream connections, waiters. |
| `SHOW CLIENTS` | Every client this node serves. |
| `SHOW SERVERS` | Every upstream connection. |
| `SHOW STATS` | Traffic and timing counters. |
| `SHOW CONFIG` | Effective configuration, with which fields are reloadable. |
| `SHOW QUOTA` | pgprox only. How the cap is divided across the fleet. |
| `SHOW PEERS` | pgprox only. Fleet membership as this node sees it. |
| `SHOW TENANTS` | pgprox only. Per-tenant connection counts and home node. |
| `SHOW CACHE` | pgprox only. Query cache occupancy and hit rate. |

`SHOW LOCAL POOLS` and `SHOW LOCAL CACHE` narrow a read to the node that
answered, rather than the fleet-wide view the plain form gives.

Buffer slab occupancy is a metric rather than a `SHOW` command:
`pgprox_buffer_slab`.

The same data is available as JSON on the admin port, which is what the API is
for: an operator reads `SHOW`, a script or an agent reads the API, and neither
has to scrape the other's format. [Admin and management](admin.md) covers both
surfaces, the operations that change a node's state, and what the admin port
does not do for you.

## Diagnosing

**Clients waiting.** Check `pgprox_wait_seconds` and `SHOW POOLS`. If upstream
connections are at the cap, the cap is doing its job and the question is
whether it is set right. If they are below the cap and clients still wait, look
at `SHOW QUOTA`: this node may be at its leased share while another holds slack.

**`53300 too many connections`.** The fleet is at its cap and the client was
told rather than queued. Expected under a synchronised burst; sustained means
the cap is too low for the load.

**Multiplexing looks poor.** Compare `pgprox_client_conns` against
`pgprox_upstream_conns`. If the ratio is near one, read `pgprox_pin_total` by
reason.

**Stale reads reported.** Should not happen: replica routing compares each
replica's replayed position against the session's write position. Check
`pgprox_replica_lag_bytes` and `pgprox_route_total{route}`. A tenant calling a
write-performing function of its own from a `SELECT` is the known limit of a
lexical classifier; `SET pgprox.route = 'primary'` is the escape hatch.

**A node disagrees about the fleet.** `pgprox_cluster_members` and
`SHOW PEERS`. A partitioned node holds its guaranteed share and stops leasing,
which is the safe direction: it can serve less, never more.
