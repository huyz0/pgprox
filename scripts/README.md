# scripts

Everything this project checks or measures runs from here. So do the git hooks,
CI, and the agent hooks, which is the point: a check implemented twice drifts,
and the copy that matters is whichever one nobody ran.

Three groups, and you almost certainly want the first.

## Checks: fast, and run on every commit

Seconds each. These are the pre-commit hook, and CI runs the same files.

| Script | What it refuses |
| --- | --- |
| `check-fmt.sh` | Unformatted Rust |
| `check-crate.sh` | clippy warnings, for one crate or the workspace |
| `check-coverage.sh` | A crate under 95% line coverage from tier 1 tests alone |
| `check-deps.sh` | An advisory, a licence outside the allowlist, a source outside crates.io |
| `check-drift.sh` | A derived file that no longer matches its canonical source |
| `check-links.sh` | A relative Markdown link that resolves to nothing |
| `check-readmes.sh` | A crate with no README, or one whose dependency claims disagree with `Cargo.toml` |
| `check-layering.sh` | A crate depending on something the dependency rule forbids |
| `check-sans-io.sh` | A library crate naming a socket type or reading the real clock |
| `check-secrets.sh` | An exposed credential reaching a formatting macro |
| `check-unsafe.sh` | An `unsafe` block that does not meet the five conditions |
| `check-wired.sh` | A symbol on the wiring watchlist that nothing reaches any more |
| `check-core-contract.sh` | A `pgprox-core` trait change arriving without its implementors or its ADR |
| `check-tests-kept.sh` | A test that disappeared without being declared |
| `check-commit-msg.sh` | A commit subject naming no backlog task |
| `check-portability.sh` | A rule only one AI tool would see |

## Measurement: slow, and run when asked

Minutes to hours. Most want Docker. None of these gate a commit; they produce a
number, and the number goes in a run document under
[`docs/internal/product/perf/`](../docs/internal/product/perf/).

| Script | What it measures | Needs |
| --- | --- | --- |
| `bench.sh` | Instruction counts for the hot paths, against the committed baseline | valgrind |
| `scale.sh` | RSS, added latency and upstream connections at N connections | Docker |
| `compare.sh` | pgprox against pgbouncer and pgcat, one workload, one machine | Docker |
| `e2e.sh` | The compose stack and the properties M6 is judged on | Docker |
| `profile.sh` | Which code the reference workload actually reaches | Docker |
| `pinning.sh` | What pinning costs multiplexing | Docker |
| `admission.sh` | What a fleet with no capacity left tells a client | Docker |
| `arena.sh` | Whether per-connection memory is the allocator's or the connection's | Docker |
| `conformance.sh` | The codec against a real Postgres | Docker |
| `driver-matrix.sh` | Every supported driver against the proxy | Docker, five language toolchains |
| `cipher-matrix.sh` | Which cipher suite each driver negotiates, per build | Docker |
| `rolling-upgrade.sh` | A fleet restarted under load, mid-transaction | Docker |
| `message-coverage.sh` | Which protocol messages the conformance suite exercised | Docker |
| `localstack.sh` | A one-node stack for poking at by hand | Docker |
| `fips-check.sh` | The FIPS variant, compiled and run | cmake, Go, clang |
| `mutants.sh` | Whether a test would notice the code being wrong | cargo-mutants |
| `miri.sh` | Undefined behaviour in the crates that hold unsafe | nightly |
| `fuzz.sh` | The decoder against bytes nobody chose | nightly, cargo-fuzz |
| `msrv.sh` | Prints the minimum supported Rust version, read by CI |  |
| `semantic_coverage.py` | Turns one instrumented replay into the three lists |  |

### Mutation testing without waiting for tomorrow

A full run is 3,694 mutants, each a build plus a test run, which is why CI
schedules it nightly across four shards. That makes it a thing that tells you on
Tuesday about a test you weakened on Monday.

`MUTANTS_DIFF` narrows it to the lines a diff touched, and to the crates that
diff reached. A normal change produces single-digit mutants and takes minutes,
which is what puts it on the commit path:

```bash
git diff origin/main...HEAD > /tmp/pr.diff
MUTANTS_DIFF=/tmp/pr.diff scripts/mutants.sh
```

It is a narrowing, not a different check: survivors are compared against the
same baseline. What it cannot see is a mutant your change made survivable
somewhere it did not touch, which is what the nightly is still for.

`MUTANTS_SHARD=k/n` runs one slice, which is how the nightly splits across
runners.

## Gates: one per milestone, in `gates/`

Forty-five files, and you are not expected to read them.

Each is one milestone's completion condition: the thing that has to keep being
true for a milestone to still count as done. They are frozen. `m7-complete.sh`
is not maintained, it is *satisfied*, and it fails on the day something quietly
stops holding.

They live in their own directory because they are more than half the files here
and none of them is something a newcomer needs. If you are looking for what a
gate checks, the milestone's section in
[roadmap.md](../docs/internal/product/roadmap.md) says so in prose and names
the script.

Every one runs in CI on every commit. `check-drift.sh` fails if a gate exists
and CI does not run it, because a gate nobody runs is a record rather than a
gate.

## Writing one

Source `lib.sh` and use its helpers. From `gates/` that is `../lib.sh`.

```bash
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cd "$REPO_ROOT"

ok   "what held"
fail "what did not, and what it costs the reader"
finish   # exits non-zero with the accumulated count
```

Four things the helpers are load-bearing for:

`finish` reports every problem rather than stopping at the first, which matters
when an agent is reading the output rather than a person.

`fail` increments a counter **in the calling shell**, so calling it inside a
pipeline's right-hand side prints a red FAIL and exits 0. `check-drift.sh` has
a rule that catches that, because it shipped once.

Thresholds are constants, never `${NAME:-95}`. A settable threshold is one
anybody can lower, including by accident from an exported variable, and the
gate then announces its own weakened bar and passes.

Paths a check reads should take a `PGPROX_*` override with the real path as the
default, so [`tests/gates/negative.sh`](../tests/gates/negative.sh) can point
the check at a deliberately broken tree. That suite is what makes the rest of
this directory worth anything: it proves the checks can fail.
