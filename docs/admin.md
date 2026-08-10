---
title: Admin and management
description: "The two admin surfaces, what each answers, the operations that change a node's state, and what the admin port does not do for you."
---

pgprox has two admin surfaces and one source of truth behind them. `SHOW`
commands on the client port, for an operator with psql already open. An
HTTP/JSON API on the admin port, for a script, a dashboard or an agent.

Neither scrapes the other's format, and neither can be more current than the
other. Both read the same internal view, so the two cannot drift into giving
different answers to the same question.

## Ask any node

Aggregate reads answer from the node's own gossip digest, which already holds
the fleet's numbers. There is no wrong pod to ask and no cost to asking one that
is not the leader.

`?scope=local` on the API, and `SHOW LOCAL ...` on the client port, narrow a
read to the node that answered. That is the one you want when you are diagnosing
a specific pod rather than the fleet.

Only drill-downs fan out to peers, and those are the ones that can come back
incomplete. A fan-out that lost a peer answers **206** with the rows it did
gather. Not 200, which would tell an operator the tenant has no clients, and not
500, which would tell them the proxy is broken. Neither is true, and the
difference decides what they do next.

## SHOW, on the client port

Nine commands, answered on the port clients already connect to, so an operator
with psql open needs nothing else installed and no second credential.

```bash
psql -h proxy -p 6432 -U pgprox_admin -c 'SHOW POOLS'
```

| Command | | pgbouncer |
| --- | --- | --- |
| `SHOW POOLS` | Per pool: clients, upstream connections, waiters | yes |
| `SHOW SERVERS` | One row per upstream connection | yes |
| `SHOW CLIENTS` | Every client this node serves | yes |
| `SHOW STATS` | Traffic and timing counters | yes |
| `SHOW CONFIG` | The configuration in force, and which fields reload | yes |
| `SHOW PEERS` | Other proxy nodes, as this one sees them | pgprox only |
| `SHOW QUOTA` | Upstream caps, usage and headroom across the fleet | pgprox only |
| `SHOW TENANTS` | Per-tenant connection counts and home node | pgprox only |
| `SHOW CACHE` | Query cache occupancy and hit rate | pgprox only |

The five pgbouncer has keep its column names and column order, so an existing
dashboard reads them unchanged. The four it does not have are free to be shaped
sensibly, because nothing can already be reading them.

Two naming notes that will otherwise waste your time. `SHOW SERVERS` is one row
per upstream **connection**, matching pgbouncer, not the capacity view; capacity
is `SHOW QUOTA`. And `SHOW CACHE` is always the answering node's own cache,
whatever scope you ask for, because caches are not shared between nodes and a
summed hit count across the fleet would describe nothing that happened anywhere.

An unrecognised `SHOW` is an error naming what it could have been, not an empty
result. A dashboard receiving no rows concludes there is nothing to report; one
receiving an error learns the command does not exist here.

## The HTTP API, on the admin port

The same information as JSON, on a separate port, for anything that is not a
person at a terminal.

Reads:

```
GET /v1/cluster            GET /v1/clients
GET /v1/pools              GET /v1/stats
GET /v1/servers            GET /v1/config
GET /v1/tenants            GET /v1/cache
GET /v1/tenants/{id}
```

Writes:

```
POST /v1/drain
POST /v1/undrain
POST /v1/pools/{server}/{database}/{user}/reset
```

**The two halves are separate routers on purpose.** Reading pool depths and
draining a node are not the same privilege, and a deployment that wants to
expose one without the other can.

The route list is not maintained beside the router, it is the router: the paths
and the handlers come from one declaration, so a path cannot be served without
appearing in the list, and the OpenAPI document is compared against that list
rather than against a hand-written copy. A route with no annotation fails a
test.

### Health and metrics

```
GET /healthz     liveness
GET /readyz      readiness
GET /metrics     Prometheus text format
```

`/readyz` and `POST /v1/drain` are the same fact seen from two sides: a draining
node reports itself unready, which is what takes it out of a load balancer's
rotation without anything else having to be told.

## Changing a node's state

**Draining, two ways.** Set `mode: draining` in the configuration document and
reload, or `POST /v1/drain`. They write the same desired state.

```bash
curl -X POST http://node:9090/v1/drain -d '{"ttl_ms": 600000}'
```

The TTL is what stops a drain started at 2am outliving the incident. A POST with
no body takes the default rather than returning 400, because `curl -X POST` with
nothing after it is what gets typed under pressure. `POST /v1/undrain` reverses
it.

A draining node stops accepting new clients and closes existing ones at their
next transaction boundary, telling each with SQLSTATE `57P01`, which every
mainstream driver reconnects from. Nothing is dropped mid-transaction.
[Operations](operations.md#draining-a-node-for-upgrade) has the upgrade
sequence.

**Resetting a pool.** `POST /v1/pools/{server}/{database}/{user}/reset` closes
that pool's **idle** upstream connections and answers with how many. Connections
in use are finishing real transactions, and an operator asking for a reset is
not asking for those to fail. The path is the pool key, and a path that names no
existing pool is a 404 rather than a success against nothing.

**Reloading configuration.** The document reloads without a restart; command
line arguments do not. Anything you might change during an incident is in the
document, which is why draining is a field there and not a signal. A document
that fails validation is rejected and the running configuration stays, so a bad
ConfigMap does not take a node down. Outcomes are counted in
`pgprox_config_reload_total`.

**Rotating TLS certificates.** The node re-reads its certificate and key on an
interval and swaps them in when they change. A rotation is measured in weeks and
needs noticing in minutes, so the check is chosen for how little it costs: two
small files read and hashed. A half-written file leaves the running certificate
in place.

## Designed to be read by something that is not a person

The machine-readable surface is not an afterthought bolted onto a text one. It
was a design goal that an operator and an agent can diagnose the same fleet
without either scraping the other's output.

That is what the shared internal view buys, and it is why the JSON is the API
rather than a rendering of the `SHOW` tables. It is also why an incomplete
answer has its own status code: something acting on the response needs to
distinguish "no rows" from "some nodes did not answer" without parsing prose.

## What the admin surface does not do

**It has no authentication of its own on the HTTP port.** The API is for
operators and is expected to be reachable only from an already-authenticated
admin surface. That is a deployment decision, and it is stated here rather than
assumed: do not put the admin port on a network a tenant can reach.

The chart used to say the opposite of that sentence with its manifests. The
admin port was a second port on the client service, so the one address every
tenant application is given also answered `POST /v1/drain`, and a client service
of `type: LoadBalancer` published it. It is now its own thing:
`adminService.enabled` creates an address for it, off by default, and the client
service carries the client port alone.

A service is not an access control, and this is worth being exact about because
the fix looks like more than it is. Pod IPs are routable whatever services
exist, so anything that can open a socket in the cluster can still reach the
admin port. Restricting it is a NetworkPolicy. What the chart can do is not
create an external address for it and not put it on the tenants' name, and that
is what it now does.

The `SHOW` surface does authenticate, with SCRAM, and a static user reaching it
gets no database connection at all. See
[Security](security.md#operators).

**No response contains a credential**, and that holds structurally rather than
by review: the internal view has no field for one, so a handler cannot leak what
it was never given. Upstream hostnames are a different matter. They appear in
pool keys because an operator debugging a pool needs them, which is another
reason the port is not for tenants.

**It is not a per-tenant view.** `SHOW TENANTS` and `/v1/tenants` are the
operator's view of every tenant on the fleet. There is nothing here that safely
narrows to one tenant for that tenant to read.
