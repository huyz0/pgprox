---
title: Going to production
description: "What changes between the bundled mock stack and a real Postgres and a real sidecar: the contract to implement, the arguments and document fields that follow from it, and what stays mock either way."
---

[Getting started](getting-started.md) runs a mock token service and a Postgres
compose brings up itself. Everything here is what changes to point the same
binary at a database you run and a sidecar you wrote, and nothing here is a
different deployment mode: it is the same `pgprox` binary reading a different
document and talking to a different socket.

## The one thing you have to build

pgprox does not validate JWTs and does not know how your control plane maps a
token to a tenant, a database or a role. It asks a service on a Unix socket,
and that service is yours. Nothing else in this list is optional; this one is
the entire integration.

The contract is [frozen](internal/product/decisions/0017-proxy-owns-the-sidecar-contract.md)
and lives at `proto/pgprox/auth/v1/auth.proto`: field numbers are never reused,
fields are never removed, and a breaking change gets a new package version
rather than an edit to this one. Read the proto itself for the message shapes;
this is what each RPC is answering and why.

**`Resolve(token, startup_database, startup_user, client_address) → tenant,
primary, replicas, ttl, pool hints, claims`.** Called once per connection
(cached after that — see below), on the client's startup packet. Validate the
token however your control plane validates tokens; pgprox does not implement a
second validator, because two validators that disagree about whether a token
is valid is a vulnerability rather than redundancy. What you hand back:

- **`tenant_id`.** Whatever your side calls this tenant. It becomes the pool
  key's tenant component and the label on `SHOW TENANTS`.
- **`primary`, `replicas`.** Host, port, database, role, password and TLS mode
  for the primary, and the same for any read replicas. The password travels as
  a `SecretString` from the moment pgprox has it: redacted in `Debug`, zeroed
  on drop, and reachable through nothing but an explicit `expose()` call that
  is a review item everywhere it appears. See
  [Security](security.md#credentials-never-reach-a-log).
- **`ttl_seconds`.** How long pgprox may cache this answer before asking again.
  Clamped to the shortest of this, the token's own `exp`, and the document's
  `grant_ttl_cap` — a generous value here cannot outlive what the token itself
  allowed, and shortening `grant_ttl_cap` is how you say a revoked token
  should stop working sooner.
- **`pool` hints.** Per-tenant `max_upstream`, pooling mode and statement
  timeout. Zero means unset, not zero: a tenant with no cap of its own is
  bounded by the server's cap and nothing tighter.
- **`claims`.** The parsed `sub`, `exp` and `iat`, so pgprox does not re-parse
  the token and risk reaching a different answer than you did.

**`RefreshTopology(primary_host, primary_port) → primary, replicas`.** Called
when a primary a session already authenticated against starts answering
`pg_is_in_recovery() = true`, asking where that primary's workload lives now.
No token, no TTL, no claims: this answers where the database is, not who may
use it, which is why it is a separate RPC and not `Resolve` with an empty
token. See ADR
[0028](internal/product/decisions/0028-topology-refresh-is-a-second-rpc-with-no-authorization-fields.md)
for the reasoning. If your control plane has no automatic failover — nothing
promotes a replica or moves a primary's DNS entry on your side — implement
this by returning the same server `Resolve` would for that primary today; a
manual failover elsewhere in your infrastructure is what makes the answer
change, not anything pgprox does.

A mock implementing both is at `crates/pgprox-auth/src/bin/mock_sidecar.rs`,
built and run by `scripts/e2e.sh`. It is a reference for the wire shapes, not a
starting point to extend: it accepts any well-formed token and routes every
tenant to the same database, which is exactly the validation step your real
one exists to do instead.

## Pointing the document at a real server

The compose stack's `deploy/config/config.yaml` names Postgres containers it
started itself. A real `servers` entry names your database and the cap the
fleet may hold on it:

```yaml
servers:
  - server: primary.db.internal:5432
    max_connections: 400
    guaranteed_fraction: 0.5
```

Set `max_connections` to the server's own connection limit **minus a reserve**
for superuser and maintenance sessions, not the raw value: a proxy that leased
every connection Postgres would allow locks the operator out at exactly the
moment they need to get in. See [Configuration](configuration.md#servers) for
`guaranteed_fraction` and what a server nothing declares a cap for does
(pools held at zero, on purpose, rather than a default that looks like one).

Replicas do not go in this file. They arrive in a `Resolve` grant, the moment a
session first presents one, and a replica inherits the cap entry of the
primary it replicates — which is why the primary needs a `servers` entry and
the replica never does, real deployment or mock.

## TLS and the flag that now guards it

The compose stack runs with `--require-tls`, a certificate mounted, and
`--upstream-ca` pointed at the container CA. A real deployment needs the same
three things, pointed at real files:

```bash
pgprox --tls-cert /etc/pgprox/tls/tls.crt --tls-key /etc/pgprox/tls/tls.key \
       --require-tls --upstream-ca /etc/pgprox/upstream-ca.crt \
       --sidecar /run/pgprox/sidecar.sock ...
```

A node always wires a JWT-capable resolver, so `--require-tls` off is refused
at startup rather than accepted: the default `Options` sets neither
`--require-tls` nor `--insecure-plaintext-auth`, and a node started with
nothing on that axis does not come up. The one flag you should not need is
`--insecure-plaintext-auth` — it exists for a benchmark harness whose load
generator cannot speak TLS at all, not for a real deployment. See
[Configuration](configuration.md#tls).

## What is still worth checking before a tenant reaches it

This page is the sidecar contract and the document fields that depend on it.
It is not the whole of what changes going from a laptop to a fleet a tenant's
traffic reaches — [Operations](operations.md) covers deploying several nodes
that share one document and one gossip view, and [Security](security.md)
covers the threat model this whole design answers to. Read both before the
first real token reaches this.
