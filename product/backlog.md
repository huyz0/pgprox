# Backlog

One task equals one commit equals one change that leaves the tree green. If a
task cannot be finished in one green commit, split it before writing code.

Task IDs are stable. Completed tasks stay here with their commit reference so
the history of why something was done survives.

Decomposition rule: only the current milestone is decomposed in detail. Future
milestones stay as roadmap entries until their turn, because decomposing them
early produces tasks that are wrong by the time they are reached.

## M-1: AI development system

- [x] `M-1.1` Repository bootstrap. `git init`, `.gitignore`, plan copied to
  `product/plan.md`, roadmap with executable completion conditions, this file.
- [ ] `M-1.2` Root context. `AGENTS.md` as the canonical instruction file,
  `CLAUDE.md` importing it. Root file stays an index and links out.
  Acceptance: both files exist, `CLAUDE.md` is the one-line import, `AGENTS.md`
  links every standards file.
- [ ] `M-1.3` Standards, part one: `rust-style.md`, `error-handling.md`,
  `async-concurrency.md`. Acceptance: each states rules that are checkable, and
  names the lint or script that enforces it where one exists.
- [ ] `M-1.4` Standards, part two: `testing.md`, `observability.md`,
  `security.md`. Acceptance: `testing.md` matches the three tiers and the hot
  path discipline in the plan without restating the plan.
- [ ] `M-1.5` Standards, part three: `contracts.md` and `behavior.md`.
  Acceptance: `behavior.md` carries the commit-granularity rule and the
  escalation conditions verbatim enough that an autonomous turn can follow it.
- [ ] `M-1.6` Product docs: `mission.md` and `architecture.md`. Acceptance:
  `architecture.md` states the crate dependency rule (everything depends on
  `pgprox-core` and nothing else in the workspace, with `pgprox-session` and
  `bin/pgprox` as the two stated exceptions).
- [ ] `M-1.7` ADRs, one per row of the decisions table in the plan. Ten records,
  each naming the alternatives rejected and why. Acceptance: `decisions/0001`
  through `decisions/0010` exist and each has a Consequences section.
- [ ] `M-1.8` Enforcement scripts: `check-fmt.sh`, `check-crate.sh`,
  `check-coverage.sh`, `check-drift.sh`. Acceptance: each runs and exits
  correctly on the current (Rust-free) tree, meaning they no-op cleanly rather
  than failing when there are no crates yet.
- [ ] `M-1.9` `lefthook.yml` calling the scripts, plus install instructions.
  Acceptance: hooks fire on a test commit and block on a seeded violation.
- [ ] `M-1.10` CI workflow running the same scripts. Acceptance: workflow file
  validates and calls `scripts/` rather than reimplementing the checks.
- [ ] `M-1.11` Claude Code hooks as accelerator, calling identical scripts.
  Acceptance: `.claude/settings.json` hooks reference `scripts/`, no check is
  implemented twice.
- [ ] `M-1.12` Skills, part one: `spec`, `tdd`, `next-task`. Acceptance: Agent
  Skills format, vendor-neutral bodies, no `.claude/` paths.
- [ ] `M-1.13` Skills, part two: `contract-change`, `crate-review`, `adr`.
- [ ] `M-1.14` Skills, part three: `hot-path`, `wire-debug`, `skill-forge`.
- [ ] `M-1.15` Skill discovery symlink and per-crate `AGENTS.md` stubs for the
  eleven planned crates.
- [ ] `M-1.16` `scripts/m-1-complete.sh`, the milestone completion condition.
  Acceptance: exits zero on a complete M-1 and non-zero with a useful message on
  each individual failure.
- [ ] `M-1.17` Portability check on a second tool. Run a small throwaway task
  under Codex CLI or Cursor and record the result as an ADR. Acceptance: the ADR
  states what worked, what did not, and what was changed as a result.

## M0 and later

Not yet decomposed. See [roadmap.md](roadmap.md). The `next-task` skill
decomposes the next milestone when the current one closes.
