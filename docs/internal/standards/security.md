# Security

Two properties define the threat model. The proxy holds credentials for every
tenant database on the fleet, and it parses bytes sent by anyone who can reach
the listener. Everything below follows from those.

## Credentials

`SecretString` from `pgprox-core` wraps every password and token. It redacts in
`Debug` and `Display`, and zeroes on `Drop`. Reaching the real value takes an
explicit `expose()` call, which is greppable and is a review item at every call
site.

- `Backend` does not derive `Debug`. It has a hand-written impl that prints host
  and database and nothing else.
- Credentials never enter a log, a span attribute, a metric label, an error
  variant, or an admin API response. See
  [observability.md](observability.md).
- The grant cache is keyed by `sha256(token)` plus the startup database and
  user, never by the token itself, so a memory dump of the cache keys is not a
  credential dump.
- Nothing writes a credential to disk. Not to a temp file, not to a debug dump,
  not to a core file. Core dumps are disabled in the deployment.

## Untrusted input

Everything a client sends is untrusted, including the length prefix of a frame.
The classic way to lose here is trusting a declared message length and
allocating it.

- Frame lengths are checked against a configured maximum before any allocation.
  A client claiming a 2 GB message gets an error, not an allocation.
- No `panic!` on any path reachable from client bytes. A malformed frame must
  not take down a node serving 100k other connections. This is why the decoder
  is fuzzed rather than only unit tested.
- No `unsafe` **in the crates that read bytes a peer chose**, so the failure
  mode of a decoder bug is a wrong answer or an error, never memory corruption.
  `pgprox-proto`, `pgprox-core`, `pgprox-route`, `pgprox-auth` and `pgprox-tls`
  carry `#![forbid(unsafe_code)]` in their own `lib.rs`, where the workspace's
  `deny` and any `#[allow]` cannot reach them.
  This was a workspace-wide `forbid` until `M27.1`. The sentence above is the
  argument and it was always narrower than the lint: it is about a decoder, not
  about the query cache's slab. Elsewhere unsafe is a governed exception with
  five conditions, in [rust-style.md](rust-style.md) and enforced by
  `scripts/check-unsafe.sh`.
- **Who chooses a map key decides its hasher.** A key a peer chooses keeps
  `RandomState`, the default, which is SipHash under a per-process seed: that
  seed is what stops a client sending a thousand keys that land in one bucket
  and turning every lookup into a scan. A key this process hands out gets
  `pgprox_core::hash::IssuedIds`, which is unseeded and much cheaper, because
  there is nobody to defend against.
  It is a rule about the key and not about the map, and the hard cases are the
  values this process computes from something a peer supplied. A prepared
  statement's global name is a hash of a name the client picked, and a hash of
  peer input is peer input. Those keep `RandomState`.
  Reaching for the fast hasher because a map looked slow in a profile is the
  mistake this is written to prevent. `M30.3` moved two maps and left four,
  and the four are named in `pgprox_core::hash`.
- The statement classifier parses untrusted SQL. When it cannot classify with
  confidence it returns unknown and the router sends the statement to the
  primary. Guessing read-only on an ambiguous statement is a correctness bug and
  a potential data-freshness bug, so the default is always the safe direction.

## Transport

TLS is required on the client side whenever JWT authentication is in use,
because the token travels in the password field. A client that skips
`SSLRequest` when `require_tls` is set gets an `ErrorResponse` explaining why,
not a silent downgrade.

Upstream TLS is negotiated the way Postgres negotiates it, with an `SSLRequest`
answered by one byte, and it verifies the certificate chain against a configured
CA. A server that answers `N` is refused rather than carried on with in the
clear, for the same reason the client side gets no silent downgrade. There is no
"skip verification" option, not even behind a flag, because that flag always
ends up set in production.

## Authentication and authorization

The sidecar owns JWT signature and claim validation. The proxy parses claims for
policy and logging, and it does not implement a second, subtly different
validator. This is deliberate: two validators disagreeing is a vulnerability.

The proxy still enforces an algorithm allowlist on the JWT header as defence in
depth: RS256, RS384, RS512, PS256, ES256, ES384. Anything else is rejected
before the sidecar is called, including `none` and the `HS*` family.

Grant cache TTL is `min(grant.ttl, exp - now, configured_cap)`. A revoked or
expired token must not keep working because the cache had a longer opinion.

## FIPS

The FIPS build is a feature flag, not a fork. `--features fips` swaps in the
validated `aws-lc-rs` module, and the process asserts `fips()` on both client and
server TLS config at startup and refuses to boot if either is false. A FIPS
binary that silently runs non-validated crypto is worse than no FIPS binary.

Because the sidecar owns JWT verification, the FIPS crypto boundary here is TLS
plus SHA-256 for cache keys. That keeps the validated surface small.

## Supply chain

`cargo deny check` runs in CI with an explicit license allowlist and a source
allowlist pinned to crates.io, so a dependency cannot quietly start pulling from
a git URL. `cargo audit` against the RustSec database. `gitleaks` runs in the
pre-commit hook, before a secret can reach history where removing it is a
rewrite.
