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
- **The listener's certificate is reloadable and the `ServerConfig` is not.**
  `CertReloader` is a `ResolvesServerCert`, so the configuration handed to the
  accept loop is fixed for the life of the process and what it resolves to is
  not. A rewrite that does not parse leaves the previous certificate serving:
  certificates are rotated by machines and a half-written file is a normal
  thing to read, so the failure mode is a log line rather than a listener that
  stops answering.
  This line described nothing until `M24.9`. `server_config` was called once
  and its answer never changed, so a cert-manager rotation served an expired
  certificate until somebody restarted the pod.
- TLS is required on the frontend whenever JWT auth is in use, since the token
  travels in the password field. A client skipping `SSLRequest` gets an
  `ErrorResponse` explaining why, never a silent downgrade.
- FIPS mode drops ChaCha20-Poly1305 and restricts TLS 1.2 to ECDHE with extended
  master secret. Driver compatibility against that suite list is an M8
  deliverable.

See ADR [0010](../../docs/internal/product/decisions/0010-fips-build-variant.md) and
[0002](../../docs/internal/product/decisions/0002-jwt-in-password-field.md).
