# 0020. An upstream that goes away is told to the client

Status: accepted

## Context

A session holds a client socket and, while it is in a transaction, an upstream
connection. When that upstream connection ends underneath it, the proxy knew
what had happened and the client did not: the error travelled back through the
relay as a plain disconnect, the session ended, and the client's socket closed
with nothing written to it.

Every driver reports that as a network fault against the proxy. The operator
then looks at the proxy and the network, and the condition was a database
restart, a `pg_terminate_backend`, or a link between this node and the server.

It was found in a scale run rather than in a test: at a thousand connections,
between three and fourteen clients per run had their sockets closed with no
message, and the proxy's own logs said nothing because the result of the
session was discarded at the accept loop.

## Decision

`ClientError::UpstreamClosed`, mapping to `08006 connection_failure`, and the
relay refuses the client with it rather than propagating the disconnect.

`08006` rather than `XX000`: the condition is a connection that failed, which
is what that code is for, and it is retryable. `is_retryable` says so, because
a client that reconnects gets a fresh upstream and carries on.

The client message names the database's connection rather than an internal
error, since what the client should do about it is reconnect.

## What was rejected

**Reusing `Internal`.** It maps to `XX000`, which tells a client the proxy
broke. The proxy did not break; the database connection did, and the two want
different responses from whoever reads the log.

**Leaving the disconnect to propagate.** It is the cheapest code and the reason
this ADR exists.

## Consequences

`ClientError` is `#[non_exhaustive]`, so the variant is additive and no
downstream match breaks. The two exhaustive matches inside `pgprox-core`,
`sqlstate` and `client_message`, are updated in the same commit, and
`standards/error-handling.md` gains the row.
