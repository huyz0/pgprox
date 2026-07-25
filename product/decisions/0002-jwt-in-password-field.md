# 0002. JWT arrives in the password field, TLS required

Status: accepted

## Context

Clients authenticate with a JWT rather than a database password. The Postgres
wire protocol has no field designed for a bearer token, so the token has to
travel inside an existing mechanism.

The constraint that decides this: tenants use stock drivers. If a driver needs
patching or a client needs a custom auth plugin, adoption fails.

## Decision

The proxy answers `AuthenticationCleartextPassword` and the client sends the JWT
as its password. Any driver can do this, and `PGPASSWORD` works from `psql`.

TLS is required on the frontend whenever JWT auth is in use. A client that skips
`SSLRequest` when `require_tls` is set gets an `ErrorResponse` explaining why,
never a silent downgrade to cleartext.

A secondary SCRAM-SHA-256 path exists for admin tooling and migrations that
cannot carry a JWT. It is selected by matching the startup user against a
configured static-credential rule.

## Consequences

- Zero client changes. This is the property that makes the whole product viable.
- The token is in the clear inside the TLS tunnel, so TLS is not optional and the
  code must make it impossible to configure otherwise for JWT tenants.
- SCRAM cannot be used for JWT tenants, because SCRAM proves knowledge of a
  password the proxy does not hold and cannot derive from a token.
- Tokens may appear in client-side connection strings and process environments.
  That is the tenant's exposure to manage, but it argues for short token TTLs,
  which the grant cache honours.
- The proxy must never log the password field. Enforced by `SecretString` and
  reviewed at every `expose()` call site.

## Alternatives rejected

**Startup packet parameter.** Avoids an auth round trip. Rejected because of
length limits and because several drivers surface startup options in
`pg_stat_activity` and logs, which would leak tokens into places nobody expects.

**SNI or TLS ALPN.** Would allow routing before any Postgres bytes arrive.
Rejected because most drivers give no control over SNI beyond the hostname, so
it cannot carry a token.

**A custom authentication message type.** Cleanest protocol design, and
unusable: it would require every driver to be modified.
