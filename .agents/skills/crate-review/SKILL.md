---
name: crate-review
description: Review changes before committing, against this project's standards. Use after implementing a task and before every commit, or when asked to review a diff, a crate, or a pull request in pgprox.
---

# Review before committing

Before, not after. A review that happens after the commit finds the same
problems at a higher cost.

## Run the checks first

```bash
scripts/check-crate.sh <crate>
scripts/check-coverage.sh <crate>
scripts/check-drift.sh
```

Anything red means keep working. The rest of this is for what the scripts
cannot see.

## Correctness

- [ ] **No `unwrap` or `expect` outside `#[cfg(test)]`.** Clippy catches these.
      If one was added with an `#[allow]`, that needs a comment explaining why
      the invariant cannot be encoded in the type.
- [ ] **No `panic!` reachable from client bytes.** A malformed frame must not
      take down a node serving 100k other connections.
- [ ] **Frame lengths validated before allocation.** A client claiming a 2 GB
      message gets an error, not an allocation.
- [ ] **Errors carry what the reader needs to act.** `AtCap` without the cap is
      a worse error for no saving.
- [ ] **New client-visible errors map to a real SQLSTATE**, from the table in
      `docs/internal/standards/error-handling.md`, not a generic internal error.

## Security

- [ ] **No credential can reach a log, span, metric label, error variant, or
      admin response.** Check every new `expose()` call site.
- [ ] **Nothing new derives `Debug` on a type holding a secret.**
- [ ] **Untrusted input has a bound.** Anything sized by a client-supplied
      number needs a maximum.
- [ ] **The classifier defaults to primary when unsure.** Guessing read-only on
      an ambiguous statement is a correctness bug.

## Architecture

- [ ] **Sans-I/O held.** If new logic needs a socket to test, it is in the wrong
      layer. This is the one that silently erodes.
- [ ] **No sideways crate dependency.** Everything depends on `pgprox-core` and
      nothing else in the workspace, except `pgprox-session` and `bin/pgprox`.
- [ ] **Time is injected**, never `Instant::now()` directly.
- [ ] **No blocking on the async runtime**, and no `std::sync::Mutex` held
      across an await.
- [ ] **Anything appearing in a `select!` is cancellation safe**, or its doc
      comment says it is not.

## Tests

- [ ] **Tests came first**, and were watched failing. If not, they are testing
      whatever got built.
- [ ] **Tests assert observable behaviour**, not private fields.
- [ ] **No wall-clock sleeps.**
- [ ] **New fakes behave like the real thing**, not just recording calls.
- [ ] **Coverage cleared without an exclusion or a lowered threshold.**

## Hot paths

If the change touches one of the seven declared hot paths in
`docs/internal/standards/testing.md`:

- [ ] No allocation added to the relay loop
- [ ] Allocation budget test still passes
- [ ] `iai` instruction count has not regressed

## The commit

- [ ] Subject starts with the backlog task ID
- [ ] The message says *why*, not what the diff already shows
- [ ] Scope matches the task. Doing more than asked breaks
      one-task-one-commit as surely as doing less.
- [ ] Anything left undone is stated plainly, in the message and to the user
