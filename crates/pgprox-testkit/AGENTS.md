# pgprox-testkit

Shared scaffolding for integration tests. Never a runtime dependency: no product
crate may depend on it.

## Why it exists

A Postgres container accepts TCP and answers a startup packet well before its
databases exist, replying `57P03` "the database system is starting up". A probe
that accepts any reply reports ready too early.

That bug shipped in the M1.11 probe, where it made the suite pass against
Postgres 17 and fail against 18 purely on timing, and was then written again
from scratch in the SCRAM tests. Two independent reproductions is why the
classification lives here rather than in whoever writes the next probe.

## Rules specific to this crate

- Sans-I/O, like everything else. `classify_startup_reply` takes bytes and
  returns a verdict; it never opens a socket. That is what makes the `57P03`
  case testable without a container.
- A genuine error is `Failed`, not `NotYet`. Retrying forever on a real failure
  turns a clear error into a timeout, which is worse to debug.
- An unexpected tag is `NotYet`. A partial read during startup is common and
  retrying costs nothing.
- Add to this crate when a test hazard recurs, not the first time it appears.
  One occurrence is a bug; two is a missing abstraction.
