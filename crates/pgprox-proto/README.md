# pgprox-proto

The Postgres wire protocol, both directions. Frontend messages a client sends,
backend messages a server sends, and the framing around both.

Sans-I/O. Decoding is a pure function of the bytes that have arrived, so a byte
sequence captured from a trace becomes a unit test directly, with no runtime
and no Postgres anywhere.

## Two rules that shape everything

**Never parse `DataRow`.** Result rows are forwarded as opaque frames. Parsing
them is the difference between a proxy and a bottleneck.

**Validate length before allocating.** This code reads bytes from anyone who
can reach the listener, so a declared length is untrusted until checked against
a maximum. No buffer is ever sized from a number a peer sent.

Decoding does not allocate: a `Frame` borrows its body from the caller's buffer
and every accessor hands out a slice. That is what keeps a 16 MiB `DataRow` off
the heap. It is not the same as "nothing here allocates", and the crate
documentation is careful about which is which.

## Where it sits

Depends on `pgprox-core` for IDs and error types, and on nothing else.

Used by `pgprox-session`, which drives it against a socket, and by `bin/pgload`,
which speaks the same protocol from the other side to measure the proxy.
`pgprox-testkit` is a dev dependency for the container probes the conformance
tests need.

## Reading it

`frame` finds message boundaries and decides how much of a body is worth
looking at. `frontend` and `backend` decode in each direction. `encode` and
`encode_frontend` go the other way. `rewrite` edits a statement name inside a
`Parse` or a `Bind` without re-encoding the message around it, which is what
prepared-statement mapping needs. `relay` forwards a frame whose body nobody
wants to see.

This crate is fuzzed rather than only unit tested, because its failure mode is
a node handling a malformed frame while serving a hundred thousand other
connections. It carries `#![forbid(unsafe_code)]` in its own source for the
same reason.
