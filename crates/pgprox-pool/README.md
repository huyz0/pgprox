# pgprox-pool

Upstream connections: which exist, who is holding one, and when it goes back.

A node holds tens of thousands of client connections against a few thousand
upstream ones. That works only because a connection is borrowed for a
transaction rather than for a session, and it stops working the moment a large
fraction of sessions become unmovable.

Two halves of this crate pull against each other and both matter. `pin` decides
which sessions genuinely cannot be moved. `statements` removes the largest
reason they otherwise would be.

## The release rule

A connection goes back only at a real transaction boundary: `ReadyForQuery`
with status `I`, no extended-query sequence outstanding, and the session
unpinned. Never on SQL text, never on a guess.

Anything else is closed rather than returned. Handing a connection that is
sitting inside someone else's transaction to a second client gives them a
session already holding locks, part way through work they know nothing about.
Nothing about that looks like an error to either side.

`UpstreamGuard` in `pgprox-core` enforces the direction by defaulting to
discard, so a guard dropped by a cancelled future or an early return closes its
connection. Reuse takes an explicit call.

## Sans-I/O

`pool` opens no sockets. It decides which connection a caller should use,
whether a new one may be opened, and who waits. The caller does the connecting.

That is what lets the release rule and the cap arithmetic be tested
exhaustively with no Postgres anywhere. `live` wraps it, adds the waiting, and
is the `UpstreamPool` implementation the rest of the program sees.

## Where it sits

Depends on `pgprox-core` and nothing else in the workspace. Used by
`pgprox-session` and `bin/pgprox`.

The fleet-wide cap is not here. This crate holds one node's own limit;
`pgprox-cluster` decides how large that limit is.

## Reading it

`pool` is the bookkeeping. `live` is the async wrapper. `pin` is the seven
reasons a session stops being movable and the metric label each carries.
`statements` maps a client's chosen statement name onto a global one derived
from the SQL. `params` records the session settings that get replayed. `reap`
closes connections that have been idle too long, which is what stops a node
holding pools for every tenant it has ever seen.
