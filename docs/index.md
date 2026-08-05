---
title: pgprox
description: "A multitenant connection pooler for Postgres, and where to start reading about it."
---

pgprox pools Postgres connections for a fleet of tenants whose credentials
differ, arrive as a JWT, and are resolved at connect time by a service you
provide.

Start where your question is.

## Run it

[Getting started](getting-started.md) brings the stack up on your machine and
sends a query through it. Fifteen minutes, needs Docker and Rust.

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
