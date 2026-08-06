# pgprox-route

Where a statement should go: the primary, or a replica that has caught up.

## The rule that matters

When the classifier is not confident, route to the primary.

A false negative costs a little throughput. A false positive is a stale read,
which is a data-correctness bug from the tenant's point of view and worse than
the slowness it was meant to avoid.

## What is here and what is not

The decision itself lives in `pgprox_core::route::decide`, not in this crate,
so the real router and every fake reach the same answer.

This crate supplies the two things that decision needs: what a statement does,
and how far each replica has replayed.

`classify` is a lexical scan rather than a parser. The first word must be on a
short allowlist, no word anywhere may be on a denylist of things that write or
lock, and no call may name a function known to have side effects. It cannot
know what a tenant's own functions do, which is the honest limit of deciding
from text, and `SET pgprox.route = 'primary'` is the escape hatch.

## Where it sits

Depends on `pgprox-core` and nothing else. Used by `pgprox-session` and
`bin/pgprox`.

`poller` asks each replica for its replayed position on an interval, so a
routing decision reads what was last seen rather than making a network call
while a client waits.

## Reading it

`classify` decides read or write. `hints` reads `SET pgprox.route` and the
`/* pgprox:replica */` comment form. `replica` holds each replica's health and
replayed position. `router` puts them together behind the `Router` trait.

The crate carries `#![forbid(unsafe_code)]` in its own source. It classifies
untrusted SQL, and a wrong answer here being a stale read rather than a crash
depends on nothing in it being able to corrupt memory.
