# pgprox

A multitenant connection pooler for Postgres. Clients authenticate with a JWT,
an external service resolves which database and credentials that token maps to,
and the proxy multiplexes a very large number of client connections onto a small
capped pool of upstream ones across several nodes.

## What it is for

One Postgres server hosts up to 5,000 tenant databases, each with its own role
and password. Those tenants' applications open thousands of connections between
them. The server has room for a few hundred.

pgbouncer solves that for one database. pgprox solves it when the credentials
differ per tenant, arrive as a JWT, and the cap has to hold across several proxy
nodes that share no memory. That last part is the hard one and it is why there
is a gossip layer.

```
  clients ──JWT──▶ pgprox ──▶ your token service (you implement this)
                     │
                     └──pooled──▶ Postgres primary + replicas
```

## Run it

```bash
scripts/e2e.sh
```

Brings up three proxy nodes, a primary, two replicas and a mock token service,
then asserts the properties the stack is meant to have. Full walkthrough in
[Getting started](docs/getting-started.md).

## Documentation

| | |
| --- | --- |
| [Getting started](docs/getting-started.md) | Run the stack and send it a query |
| [Features and limits](docs/features.md) | Pooling, pinning, replicas, caching, and what is not supported |
| [Multitenancy](docs/multitenancy.md) | What keeps tenants apart, and where the boundary really is |
| [Configuration](docs/configuration.md) | Every setting, what it does, what it defaults to |
| [Operations](docs/operations.md) | Deploy, drain, observe, diagnose |
| [Clustering and deployment](docs/clustering.md) | How nodes hold one cap between them, and how to deploy them |
| [Admin and management](docs/admin.md) | Every `SHOW`, every endpoint, every state change |
| [Security](docs/security.md) | Threat model, authentication, credential handling |
| [FIPS builds](docs/fips.md) | The validated build, what it costs, how to verify it |
| [Architecture](docs/architecture.md) | How it works and why it is built this way |
| [Request flow](docs/request-flow.md) | One frame through the proxy, and what touches it |
| [Performance](docs/performance.md) | What has been measured, on what, and what has not |
| [Optimizations](docs/optimizations.md) | The work behind those numbers, including what was refused |

## What it will not do

Exceed a configured upstream connection cap. Everything else degrades; this
does not, because breaching the cap can lock an operator out of the database for
every tenant on it.

Serve a stale read to a session that has written. Replica routing tracks each
session's write position and will not send a read to a replica behind it.

Drop a connection mid-transaction for any reason under its own control. Drain,
rebalance and shed all wait for a transaction boundary.

## Licence

Apache-2.0.
