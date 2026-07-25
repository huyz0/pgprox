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
- The grant cache is keyed by `sha256(token)`, never by the token itself, so a
  memory dump of the cache keys is not a credential dump.
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
- No `unsafe`, so the failure mode of a decoder bug is a wrong answer or an
  error, never memory corruption.
- The statement classifier parses untrusted SQL. When it cannot classify with
  confidence it returns unknown and the router sends the statement to the
  primary. Guessing read-only on an ambiguous statement is a correctness bug and
  a potential data-freshness bug, so the default is always the safe direction.

## Transport

TLS is required on the client side whenever JWT authentication is in use,
because the token travels in the password field. A client that skips
`SSLRequest` when `require_tls` is set gets an `ErrorResponse` explaining why,
not a silent downgrade.

Upstream TLS verifies the certificate chain against a configured CA. There is no
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
