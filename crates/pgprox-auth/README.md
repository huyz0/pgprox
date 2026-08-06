# pgprox-auth

Turning a client's token into the credentials for its database.

A tenant authenticates with a JWT in the password field. Your token service
validates it and answers with a grant: which tenant this is, where its database
lives, and how to connect. This crate asks, caches the answer, and collapses
concurrent lookups so a reconnect storm produces one call rather than
thousands.

## What it does not do

It does not verify signatures. Your service owns validation, and two validators
that disagree about whether a token is valid is a vulnerability rather than
redundancy. See [ADR 0003](../../docs/internal/product/decisions/0003-sidecar-grpc-over-uds.md).

What it does check is the header's algorithm, against an allowlist, before the
call is made. `none` and the `HS*` family are refused there. That is defence in
depth and costs one base64 decode.

## The cache key is a hash, and not of the tenant

Entries are keyed by `sha256(token)` plus the requested database.

Keying by tenant would let a revoked token keep working for as long as some
other valid token for the same tenant sat in the cache, which is a revocation
bypass wearing a cache optimization's clothes. Hashing rather than storing the
token means a memory dump of the keys is not a dump of credentials.

Positive entries expire on the shortest of the grant's TTL, the token's `exp`,
and a configured cap. Refusals are cached too, for much less time, because a
refusal can be reversed by something outside this process.

## Where it sits

Depends on `pgprox-core`. Used by `bin/pgprox` and by `bin/pgload`, which needs
the SCRAM exchange to authenticate against a real Postgres.
`pgprox-testkit` is a dev dependency for container readiness.

## Reading it

`jwt` extracts and checks the header. `client` is the gRPC client over a Unix
socket. `cache` adds caching, singleflight and negative caching to any
resolver. `scram` is the SCRAM-SHA-256 exchange, used both for the static admin
user and, from the client side, for dialling upstream.

`#![forbid(unsafe_code)]` in its own source: this crate parses a JWT header and
runs a SCRAM exchange against bytes a peer chose.
