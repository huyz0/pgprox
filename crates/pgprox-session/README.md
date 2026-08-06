# pgprox-session

One client connection, from the first byte to the last. This is where the
pieces meet.

Everything before it was built against fakes on purpose. `pgprox-session` is
one of two crates allowed to compose others, and it composes the three that
have to agree with each other on every frame: the codec, the pool, and the
router.

## The shape

Every stage of a connection is a sans-I/O state machine, and the I/O shell that
drives them is generic over `AsyncRead + AsyncWrite + Unpin`.

A test therefore drives a whole session over `tokio::io::duplex` without
binding a port. That is what makes the error cases reachable at all: a client
that sends `SSLRequest` twice, or disconnects halfway through a frame, is a
function call here and a piece of theatre in an integration test.

## The hazard

A read can pull in bytes belonging to the next stage.

So the buffer belongs to the connection, never to the function handling the
current stage. This has already caused one bug in this project, in the SCRAM
tests, and the crate's `AGENTS.md` goes through it at length. If you are
touching anything in `shell`, read that first.

## Where it sits

Depends on `pgprox-core`, `pgprox-proto`, `pgprox-pool` and `pgprox-route`.
Used only by `bin/pgprox`.

It does not depend on `pgprox-auth`, `pgprox-cache` or `pgprox-cluster` even
though a session uses all three. Those arrive as trait objects from the
composition root, which is what keeps this crate testable without a sidecar.

## Reading it

`shell` owns the wire and the buffer. `state` is the connection's state
machine. `auth` runs the handshake, `connect` the upstream dial, `relay` the
per-frame loop once both ends are up. `sequence` tracks an extended-query
sequence so the pool knows when one is outstanding. `cancel` holds the
key-to-connection map, which exists only between acquire and release. `resume`
replays what a session needs on a connection it has just landed on.
