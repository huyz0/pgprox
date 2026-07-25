# pgprox-tls

rustls setup, the FIPS feature gate, certificate hot reload. Shared by the
frontend listener and upstream connections.

## Rules specific to this crate

- **The FIPS assertion is load-bearing.** Under `--features fips` the process
  asserts `ServerConfig::fips()` and `ClientConfig::fips()` at startup and
  refuses to boot if either is false. A FIPS binary that silently runs
  non-validated crypto is worse than no FIPS binary, because it passes an audit
  it should fail.
- **There is no skip-verification option for upstream TLS**, not even behind a
  flag. That flag always ends up set in production.
- TLS is required on the frontend whenever JWT auth is in use, since the token
  travels in the password field. A client skipping `SSLRequest` gets an
  `ErrorResponse` explaining why, never a silent downgrade.
- FIPS mode drops ChaCha20-Poly1305 and restricts TLS 1.2 to ECDHE with extended
  master secret. Driver compatibility against that suite list is an M8
  deliverable.

See ADR [0010](../../product/decisions/0010-fips-build-variant.md) and
[0002](../../product/decisions/0002-jwt-in-password-field.md).
