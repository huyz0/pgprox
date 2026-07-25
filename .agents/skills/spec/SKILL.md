---
name: spec
description: Turn a feature request or milestone into a spec directory with contracts, acceptance criteria, and an ordered task list. Use before building anything non-trivial, especially work that crosses crate boundaries or that another track will code against.
---

# Write the spec first

Five tracks run in parallel against contracts. A spec is how two agents working
different crates avoid silently disagreeing about an interface.

## Layout

```
specs/<YYYY-MM-DD>-<slug>/
  spec.md         what and why, acceptance criteria
  contracts.md    exact type signatures this work owns
  tests.md        the test plan, written before the code
  tasks.md        ordered, commit-sized
```

## spec.md

**Problem.** What is broken or missing. Concrete, with the failure it causes.

**Scope.** What this covers, and explicitly what it does not. The second list
prevents the slow widening that breaks one-task-one-commit.

**Acceptance criteria**, as observable behaviour in given/when/then form, so
they translate directly into tests:

> Given a session with an open transaction
> When the client sends a statement that would route to a replica
> Then it routes to the primary instead, because the transaction is already
> bound to a connection

Criteria phrased as implementation steps ("add a field to X") are not criteria.
They cannot be verified and they overconstrain the solution.

**Open questions.** Anything needing a human decision, listed rather than
guessed. If there are blocking ones, stop after writing them.

## contracts.md

The exact signatures this work introduces or changes. Copy-pasteable Rust, not
prose description.

This file is the artifact other tracks read. Getting it right matters more than
getting the implementation right, because the implementation is local and the
contract is not.

If the work changes anything already in `pgprox-core`, stop and use the
`contract-change` skill instead. A contract change has blast radius that a
normal spec does not account for.

## tests.md

What proves it works, tier by tier:

- Tier 1, the unit tests carrying the coverage
- Tier 2, integration or conformance if any
- Properties worth a `proptest`, and their invariants
- What is deliberately not tested, and why

Writing this before the code is what stops the test plan becoming a description
of whatever happened to get built.

## tasks.md

Ordered, commit-sized, each with an ID and acceptance criteria. Use the
`next-task` skill's sizing rules. Cross-reference into `product/backlog.md` so
there is one place to look for what is in flight.

## When a spec is not worth it

A one-line fix with an obvious test does not need four files. The signal that a
spec is warranted: the work crosses a crate boundary, another track will code
against it, or you cannot state the acceptance criteria without thinking.
