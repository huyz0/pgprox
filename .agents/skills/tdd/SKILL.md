---
name: tdd
description: Write a failing test first, watch it fail, then implement. Use when adding any behaviour to a pgprox crate, fixing a bug, or when a task's acceptance criteria describe observable behaviour. Enforces the red/green/refactor cycle the 95% coverage gate depends on.
---

# Test first

The coverage gate is 95% per crate from tier 1 tests alone. That number is
reachable when tests come first and unreachable when they come last, because
retrofitted tests cover the code that was easy to test rather than the code that
matters.

## The cycle

**1. Write the failing test.**

Name it after the behaviour, not the function: `releases_upstream_only_on_idle`,
not `test_release`. A year from now the failure message is the only
documentation anyone reads.

Assert on observable behaviour. A test asserting that a private field changed
will break on every refactor while catching nothing.

**2. Run it. Watch it fail.**

Not optional, and not a formality. A test that passes before the implementation
exists is testing nothing, and this is the single most common way a suite ends
up at 95% coverage while catching no bugs.

Check the failure message is the one you expected. A test failing for the wrong
reason is a test that will pass for the wrong reason later.

**3. Implement the smallest thing that passes.**

Resist writing the general case. The next test drives that.

**4. Run it. Watch it pass.**

**5. Refactor with the test green.**

## What to test in this codebase

The sans-I/O state machines are where the logic lives and where tests are cheap.
Drive them with input events and assert on output actions. No runtime, no
sockets, memory speed.

```rust
let mut s = SessionState::new();
assert_eq!(s.on_frame(Frame::ReadyForQuery(Status::Idle)), Action::ReleaseUpstream);
assert_eq!(s.on_frame(Frame::ReadyForQuery(Status::InTx)), Action::Hold);
```

For the I/O shell, use `tokio::io::duplex`. Never bind a port in tier 1.

For anything with a clock, take the `Clock` trait and use `tokio::time::pause`.
A test that sleeps in wall-clock time is a bug and is how a two-minute suite
becomes a twenty-minute one.

## Property tests

Reach for `proptest` when the input space is large and the invariant is simple.
The three that matter most here:

- Codec: any frame round-trips
- Classifier: no DML-bearing statement is ever classified read-only
- Quota: guaranteed plus leased never exceeds the cap

A property test is worth ten example tests when the failure mode is an input
nobody thought of.

## Fakes, not mocks

`pgprox-core` ships a working in-memory implementation of every trait behind the
`test-fakes` feature. Use those.

A fake pool actually tracks acquisitions and actually refuses past its cap. A
mock asserting call counts passes while the integration is broken, and the bug
surfaces at M6 where it costs ten times more to find.

## Before committing

```bash
scripts/check-crate.sh <crate>
scripts/check-coverage.sh <crate>
```

If coverage is short, write tests. Never lower the threshold, never add an
exclusion, never delete the failing test. See `standards/behavior.md`.
