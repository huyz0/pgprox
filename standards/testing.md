# Testing

Three tiers split by cost, plus a fourth discipline that is not about
correctness at all. The rule that shapes everything: **tier 1 alone carries the
95% number**, so coverage never waits on Docker.

## Tier 1: pre-commit, budget 2 minutes

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo llvm-cov nextest --lib --bins --fail-under-lines 95
gitleaks protect --staged --redact
```

Nothing in tier 1 opens a socket, starts a container, or sleeps. Anything that
needs to is gated behind `#[cfg(feature = "integration")]` and excluded here.

Staying inside two minutes with an instrumented build is deliberate, not luck:

- `cargo-nextest`, not `cargo test`. Real parallelism, no serialized harness
  startup.
- A dedicated `CARGO_TARGET_DIR` for coverage runs, so the instrumented build
  cache stops thrashing against the normal dev cache. Without this the hook
  rebuilds the world on every commit.
- `mold` as the linker. Link time dominates a workspace this shape.
- `opt-level = 1` on `[profile.test]`. The simulation and property tests are
  compute-bound and unoptimized builds cost more than they save.

When the budget starts slipping, narrow to crates touched by the staged diff
(`cargo llvm-cov -p`) and keep the full-workspace gate in CI. Do not lower the
threshold.

## Coverage rules

- **Per crate, not workspace-wide.** A global number lets a 99% crate mask a 70%
  one. CI parses `cargo llvm-cov --json` and asserts each crate separately.
- **Exclusions are `bin/pgprox/src/main.rs` and generated code under `OUT_DIR`.**
  Nothing else, and adding one needs a reason in the commit message. Error paths
  are explicitly not excluded; the fakes exist so failure branches are reachable.
- **Fakes over mocks.** `pgprox-core` ships a working in-memory implementation of
  every trait behind the `test-fakes` feature. A fake that behaves like the real
  thing catches integration mistakes that a mock asserting call counts cannot.
- **Coverage is a floor, not a goal.** 95% with weak assertions is the standard
  failure mode, and coverage cannot see it: a line that ran is not a line that
  mattered. `scripts/mutants.sh` runs `cargo-mutants` in the nightly job against
  every crate in the workspace and both binaries: four when `M10.3` built it,
  fourteen after `M14`, sixteen after `M17.2`. A surviving mutant is a missing
  test. One that is accepted instead goes in `product/mutants-baseline.txt` with
  a reason, and one that is in neither place fails the script.
  **A crate's decisions are tested in that crate.** An end-to-end test
  somewhere downstream does not discharge it, and the difference is measurable
  rather than stylistic. `M20` added six functions across three crates in one
  week. Four were tested where they live and every mutant of them dies;
  `Upstreamed::unfit` and `Upstreamed::goodbye` were tested only from
  `bin/pgprox`, and `M22.1` found `goodbye` surviving replacement by `()` and
  `unfit` surviving all three of its possible answers. All three answers of a
  boolean surviving is not a weak test, it is no test, and the integration
  tests that did cover both were passing throughout.
  The reason is what a sweep measures: it mutates one crate and runs that
  crate's tests. A decision whose only witness is downstream is invisible to
  the tool that exists to find untested decisions, which makes this the one
  rule here that mutation testing cannot enforce for you.
  The suite runs under nextest there, with a per-test timeout, and that is not
  a detail. `cargo mutants` budgets the whole suite and calls the mutant a
  timeout when the budget runs out; under `cargo test` one hung test costs the
  run its verdict, so a mutant is reported as surviving even when another test
  failed it. `M10.13` found that by writing assertions that fail six mutants
  and watching all six come back as timeouts. Twenty-three baseline entries
  turned out to be saying something about the runner rather than about the
  tests. **A timeout is a run nobody read, not a mutant the suite caught.**
  The cap has a second edge, and `M17.7` is about it. nextest reports a
  terminated test as a failure and `cargo mutants` reads any failure as a kill,
  so a cap that is too tight does not merely kill a slow test: it reports a
  **kill for a mutant nothing detected**. That is worse than the timeouts
  above, which at least fail loudly. Both numbers are therefore derived from a
  measured suite under the parallelism the run actually uses, and the
  measurement is written beside each of them, in `.config/nextest.toml` and in
  `scripts/mutants.sh`. `M10.13` asked for exactly that and it was not done:
  its own entry says "Both numbers want measuring rather than picking".
  This ran nowhere until `M10.3`, and the sentence claiming it did was three
  milestones old. M9 is why it was worth building rather than deleting: three of
  its defects survived every gate because a fake answered something Postgres
  refuses, and a fourth was a fix that went in half-applied and green. Each is a
  line whose removal changed nothing any test could see.

## Tier 2: pre-push and CI

No time budget. Everything above plus conformance against real Postgres 17 and
18 driven by five drivers, the cluster simulation, pooling correctness under
concurrent session features, replica consistency under injected lag, drain under
load, `cargo deny check`, `cargo audit`, Semgrep, and short fuzz runs.

The wire decoder and the statement classifier both parse untrusted bytes off the
internet. Fuzzing them is a security control, not a nicety. Corpora are
committed; any crash found becomes a unit test.

## Tier 3: nightly and pre-release

Mutation testing, long fuzz runs, the FIPS build and its driver cipher-suite
matrix, and the 100k-connection scale run.

## Hot-path coverage: a different question

Line coverage asks whether a test touched a line. It says nothing about whether
that line runs a billion times a day, and a proxy lives on that distinction.
This never runs in pre-commit.

Everything measures against one committed **reference workload**: a realistic
tenant mix (a few hot tenants, a long tail of idle ones), query shape
distribution, connection churn rate, transaction size distribution, replica read
fraction. Without a fixed reference, profiles are not comparable week to week.

Replaying it against an instrumented binary and keeping LLVM **execution counts**
rather than hit/miss produces a cost profile, cross-referenced into three lists
that each imply a different action:

- **Hot and under-tested**: high count, low assertion density or surviving
  mutants. The highest-risk code in the repo and the list that earns tests
  first. A better signal than uncovered-line count, which mostly points at error
  paths nobody hits.
- **Hot and expensive**: count times per-call cost. The optimization queue,
  ordered by total contribution rather than by what looks interesting.
- **Cold and complex**: near-zero count, high complexity or visible
  hand-optimization. Candidates for deletion.

**Gate on allocation counts and instruction counts, never wall clock.**
`dhat-rs` asserting "relaying a 1 KiB DataRow allocates zero times" and
`iai-callgrind` instruction counts are deterministic, so a 3% regression is
visible on noisy shared runners where `criterion` reports noise. Keep
`criterion` for numbers that inform rather than gate.

The declared hot paths, with budgets, are:

1. The steady-state relay loop, both directions. **Zero allocations** per frame
   once warm, asserted in `crates/pgprox-proto/tests/budgets.rs`.
2. Frame boundary scanning (type byte plus length). **Zero**, same file. Decode
   returns a borrowed frame, so finding a boundary copies nothing.
3. `ReadyForQuery` status handling and the pool release decision. **Zero**,
   asserted in `crates/pgprox-pool/tests/budgets.rs`.
4. Warm-pool acquire. **Zero** once warm, same file. Acquire and release move a
   connection between two collections that keep their capacity.
5. Route decision: classification plus replica eligibility. **Zero**, asserted
   in `crates/pgprox-route/tests/budgets.rs`. It was one allocation per
   statement until M7.10 measured it.
6. Grant cache lookup on connect. **At most 17 allocations** per connection on
   a hit, asserted in `crates/pgprox-auth/tests/budgets.rs`. Not zero, and the
   file says what the fifteen it measured are made of.
7. Gossip digest encode and decode. **At most 14 to encode and 32 to decode**
   per digest, asserted in `bin/pgprox/tests/budgets.rs`. Decoding costs more
   because JSON hands every field over as an owned string first.

Every declared path now has a number rather than a claim. Two of them are not
zero, and each states what its allocations are made of, because a budget
nobody can account for is a number that gets raised the first time it fails.

A budget test asserts its own harness first: it allocates deliberately once and
checks the counter moved. A budget that measured nothing would pass forever.

The reference workload profile also feeds a PGO build, which typically returns 5
to 15% on branch-heavy code like a codec. It costs almost nothing extra since the
profile is already being collected.
