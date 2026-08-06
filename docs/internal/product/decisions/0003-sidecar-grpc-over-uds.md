# 0003. Credential resolution over gRPC on a Unix socket, sidecar validates

Status: accepted

## Context

An external sidecar validates the JWT and returns the real backend host,
database, user, and password. This call sits on the connection path, so its
latency is added to every new connection, and its availability bounds the
proxy's.

A question with security consequences: does the proxy also validate the token?

## Decision

gRPC over a Unix domain socket, using `tonic`. The proxy sends the raw token
plus the startup database, user, and client IP. The sidecar returns a grant:
primary backend, replica backends, pool hints, TTL, and parsed claims.

The sidecar owns signature and claim validation. The proxy does not implement a
second validator. It parses claims for policy and logging, and enforces an
algorithm allowlist on the JWT header as defence in depth.

Results are cached keyed by `sha256(token) || startup_db` with a singleflight,
so a reconnect storm produces one RPC rather than thousands. Cache TTL is
`min(grant.ttl, exp - now, configured_cap)`.

## Consequences

- One validator, so there is no way for two implementations to disagree. Two
  validators disagreeing about token validity is a vulnerability, not a
  redundancy.
- The FIPS crypto boundary shrinks to TLS plus SHA-256 for cache keys, since the
  proxy verifies no signatures. This sidesteps the awkward question of EdDSA's
  status in validated modules. See [0010](0010-fips-build-variant.md).
- A Unix socket means no network hop and no TLS between proxy and sidecar, which
  is correct only because they share a pod. This must be stated in the
  deployment docs, because running the sidecar off-pod would silently expose
  credentials on the wire.
- The sidecar is a hard dependency on the connection path. Its unavailability
  maps to SQLSTATE `08006` and existing connections keep working, but new ones
  fail. Cache TTL is the only thing softening this.
- The `.proto` is the one interface not under this repo's control, so it is
  treated as public API from the first commit and changes need sidecar-team
  agreement first.

## Alternatives rejected

**HTTP/JSON.** Easier to implement in any language and trivial to curl while
debugging. Rejected on typed-contract grounds: gRPC gives generated types on
both sides, which matters more when two teams develop in parallel. A JSON
gateway can be added later without changing the proxy.

**Proxy verifies the JWT locally via JWKS, sidecar only resolves credentials.**
Faster reject path for bad tokens. Rejected because it duplicates validation
policy across two systems that will drift.

**Caching by tenant claim instead of token hash.** Fewer cache entries.
Rejected because it would let a revoked token keep working as long as another
valid token for the same tenant was cached.
