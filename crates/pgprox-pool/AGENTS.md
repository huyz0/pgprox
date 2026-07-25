# pgprox-pool

Upstream pools, lifecycle, idle reap, pinning, prepared statement mapping.

## Rules specific to this crate

- **Release only on `ReadyForQuery('I')`**, with no extended query sequence
  outstanding and the session unpinned. Never on SQL text or a heuristic.
- **An upstream connection released mid-transaction is closed, not returned.**
  This is the cancellation-safety case that matters most here.
- **Prepared statement mapping is mandatory**, not an optimization. Without it
  the pool pins nearly every real session and transaction pooling silently
  degrades into session pooling. See ADR 0011.
- `min_pool` is 0. Idle upstream connections are reaped aggressively, which is
  what makes tenant fan-out across nodes collapse on its own.
- Pin triggers are recorded by reason for `pgprox_pin_total{reason}`. A rising
  pin rate is the early warning that multiplexing is degrading.
- **Do not write another SQL scanner.** `pgprox_core::sql` decides which text is
  SQL and which is data. This crate once had its own, which did not honour
  backslash escapes inside `E'...'`, and a missed pin hands one client another
  client's state.

Warm-pool acquire is a declared hot path. Use the `hot-path` skill before
touching it.

See ADR [0001](../../product/decisions/0001-transaction-pooling-with-auto-pin.md).
