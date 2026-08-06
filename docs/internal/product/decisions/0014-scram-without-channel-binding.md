# 0014. SCRAM-SHA-256 without channel binding

Status: accepted

## Context

ADR 0002 chose SCRAM passthrough for clients that cannot carry a JWT: admin
tooling, migrations, and monitoring. That path was never built, so those clients
cannot connect at all today.

SCRAM comes in two forms. `SCRAM-SHA-256` proves knowledge of a password without
sending it. `SCRAM-SHA-256-PLUS` additionally binds the exchange to the TLS
channel, which defends against an attacker who has terminated TLS in the middle
and can therefore replay the SCRAM exchange onto a different connection.

For a proxy, that middle position is not hypothetical. The proxy *is* a TLS
terminator: it holds one TLS session with the client and a separate one with the
upstream. Channel binding is precisely the mechanism that detects this.

## Decision

Implement `SCRAM-SHA-256`. Do not offer or accept `SCRAM-SHA-256-PLUS`.

The mechanism list is a constant with one entry, and our preference decides
rather than the server's ordering, so a server offering `-PLUS` first cannot
force it.

## Consequences

- Admin tooling can connect, which it cannot today.
- **A client that requires channel binding will refuse to connect through this
  proxy, and that is the correct outcome.** `-PLUS` exists to detect exactly what
  a terminating proxy does. Offering it and then binding to the proxy's own TLS
  channel would produce a client that believes it has end-to-end protection it
  does not have, which is worse than a clear refusal.
- Clients configured with libpq's `channel_binding=require` will fail. That
  needs to be in the deployment documentation as a known incompatibility rather
  than discovered during an incident.
- The security of the client-to-proxy hop rests on TLS and on the proxy being
  trusted, which is already true of every credential it holds. The proxy holds
  every tenant's database password; a client that does not trust it has a larger
  problem than channel binding.
- Nothing here forecloses `-PLUS` later. It would need the TLS exporter from
  `pgprox-tls` and a check against the FIPS cipher-suite list, since the FIPS
  provider restricts which suites can produce an exporter.

## Alternatives rejected

**Offer `-PLUS` and bind to the proxy's TLS channel.** Superficially better
security. Rejected because it is a lie: the client would verify a binding to the
proxy rather than to the database, and would report success for a property it
does not have. A security mechanism that reports a false positive is worse than
its absence.

**Pass SCRAM through end to end, relaying the exchange untouched.** Would let
`-PLUS` work honestly and would mean the proxy never sees a password. Rejected
because it is incompatible with pooling: the SCRAM exchange authenticates one
specific connection, and a pooled upstream connection was authenticated earlier
by someone else. This is worth revisiting only for session-pinned connections.

**Refuse SCRAM entirely and require JWT everywhere.** Simplest. Rejected because
ADR 0002 already established that migrations and admin tooling cannot always
carry a token, and leaving them unable to connect is not a design, it is the
current bug.
