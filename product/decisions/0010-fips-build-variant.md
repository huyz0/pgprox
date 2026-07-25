# 0010. FIPS is a build variant, not the default

Status: accepted

## Context

Some deployments require FIPS 140-3 validated cryptography. Most do not, and the
FIPS build carries real costs: the validated module needs cmake, Go, and clang
in the build image, and it restricts the available cipher suites.

Building everything against the validated module would slow every developer's
build to serve a subset of deployments.

## Decision

One codebase, two build profiles.

Default builds use `rustls` with the `aws-lc-rs` provider. `--features fips`
swaps in the validated FIPS module (`aws-lc-fips-sys`, FIPS 140-3 certificate
#4816), calls `rustls::crypto::default_fips_provider()`, and asserts
`ServerConfig::fips()` and `ClientConfig::fips()` at startup, refusing to boot if
either is false.

Two Dockerfile stages, since the FIPS module needs a toolchain the default build
does not.

Keeping one provider family across both profiles means behaviour is identical
apart from the validated module itself.

## Consequences

- Developers get fast builds; regulated deployments get a validated binary from
  the same source.
- The startup assertion is the important part. A FIPS binary that silently falls
  back to non-validated crypto is worse than no FIPS binary, because it passes
  an audit it should fail.
- FIPS mode drops ChaCha20-Poly1305 and restricts TLS 1.2 to ECDHE suites with
  extended master secret enforced. Client driver compatibility against that
  suite list must be verified before committing to FIPS in production, which is
  why the driver cipher-suite matrix is an M8 deliverable rather than an
  afterthought.
- The crypto boundary is small because the sidecar owns JWT verification. See
  [0003](0003-sidecar-grpc-over-uds.md). Only TLS and SHA-256 for cache keys are
  in scope, so the awkward question of EdDSA's status in validated modules never
  arises.
- Two build profiles means CI builds both, and the FIPS path is exercised only
  nightly and pre-release rather than per PR, on build-time grounds.

## Alternatives rejected

**FIPS everywhere, one profile.** Simplest to reason about, no risk of shipping
the wrong binary. Rejected on developer build times and on forcing a restricted
cipher suite onto deployments that gain nothing from it.

**FIPS for frontend TLS only.** Faster to build. Rejected because if compliance
scope covers the data path, upstream connections are part of it, and a partial
FIPS story tends to fail audit anyway.

**OpenSSL with the FIPS provider module.** A well-trodden path outside Rust.
Rejected because it means a C dependency, a more painful build, and abandoning
rustls' memory-safety properties on the code that parses untrusted network data.
