# pgprox-auth

JWT extraction, sidecar gRPC client over a Unix socket, grant cache.

## Rules specific to this crate

- **The sidecar owns validation.** This crate does not verify signatures. Two
  validators that disagree about token validity is a vulnerability, not
  redundancy.
- An algorithm allowlist is still enforced on the JWT header as defence in
  depth: RS256, RS384, RS512, PS256, ES256, ES384. Reject `none` and the `HS*`
  family before calling the sidecar.
- **Cache by `sha256(token) || startup_db`**, never by tenant claim. Keying by
  tenant would let a revoked token keep working while another valid token for the
  same tenant was cached.
- Cache TTL is `min(grant.ttl, exp - now, configured_cap)`.
- Singleflight on the resolve path. A reconnect storm must produce one RPC, not
  thousands.
- The `.proto` is not under this repo's control. Treat it as public API: field
  numbers never reused, fields never removed, changes agreed with the sidecar
  owners before the Rust side moves.

Grant cache lookup is a declared hot path.

See ADR [0003](../../docs/internal/product/decisions/0003-sidecar-grpc-over-uds.md).
