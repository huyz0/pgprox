---
name: wire-debug
description: Capture and decode a Postgres wire protocol exchange when something misbehaves at the protocol level. Use for driver incompatibilities, hangs during auth or extended query, unexpected pinning, prepared statement desync, or any "works with psql but not with this driver" report.
---

# Debugging the wire

Protocol bugs look like application bugs. The symptom is usually a hang or a
confusing error several steps downstream of the actual mistake, so the first job
is always to see the bytes.

## Get a trace

In order of preference:

1. **The proxy's own frame trace.** `RUST_LOG=pgprox_proto=trace` logs decoded
   frames on both sides with the connection ID. Cheapest, and it shows what the
   proxy *thinks* it saw, which is often the bug.
2. **libpq's trace**, for comparing against what a working client does:
   `PGOPTIONS` with `PQtrace`, or `psql` with tracing enabled.
3. **tcpdump**, only when TLS is off or you can decrypt. Last resort, because
   the frontend requires TLS for JWT tenants.

Always capture both sides. A bug is usually a disagreement between them.

## Read the trace

Message flow, in the order it should appear:

```
SSLRequest        -> 'S'
                     [TLS handshake]
StartupMessage    -> AuthenticationCleartextPassword
PasswordMessage   -> AuthenticationOk, ParameterStatus*, BackendKeyData, ReadyForQuery('I')
```

Then either simple query (`Query` -> results -> `ReadyForQuery`) or extended
query (`Parse`/`Bind`/`Describe`/`Execute` -> ... -> `Sync` -> `ReadyForQuery`).

## What to check, in order

**The transaction status byte in `ReadyForQuery`.** `I` idle, `T` in
transaction, `E` failed transaction. This is the authoritative release signal.
If the pool released on `T`, that is the bug and everything after it is noise.

**Extended query sequences must end with `Sync`.** A missing or misplaced `Sync`
is the classic pipelining bug. The connection cannot be released mid-sequence
even at an apparent idle.

**Prepared statement names.** The proxy rewrites client-local names to global
ones. A desync between the map and the server's actual statement set produces
`prepared statement "s1" does not exist`, and the trace shows whether the
replay-on-acquire happened. See ADR 0011.

**Protocol version.** A client asking for 3.2 should get either 3.2 or a
`NegotiateProtocolVersion` down to 3.0. A client that hangs right after startup
is often one that did not expect the negotiation message.

**`ParameterStatus` set.** Some drivers require specific parameters at startup
and hang without them. Compare against a direct connection to the same backend.

**Pinning.** If a session pinned unexpectedly, the trace shows the triggering
frame. `pgprox_pin_total{reason}` names the category, the trace names the
statement.

## Reproduce as a test

A protocol bug found by trace and fixed by inspection will come back. Convert it
before fixing:

```rust
#[test]
fn sync_after_error_still_reaches_ready_for_query() {
    let mut s = SessionState::new();
    // bytes taken verbatim from the captured trace
    ...
}
```

The codec is sans-I/O, so a captured byte sequence becomes a unit test directly,
with no runtime and no Postgres. Any crash found by fuzzing goes into the corpus
the same way.

## Driver differences worth knowing

`psql` uses simple query for most things, so "works with psql" often means "the
extended query path is untested". pgx, asyncpg, JDBC, and npgsql all use named
prepared statements by default and each pipelines differently. The conformance
suite runs all five for exactly this reason.
