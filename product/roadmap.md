# Roadmap

Every milestone carries a completion condition that is a command, not a
judgement. `/goal` hands that command to its checker, so a milestone whose
"done" cannot be expressed as an exit code cannot be driven autonomously.

Run one `/goal` per milestone. A single goal spanning the whole project gives
the checker nothing it can actually test.

The full design is in [plan.md](plan.md). This file is the execution view.

## Status

| Milestone | Name | State |
| --- | --- | --- |
| M-1 | AI development system | complete |
| M0 | Contracts and quality gates | complete |
| M1 | Protocol and TLS (track A) | complete |
| M1R | Protocol revision: streaming and test breadth | complete |
| M1F | Full protocol coverage | complete |
| M2 | Auth and sidecar (track B) | complete |
| M3 | Cluster (track C) | in progress |
| M4 | Operations (track D) | ready |
| M5 | Pooling and routing (track E) | ready |
| M6 | Integration | blocked by M1, M5 |
| M7 | Scale and performance | blocked by M6 |
| M8 | FIPS and release | blocked by M7 |
| M9 | Query cache (post-MVP) | blocked by M8 |

M-1 and M0 are hard barriers. Tracks A through E run in parallel once M0 lands.

## M-1: AI development system (complete)

No Rust. Standards, product docs, ADRs, portable skills, and the enforcement
layer, all validated before any code depends on them.

```bash
scripts/m-1-complete.sh
```

Checks: every standards file present and non-empty, `AGENTS.md` and its
`CLAUDE.md` import in place at root and in each planned crate directory, every
skill validated by `skill-forge`, `pre-commit` hooks installed and firing,
`scripts/check-drift.sh` clean, and the second-tool portability check recorded
in `product/decisions/`.

## M0: contracts and quality gates (complete)

`pgprox-core` complete with a tested fake for every trait, plus the entire
quality apparatus enforcing on a nearly empty codebase.

```bash
scripts/m0-complete.sh
```

Checks: workspace builds, `cargo fmt --all --check` clean, `cargo clippy
--all-targets --all-features -- -D warnings` clean, `cargo llvm-cov nextest
--fail-under-lines 95` passing per crate, `cargo deny check` clean, and every
public trait in `pgprox-core` having a fake with its own tests.

## M1: protocol and TLS (complete)

Frame codec both directions, startup and auth flows, extended query, COPY,
protocol negotiation, cancellation.

```bash
scripts/conformance.sh 17 18
```

Checks: the conformance suite passes against Postgres 17 and 18, driven by
psql, pgx, asyncpg, JDBC, and npgsql. The codec is tested from both sides: as a
client against real Postgres in Docker, and as a server via the harness in
`crates/pgprox-proto/examples/conformance_server.rs`. Drivers whose toolchain is
missing are reported as skipped, never silently dropped.

## M1R: protocol revision (complete)

Raised by review after M2: the codec cannot stream, its size cap rejects
legitimate large results, and the conformance suite is narrow. See
[backlog.md](backlog.md) for the findings in full.

```bash
scripts/m1r-complete.sh
```

Checks: a header-only decode and a relay state machine exist, the inspect cap is
separate from the passthrough cap, and the conformance suite covers each gap the
review named by name rather than by count.

## M1F: full protocol coverage (complete)

Measured against pgdog, pgbouncer and odyssey rather than guessed at.

```bash
scripts/m1f-complete.sh
```

Checks: the message surface, SCRAM against published vectors, the frozen sidecar
contract, and that protocol 3.2 and replication scope are recorded decisions
rather than omissions.

## M2: auth and sidecar (complete)

The `.proto` contract, tonic client over UDS, grant cache with singleflight,
negative caching, and a mock sidecar binary.

```bash
cargo nextest run -p pgprox-auth --features integration
```

Checks: unit and integration suites pass, the mock sidecar starts and serves,
and the coverage gate holds for the crate.

## M3: cluster

Gossip, leases, leader election, rendezvous hashing, reservations, shedding.

```bash
cargo nextest run -p pgprox-cluster --features sim -- --test-threads=1
```

Checks: the quota invariant (guaranteed plus leased never exceeds the cap) holds
across the full randomized schedule set including partitions, leader loss, and
simultaneous restarts.

## M4: operations

Config providers with hot reload, metric and span registry, admin handlers, the
`SHOW` parser.

```bash
cargo nextest run -p pgprox-config -p pgprox-observe -p pgprox-admin
```

Checks: suites pass and the generated OpenAPI document validates.

## M5: pooling and routing

Pool lifecycle, idle reap, pinning, prepared-statement mapping, statement
classifier, replica poller, LSN watermarks.

```bash
cargo nextest run -p pgprox-pool -p pgprox-route
```

Checks: suites pass, and the classifier property test finds no case where a
DML-bearing statement is classified read-only.

## M6: integration

`pgprox-session` and `bin/pgprox` composing the real implementations.

```bash
scripts/e2e.sh
```

Checks: docker-compose brings up 3 proxy nodes, a primary, 2 replicas, and the
mock sidecar; pgbench runs clean; the drain test reports zero failed
transactions; replica reads never land behind the session watermark.

## M7: scale and performance

Reference workload, semantic coverage report, allocation budgets, `iai`
benchmarks, buffer reclaim, the 100k-connection harness.

```bash
scripts/scale.sh
```

Checks: 100k connections against one node with userspace RSS under 500 MB,
added p99 latency under 1ms against a direct connection, and upstream
connection count at or under the configured cap. Allocation budget tests pass
for every declared hot path.

## M8: FIPS and release

FIPS build stage, driver cipher-suite matrix, Helm chart, probe and preStop
wiring, rolling upgrade rehearsal.

```bash
scripts/release-check.sh
```

Checks: the FIPS binary starts and asserts `fips()` true on both client and
server config, the cipher-suite compatibility matrix is recorded for every
supported driver, and the rolling upgrade rehearsal shows zero failed
transactions.

## M9: query cache (post-MVP)

`pgprox-cache` behind the trait stubbed in M0.

```bash
cargo nextest run -p pgprox-cache
```
