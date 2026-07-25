# pgprox-proto

The Postgres wire codec, both directions.

This crate parses bytes sent by anyone who can reach the listener, so it is the
primary attack surface in the process.

## Rules specific to this crate

- **Sans-I/O.** The codec is a pure function of bytes in, frames out. A captured
  byte sequence from a trace becomes a unit test directly, with no runtime.
- **Never parse `DataRow`.** Result rows are forwarded as opaque frames. Parsing
  them is the difference between a proxy and a bottleneck.
- **Validate length before allocating.** A client claiming a 2 GB message gets an
  error, not an allocation.
- **No panic on any input.** A malformed frame must not take down a node serving
  100k other connections. This is fuzzed, not assumed.
- Cite the Postgres documentation section or message name for anything a reader
  would otherwise have to reverse engineer.
- Protocol 3.0 is accepted; a client asking for 3.2 gets either 3.2 or a
  `NegotiateProtocolVersion` down to 3.0.

Transaction boundaries come from the status byte in `ReadyForQuery`: `I` idle,
`T` in transaction, `E` failed. Not from the SQL text. See ADR 0001.

The `wire-debug` skill covers tracing and driver differences.
