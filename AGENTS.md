# pgprox

A multitenant Postgres proxy in Rust. Clients authenticate with a JWT, an
external sidecar resolves the real backend, and the proxy multiplexes a very
large number of client connections onto a capped pool of upstream connections
across several proxy nodes.

This file is loaded into every session by every agent tool. It is deliberately
an index. Detail lives in the linked files, and the nearest `AGENTS.md` in the
crate you are working in adds context specific to that crate.

## Start here

- [docs/internal/product/mission.md](docs/internal/product/mission.md) what this is and what it must never do
- [docs/internal/product/architecture.md](docs/internal/product/architecture.md) crate map and dependency rules
- [docs/internal/product/roadmap.md](docs/internal/product/roadmap.md) milestones and their completion conditions
- [docs/internal/product/backlog.md](docs/internal/product/backlog.md) the current task list
- [docs/internal/product/plan.md](docs/internal/product/plan.md) the full design, the reference for anything not covered elsewhere
- [docs/internal/product/decisions/](docs/internal/product/decisions/) architecture decision records

## Standards

Rules that are always true. Read the one that covers what you are touching.

- [docs/internal/standards/behavior.md](docs/internal/standards/behavior.md) how to work in this repo, read this first
- [docs/internal/standards/rust-style.md](docs/internal/standards/rust-style.md)
- [docs/internal/standards/error-handling.md](docs/internal/standards/error-handling.md)
- [docs/internal/standards/async-concurrency.md](docs/internal/standards/async-concurrency.md)
- [docs/internal/standards/testing.md](docs/internal/standards/testing.md)
- [docs/internal/standards/observability.md](docs/internal/standards/observability.md)
- [docs/internal/standards/security.md](docs/internal/standards/security.md)
- [docs/internal/standards/contracts.md](docs/internal/standards/contracts.md)

## Skills

Procedures, in [.agents/skills/](.agents/skills/), written to the Agent Skills
spec so they work in any tool that reads `SKILL.md`. Where a skill needs to run
something it calls a script in `scripts/`, never a tool-specific built-in.

## Non-negotiables

These are the ones that cause the most damage when missed. Six of the seven are
enforced by a script and the script is named beside each. **Rule 3 is not, and
cannot be**: no script reads whether you ran the thing you say you ran. It is
here because it is the one that makes the other six trustworthy, and it is
marked so that this list stops claiming an enforcement it does not have.

That distinction was not free. `M13` audited this sentence when it read "Each is
enforced by a script, not by good intentions", and found four of the seven with
no script or with the wrong one credited.

1. One task equals one commit equals one change that leaves the tree green.
   Split anything that cannot meet that. `scripts/check-commit-msg.sh` requires
   the subject to name a task the backlog actually lists; the pre-commit hooks
   are what "green" means.
2. Never lower a threshold or delete a test to make a check pass. Thresholds are
   constants and `scripts/check-drift.sh` refuses to let one become settable;
   `scripts/check-tests-kept.sh` names any test that disappears and requires a
   `Removes-test:` line in the commit message.
3. Never claim a test passes without having run it. **No script enforces this.**
   It is a rule about what you say, and nothing can check a claim against an
   intention. Every other rule here rests on it: a green gate reported by
   someone who did not run it is worth less than no gate.
4. Every crate holds 95% line coverage on its own, from tier 1 tests alone.
   `scripts/check-coverage.sh`, against a constant that no environment can
   move.
5. Business logic is sans-I/O. If it needs a socket to test, it is in the wrong
   layer. Enforced by `scripts/check-sans-io.sh`: no library crate names a
   concrete socket type or reads the real clock outside the two places that
   exist to hold them. See
   [docs/internal/standards/async-concurrency.md](docs/internal/standards/async-concurrency.md).
6. Changing a `pgprox-core` trait means updating the trait, every fake, every
   implementation, and the ADR in one commit. `scripts/check-core-contract.sh`
   holds the two mechanical halves: every implementor is in the commit, and so
   is an ADR. Call sites and dependent specs stay with the skill and review. See
   [docs/internal/standards/contracts.md](docs/internal/standards/contracts.md).
7. Credentials never reach a log. `scripts/check-secrets.sh` holds the static
   half: `SecretString` cannot be printed, so the one route to a real value is
   `expose()`, and no result of it may reach a formatting macro.
   `scripts/e2e.sh` holds the claim itself, searching every node's log for the
   token it authenticated with and the backend password the sidecar returned,
   with a positive control so a clean result means something. See
   [docs/internal/standards/security.md](docs/internal/standards/security.md).

## Checks

[scripts/README.md](scripts/README.md) is the index: what every script is for,
which run on every commit, and which want Docker and an hour. It is checked for
completeness, so a script added and left out of it fails the pre-commit hook.

The ones you will run most:

```bash
scripts/check-crate.sh <crate>    # fmt and clippy for one crate
scripts/check-coverage.sh <crate> # the 95% gate
scripts/check-drift.sh            # derived files still match canonical source
```

Measurement is slower and runs when asked rather than per commit:

```bash
scripts/bench.sh                  # instruction counts against the baseline
scripts/scale.sh <connections>    # RSS, added latency, upstream connections
scripts/e2e.sh                    # the compose stack and M6's three properties
```

The forty-five milestone gates are in `scripts/gates/`, one per milestone, all
run by CI. They are satisfied rather than maintained, and the roadmap section
for a milestone says in prose what its gate checks.

These same scripts run from git hooks, from CI, and from agent hooks. If you
add a check, add it to a script so all three pick it up.
