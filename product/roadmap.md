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
| M3 | Cluster (track C) | complete |
| M4 | Operations (track D) | complete |
| M5 | Pooling and routing (track E) | complete |
| M6 | Integration | complete |
| M7 | Scale and performance | complete; 100k connections held at 546 MB against a 500 MB target, and latency demonstrated at 1000 |
| M8 | FIPS and release | complete |
| M9 | Query cache (post-MVP) | complete; 7% of median latency and of CPU per statement, and the pool lock is untouched |

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

## M6: integration (complete)

`pgprox-session` and `bin/pgprox` composing the real implementations.

```bash
scripts/m6-complete.sh && scripts/e2e.sh
```

The first is the part that runs without Docker: the two seams left open on
purpose (`Connector` and `ReplicaProbe`), the live `Observatory`, and the
composition root. The second is the milestone's real judgement.

Checks: docker-compose brings up 3 proxy nodes, a primary, 2 replicas, and the
mock sidecar; pgbench runs clean; the drain test reports zero failed
transactions; replica reads never land behind the session watermark.

## M7: scale and performance

Reference workload, semantic coverage report, allocation budgets,
instruction-count benchmarks, buffer reclaim, the connection harness.

```bash
scripts/m7-complete.sh        # the apparatus, without Docker
scripts/scale.sh 1000         # a run, against the compose stack
scripts/scale.sh 1000 --local # a run, against one node on this machine
```

Checks: 100k connections against one node with userspace RSS under 500 MB,
added p99 latency under 1ms against a direct connection, and upstream
connection count at or under the configured cap. Allocation budget tests pass
for every declared hot path.

**Where it stands.** The apparatus is built. Every declared hot path has an
allocation budget and an instruction count, the reference workload is
committed and versioned, the semantic coverage report crosses execution counts
against what the tests reach, and `bin/pgload` generates load pgbench cannot:
a tenant mix, connection churn, think time, and both wire protocols.

The runs are at 1000 connections against the compose stack, recorded in
`product/perf/run-2026-07-27-1000-compose.md`. The fleet respects its upstream
cap, uses 40 of the 60 connections it is allowed, serves 12% of statements
from a replica, and refuses a handful of clients with `53300` when a thousand
of them offer more work than the database can take. The hop costs well under a
millisecond at p50; this stack cannot resolve it at p99.

Nine defects came out of those runs rather than out of review, including one
that meant no node in a fleet had ever granted a quota lease.

**The 100k condition, and which part of it is met.** One node held 100,000
connections at 546 MB of userspace, 5,726 bytes each, flat for six minutes with
99,940 registered. That is 9% over the 500 MB target and it is the memory half
of the condition, recorded in `product/perf/run-2026-07-28-100k-hold.md`.

The other half is not demonstrated at that scale. Those connections were idle:
the workload has them think for ten to fifteen minutes before their first
statement, so added p99 and the upstream cap are still shown at a thousand
connections rather than a hundred thousand. Behind 100k connections here sits
one Postgres with a 60-connection cap sharing twenty cores with five load
generators, so serving that many is a fact about the machine before it is a
fact about the proxy.

**What a complete 100k run still needs**: the load generators on their own
machines, a database that can absorb the offered load, and a real network
between the three, since every latency number recorded so far is loopback and
is therefore a floor.

**And the constraint that turns out not to be memory.** Measured after M8, in
`product/perf/run-2026-07-29-connection-cost.md`: the proxy spends about 2ms of
CPU per connection per second once a fleet is *active*, near enough regardless
of what each connection asks for. Two runs at the same statement rate and four
times apart in connection count gave 2.02ms and 2.23ms. One core holds about
five hundred such connections.

That does not contradict the 100k hold run, and the pair is the useful part:
100,000 *idle* connections cost 546 MB and almost no CPU, while 2,000 active
ones cost five cores. So the roadmap's 100k target is reachable for connections
that are mostly idle, which is the design point, and the number that decides
how many of them can be busy at once is this one rather than memory. What the
2ms actually is has not been named yet; `M7.56` is that.

## M8: FIPS and release (complete)

FIPS build stage, driver cipher-suite matrix, Helm chart, probe and preStop
wiring, rolling upgrade rehearsal.

```bash
scripts/release-check.sh      # the gate, seconds, no Docker
scripts/fips-check.sh         # the FIPS variant, built and run
scripts/cipher-matrix.sh      # five drivers against both builds
scripts/rolling-upgrade.sh    # the rehearsal, in a kind cluster
```

Checks: the FIPS binary starts and asserts `fips()` true on both client and
server config, the cipher-suite compatibility matrix is recorded for every
supported driver, and the rolling upgrade rehearsal loses no transactions.

**Where it stands.** All four run clean. The FIPS image builds and logs
`crypto=aws-lc-rs-fips`; all five drivers connect to both builds; the chart
renders manifests a live API server accepts, with the readiness probe, the
liveness probe and the `preStop` hook the drain needs; and a rolling restart of
a three-node fleet under load lost none of 21,042 transactions while a node
killed outright lost 22 of 21,088.

**Two things the numbers do not say.** Every driver on the machine that
generated the cipher matrix negotiated TLS 1.3, whose suites are all
FIPS-approved, so the restriction FIPS mode actually imposes on TLS 1.2 was
never reached. And the rehearsal is three nodes on one machine: it does not say
what happens when a fleet at its connection cap loses a third of itself, which
is where shedding has to work.

**What this milestone found by running rather than by reading.** `Flush`
deadlocked the relay, so asyncpg could not run a single extended query through
the proxy. "Zero failed transactions" was a target a working drain could never
hit, because the load client counted a relocation as a loss. The FIPS and
default Docker stages shared one cargo cache and the default image shipped the
FIPS binary. And the chart asked for a sysctl the kubelet refuses, so every pod
failed to start.

## M9: query cache (post-MVP, complete)

`pgprox-cache` behind the trait `pgprox-core` has carried since M0.

```bash
scripts/m9-complete.sh
```

That replaces `cargo nextest run -p pgprox-cache`, which says the crate's own
tests pass and nothing about whether the cache is correct to use. The three
things that decide that are elsewhere: the ADR stating what it promises, the
rule deciding what may be cached at all, and a recorded run showing whether it
helped.

Checks: an ADR states the staleness contract, `pgprox-cache` implements
`QueryCache` and is bounded by bytes rather than by entry count, a cacheability
rule exists, the config document can turn it off, and a run is recorded.

**Why this is worth doing, which is not why it was filed.** The plan filed it
as post-MVP throughput work. `M7.56` then measured where the proxy's CPU goes
and found 45% of it in the upstream pool's lock, with the cost landing per
connection because contention tracks how many are queued. A cache hit is a
statement that never acquires a connection, so it neither queues nor contends.
That makes this the cheapest thing to try against the constraint `M7.57` is
about.

**What it may promise.** Bounded staleness, and nothing stronger. A replica's
lag is measurable and ADR 0009 gates on it; a cache entry carries no version of
the data it copied. This proxy sees its own traffic, can invalidate on writes
that pass through it, needs gossip for writes through another node, and cannot
see a migration or an operator with psql at all. So the TTL is the guarantee
and everything else is an improvement on it.

**What it turned out to be worth.** Seven percent of median latency and seven
percent of CPU per statement, over five matched pairs whose two sets do not
overlap, serving 11% of statements at a 39% hit rate. Recorded in
`product/perf/run-2026-07-29-cache.md`.

It is not the answer to `M7.56`. A profile with the cache on puts the pool and
its wakeups at ~49% against ~50% with it off: the share is flat because it is a
share of a smaller total, and the shape does not change. Contention tracks how
many callers are queued, and 89% of statements still queue. `M7.57` is still
the task that matters for 100k *active* connections, and this run is more
evidence for it rather than less.

**Where the ceiling is.** Both halves of it are in the workload rather than in
the cache. Half of the reference workload goes through the extended protocol,
which is all miss until `M9.12` teaches the codec to read a `Bind`'s parameter
values. Thirty percent of its statements are writes, and each one drops the
tenant's entries, so the cache is emptied roughly every other lookup. A 39% hit
rate under that is better than it sounds, and a more read-heavy tenant is the
one this feature is for.

**What building it found.** `Flush` was not the only thing the relay got wrong
about statements it answered itself: a cache hit returned before
`record_statement`, so `pgprox_route_total` missed every one and the first
measurement read as 8.7% *fewer* statements and worse CPU. A denominator
missing its best cases is worse than no denominator, because it reads as a
result. And the first cache-on run measured nothing at all, because the mock
sidecar names its tenant after the token's first eight bytes and the config
document had opted in a tenant that never arrives.
