---
title: Configuration
description: "Every setting a node reads: the YAML document, the command-line arguments, and what each defaults to."
---

A node reads two things: command-line arguments that say where it is and what
it listens on, and a YAML document that says what the fleet is allowed to do.
The split is deliberate. Arguments are per-pod and set once; the document is
shared across the fleet, mounted from a ConfigMap, and reloaded without a
restart.

## The document

Default path `/etc/pgprox/config.yaml`, overridden with `--config`. Every node
in a fleet reads the same file and finds itself in it by name.

```yaml
max_client_conns: 10000
drain_grace: 60s
grant_ttl_cap: 300s

servers:
  - server: primary.db.internal:5432
    max_connections: 400
    guaranteed_fraction: 0.5

nodes:
  pgprox-1: {}
  pgprox-2: { mode: draining }

query_cache:
  max_bytes: 64MiB
  max_entry_bytes: 1MiB
  ttl_cap: 30s
  tenants:
    acme: { ttl: 5s }
```

### Top level

| Field | Default | What it does |
| --- | --- | --- |
| `max_client_conns` | `10000` | Client connections one node accepts. The 10,001st is refused with a message rather than dropped. |
| `drain_grace` | `60s` | How long a draining node waits for transactions to end before force-closing what is left. |
| `grant_ttl_cap` | `300s` | Upper bound on how long a resolved token may be cached, whatever TTL the token service returns. |

### `servers`

One entry per upstream Postgres. The cap here is the fleet's total, not one
node's.

| Field | What it does |
| --- | --- |
| `server` | Host and port of the upstream. |
| `max_connections` | Connections the whole fleet may hold on this server. |
| `guaranteed_fraction` | Share of the cap divided evenly across nodes as a floor. The rest is a free pool nodes lease from the leader. |

Set `max_connections` to the server's own `max_connections` minus a reserve for
superuser and maintenance sessions. Using the raw value risks locking the
operator out at exactly the moment they need to intervene.

`guaranteed_fraction` trades responsiveness against fairness. At `1.0` every
node gets a fixed share and never coordinates, which wastes capacity when load
is uneven. At `0.0` every connection needs a lease from the leader. The default
of `0.5` gives each node a floor it can use without asking and leaves half the
cap to follow the load.

Both fields reload. A cap raised during an incident reaches the fleet on the
next tick rather than at the next restart.

#### Replicas, which you cannot list in advance

Replicas do not appear here and mostly cannot: they arrive from the token
service in a grant, so a node learns a replica host at the moment a session
first presents it. **A replica inherits the entry of the primary it replicates**,
which is why the primary needs one and the replica does not.

An upstream that nothing declares a cap for, directly or by inheritance, has its
pools **held at zero** and logs a line naming the server. That is deliberate and
it is the one place this document fails closed rather than choosing a default.
A cap nobody wrote down is not a cap, and
[the mission](internal/product/mission.md) gives exceeding one no graceful
degradation. The symptom is clients queueing on that server, and the fix is
either an entry for it or an entry for the primary it replicates.

### `nodes`

Keyed by node name, which is the `--node-name` a pod was started with. An empty
map value means the node runs normally.

| Field | Values | What it does |
| --- | --- | --- |
| `mode` | `active`, `draining` | `draining` stops the node accepting new clients and closes existing ones at their next transaction boundary. |

Draining a node is a config change, not a signal. Write `mode: draining`,
reload, and the node stops taking work without dropping anything mid
transaction. See [Operations](operations.md#draining-a-node-for-upgrade).

### `query_cache`

Off unless configured, and off per tenant unless that tenant is listed. A
tenant opting in is stating that reads this stale are acceptable for its own
workload, which nobody else can decide for it.

| Field | What it does |
| --- | --- |
| `max_bytes` | Total result bytes this node may hold. A byte budget rather than an entry count, because nothing bounds the size of one result. |
| `max_entry_bytes` | Largest single answer the proxy will hold while deciding whether to cache it. Spent per session in flight, so it is a separate resource from `max_bytes`. |
| `ttl_cap` | Ceiling on any tenant's TTL. |
| `tenants` | Per-tenant opt-in, each with its own `ttl`. |

The cache promises bounded staleness and nothing stronger. It invalidates on
writes it sees, which improves on that bound but does not make it
read-your-writes: it only sees writes that pass through the same node.

### `retry`

Off unless configured. When a new connection to an upstream fails outright —
refused, unreachable, the database mid-restart — pgprox normally reports that
to the client on the first attempt, the same as it always has. This section
says how many more chances to give it first.

| Field | Default | What it does |
| --- | --- | --- |
| `attempts` | `0` | How many times to retry after the first failure. `0` disables retry entirely. |
| `base` | `20ms` | The delay before the first retry, before backoff or jitter. |
| `max` | `2s` | The delay no retry waits longer than. |

```yaml
retry:
  attempts: 3
  base: 20ms
  max: 2s
```

Backoff is full jitter: each retry waits a random amount between zero and
`min(max, base * 2^attempt)`, doubling the ceiling each time up to the cap.
That is what keeps a fleet of proxies whose upstream just came back from
retrying in lockstep and hitting it with the same burst that just knocked it
over.

**This applies only to opening a new connection, never to a statement already
sent.** Nothing has reached any server when a dial fails, on any attempt, so
retrying costs nothing and risks nothing: it cannot duplicate a write, because
no write has happened yet. A statement mid-flight is a different question this
does not answer; see ADR
[0029](internal/product/decisions/0029-retry-is-scoped-to-a-dial-that-sent-nothing.md)
for the reasoning and what is deliberately left undone.

This section does not reload without a restart. `max_client_conns` and each
server's cap in `servers:` do, because the tick loop that applies them was
built to; `retry` was not wired through it. A change here takes effect the
next time the node starts.

## Command-line arguments

| Argument | Default | What it does |
| --- | --- | --- |
| `--config` | `/etc/pgprox/config.yaml` | Where the document is. |
| `--sidecar` | `/var/run/pgprox/sidecar.sock` | Unix socket of the token service. |
| `--node` | `1` | This node's number, used in cancel keys and quota requests. |
| `--node-name` | `pgprox-1` | This node's key in the document's `nodes` map. |
| `--listen` | `0.0.0.0:6432` | Where clients arrive. |
| `--admin` | `0.0.0.0:9090` | Metrics, health probes and the admin API. |
| `--gossip` | `0.0.0.0:6433` | Where peers exchange state. |
| `--tls-cert`, `--tls-key` | none | The certificate this node presents to clients. |
| `--require-tls` | off | Refuse clients that will not use TLS. |
| `--upstream-ca` | none | CA for verifying upstream Postgres certificates. |
| `--admin-user` | none | Name of a role allowed to authenticate with SCRAM instead of a JWT. Its password comes from the environment, never the command line. |
| `--peer` | none | A peer as `number=host:port`. Repeatable. |

Port 6432 rather than 5432 so a proxy sharing a host with the database it
fronts does not collide with it.

### TLS

Without `--tls-cert` a client asking for TLS is told no and decides for itself
whether to continue in the clear. With `--require-tls` it is refused instead.

A deployment carrying JWTs in the password field wants `--require-tls`. The
default is off because a node with no certificate and `require_tls` on would
refuse every client, which is a worse first experience than a node that says
what it is doing.

`--upstream-ca` is separate and applies to connections the proxy makes. Without
it the root store is empty, so any backend whose grant asks for a verified
connection fails to verify. That is the safe direction and it is not a working
deployment against a TLS-requiring database.

### Peers

Given, not discovered. A node that discovered its own fleet would be deciding
the fleet size, and the guaranteed share is divided by the configured size on
purpose. Keyed by number because a quota request has to reach one specific node,
the leader, rather than whichever peer answers first.

```bash
pgprox --node 1 --node-name pgprox-1 \
       --peer 2=pgprox-2:6433 --peer 3=pgprox-3:6433
```

## Reloading

The document reloads without a restart. Arguments do not. Anything you might
change during an incident lives in the document, which is why draining is a
field there rather than a signal.

Reloads are counted in `pgprox_config_reload_total` by outcome. A document that
fails validation is rejected and the running configuration stays, so a bad
ConfigMap does not take a node down.

Every validation error names the offending field. A config error at startup
with no field name means reading the whole file to guess.
