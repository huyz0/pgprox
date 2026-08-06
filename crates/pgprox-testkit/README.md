# pgprox-testkit

Test scaffolding, and one piece of knowledge that was learned twice.

## The bug this crate exists to hold

A Postgres container accepts TCP, completes a TLS handshake, and answers a
startup packet well before its databases exist. During that window it replies
`ErrorResponse` with SQLSTATE `57P03`, the database system is starting up.

A readiness probe that accepts any reply therefore reports ready too early.

That shipped once in the M1.11 Postgres probe, where it made the suite pass
against Postgres 17 and fail against 18 purely on timing. Then it was written
again from scratch in the SCRAM tests. Two independent reproductions is the
evidence that it belongs in one place.

## Where it sits

Depends on nothing in the workspace. A dev dependency of `pgprox-auth` and
`pgprox-proto`, which are the two crates with tests that need a real container.

Never a runtime dependency of anything. If you find it in a
`[dependencies]` section, that is the bug.
