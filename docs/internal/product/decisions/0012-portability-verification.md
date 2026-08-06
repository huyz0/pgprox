# 0012. Portability is verified mechanically, and once by hand

Status: accepted, with one item outstanding

## Context

The development system is meant to work in any coding agent, not only Claude
Code. That claim is easy to make and easy to quietly break: someone adds a
useful check that reads a vendor-specific path, and six months later the repo
only really works in one tool.

Two open standards make the content layers portable without adapters.
`AGENTS.md` came out of OpenAI in August 2025, moved to the Linux Foundation's
Agentic AI Foundation, and is read natively by Codex CLI, Cursor, Copilot,
Gemini CLI, Aider, Windsurf, Zed, and others. `SKILL.md` was published as an
independent standard in December 2025 and is read unchanged by around forty
products. Claude Code is the one holdout on the first, reading `CLAUDE.md`,
which is why that file is a one-line `@AGENTS.md` import and nothing else.

Enforcement has no standard, which is the actual portability risk.

## Decision

Portability is a continuously checked property, not a one-time claim.
`scripts/check-portability.sh` verifies:

- `AGENTS.md` exists and uses no vendor-specific syntax. The `@`-import form is
  Claude Code specific and belongs only in the adapter files.
- Every skill parses as the Agent Skills format, with a `description` long
  enough to function as a retrieval surface.
- Skills that run commands call `scripts/`, never a tool built-in. Script
  invocation is the one capability every coding agent has.
- No executable logic depends on a single vendor's files. A line naming two or
  more adapters is an accept-any list and passes; a line naming exactly one is a
  hard dependency and fails.

## Consequences

- The claim is enforced rather than asserted, and it degrades visibly instead of
  silently.
- Writing this audit immediately found a real defect. `scripts/gates/m-1-complete.sh`
  hard-required `.claude/settings.json`, which meant **M-1 could never pass for
  a developer working only in Cursor or Codex**. The milestone gate itself was
  the least portable thing in the repository. It now accepts any known adapter.
- The multi-vendor rule is deliberately shaped to make the correct pattern
  cheaper than the incorrect one: enumerating every adapter you accept passes,
  naming one fails.
- The audit is mechanical and therefore limited. It proves a second tool can
  *read* the system. It cannot prove another agent *follows* it, which is a
  question about model behaviour and needs a human.

## Outstanding

**The interactive half of M-1.17 is not done.** No second agent tool is
installed on this machine (checked: `codex`, `cursor`, `cursor-agent`, `gemini`,
`aider`, `goose`, `opencode`). Running a real task under one of them, and
recording what worked, what did not, and what changed as a result, still needs a
human.

Until that happens this record documents structural portability only. The
distinction matters: a system that another tool can read but consistently
ignores is not portable in any way that helps.

## Alternatives rejected

**Generating vendor-specific files from a canonical source.** The design this
replaced, and it was more machinery than the problem needs now that two real
standards exist. What remains is one symlink and one import line.

**Trusting review to catch vendor leaks.** Rejected because the leak that
prompted this ADR was written, reviewed, and committed by the same process that
was supposed to catch it, and survived until a script looked for it.

**Skipping the human check as unnecessary given the standards.** Rejected
because the standards guarantee the file format, not that a given model treats
the content as binding, and that is the part actually being relied on.
