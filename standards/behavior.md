# How to work in this repo

Read this before the others. It is the working agreement, and it applies to
every turn including autonomous ones driven by `/goal`.

## One task, one commit

A task equals a commit equals one coherent change that leaves the tree green:
fmt, clippy, tests, and the coverage gate all passing. If a task cannot be
finished in one green commit, split it before writing any code.

This is what makes an unattended loop safe. Every commit is a known-good state,
so a bad turn costs one revert instead of a bisect through half a milestone. It
is also why the backlog decomposes only the current milestone: tasks written
three milestones ahead are wrong by the time they are reached.

Commit directly to `main`. No feature branches. This only works because every
commit is green and every commit is small, which is exactly what the rule above
guarantees; the hooks are what make it true rather than aspirational. A bad
commit is reverted, not merged around.

Never push unless asked.

The commit subject starts with the backlog task ID (`M-1.7: add ADRs`),
enforced by the `commit-msg` hook, so history stays traceable to the plan.

## The cycle

1. Read [roadmap.md](../product/roadmap.md) and
   [backlog.md](../product/backlog.md). If the next milestone is not decomposed,
   decompose it into commit-sized tasks with acceptance criteria and write them
   down before writing code.
2. Take the top unblocked task.
3. Implement it test-first: write the failing test, run it, watch it fail, then
   implement, then watch it pass. The `tdd` skill has the detail.
4. Review before committing, not after. Run the checks. Anything red means keep
   working.
5. Commit, referencing the task ID in the subject.
6. Tick the task in the backlog. If the work invalidated part of the roadmap,
   amend the roadmap and say so.

## Never

- Never claim a test passes without having run it. If you did not run it, say
  you did not run it.
- Never lower a coverage threshold, delete a test, or add an exclusion to make a
  check pass. The check is the point.
- Never commit a tree you know is broken, including "I will fix it in the next
  commit".
- Never change a `pgprox-core` trait without updating the trait, every fake,
  every implementation, and the ADR in the same commit. See
  [contracts.md](contracts.md).
- Never edit generated code. Change the generator or the `.proto`.
- Never widen scope silently. Doing more than the task asked is as much a
  problem as doing less, because it breaks the one-task-one-commit property.

## Always

- State plainly what was left undone and why. A task reported complete that is
  not complete is the most expensive failure mode in an autonomous loop, because
  everything after it builds on a false premise.
- When something in the plan turns out to be wrong, say so and stop. Do not
  quietly route around it.
- Prefer the smallest change that satisfies the acceptance criteria.

## Escalate, do not improvise

Stop and report when:

- The task needs a decision from the open items list in
  [plan.md](../product/plan.md), or any other genuinely product-level call.
- A `pgprox-core` contract needs to change in a way that touches more than one
  track.
- The gate cannot be made green after a bounded number of attempts. Three is a
  reasonable bound. Repeatedly rewriting a test to fit broken code is the
  failure mode this exists to prevent.
- A roadmap assumption turns out to be wrong, for example an upstream crate
  cannot do what the design assumed.

`/goal` will otherwise keep trying, and a loop that keeps trying past the point
of understanding produces a large amount of work that has to be thrown away.

## Working across tracks

Once M0 lands, tracks A through E run in parallel. Stay inside your crate. The
contract in `pgprox-core` is the interface to everyone else, and the fakes are
how you test against them. If you need something another track owns, that is a
contract change, which is a spec change first.
