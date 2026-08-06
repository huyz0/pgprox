# Contracts

`pgprox-core` holds the traits and types every other crate depends on. It is the
reason five tracks can run in parallel, and it is the thing that breaks them all
if it changes carelessly.

## The dependency rule

Every crate depends on `pgprox-core` and on nothing else in the workspace. Two
stated exceptions: `pgprox-session` composes `pgprox-proto`, `pgprox-pool`, and
`pgprox-route`; `bin/pgprox` composes everything.

`pgprox-core` itself depends on no other workspace crate and performs no I/O. If
something in core needs a socket, it belongs somewhere else.

This is checked mechanically, not by review. A crate reaching sideways into a
peer is a build failure.

## What belongs in core

Traits, DTOs, error types, ID newtypes, and the fakes. Nothing that does work.

The test for whether something belongs: would two different tracks both need it,
and would they be wrong to define it separately? A `PoolKey` qualifies. A pool
implementation does not.

## Every trait ships with a fake

A trait without a working in-memory fake is not done. The fake lives in the same
crate behind the `test-fakes` feature, and it has its own tests.

Fakes behave like the real thing. A fake pool actually tracks acquired
connections and actually refuses past its cap, because a fake that just records
calls lets integration bugs through to M6 where they are expensive. If the fake
and the real implementation can diverge, write one shared test suite that runs
against both.

## Changing a contract

A contract change is a spec change first, then one atomic commit containing:

1. The trait or type change itself
2. Every fake updated
3. Every implementation updated
4. Every call site updated
5. The ADR recording why, including what was rejected
6. Any dependent track's spec updated

The `contract-change` skill walks this. Do not do it by hand and hope.

If the change touches more than one track, stop and escalate before starting.
The cost is not the edit, it is the merge conflict across five parallel branches
and the half-day of everyone rebuilding against a moved target.

## Additive first

Prefer adding to changing. A new trait method with a default implementation
breaks nobody. A changed signature breaks everyone at once.

`#[non_exhaustive]` on public enums describing external state, so adding a
variant later is not a breaking change. New fields on a struct get a default
where that makes sense.

When a breaking change is genuinely right, batch it. One painful contract change
is much cheaper than four small ones spread across a week, because each one
costs every track a rebase.

## Versioning the sidecar contract

The `.proto` is the one interface not under this repo's control. Treat it as
public API from the first commit: field numbers are never reused, fields are
never removed, and anything optional is genuinely optional in the Rust type.

Changes there need agreement from the sidecar owners before the Rust side moves,
not after.
