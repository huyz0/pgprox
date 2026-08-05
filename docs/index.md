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

## Configure it

[Configuration](configuration.md) is the reference: the YAML document a node
reads, the command-line arguments it takes, and what each one defaults to.

## Operate it

[Operations](operations.md) covers deploying a fleet, draining a node for
upgrade, the metrics worth alerting on, and the `SHOW` commands for finding out
what a node is doing right now.

## Understand it

[Architecture](architecture.md) explains transaction pooling, why a session
sometimes gets pinned to one connection, how the cap holds across nodes that
share no memory, and how replica routing avoids stale reads.

[Performance](performance.md) carries the measured numbers, the conditions each
was taken under, and the ones that are still targets rather than results.

## The token service

pgprox does not validate JWTs itself. It sends the token to a service you
provide and receives the tenant, the backend credentials and the pool policy
back. The contract is in [Architecture](architecture.md#the-token-service), and
a mock for testing ships with the repo.
