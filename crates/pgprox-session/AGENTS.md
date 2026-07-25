# pgprox-session

The per-client state machine and the relay loop. One of two crates allowed to
compose others: `pgprox-proto`, `pgprox-pool`, and `pgprox-route`.

This is the most correctness-critical code in the project. It gets property
tests and mutation testing.

## Rules specific to this crate

- **Sans-I/O state machine, thin I/O shell.** The state machine is a pure
  function of state and frame. The shell is generic over
  `AsyncRead + AsyncWrite + Unpin` so tests use `tokio::io::duplex` and never
  bind a port.
- **The relay loop allocates nothing.** Buffers are borrowed from the slab when
  the socket becomes readable and returned when quiescent. An idle connection
  holds a socket and roughly 200 bytes, not 32 KiB.
- **Cancellation safety is not optional here.** A client disconnecting mid-frame
  must not leave the codec believing it consumed bytes it did not, and must not
  return a mid-transaction upstream connection to the pool.
- Drain, shed, and socket-pressure eviction share one path for closing a client
  cleanly: wait for `ReadyForQuery('I')`, send `57P01`, close.

## The hazard that has already bitten twice

**A read can pull in bytes past the stage you are in, and dropping them loses
the start of the next stage.**

In the SCRAM test a helper owned its read buffer locally, so bytes read past the
handshake vanished when it returned and the session appeared to close. The same
shape will appear here at every boundary: startup to authentication,
authentication to query, query to COPY, and either direction into a pinned
replication stream.

The buffer belongs to the connection, not to the function handling the current
stage. Any helper that reads must take it by reference rather than owning one.

The relay loop and frame scanning are declared hot paths. Use the `hot-path`
skill before touching them.

See ADR [0001](../../product/decisions/0001-transaction-pooling-with-auto-pin.md)
and [0008](../../product/decisions/0008-buffer-reclaim-on-idle.md).
