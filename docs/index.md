---
title: pgprox
description: "A multitenant connection pooler for Postgres, and where to start reading about it."
---

pgprox pools Postgres connections for a fleet of tenants whose credentials
differ, arrive as a JWT, and are resolved at connect time by a service you
provide.

## The shape of fleet it is for

Multitenancy is not a feature here. It is the case the whole design answers, and
the case looks like this.

You run a hundred or so Postgres clusters. Each holds a few thousand databases,
one per tenant, because a database is the isolation boundary your auditors
understand. In front of them sit hundreds of application nodes running a
thread-per-request web stack, so a worker thread holds a database connection for
the length of a request and gives it back at the end.

Nothing about that is exotic, and it does not fit.

![Without a pooler, every application node needs its own connections on every
cluster: 300 nodes times 100 clusters is 30,000 separate claims on caps that
were never divided](img/nxm-without-pgprox.png)

A connection to Postgres is a process on the server, so every cluster has a cap
and the cap is small next to the fleet asking for it. Divide one cluster's five
hundred connections across three hundred application nodes and each node's share
is under two, while a node with two hundred worker threads can want two hundred
at once. The nodes cannot lend to each other, because they share no memory and
do not know what the others have already taken.

So the pool that would have made this cheap cannot exist. The application
connects, runs its statement, and drops the connection seconds later, paying a
TCP handshake, a TLS handshake, a SCRAM exchange and a backend process fork on
the request path. That is the N by M problem: demand grows with nodes times
clusters, and the cap it is spent against does not move.

![With pgprox, application nodes hold one long-lived local pool and six proxy
nodes hold the capped upstream pools, so 30,000 claims become
600](img/nxm-with-pgprox.png)

pgprox is where the multiplication stops. Application nodes keep the pool they
wanted, pointed at a proxy rather than at a database, and those connections are
cheap: a client connection here is a task, not a process, and a node has been
measured holding a hundred thousand of them. Behind it a small fleet of proxy
nodes holds warm upstream pools and divides each cluster's cap between
themselves, by a rule that does not break when they cannot see each other.

The tenants stay separated while that happens, which is the part worth being
careful about.

![Database per tenant: the pool key is server, database and role, the query
cache key has six components, and the grant cache is keyed by token hash rather
than by tenant](img/tenant-fanout.png)

Adding a tenant adds a database and a pool key. It does not add a connection to
every application node in the fleet.

Start where your question is.

## Run it

[Getting started](getting-started.md) brings the stack up on your machine and
sends a query through it. Fifteen minutes, needs Docker and Rust.

[Going to production](going-to-production.md) is what changes from there: the
sidecar contract you implement instead of the mock, pointing `servers` at a
database you run, and the TLS arguments a real deployment needs.

[Migrating from PgBouncer](migrating-from-pgbouncer.md) maps `pgbouncer.ini`'s
pooling settings onto their pgprox equivalent, names what has none — the
credential model — and what to check before cutting an existing deployment's
traffic over.

## Know what it does

[Features and limits](features.md) covers pooling modes, when a session pins,
replica routing and LSN watermarks, the query cache, and the things pgprox
deliberately does not do.

[Multitenancy](multitenancy.md) covers what keeps one tenant's data, credentials
and capacity away from another's, and where the isolation boundary actually
sits.

## Configure it

[Configuration](configuration.md) is the reference: the YAML document a node
reads, the command-line arguments it takes, and what each one defaults to.

## Operate it

[Operations](operations.md) covers deploying a fleet, draining a node for
upgrade, the metrics worth alerting on, and the `SHOW` commands for finding out
what a node is doing right now.

[Clustering and deployment](clustering.md) covers how several nodes hold one
upstream cap between them, how they find each other, and what the Kubernetes
deployment looks like.

[Admin and management](admin.md) is the surface itself: every `SHOW` command,
every API endpoint, and the operations that change a node's state.

## Satisfy a review

[Security](security.md) covers the threat model, how clients and operators
authenticate, what a grant authorizes, and how credentials are kept out of logs.

[FIPS builds](fips.md) covers building against a validated crypto module, what
it costs in cipher suites, and how to verify the binary you got.

## Understand it

[Architecture](architecture.md) explains transaction pooling, why a session
sometimes gets pinned to one connection, how the cap holds across nodes that
share no memory, and how replica routing avoids stale reads.

[Request flow](request-flow.md) walks one client frame through the proxy and
names the component doing each step, from admission to the connection going
back to the pool.

[Performance](performance.md) carries the measured numbers, the conditions each
was taken under, and the ones that are still targets rather than results.

[Optimizations](optimizations.md) is the work behind those numbers, including
the candidates that were measured and refused.

## The token service

pgprox does not validate JWTs itself. It sends the token to a service you
provide and receives the tenant, the backend credentials and the pool policy
back. The contract is in [Architecture](architecture.md#the-token-service), and
a mock for testing ships with the repo.
