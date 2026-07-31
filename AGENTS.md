# pgprox

A multitenant Postgres proxy in Rust. Clients authenticate with a JWT, an
external sidecar resolves the real backend, and the proxy multiplexes a very
large number of client connections onto a capped pool of upstream connections
across several proxy nodes.

This file is loaded into every session by every agent tool. It is deliberately
an index. Detail lives in the linked files, and the nearest `AGENTS.md` in the
crate you are working in adds context specific to that crate.

## Start here

- [product/mission.md](product/mission.md) what this is and what it must never do
- [product/architecture.md](product/architecture.md) crate map and dependency rules
- [product/roadmap.md](product/roadmap.md) milestones and their completion conditions
- [product/backlog.md](product/backlog.md) the current task list
- [product/plan.md](product/plan.md) the full design, the reference for anything not covered elsewhere
- [product/decisions/](product/decisions/) architecture decision records

## Standards

Rules that are always true. Read the one that covers what you are touching.

- [standards/behavior.md](standards/behavior.md) how to work in this repo, read this first
- [standards/rust-style.md](standards/rust-style.md)
- [standards/error-handling.md](standards/error-handling.md)
- [standards/async-concurrency.md](standards/async-concurrency.md)
- [standards/testing.md](standards/testing.md)
- [standards/observability.md](standards/observability.md)
- [standards/security.md](standards/security.md)
- [standards/contracts.md](standards/contracts.md)

## Skills

Procedures, in [.agents/skills/](.agents/skills/), written to the Agent Skills
spec so they work in any tool that reads `SKILL.md`. Where a skill needs to run
something it calls a script in `scripts/`, never a tool-specific built-in.

## Non-negotiables

These are the ones that cause the most damage when missed. Each is enforced by
a script, not by good intentions.

1. One task equals one commit equals one change that leaves the tree green.
   Split anything that cannot meet that.
2. Never lower a threshold or delete a test to make a check pass.
3. Never claim a test passes without having run it.
4. Every crate holds 95% line coverage on its own, from tier 1 tests alone.
5. Business logic is sans-I/O. If it needs a socket to test, it is in the wrong
   layer. Enforced by `scripts/check-sans-io.sh`: no library crate names a
   concrete socket type or reads the real clock outside the two places that
   exist to hold them. See
   [standards/async-concurrency.md](standards/async-concurrency.md).
6. Changing a `pgprox-core` trait means updating the trait, every fake, every
   implementation, and the ADR in one commit. See
   [standards/contracts.md](standards/contracts.md).
7. Credentials never reach a log. See [standards/security.md](standards/security.md).

## Checks

```bash
scripts/check-fmt.sh              # workspace formatting
scripts/check-crate.sh <crate>    # fmt and clippy for one crate
scripts/check-coverage.sh <crate> # the 95% gate
scripts/check-drift.sh            # derived files still match canonical source
scripts/check-sans-io.sh          # business logic touches no socket and no clock
scripts/check-secrets.sh          # no exposed credential reaches a formatter
```

Measurement, which is slower and runs when asked rather than per commit:

```bash
scripts/bench.sh                  # instruction counts against the baseline
scripts/profile.sh                # replay the workload, semantic coverage
scripts/scale.sh <connections>    # RSS, added latency, upstream connections
scripts/e2e.sh                    # the compose stack and M6's three properties
```

These same scripts run from git hooks, from CI, and from agent hooks. If you
add a check, add it to a script so all three pick it up.
