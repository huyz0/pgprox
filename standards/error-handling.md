# Error handling

A proxy fails in front of a customer's database. An error that says
`connection refused` and nothing else costs someone an hour. Errors here carry
enough to act on.

## The shape

`thiserror` for library crates, one error enum per crate, named `<Crate>Error`
and exported. No `anyhow` below `bin/pgprox`, because a library returning
`anyhow::Error` forces every caller to give up on matching.

`bin/pgprox` may use `anyhow` at the top level for startup errors, where the
only consumer is a human reading a log line before the process exits.

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PoolError {
    #[error("upstream {server} is at its connection cap of {cap}")]
    AtCap { server: ServerId, cap: u32 },
    #[error("timed out after {waited:?} waiting for a connection to {server}")]
    AcquireTimeout { server: ServerId, waited: Duration },
}
```

Each variant carries the values needed to understand it. `AtCap` without the cap
is a worse error for no saving.

## Rules

- No `unwrap` or `expect` outside `#[cfg(test)]`. Enforced by
  `clippy::unwrap_used` and `clippy::expect_used`. An invariant you are certain
  of is either encodable in the type or worth a real error.
- No `panic!` in a code path reachable from a client connection. A malformed
  frame from the network must never take down a node serving 100k other
  connections. Fuzzing exists to find where this is violated.
- Do not stringify an error to pass it upward. Wrap it with `#[from]` or `#[source]`
  so the chain survives and callers can still match on the root cause.
- Do not log an error and also return it. Pick one. The convention here is
  return it, and log at the boundary that decides what to do about it, which is
  the only place with enough context to choose a level.

## Mapping to the wire

Every error that can reach a client maps to a Postgres `ErrorResponse` with a
real SQLSTATE, never a generic internal error. The mapping is one function in
`pgprox-core` so it cannot drift per crate. The ones that matter:

| Condition | SQLSTATE | Code |
| --- | --- | --- |
| Node draining, or connection shed for rebalance | `57P01` | `admin_shutdown` |
| Upstream cap reached, no connection available | `53300` | `too_many_connections` |
| JWT invalid, expired, or rejected by the sidecar | `28000` | `invalid_authorization_specification` |
| TLS required but the client did not request it | `28000` | `invalid_authorization_specification` |
| Sidecar unreachable | `08006` | `connection_failure` |
| Acquire timeout | `57014` | `query_canceled` |
| A failure that is the proxy's own, such as no system entropy | `XX000` | `internal_error` |

`57P01` is chosen deliberately for shedding: every mainstream driver treats it
as a clean server-initiated close and reconnects, which is the entire point of
the rebalance mechanism.

## What an error must not contain

Never the password, the JWT, or any part of a `Backend`. The redacting
formatter in `pgprox-core` is the only way credentials get near a format string,
and it prints nothing useful by design. An error reaching a client must also not
leak the upstream hostname or the internal topology, since the client is
untrusted. Log the detail, return the generic form.
