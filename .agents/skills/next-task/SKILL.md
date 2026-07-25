---
name: next-task
description: Pick the next thing to work on, and decompose a milestone into commit-sized tasks when the backlog runs dry. Use at the start of any working session, when asked "what's next", or when a milestone completes and the next one has no tasks yet.
---

# Pick the next task

## Read the state

1. `product/roadmap.md` for the current milestone and its completion condition
2. `product/backlog.md` for tasks
3. Run the current milestone's completion condition. It tells you what is
   actually left, which is often not what the backlog says.

The completion condition is the source of truth. The backlog is a plan, and
plans drift.

## If the backlog has unblocked tasks

Take the top one. Do not shop for a more interesting one further down; ordering
encodes dependencies that are not always written out.

Check the acceptance criteria are still right given what has been built since
they were written. If they are not, fix them in the same commit and say so.

## If the backlog is dry, decompose

Write tasks before writing code. The rule that makes decomposition correct:

> One task equals one commit equals one change that leaves the tree green.

Green means fmt, clippy, tests, and the coverage gate all pass. If a task cannot
meet that, it is too big. Split it.

Sizing tests, in order of usefulness:

- **Does it leave the tree green on its own?** A task that adds a trait but not
  its fake fails the gate, so it is half a task.
- **Can it be described in one sentence without "and"?** Two verbs usually means
  two tasks.
- **Would reverting it undo one coherent thing?** If a revert would leave the
  tree broken, the split is wrong.

Each task gets an ID (`M0.4`), a one-line description, and acceptance criteria
written as observable behaviour rather than as implementation steps.

Only decompose the current milestone. Tasks written three milestones ahead are
wrong by the time they are reached, and rewriting them costs more than writing
them fresh.

## Legitimately merging tasks

Occasionally two backlog tasks cannot be committed separately without leaving
the tree red. That happens when their acceptance criteria are mutually
dependent, such as a script and the config file it validates.

Merge them, and say so in the commit message with the reason. The green-tree
rule outranks the one-task-one-commit rule, because green commits are what make
the history safe to revert.

## When to stop instead of picking

Escalate rather than improvising when the next task:

- Needs a decision from the open items list in `product/plan.md`
- Requires a `pgprox-core` contract change touching more than one track
- Rests on a roadmap assumption that has turned out to be wrong

See `standards/behavior.md`. A loop that keeps working past the point of
understanding produces a lot of work that gets thrown away.
