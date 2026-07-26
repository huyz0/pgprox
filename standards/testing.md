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
  failure mode. `cargo-mutants` runs nightly against the pure state machines and
  surviving mutants are treated as missing tests.

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
7. Gossip digest encode and decode.

Path 7 was written to be allocation-free and are claims until the
milestone that asserts them, and they are written down here so the assertion
has something to check rather than a number to discover.

A budget test asserts its own harness first: it allocates deliberately once and
checks the counter moved. A budget that measured nothing would pass forever.

The reference workload profile also feeds a PGO build, which typically returns 5
to 15% on branch-heavy code like a codec. It costs almost nothing extra since the
profile is already being collected.
