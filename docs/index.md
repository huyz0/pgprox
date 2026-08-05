# pgprox documentation

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

## Before you rely on any of this

pgprox is pre-1.0 and has never run in production. Its scale figures come from
one 20-core developer machine with everything in containers, so every latency
number is loopback and is a floor rather than a measurement. The performance
page says which is which for each figure.

The token service is not included. pgprox defines the contract and ships a mock
for testing; you implement the real one. See
[Architecture](architecture.md#the-token-service).
