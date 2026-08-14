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
| M7 | Scale and performance | complete; 100k connections held at 546 MB against a 500 MB target, and `M7.58` later took CPU per statement from 687us to 43.7us |
| M8 | FIPS and release | complete |
| M9 | Query cache (post-MVP) | complete; it costs 7.8% of the median on the reference workload, which is the opposite of what `M9.10` measured and is a fact about `M7.58` |
| M10 | The claims nothing enforces | complete; the three claims now fail when they stop holding, and the cache turns out to change sign with load rather than with workload |
| M11 | The gaps the completed milestones name | complete; two of its four measurable questions corrected the claim that raised them, and pinning costs 0.650 upstream connections per pinned session with no threshold |
| M12 | The gates that count files | complete; five gate checks now read what a file says instead of matching its name, and every gate is proven able to fail |
| M13 | The non-negotiables that nothing enforces | complete; six of the seven rules have a script and the seventh is marked as having none, which is the honest half of the answer |
| M14 | The crates mutation testing never reached | complete; 2,926 mutants across all fourteen crates, 155 survivors, 137 killed and 18 argued |
| M15 | The protocol crate under a second reading | complete; thirteen findings, of which three are memory bounds a peer controlled, two are connection state that never came back, and one is a mutant in a test this milestone wrote after writing about that exact mistake |
| M16 | The streaming relay nothing streams through | the two directions are done and 16,777,216 bytes held becomes 512; the 100k run with a large result set needs three machines and is blocked |
| M17 | The binaries mutation testing never reached | complete; 571 mutants in `pgprox` and 124 in `pgload`, every survivor argued, and the two timeout constants re-derived from a measured suite |
| M18 | What the deployment story assumes | complete; an ADR that described a transport nobody built, a seam specified rather than guessed, and a rule that a milestone cannot close with nothing to run |
| M19 | A seam for peer discovery | complete; the seam exists and three consumers read through it, and two of the eight tasks were corrections of claims this milestone made about its own fakes |
| M20 | The protocol layer against pgbouncer, pgcat and odyssey | complete; eight findings, of which one corrupted a pooled connection for every session after it, three were things a client asked for and was silently not given, and one was found by the hunt rather than by the reading |
| M21 | The driver matrix does not cover what M20 changed | complete; the suite it proposed building already existed, and three of its four cases were wrong in ways only a reverted build could show |
| M22 | The mutants nobody has swept since M17 | complete; 3,835 mutants across all sixteen crates, nine new survivors, and no two of them had the same cause |
| M23 | The streaming question M16 left open, at the scale one machine has | complete; no measurable per-connection cost to a megabyte row at 600 connections, and the second pair corrected what the first appeared to show |
| M24 | A reading of every crate, and the nine things it found | complete; nine findings, four of them one cause, and two fixed only in part with the measurement that decided how far |
| M25 | The query cache against pgpool-II | complete; three findings, all one constant, and the two places pgpool is ahead stay open as limits rather than becoming tasks |
| M26 | What the query cache costs, measured for the first time | complete; a hit is 65% cheaper and allocates nothing, a write is 97% cheaper, and the lock the store worried about was never the problem |
| M27 | Unsafe becomes a governed exception rather than a closed door | complete; five conditions, a script that enforces them, nine cases proving each can fail, and no unsafe written |
| M28 | The build configuration nobody had measured | complete; one lever of the four was actually available, it is worth 7 to 15% on the route decision, and a benchmark that reported a regression from it turned out to be measuring a random seed |
| M29 | The first exception the unsafe policy was asked for | complete; it refused one, on the evidence it asks for, because LLVM had already elided the bounds checks |
| M30 | The same procedure, applied to every crate | complete; four of five findings were work that did not need doing at all rather than a place for unsafe, and the fifth was refused by the closed-crate policy |
| M31 | The comments at M30's optimisation sites | complete; three ways a comment fails the bar, a `debug_assert!` added at the two sites that had one available, no figure moved |
| M32 | The comparison against pgbouncer and pgcat | complete; pgbouncer holds 200 connections in 4.5 MB against pgprox's 13.9, pgprox multiplexes onto 50 upstream connections where both others use 60, and throughput is the same across all three |
| M33 | What pgbouncer and pgcat do differently | complete; three things pgprox found by profiling pgbouncer has had since 2007, and setting both buffers to 4 KiB moved the per-connection cost by 205 bytes out of 22,835 |
| M34 | The seventeen kilobytes that are not the buffers | complete; the allocator arenas are not it, the worker count is, and roughly 12.7 KB stayed unexplained |
| M35 | Per-connection memory is a curve, not a number | complete; measured at 100, 200 and 400 connections the reported per-connection figure fell as connections rose in every arm, and `M33`'s and `M34`'s per-connection totals are withdrawn while their arm-to-arm comparisons stand |
| M36 | What an open, quiet connection costs | complete; pgprox needs 20.8 MB for 800 idle connections against pgbouncer's 4.1, and `M38` later corrected the extrapolation this milestone drew from it |
| M37 | What a spawned task costs beyond its future | complete; `tokio::spawn`'s overhead is a constant 128 bytes across a future that grows sixteenfold, and every memory candidate any milestone had named is now eliminated with roughly 10 KB per idle connection still unaccounted for |
| M38 | The extrapolation M36 did not need to make | complete; `M36`'s slope-based 1.47 GB at 100k connections is corrected against the measured 546 MB, and the wrong figure stays visible marked superseded rather than deleted |
| M39 | Documentation for people who are not this repo | complete; six pages under `docs/` plus a README, each checked against the code it describes rather than proofread |
| M40 | A control that only worked where nothing else was broken | complete; three negative test cases passed with the check they exist to test deleted entirely, on a machine with no Postgres container running |
| M41 | The docs become a site | complete; Astro Starlight reads `docs/` directly so the same files serve GitHub and the built site, and a build-time rewrite keeps GitHub-relative links working as site routes |
| M42 | The site's toolchain leaves the repository root | complete; `package.json`, the lockfile and `src/` moved to `docsite/`, leaving the root a Rust project |
| M43 | What it does, and what one request touches | complete; two pages, features/limits and request flow, with pin reasons read out of the enum so a variant added later cannot go undocumented |
| M44 | The pages a review asks for | complete; six pages plus a gate reading eight lists straight from the code, including `SHOW MEM`, which had a test rejecting it by name through four milestones of documentation work |
| M45 | One directory for the pages and the thing that builds them | complete; `docsite/` folded back beside `docs/`, trading four files of noise in the listing for one directory instead of two with a `../` between them |
| M46 | The licence three files have claimed and none granted | complete; `LICENSE` now holds the Apache-2.0 text verbatim and `check-drift.sh` holds every other file that names a licence to it |
| M47 | The links nothing was checking | complete; fifteen broken links, all in the roadmap, all the same one-`../`-too-many mistake, now caught on every commit rather than by clicking |
| M48 | The design record moves under docs/ | complete; `product/`, `standards/` and `specs/` moved under `docs/internal/`, visible rather than hidden so `rg` and `fd` still find them by default |
| M49 | One place for what a run leaves behind | complete; `reference/` became `.tmp/reference/`, and the eight tool-specific `.gitignore` patterns that could not fold into it stay, with the reason written down |
| M50 | A README in every crate | complete; sixteen READMEs checked against `Cargo.toml` in both directions, which caught the crate map still crediting `pgprox-cluster` with SWIM gossip twenty-five milestones after the ADR was corrected |
| M51 | Eighty scripts and no index | complete; the forty-five gates moved to `scripts/gates/`, `scripts/README.md` indexes the rest, and a real concurrency bug surfaced by a one-in-a-few-runs flake was fixed along the way |
| M52 | Two failures from the CI replay, and what each turned out to be | complete; one was a Docker Desktop dynamic-port-publish quirk under WSL2 with a definite fix, the other an intermittent coverage failure that could not be reproduced and is now diagnosable rather than fixed |
| M53 | The scripts read as stale, and two of them were | complete; forty-two of forty-four scripts were not stale, `cargo fmt` ran twice on every push, and `check-wired.sh`'s summary oversold what its own body already argued against |
| M85 | Eighty-seven milestones and no way to jump to one | complete; a table of contents for `backlog.md` and a `check-drift.sh` rule that fails if a heading has no matching line |
| M86 | The status table nobody kept adding rows to | complete; rows added for `M30` through `M53` and `M85`, and backfilling them found two milestones whose completion condition was prose with no command for `check-drift.sh` to read |
| M87 | The mutants nobody has swept since M22 | complete; all sixteen crates and binaries freshly swept, real testing gaps found and fixed across `pgprox-tls`, `pgprox-auth`, `pgprox` and `pgprox-pool` (the last reachable only through a new test-only accessor), two memory-exhaustion mutants fixed with `debug_assert!` invariants after one took the machine from 30 GB free to swapping in under ten seconds, and every remaining survivor accepted in the baseline with a written reason |
| M88 | A second reading of every crate, and the eighteen things it found | open |

`M54` through `M84` are complete — `backlog.md` has their tasks and commit
references — but do not yet have roadmap sections of their own; this table
stops naming them here until that catch-up is done. M-1 and M0 are hard
barriers. Tracks A through E run in parallel once M0 lands.

## M-1: AI development system (complete)

No Rust. Standards, product docs, ADRs, portable skills, and the enforcement
layer, all validated before any code depends on them.

```bash
scripts/gates/m-1-complete.sh
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
scripts/gates/m0-complete.sh
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
scripts/gates/m1r-complete.sh
```

Checks: a header-only decode and a relay state machine exist, the inspect cap is
separate from the passthrough cap, and the conformance suite covers each gap the
review named by name rather than by count.

## M1F: full protocol coverage (complete)

Measured against pgdog, pgbouncer and odyssey rather than guessed at.

```bash
scripts/gates/m1f-complete.sh
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
scripts/gates/m6-complete.sh && scripts/e2e.sh
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
scripts/gates/m7-complete.sh        # the apparatus, without Docker
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
how many of them can be busy at once is this one rather than memory.

**What the 2ms was.** `M7.56` put 45% of the proxy's CPU in the upstream pool's
lock. `M7.58` found why, and it was not saturation: `LivePool::release` woke
*every* waiter, so one connection handed back woke roughly four hundred and
forty tasks, each of which took the pool mutex three times on its way back to
sleep. Waking one waiter per released connection took CPU per statement from
687us to 43.7us, a 15.7x reduction, and halved the p99. `lock_contended` and
`LivePool::acquire` left the top of the profile entirely, and the sample count
under the same load fell from 4,119 in twenty seconds to 161. See
`product/perf/run-2026-07-29-thundering-herd.md`.

On the same workload that puts the per-connection cost at roughly 0.13ms per
second rather than 2ms, so a core holds something closer to seven thousand
active connections than five hundred. The figures above are left as they were
measured, because a run is a record of what was true when it was taken, and the
correction belongs here rather than in a rewritten history.

## M8: FIPS and release (complete)

FIPS build stage, driver cipher-suite matrix, Helm chart, probe and preStop
wiring, rolling upgrade rehearsal.

```bash
scripts/gates/release-check.sh      # the gate, seconds, no Docker
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
never reached. *`M11.2` has since reached it: a client pinned to TLS 1.2 and
ChaCha20-Poly1305 is taken by the default build and refused by the FIPS build,
with an AES-GCM probe beside it as the control.* And the rehearsal is three nodes on one machine: it does not say
what happens when a fleet at its connection cap loses a third of itself.
*`M11.3` corrected the second half of that sentence, which used to end "which is
where shedding has to work". It is not: `shed::decide` returns
`Keep(NoHeadroomAtHome)` when the home node is full, and has been tested that
way since M3. The cap is where shedding is designed not to fire, because moving
a client to a node that is also full is churn. What is still untested there is
admission, which `M11.6` asks properly.*

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
scripts/gates/m9-complete.sh
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
many callers are queued, and 89% of statements still queue.

**And that seven percent has since gone the other way, which is the useful part
of this milestone.** `M7.58` took CPU per statement from 687us to 43.7us by
waking one waiter per released connection instead of all of them. The cache's own
cost per statement did not change; what it was buying shrank by a factor of
fifteen underneath it. Re-measured after the extended protocol landed, the same
comparison puts the cache 7.8% *worse* on the median at five hundred
connections, over three matched pairs whose sets do not overlap. See
`product/perf/run-2026-07-30-extended-cache.md`.

Two thirds of that cost is the hits rather than the bookkeeping, which a third
configuration separated: opted in with a 64-byte budget, so every lookup and
every withheld sequence happens and nothing can be stored, the median is 1% worse
rather than 8%. Throughput is identical in all three, pinned by the database. So
the cache cannot make the fleet do more work, and what serving 3% of statements
instantly does is return those clients to the queue sooner, which lengthens it
for the other 97%. A cache in front of a saturated resource moves work from the
front of the queue to the back of it.

**One sentence of this milestone is now known to be wrong, and `M11.1` is where.**
`M9.24` concluded that throughput is pinned by the database, so the cache cannot
make the fleet do more work and can only change who waits. Eight matched pairs
at saturation say the fleet does 4.11% more work with the cache on, eight out of
eight, 95% CI +1.14% to +7.08%. The queueing mechanism survives and explains
both numbers: the served statements are nearly free rather than merely
reordered, so they add completions while the statements that still reach the
database wait longer. Total throughput up, median down, and the median statement
is in the slower two thirds. See `product/perf/run-2026-07-31-throughput.md`.
The runs below are left as they were written.

**Where the ceiling is.** Both halves of it are in the workload rather than in
the cache. Half of the reference workload goes through the extended protocol,
which is all miss until `M9.12` teaches the codec to read a `Bind`'s parameter
values. Thirty percent of its statements are writes, and each one drops the
tenant's entries, so the cache is emptied roughly every other lookup. A 39% hit
rate under that is better than it sounds, and a more read-heavy tenant is the
one this feature is for.

**What the extended half turned out to need.** `M9.12` read a `Bind`'s
parameters and `M9.17` was to carry them into the key, which is a paragraph of
work until you ask where the hit happens. By the time an `Execute` is decoded
the session is already holding a pooled connection, because the `Parse` or the
`Bind` before it was forwarded and forwarding acquires. So the obvious shape
would move the query off the database and leave every bit of the pool work
`M7.56` measured exactly where it is. And the answer to a sequence is not a
function of the SQL and the parameters: whether a `RowDescription` belongs in it
depends on the client's framing, so one driver's recorded bytes desynchronise
another driver. ADR 0022 is the decision, `M9.18` through `M9.27` are the work,
and `scripts/gates/m9-complete.sh` passes either way because none of this changes what
the cache promises.

**What building that found, all of it in the fakes.** Three defects, and each was
invisible because a fake upstream was kinder than Postgres. It answered a second
`Parse` under a name it had already prepared, so a connection whose record of its
own statements had diverged looked correct: `M9.25`. It answered the write
position query with a bare completion, so `relay.wrote()` never cleared and
nothing after a write in that world was cacheable, which kept a test away from the
path it was written for: `M9.26`. And it answered a `SELECT` with no
`RowDescription`, a shape no server produces, which hid the fact that the two
protocols do not store the same payload and that a simple query was being served
rows with nothing describing them: `M9.27`.

`M9.25` also went in half-applied and green, taking a run from 1,083 errors to
207, which read as progress and was the same bug wearing a different symptom.
Two of these three arrived from `M9.24` running rather than from review, which is
the milestone's own argument for its completion condition being a run.

**What building it found.** `Flush` was not the only thing the relay got wrong
about statements it answered itself: a cache hit returned before
`record_statement`, so `pgprox_route_total` missed every one and the first
measurement read as 8.7% *fewer* statements and worse CPU. A denominator
missing its best cases is worse than no denominator, because it reads as a
result. And the first cache-on run measured nothing at all, because the mock
sidecar names its tenant after the token's first eight bytes and the config
document had opted in a tenant that never arrives.


## M10: the claims nothing enforces

Every milestone through M9 is complete, and this one exists because three of the
things this repo says about itself are not true today. None is a feature. Each is
a claim a reader would take at face value, with nothing that fails when it stops
holding.

```bash
scripts/gates/m10-complete.sh
```

Checks: every milestone gate that does not need Docker runs in CI, the fuzz
target runs on a schedule rather than only by hand, mutation testing exists as a
script with a recorded baseline, and `standards/testing.md` describes what
actually runs.

**The three claims.**

*Eight gates that fired once.* Eleven `scripts/m*-complete.sh` exist and CI runs
three. Each passed on the commit that closed its milestone and nothing has
checked it since, so a regression in M0's contract rules or M5's classifier
property would surface whenever somebody next ran the script by hand. All eight
are Docker-free and take seconds.

*A codec that is "fuzzed, not assumed".* `pgprox-proto`'s own `AGENTS.md` says
so, `scripts/fuzz.sh` exists, and nothing runs it. The most exposed parser in the
process is fuzzed exactly as often as somebody remembers to.

*Mutation testing that "runs nightly".* `standards/testing.md` says
`cargo-mutants` runs against the pure state machines and that surviving mutants
are treated as missing tests. There is no script, no job, and the tool is not
installed. M9 is the argument for doing it rather than deleting the claim: three
of its defects were invisible because a fake was kinder than Postgres, and one
fix went in half-applied and green. All four are mutation-shaped.

**And one measurement, because M9.24 named it as the cheapest thing left.** That
run says the cache costs 8% of the median on a workload with 30% writes and two
thirds of its statements inside a transaction, and that it says nothing about the
workload the feature is for. A read-heavy workload document and a matched pair
against it is a day's answer to a question the milestone left open.

**Where it stands.** All three claims are now enforced by something that fails.
Eleven milestone gates run in CI, the fuzz target and the mutation run are
nightly jobs with artifacts, and `standards/testing.md` describes what runs.

**What mutation testing was worth, which is the part that could not be predicted
from the claim.** 89 survivors against line coverage of 96% to 99%, and working
through them found real gaps rather than bookkeeping: a `StaticCredentials`
blanket impl that production takes and no test did, so forwarding that returned
`None` would have refused every login on a real node while the suite stayed
green; `is_done` and `is_closed` asserted in one direction only, which is the
shape of a session admitted without authenticating; three `Debug` impls whose
redaction tests asserted only what must not appear, so an impl that printed
nothing passed; a `Bind` and a `Close` that were never counted because an
`Execute`'s completion settled the sequence either way; and a bounds check in
`sequence::split` whose test had to be written twice, because a payload
truncated after the completion is refused for another reason entirely.

**And one finding about the tool rather than the code.** `cargo mutants` gives
the whole suite one budget, so under `cargo test` a mutant that hangs one test
costs the run its verdict and is reported as surviving whether or not another
test failed it. `M10.13` found that by writing assertions that fail six mutants
and watching all six come back as timeouts. Running the suite under nextest with
a per-test cap turned twenty-three baseline entries into kills without a line of
test code. What is left is nine equivalent mutants, each with an argument beside
it. **A timeout was a run nobody read, not a mutant the suite caught**, and
every reason written beside those entries had said otherwise.

**What the measurements said.** The cache is 24.4% better on a read-heavy
workload at five hundred connections and 17.5% worse on the same document at two
thousand, where that workload saturates. So whether it helps is a property of
the workload *and the load*, not of the workload alone, and `M9.24`'s queueing
explanation survives the test built to break it. Serving 36% of statements costs
17.5% of the median where serving 3% cost 7.8%: the regression grows with the
hit rate, which is the mechanism's own signature. See
`product/perf/run-2026-07-30-cached-workload.md` and
`run-2026-07-31-saturation.md`.

## M11: the gaps the completed milestones name (complete)

Ten milestones are complete and each wrote down what its own numbers do not say.
This one works that list. Nothing in it is a feature: every task is a claim some
milestone made and then qualified in its own words.

```bash
scripts/gates/m11-complete.sh
```

Four are measurable here: the throughput question `M10.9` declined to claim, the
TLS 1.2 restriction `M8`'s cipher matrix never reached, a node lost from a fleet
already at its connection cap, and the curve behind ADR 0001's open question
about pinning.

Three are not, and are recorded as blocked rather than filed. A complete 100k
run needs three machines and a real network; ADR 0012's interactive half needs a
second agent tool and a human's judgement; and the plan's three M0 open items
each need an owner outside this repo.

The gate for this milestone was written before the milestone needed closing,
which `M10.17` is the reason to mention: it is part of the work rather than
something to discover at the end of it.

What the four found, none of which was the expected answer:

- `M11.1` The cache raises fleet throughput 4.11%, 8 of 8 matched pairs
  positive, 95% CI [+1.14%, +7.08%]. `M10.9` declined to claim this and the
  reason it declined, that a cache only reorders who waits, is wrong: served
  statements are nearly free rather than merely reordered, so they add
  completions while the 64% that still reach the database wait longer.
- `M11.2` FIPS mode's TLS 1.2 restriction is real and now has a control. A
  ChaCha20-Poly1305 suite the default build accepts is refused by the FIPS
  build, and an AES-GCM suite is taken by both.
- `M11.3` The premise was wrong, found by reading `shed::decide` rather than by
  running anything. Shedding cannot fire at the connection cap: it refuses with
  `NoHeadroomAtHome`. The cap is where shedding is designed *not* to work, and
  the roadmap sentence claiming otherwise is gone. `M11.6` then measured what
  actually happens to displaced clients, which is nothing: they are served, in
  about a seventh of a second, at every point across the kill, and neither
  `53300` nor `57014` reaches a client at any moment of the run.
  (`M11.11` first summarised that as "which is `53300`", the opposite of the
  run's own headline. Found by `M12.4` while writing the check that reads the
  run instead of its filename, which is the milestone working as intended one
  task after the summary was written.)
- `M11.7` Pinning costs `0.650` upstream connections per pinned session,
  linearly, with no knee and no safe fraction. Zero free parameters, R^2 =
  0.994. With every session pinned the fleet held one connection per client,
  which is ADR 0001's "collapses back to session pooling" as an identity rather
  than an analogy. Throughput is unaffected while the pool has headroom.

Two of the four corrected a claim rather than confirming one, and in both cases
reading the code beat running the experiment.

## M12: the gates that count files (complete)

Eleven milestones are gated by a script, and the scripts are why this repo
trusts its own history. `M11` then spent its whole length finding that recorded
claims do not always say what the milestone thinks they say. This one turns the
same question on the gates themselves.

```bash
scripts/gates/m12-complete.sh
```

The defect has one shape. A check asks whether a file exists whose name matches
a pattern, and reports the claim that the file was supposed to establish. It
passes on an empty file, on a file about something else, and on a file that
says the opposite.

Found before the milestone was written, which is why it is a milestone rather
than a suspicion:

- `m11-complete.sh` globbed `product/perf/*pinning*.md` and reported "the
  pinning curve is recorded". It passed for a day on a document titled *why this
  run does not answer it*. Fixed in `M11.7`, and the fix is the template: read
  the recorded counts, require the control arm below the cap, require the axis
  to have moved.
- `m7-complete.sh` reports "a scale run is recorded (16 file(s))" from
  `product/perf/run-*.md`. Three of those sixteen declare themselves scale runs.
  The rest are cache, admission, throughput, saturation and pinning documents
  that the glob cannot tell apart, and the check would pass with none of the
  three present. (`M12.0` first wrote five here, by eye. The replacement check
  counts three, and where a number and the check that measures it disagree, the
  number is the one that is wrong.)
- `check-commit-msg.sh` says a subject "references the backlog task so history
  stays traceable to the plan", and checks only that the ID is well formed.
  `M11.11` was committed with no such task in the backlog and the hook passed.
  The task was filed afterwards, which is the wrong order and is recorded in
  the entry rather than tidied away.
- `m1f-complete.sh` and `m9-complete.sh` carry the same glob-for-a-filename
  shape against ADRs and cache runs.

And one near miss worth keeping: the replacement check in `M11.7` first piped
`awk` into a block that called `fail`. It printed `FAIL` and exited 0, because
the right-hand side of a pipeline is a subshell and the failure counter never
reached the parent. A gate that cannot fail is worse than no gate. No other
site in `scripts/` has that shape today, and nothing stops one appearing.

So the milestone has two halves. Replace the file-counting checks with checks
that read what the file says. Then prove the gates can fail at all, by feeding
each one a broken artefact and asserting a non-zero exit, because the only way
that near miss became visible was checking the exit code instead of the output.

**What it found.** Five checks rewritten, and four of the rewrites caught
something on their first reading rather than merely tightening a rule:

- `M12.1`'s hook found that `M11.11` was the third commit to reference a task
  that did not exist, not the first. `M1F.0` and `M-1.18` were the others, found
  by running the tightened hook over all 321 commits. `M12.10` filed both.
- `M12.2` corrected the plan that filed it. `M12.0` said five of sixteen run
  documents were scale runs, counted by eye; three declare themselves so.
- `M12.4` found a wrong claim in prose written one task earlier. `M11.11`
  summarised `M11.6` as measuring that displaced clients get `53300`, and the
  run's own headline is that they get served and neither refusal code reaches
  them.
- `M12.8`'s own `continue-on-error` check failed on the comment explaining why
  that flag had been removed, matching the word rather than the construct. The
  milestone's defect appeared inside the gate written to detect it.

`tests/gates/negative.sh` is the deliverable: 38 cases, each breaking an
artefact and asserting a non-zero exit, including a floor that runs all fourteen
gates against a tree holding none of their artefacts. It asserts exit codes and
never output, because the bug that motivated it printed the right message in red
and returned 0.

## M13: the non-negotiables that nothing enforces (complete)

`AGENTS.md` lists seven non-negotiables and says of them: "Each is enforced by a
script, not by good intentions." `M12` spent its length finding that gates can
report conclusions nothing checks. This asks the same question of the sentence
that introduces the rules those gates exist to serve.

```bash
scripts/gates/m13-complete.sh
```

Audited before the milestone was written, one rule at a time. Three hold. Four
do not, and one of the four was introduced by `M12` itself.

| # | rule | enforced by | holds |
| --- | --- | --- | --- |
| 1 | one task, one commit, green tree | `check-commit-msg.sh`, pre-commit | yes, since `M12.1` |
| 2 | never lower a threshold or delete a test | nothing | **no** |
| 3 | never claim a test passes without running it | nothing directly | partly |
| 4 | 95% line coverage per crate, tier 1 alone | `check-coverage.sh` | yes, but see 2 |
| 5 | business logic is sans-I/O | `check-layering.sh` | **no, it checks something else** |
| 6 | a core trait change updates trait, fakes, impls and ADR in one commit | `m0-complete.sh`, partly | **no** |
| 7 | credentials never reach a log | one unit test | **no** |

**Rule 2 is the sharpest.** Three values decide pass or fail and all three can be
moved from the environment: `COVERAGE_MIN` at 95, `BENCH_TOLERANCE` at 5, and
`PGPROX_SCALE_MINIMUM` at 1000. Run
`COVERAGE_MIN=10 scripts/check-coverage.sh pgprox-route` and it prints
`ok coverage (pgprox-route): 99.65% >= 10%` and exits 0. The gate announces its
own weakened threshold and passes anyway. Nothing detects a deleted test at all.

The third of those was added by `M12.2`, in a milestone about checks that do not
check. That is worth stating rather than quietly fixing: the defect class is easy
to reproduce while actively looking for it.

**Rule 5 names a property and the script checks a different one.** `check-layering.sh`
enforces the crate dependency rule, that every crate depends on `pgprox-core` and
nothing else in the workspace. That is a real rule and it is not the sans-I/O
rule. Nothing checks that business logic has no sockets in it.

**Rule 7 is a repo-wide claim held up by one test**, `a_token_cannot_reach_a_log`
in `crates/pgprox-session/src/auth.rs`, across fifteen crates.

The milestone fixes what can be fixed and rewords what cannot. A rule that
genuinely cannot be scripted should say so in `AGENTS.md` rather than sit under a
sentence promising it is enforced, because a false claim about enforcement is
worse than an honest claim about intent.

**What it came to.** Six of the seven have a script and it is named beside the
rule. Rule 3, never claim a test passes without having run it, has none and
cannot: nothing checks a claim against an intention. It stays in the list,
marked, because every other rule rests on it.

Four scripts were written: `check-tests-kept.sh` names any test that disappears
and requires a `Removes-test:` line, `check-secrets.sh` refuses an exposed
credential reaching a formatter, `check-sans-io.sh` refuses a socket or a real
clock in a library crate, and `check-core-contract.sh` requires a core trait
change to bring its implementors and an ADR. Three thresholds became constants.

Two of the audits found the rule already held everywhere and nothing watching
it. Every `now()` call in the library crates is in test code but six, four of
which are `pgprox-core::clock` itself. That is the pattern `M12` kept producing,
one layer up.

**And the milestone reproduced its own defect, in the commit that fixed the
security rule.** `M13.3`'s first lint used `\b`, a GNU extension the awk on this
machine does not have, so it matched no line and reported
`ok no exposed credential reaches a formatter (133 file(s))`. It was found by
planting a leak and watching nothing happen. Everything after it plants a
violation and requires the rule to object, and `M13.8`'s end-to-end check carries
a positive control for the same reason.

## M14: the crates mutation testing never reached (complete)

```bash
scripts/gates/m14-complete.sh
```

`scripts/mutants.sh` says in its own header that it runs "against the crates
whose logic is a pure state machine", and lists four: `pgprox-proto`,
`pgprox-route`, `pgprox-cache`, `pgprox-session`. `M13.4` then proved that
*every* crate under `crates/` is sans-I/O, because that is now enforced. The
criterion and the list disagree, and they have since `M10.3` wrote the script.

Measured before planning:

| | lines | tests | mutation tested |
| --- | --- | --- | --- |
| covered | 37,536 | 576 | 4 crates |
| **not covered** | **49,725** | **857** | **10 crates** |

More than half the codebase, by line and by test, has never had a mutant run at
it. The three that matter most:

- **`pgprox-cluster`**, 11,078 lines, 280 mutants. It holds the quota invariant,
  guaranteed plus leased never exceeds the cap, which is M3's entire completion
  condition and the roadmap's headline safety claim.
- **`pgprox-pool`**, 8,544 lines, 273 mutants. The pool state machine, whose
  `53300` and pinning behaviour `M11` spent four tasks measuring.
- **`pgprox-core`**, 13,362 lines, 536 mutants. Every contract and every fake.
  A fake that answers something the real thing would refuse is exactly how `M9`
  hid three defects, which is the argument `mutants.sh` opens with.

The milestone is those runs and their triage, in that order, plus making the
script's list match its stated criterion so the gap cannot silently reopen.

A surviving mutant is a missing test. It is killed by writing one, or it goes in
`product/mutants-baseline.txt` with an argument for why no test can tell the
difference. `M10` established that an entry there is an argument, never an
assertion, and that "detected by hanging" was a claim about the runner rather
than about the code.

**What it came to.** 2,926 mutants across all fourteen crates. 155 survived,
137 were killed by tests, 18 are accepted with a written argument. The list and
its header now say the same thing.

The survivors clustered somewhere more mundane than expected. Almost none were
in complicated logic; they were in accessors, counters, constants and boundaries
that only one state was ever asked about. Four shapes recurred:

- **An assertion compared against the constant that produced it.** Three
  separate cases, including one where a value duplicated across two crates was
  documented as "held together by a test" and held by neither side.
- **An upper bound a small constant satisfies.** `max_series() <= CEILING` is
  the cardinality budget for the entire metric surface, and `0` passes it.
- **A counter asserted to be zero.** `futile_wakeups() == 0` is the whole of
  `M7.58`'s thundering-herd measurement, and a frozen counter makes it
  unfalsifiable while still reading as evidence.
- **A method nothing calls.** `is_healthy` survived being replaced by `true`
  *and* by `false` in three separate places, which is only possible if nothing
  asks it.

Three defects were found and fixed rather than merely covered: `gossip` took a
node's mode from a digest it had already rejected as stale, which let an
out-of-order message undo a drain; `mutants.sh` reported a crate clean when its
unmutated baseline had failed to build; and an exclusion added during the
milestone matched nothing, because cargo-mutants compares a glob containing a
slash against the whole path.

The lesson the milestone kept teaching, in four different costumes, is that
running is not constraining. A test can execute a line without pinning it, a
flag can be passed without matching anything, an assertion can compare a value
against itself, and a harness can report success for a run that tested nothing.

## M15: the protocol crate under a second reading

```bash
scripts/gates/m15-complete.sh
```

`pgprox-proto` is the only crate in the workspace that parses bytes chosen by
whoever can reach the listener, and it is the one crate whose rules are written
down at the top of its own `lib.rs`. This milestone reads it against those
rules, against `pgbouncer`, and against a profiler, in that order.

Two of its rules do not hold.

**"Validate length before allocating."** The length is validated. What is not
bounded is what the validated length is then allowed to buffer.
`DEFAULT_MAX_INSPECT` exists to be that bound, says so in its own doc comment,
and has no caller anywhere in the workspace. `FrameRelay` sets
`want_inspect = header.body_len` for any `Inspect::Whole` tag, and four of those
are frontend tags an unauthenticated client can send. A `Sync` declaring 8 MiB
makes the relay hold 8 MiB, measured, not argued.

**"Nothing here allocates at all."** Five separate places do. Four of them have
a reason and the sentence is what is wrong; one of them, `select_sasl_mechanism`,
builds a vector in order to search it and does not need to.

The comparison against `pgbouncer` was worth more than expected. It found the
COPY leak (`M15.2`) because pgbouncer's `server.c` carries the comment
"ErrorResponse and CommandComplete show end of copy mode" against code that has
no counterpart here, and the prepared-statement gap (`M15.3`) because pgbouncer
matches `DEALLOCATE ALL` and `DISCARD ALL` against the `CommandComplete` tag and
frees both the client and the server maps. `pgprox` has both maps and both
clearing functions, and calls neither from anything but a test.

Completion condition: every finding above either fixed with a test that fails
without the fix, or recorded with an argument for why it stands. The
performance items carry a measurement rather than a claim.

**What it came to.** Thirteen findings across three readings. The first was the
crate against its own header, the second was a second pass plus a mutation
sweep, the third was a fuzz run and a sweep for the two patterns the first two
kept turning up.

Three were memory bounds a peer chose. `DEFAULT_MAX_INSPECT` documented itself
as the ceiling on what one message may buffer and had no caller, so a `Sync`
declaring 8 MiB held 8 MiB, from a client that had not authenticated. The cap
alone was half a fix, because `Vec::clear` keeps its allocation and one frame
per connection would then have bought a permanent megabyte. And the handshake
itself was read against the 1 GiB relay cap, where Postgres allows 10000 bytes.

Two were connection state that never came back. A failed COPY held its upstream
connection for the life of the session, because `ReadyForQuery` ended an
extended sequence and did not end COPY. `DISCARD ALL` deallocated the server's
prepared statements while both maps went on believing in them, and both
clearing functions had existed since M5 with no caller outside their own tests.

The rest were smaller: a scalar scan on the hottest loop in the crate, a
five-byte copy on every frame, a count that could disagree with its list, a
capacity reserved from an unread number, a parameter that pinned where it could
replay, and a header sentence that claimed the crate never allocates in five
places where it does.

**Two things are worth saying plainly.**

`pgbouncer` was worth more than reading our own code again. Two of the three
correctness bugs came from comparing against it, and neither was visible from
inside: the COPY leak is a line pgbouncer has and we did not, and the
prepared-statement desync is a rule it applies that we had written down and
never called. Reading a mature implementation of the same protocol found what
re-reading ours did not.

`M15.12` is the one to keep. A mutation run found a survivor in a test this
milestone wrote: an assertion compared against the constant that produced it,
which is the first of the four shapes M14 catalogued, and which `M15.6` quoted
while fixing a different instance of it. Knowing the failure mode, and having
just written about it, did not stop me committing it. That is the argument for
running the check rather than for knowing the rule.

## M16: the streaming relay nothing streams through

```bash
scripts/gates/m16-complete.sh
```

`M15` read `pgprox-proto` three times and did not ask the one question that
turned out to matter most: who calls this.

`FrameRelay` was written so that a large message is forwarded as it arrives
rather than held. Its module header states the alternative it exists to prevent
in as many words: a relay built on `decode` "must accumulate an entire body
before forwarding a byte", and "a single large `DataRow` would then hold up to a
gigabyte, and ADR 0008's whole premise is that an idle connection costs roughly
200 bytes".

The relay loop in `bin/pgprox/src/serve.rs` is built on `decode`. `FrameRelay`
has no caller in the workspace outside its own module, its tests and its
benches. The one other mention of it anywhere is a comment in `shell.rs`
pointing at it for an unrelated reason.

This is the same shape as `M15.1`, where `DEFAULT_MAX_INSPECT` documented a
bound nothing read, and as `M15.3`, where two clearing functions existed with no
caller outside their own tests. Three times in one review, the defect was not
wrong code but correct code nothing reached. That is worth naming: this project
tests what it writes and does not check what it wires.

`M7` held 100k connections at 546 MB. That run used small rows, so it does not
contradict any of this and does not answer it either.

Completion condition: a measurement first, then the two directions, then the
same 100k run with a result set large enough that the difference would show.

**Where it got to.** Both bulk directions stream, and the number is
16,777,216 bytes held for one 16 MiB `DataRow` becoming 512.

Server to client covers every `DataRow` and every `CopyData` of every uncached
statement, which is the traffic this was about. Client to server covers the
COPY-IN loop, which is the whole of a `COPY ... FROM STDIN`. Both were
validated against things that can disagree: `conformance.sh` against real
Postgres 17 and 18 across psql, pgx, asyncpg, JDBC and npgsql, and `e2e.sh`
against the three-node stack, whose `pgbench --initialize` loads 100,000 rows
through the proxy with a real `COPY`. Both suites were run before the change as
well, so a green run after it means something. No existing test was relaxed.

Two things came out of the work that were not in the plan.

The split forced a cancellation-safety question into the open. `read_header`
consumes five bytes before the body arrives, so the *pair* must not straddle a
cancellation point. It is itself safe, since it consumes only after the five
bytes decode and its only await is the fill before that. The pump and the COPY
loop have nothing racing them; the relay loop has a `select!` whose drain
branch can drop a read mid-frame, so `read_tagged` stays atomic and stays what
that loop calls. That is written on the function rather than left to be found.

And the streaming decision rests on an implication nobody had stated: an
uninspected tag decodes to `Opaque` without reading a body. Two lists in two
modules happened to agree. `what_is_not_inspected_is_not_decoded_either` now
walks all 256 tags and says so.

**`M16.6` is done too, and its design pass is the part worth reading.** Writing
it out before writing it found five hazards, and the fifth set the scope:
`pin_reason` scans every statement in a query's SQL, not the first, so a
truncated scan is a missed pin and a missed pin hands one client another
client's state. `Query` and `Parse` are therefore read whole whatever the policy
says. That pointed at the right target rather than away from it, because a
`Bind` carries parameter values and not SQL, and a 100 MB `Bind` parameter is
both the case that matters most and the one that is safe. It now costs 4 KiB.

**And the milestone's own subject got a script.** Five findings across `M15` and
`M16` were correct, tested code that nothing reached, and one of them was
committed one commit after its author wrote that a primitive with no caller is
the defect the milestone exists to fix. `dead_code` cannot see them; they are
all `pub`. `scripts/check-wired.sh` can, and its own first two versions had the
flaw in miniature, counting a `pub use` and then a doc comment as callers, so
both would have passed while the defect was live. Those are planted in
`negative.sh` rather than described.

The 100k run with a large result set is the other half of the completion
condition and needs the three machines `M7`'s full run needed.

## M17: the binaries mutation testing never reached (complete)

```bash
scripts/gates/m17-complete.sh
```

`M14` put every crate under `crates/` into the mutation sweep and `M14.4` wrote
down which stayed out and why. It never considered the two binaries. They were
not excluded, they were not thought about, and `scripts/mutants.sh` had no line
about them either way.

That mattered more after `M16`, which moved seven correctness decisions into
`bin/pgprox/src/serve.rs`. The code three mutation runs had not touched was the
code most recently written.

**Where it got to.** 571 mutants in `pgprox` and 124 in `pgload`, with every
surviving mutant either killed or carrying a written argument.

Two of the survivors were defects rather than missing tests. `Sessions::set_pinned`
counted a pin for a client that had already gone, so `pgprox_pin_total` could
climb while `SHOW CLIENTS` showed nothing pinned, and its sibling `shed` had a
test asserting the opposite since the day it was written. And `apply_quota`
locked and cloned the whole pool map twice for every configured server, every
tick.

Three tests did not test what their names said. The sharpest is that every
`TlsMode::Verified` test in `dial.rs` asserted a *failure*, so nothing had ever
proved an upstream TLS connection can succeed and the arm performing the
handshake could be deleted whole.

Two tests were caught being flaky before they landed, both by running them a
dozen times rather than once. `M17.5` earned that habit by reading one mutation
sample as fact and writing two baseline arguments the next run contradicted.

**And the measurement turned out to be measuring itself.** `M17.7` found the
per-test cap and the whole-suite budget had been sized in `M10.13` against a
suite of 0.321s across four crates, then never re-derived as ten crates and two
binaries were added. Under the parallelism the sweep actually uses, the worst
honest test runs 6.66s against a 10s cap. That inflates in both directions: a
tight budget reports a timeout for a mutant nothing escaped, and a tight cap
terminates an honest test, which nextest reports as a failure and cargo-mutants
reads as a **kill for a mutant nothing detected**. Both numbers now carry the
measurement in the file that sets them.

## M18: what the deployment story assumes

```bash
scripts/gates/m18-complete.sh
```

`M17` closed the last mutation survivor and the backlog went dry, which is when
the questions that were never tasks become visible. Three of them are about the
gap between what this project's documents say it does and what it does, in the
one area no test covers because it is not code: how the fleet is deployed and
how nodes find each other.

The trigger was a question about running this behind the Kubernetes API for
membership rather than gossip. Answering it meant reading `pgprox-cluster`
against ADR 0004, and the ADR describes a system that was never built.

**ADR 0004 says SWIM over UDP using `foca`, seeded from headless Service DNS,
with sub-second failure detection.** The implementation is TCP carrying
newline-delimited JSON over a peer list passed as `--peer` flags, with a
two-second peer timeout. There is no `foca` dependency and no `UdpSocket` in the
workspace. That is not a small drift: the ADR is the document a reader consults
before changing any of this, and it would send them looking for a gossip library
that is not there.

**Membership and load are one message, and only one of them is membership.**
A digest carries mode, client counts, upstream counts per server and per-tenant
usage for homed tenants; liveness is derived from when digests arrive. An
external membership source can supply who exists. It cannot supply what each
node is holding, which is what quota, shedding and reservations run on. Any
pluggable-membership design has to say which half it replaces, and the answer is
at most one of them.

**A milestone can close with no completion condition, and the gate that should
have caught that only checks the other direction.** `check-drift.sh` walks
`scripts/m*-complete.sh` and requires each to be named in CI. Nothing requires a
milestone to have one. `M16` has a prose condition and no script; `M17` has
neither, and it closed anyway. `M10.17` established that a milestone whose
completion condition does not exist cannot be closed, and this is that rule with
nothing enforcing it.

Completion condition: `scripts/gates/m18-complete.sh`, which is itself part of the
milestone rather than an afterthought, for the reason above.

**Where it got to.** Three findings, and none of them was code.

ADR 0004 described SWIM gossip over UDP using a library this workspace has never
depended on. The transport is TCP carrying newline-delimited JSON over a peer
list passed as flags, and the failure detector is three and ten seconds rather
than sub-second. Everything the ADR decided above the transport was intact and
is what the property tests hold, so the fix was a correction rather than a
reversal. The old sentence is quoted rather than deleted, because an ADR that
quietly changes is worse than one that is wrong.

The question that found it was whether the Kubernetes API could supply
membership instead of gossip. It cannot, and the answer is now a spec rather
than a conversation. A gossip round carries load and liveness in one message:
the API can say which pods are Ready, and it cannot say what any of them is
holding, which is what quota and shedding run on. Liveness has to stay
first-party besides, because a pod partitioned from its peers but still able to
reach the control plane would be told the fleet is healthy and would keep
granting from the free pool while the other side elected a replacement. So the
seam is discovery, and the rule that keeps it safe is in the trait's own doc
comment rather than only in the spec.

And a milestone could close with nothing to run. `check-drift.sh` required every
gate that exists to be wired into CI and never asked whether a milestone had
one, so `M16` closed with a prose condition and `M17` with neither. Six rows
lacked an `mNN-complete.sh`, not two, and three of those six point at a real
gate under another name, which is why the rule is about naming a runnable
command rather than about a filename.

**What this milestone did not do, and deliberately.** It did not implement the
Kubernetes peer source, and it did not move the fleet off a StatefulSet. The
node id is the StatefulSet ordinal and it is encoded into every `ConnId` so a
cancel landing on any pod routes to the owner, which makes "pods behind a
Service" a change to what clients see on the wire rather than a deployment
choice. That is a separate spec and a larger one.

## M19: a seam for peer discovery (complete)

```bash
scripts/gates/m19-complete.sh
```

`M18.2` specified this and deliberately did not file its tasks, because filing
work nobody has scheduled puts entries in a backlog nobody can start. It is
scheduled now.

A node learns its peers from `--peer` flags rendered by a shell loop in the
StatefulSet template, and hands the resulting table to three places that never
see it change: the quota transport, the observatory's client fan-out, and the
cancel router. So a deployment that wants Kubernetes to supply peers has nowhere
to put it, and scaling the fleet means restarting every pod so each re-reads its
flags.

**The seam is discovery, and that distinction is the milestone.** A gossip round
carries load and liveness in one message. An external source can say which pods
exist; it cannot say what any of them is holding, which is what quota, shedding
and reservations run on. And liveness has to stay first-party, because a node
counts a peer alive from digests that *arrived*, which is what makes a one-way
network failure safe. A third party telling a partitioned node the fleet is
healthy is the two-leaders case ADR 0004's majority rule exists to prevent.

Completion condition: `scripts/gates/m19-complete.sh`, which exists from this
milestone's first commit and gains a check as each task lands. `M18.3` made a
milestone with nothing to run a failure, and a gate that waited until the end
would be the same defect wearing a schedule.

### Where it got to

The seam is `PeerSource` in `pgprox-core`, with `StaticPeers` behind the
`--peer` flags and a `watch` channel so a table published later reaches a
consumer built earlier. All three consumers read through it, and each is held
by a test that publishes *after* the consumer exists, which is the only shape
that can tell a source from a copy. Nothing discovers peers yet, and that is
deliberate: the milestone is the seam, and a Kubernetes source is an opt-in
that plugs into it. ADR 0023 holds the line the seam may not cross, which is
that a source may add a peer to gossip with and may never make one count as
alive.

**Two of the eight tasks were corrections of claims this milestone made, and
both were about a fake rather than about the system.** `M19.4` reported a quota
cap breach, deterministic and with no network faults, and `M19.5` found it in
the simulation: `gossip_over_peers` sent the initiator's digest and nothing
back, and the transport has carried both halves of an exchange on one
connection since M6. `M19.6` chased a `pgload` test failing one run in
twenty-five and found its fake sending `57P01` at whatever statement a shared
counter landed on, so the scheduler was choosing between a relocation and a
lost transaction while the test asserted the first. Both reductions were kept
and inverted into assertions.

The third finding was found by the hunt rather than by the milestone. Running
the workspace under `cargo test` instead of nextest to reproduce `M19.6` turned
up `M19.7`: `entry::serve` installed a process-wide subscriber, so two tests in
one binary raced for the one install a process gets. Nextest gives every test
its own process, so the gate had been green over it throughout. That is the
part worth carrying forward. A gate that isolates every test cannot see a
collision between two of them, and this project's gate does exactly that.

## M20: the protocol layer against pgbouncer, pgcat and odyssey (complete)

```bash
scripts/gates/m20-complete.sh
```

`M15` read `pgprox-proto` against its own header and against `pgbouncer`, and
found thirteen things. This reads the whole path a frame travels rather than
one crate: the codec, `pgprox-session`, and the relay in `bin/pgprox`. It adds
`pgcat` and `odyssey` because two readings against one outside implementation
is one opinion consulted twice.

**The first finding is why the milestone exists.** A client's protocol `Close`
of a prepared statement is rewritten and forwarded, the server deallocates the
statement, and neither of this proxy's two maps hears about it. The next `Bind`
of that SQL names something that is gone, and the connection stays wrong after
the session that closed it has left. It reproduced on the first attempt, from a
sequence every driver with a statement cache sends.

It is the same finding as `M15.3`'s `DISCARD ALL`, through the one door that
fix left open, and it survived four readings because the fake answered `Close`
as though it were a simple query. `M9.24` had already written, next to the arm
it added for `Parse`, that "the proxy's record of what a connection holds is
only correct if something notices when it is not". Nothing noticed for the
other two halves.

Completion condition: `scripts/gates/m20-complete.sh`, which exists from this
milestone's first commit and gains a check as each task lands.

### Where it got to

Eight findings, all fixed.

**One was a live defect.** A client's protocol `Close` of a prepared statement
was rewritten and forwarded, the server deallocated the statement, and neither
of this proxy's maps heard about it, so the next `Bind` of that SQL named
something that was gone. It outlived the session that caused it: the connection
went back to the pool still mis-recorded, so the next session to bind that SQL
failed the same way. It reproduced on the first attempt from a sequence every
driver with a statement cache sends.

**Three were things a client asked for and was silently not given.** A
`search_path` in the connection string, a runtime setting sent as a plain
startup parameter, and a `_pq_.` protocol extension. All three were parsed,
some were stored, and none reached anything. The extension is the sharpest of
them: `NegotiateProtocolVersion` is the only message that says a request was
not recognised, so saying nothing is how the protocol says yes.

**Two were the pool not looking at its own sockets.** Nothing said `Terminate`
to a connection it was closing, in a design where reaping at thirty seconds is
the steady state, so every routine close was a line on the database that read
like a crash. And nothing read an idle connection at all, so a server that went
away between borrowers was discovered by the next client's query. That one was
worse than it was reported as: the client did not get an error, it got its
socket closed.

**One was a semantic difference nobody would have noticed.** The unnamed
prepared statement was rewritten into a named one, so every one-shot query a
driver sent through it became a permanent entry under the per-connection cap.

The eighth is `replication`, which was ignored rather than answered.

**Where they came from is the argument for doing this again.** `M15` read this
code against pgbouncer and found thirteen things; this read it against three
implementations and found eight more, and the ones that needed a second opinion
were the ones about what a client is owed rather than what the code does.
pgcat's `anonymous()` is what named the unnamed-statement finding. pgbouncer's
`disconnect_server` is what named the missing `Terminate` and its `SV_IDLE`
handling is what named the unread idle connection.

But `M20.7`, the plain startup parameters, came from scoping `M20.2` honestly
rather than from any of them. And the process-wide logging collision `M19.7`
fixed came from running the suite the wrong way while hunting something else.
Reading another implementation finds what you were not looking for; running
your own code differently finds what your gate cannot see.

**Three representations in this milestone were decided by a memory budget.**
`one_session_costs_less_than_the_slab_buffer_it_no_longer_holds` failed three
times: on the startup settings, on the unnamed statement's SQL, and on carrying
two lists where one would do. Each time the threshold stayed where it was and
the code got smaller. A future is the union of what is alive across its awaits,
and the relay loop is nothing else.

## M21: the driver matrix does not cover what M20 changed (complete)

```bash
scripts/gates/m21-complete.sh
```

`scripts/driver-matrix.sh` has run all five drivers against `bin/pgprox`, over
TLS onto a real Postgres, since `M8.13`. `tests/proxy-drivers/_env.sh` records
why it had to exist: asyncpg deadlocked on its first parameterised query from
M6 until M8, and `scripts/conformance.sh` stayed green throughout, because the
harness answers a `Flush` the same wrong way the proxy did. The codec and the
harness are the same code, so a misunderstanding shared between them is
invisible by construction.

This milestone was proposed as building that suite, before anyone checked
whether it existed. It did. What follows is what running it actually found.

**The matrix passes, and covers none of what M20 changed.** Its depths are both
wire protocols, a prepared statement reused, a result larger than one segment,
a transaction, and an error with a statement after it. M20 added a protocol
`Close` and a re-prepare, the unnamed statement, a `search_path` from
`options`, an `application_name` from the startup packet, and a refused
`replication` connection. No driver probes any of the five.

**And the report is committed evidence with a date on it that nothing checks.**
`product/conformance/driver-matrix.md` read "Generated on 2026-07-28" through
thirteen milestones, one of which changed the wire. `m1f-complete.sh` asserts
the script exists and the report exists. Neither would fail on a report that
predates everything it is evidence about, which is `M18.1`'s finding in a new
place: evidence describing a tree that no longer exists is worse than none,
because it gets quoted.

Completion condition: `scripts/gates/m21-complete.sh`, which exists from this
milestone's first commit and gains a check as each task lands.

### Where it got to

The matrix now covers what M20 changed: a statement given back with a protocol
`Close` and prepared again, in the three drivers that keep a cache; the unnamed
statement, counted on the server rather than merely run; and the startup
packet. The report records which commit it describes, so a stale one says how
far behind it is and names the commits.

**Three of the four cases were wrong first, and each was wrong in a way only a
reverted build could show.**

`M21.2` used pgx's `DeallocateAll` and npgsql's `UnprepareAll`, and both passed
against a proxy built without `M20.1`. Those send `DEALLOCATE ALL` as SQL,
which this proxy has handled since `M15.3`. Two probes were standing evidence
for a fix from five milestones earlier while claiming to cover this one.

`M21.3` was filed as "run a one-shot query more than once and survive", which
both behaviours do. It counts what the server was left holding instead, and the
counting was itself wrong twice: it counted named statements the same probe
legitimately prepares, and then it counted itself, because the count query's
own text contained the marker it matched on.

`M21.1` was filed asking for a gate that fails on a stale report. That gate
would be red from the first edit to `bin/pgprox` and permanently red in CI,
which has neither Docker nor the five toolchains needed to clear it. It fails
on absent provenance instead and reports staleness with the commits behind it.

**And the milestone opened by proposing to build something that already
existed.** `scripts/driver-matrix.sh` has pointed all five drivers at the proxy
since `M8.13`. The refutation was in `tests/proxy-drivers/_env.sh`, in a
directory that was never listed.

What the four corrections have in common is the shape of the mistake, not the
subject: a check believed because it passed. Every one of them was caught by
running the thing it was supposed to catch, against a build with the fix taken
out. A green probe says nothing until it has been seen to go red.

## M22: the mutants nobody has swept since M17 (complete)

```bash
scripts/gates/m22-complete.sh
```

`product/mutants-baseline.txt` was last written by `M17.4` on 2026-08-01, and
eighteen commits have landed on the mutated crates since: all of M18, M19, M20
and M21. Everything those milestones added has never been mutation tested.

Coverage says a line ran. This says the line mattered, and the difference is not
academic here: M17's sweep of `pgprox` found two real defects, a pin counted for
a client that had already gone and a lock taken twice per server per tick, and
neither was visible to any test in the suite.

**And nothing notices the baseline is stale.** Four gates read its contents,
`m10`, `m14`, `m15` and `m17`, and not one asks whether it describes the tree it
is a claim about. That is `M21.1`'s finding again, in the file four gates depend
on, so it gets the same answer: the baseline records the newest commit touching
the crates it covers, and the gate says how far behind it is.

One crate per commit. A sweep that finds a missing test produces a commit with a
test in it, and those must not be bundled with a re-baseline.

Completion condition: `scripts/gates/m22-complete.sh`, which exists from this
milestone's first commit and gains a check as each crate is swept.

### Where it got to

3,835 mutants across all sixteen crates, 39 surviving and argued, nine of them
new. The baseline now records, per crate, the commit its sweep ran against.

**No two of the nine had the same cause**, and that is the result.

Two were tests in the wrong crate. `Upstreamed::unfit` and
`Upstreamed::goodbye` were covered only from `bin/pgprox`, and a sweep mutates
one crate and runs that crate's tests, so a decision whose only witness is
downstream is invisible to the tool built to find untested decisions. `M22.7`
wrote that into `standards/testing.md`, where it had never been said.

Four were a decision made where a test cannot stand. `M20.6`'s unnamed-statement
checks lived inside `ready_statement`, which takes a socket, and they had
in-crate tests that still could not reach them. The worst of them, `holds_unnamed`
replaced by `true`, is a session that moved connections binding against whatever
the previous borrower left unnamed: no error, the wrong query's rows.
**In-crate is necessary and not sufficient**, which is the correction M22.4 had
to make to M22.2's neat rule one task after stating it.

Two were `map_statement_name` and `unnamed_statement` from the same milestone,
and one was a `Timeout` that was neither a missing test nor machine contention.
`Lexer::next` guards its word arm with `is_word_char` and advances by
`word_end`, and `word_end` restates the rule inline over bytes for speed. Let
them disagree about one character and the lexer stops making progress and spins
forever on a live connection. That is `pgprox-pool`'s "do not write another SQL
scanner" happening inside the scanner, between two halves of one rule, and it
was found by mutation rather than by reading because no test could fail: they
all hung.

The eleven crates with no new logic were quiet, which was a prediction before it
was a fact and is recorded as a result for that reason.

What generalises is not any of the nine. It is that **coverage was 95% or better
in every one of these crates throughout**, and the sweep is what said the lines
had run without mattering. That claim has been in `standards/testing.md` since
M-1 and nothing ran it until `M10.3`.

## M23: the streaming question M16 left open, at the scale one machine has (complete)

```bash
scripts/gates/m23-complete.sh
```

`M16.1` measured one 16 MiB `DataRow` on one connection: 16,777,216 bytes held
on the path the proxy used, zero on the streaming relay it did not. `M16`'s
completion condition asks for "the same 100k run with a result set large enough
that the difference would show", and that half is blocked on three machines.

**The connection count is not what makes the difference visible. The row size
is.** `M7`'s 100k run used pgbench's rows, so a proxy holding every row entire
would have produced identical numbers. A pair of runs at one connection count,
differing in nothing but their statements, answers the memory question on one
machine.

Two pairs, and the second corrects the first. At 200 connections a 1 MiB result
appeared to cost 8,581 more bytes per connection; at 600 the difference is -403,
which is less than the measurement's own variability. A cost that disappears
when you look harder was never a cost, and one pair could not tell a
per-connection cost from a constant landing differently across two runs. Holding
each row would cost 1,048,576 per connection.

The 100k half stays blocked, and this narrows what is unknown rather than
closing it. What remains unmeasured is whether the same holds at a hundred
thousand connections on real network hardware.

Completion condition: `scripts/gates/m23-complete.sh`, which checks the two workloads
differ where they claim to and nowhere else, because that is what makes the
pair a comparison rather than two numbers.

### Where it got to

Two pairs of runs, each pair at one connection count, the two workloads
differing in their statements and nothing else.

| connections | `workload.yaml` | `workload-large.yaml` | difference |
| --- | --- | --- | --- |
| 200 | 26,112 bytes/conn | 34,693 bytes/conn | +8,581 |
| 600 | 17,674 bytes/conn | 17,271 bytes/conn | -403 |

A relay that held each row entire would show +1,048,576. At 600 sessions each
pulling a megabyte through the real proxy, a real pool and a real Postgres,
there is no measurable per-connection cost to the row being a megabyte rather
than a few bytes.

**The second pair corrected the first**, and that is the part worth keeping.
With only the run at 200 this milestone had recorded that a megabyte costs 8,581
more bytes per connection. It does not: at 600 the difference is negative, which
is to say smaller than the measurement's own variability. The 8,581 was fixed
overhead landing differently across two runs at a count where fixed overhead
still dominates, which the same workload says plainly across three counts on one
machine: 69,959 bytes per connection at 50, 34,693 at 200, 17,271 at 600.

One pair could not have told those apart, and the two readings mean opposite
things. A per-connection cost that grows with the count is something
accumulating, which is what the streaming relay exists to prevent; a constant is
fixed overhead. The first pair was consistent with both.

**What this does not close.** `M16`'s 100k half stays blocked and this narrows
it rather than answering it. What remains unmeasured is whether the same holds
at a hundred thousand connections on real network hardware, which needs the
three machines `M7`'s full run needed. The extrapolations here are worthless in
both directions: 17,271 bytes reaches 1,647 MB at 100k against a 500 MB target,
and the reference workload reaches 1,685 MB on the same machine where `M7`
measured 546 MB on three.

The reframing is what made the milestone possible at all. `M16`'s blocked half
had been read as blocked on the connection count. It was not: `M7`'s 100k run
used pgbench's rows, so a proxy holding every row entire would have produced
identical numbers. The row size is what makes the difference visible, and that
costs one machine rather than three.

The gate checks the property that makes a pair a comparison rather than two
numbers: that the two workloads differ where they claim to and nowhere else.
Every derived workload in `product/perf/` asserts that in its header, including
the pair `M7.55`'s conclusion rests on, and until this milestone nothing checked
one of them.

## M24: a reading of every crate, and the nine things it found (complete)

```bash
scripts/gates/m24-complete.sh
```

Sixteen crates read against correctness, completeness, design, performance and
test quality. Nine findings. The test quality question came back empty, which is
worth saying: seven test functions in the workspace assert nothing, and six of
those are "this input does not panic" and "this input does not hang", which is
the assertion.

**Four of the nine are one shape.** A decision that reads SQL, taken by a
scanner that is not the shared one, or by the shared one asked the wrong way.
`pgprox-pool` and `pgprox-route` each carry a written rule against this, in the
same words, because the two crates once had a scanner each and the two
disagreed about where an `E'...'` string ends. `pgprox-pool/src/params.rs` has
its own scanner anyway, and `pgprox-pool/src/pin.rs` reads the shared one
through `statement_words(sql, true)` while `pgprox-route` reads it raw, so the
two answer differently about the same text.

The rule was right and it was applied to the two places that had already been
caught. Nothing looked for a third.

Completion condition: `scripts/gates/m24-complete.sh`, which runs a named test for
each finding and reads its exit status, per `M12.8`.

### Where it got to

Nine findings, all fixed, two of them only in part and both saying which part.

**Four were one cause.** A decision that reads SQL, taken by a scanner that is
not the shared one or by the shared one asked the wrong way.

| Finding | What read the SQL | What it cost |
| --- | --- | --- |
| `M24.1` | `params.rs`, its own scanner, first statement only | `SET a=1; SET search_path=x` dropped the search path and pinned on neither |
| `M24.2` | `statement_words`, which drops quoted text | `SET "search_path" = 'x'` was neither recorded nor pinned |
| `M24.3` | `statement_words(sql, true)`, which joins qualified names | `pg_catalog.pg_advisory_lock` did not pin |
| `M24.4` | nothing; the cache key simply had fewer fields than the pool key | one tenant's two databases and two roles shared entries |

`pgprox-pool` and `pgprox-route` each carry a written rule against a second
scanner, in the same words, because the two crates once had one each and
disagreed about where an `E'...'` string ends. `pgprox-pool/src/params.rs` had a
third. The rule was right and it was applied to the two places that had already
been caught; nothing looked for a third.

The other three in that table are subtler and worth naming separately, because
none of them is a second scanner. They are three callers of the *same* function
asking it differently: raw tokens, words, words with dots joined. Each choice is
correct for the caller that made it, and no two of them answer the same question
about the same text.

**Two are fixed only in part**, and the honest half is which:

- `M24.7` widened a prepared statement's global name from 64 bits to 128, and
  left the unnamed statement's guard at 64. Widening the name cost nothing,
  because it is a `String` either way. Widening the guard costs 32 bytes of
  session future, taking `one_session_costs_less_than_the_slab_buffer_it_no_
  longer_holds` from 5,112 to 5,144 against a ceiling of 5,120. The ceiling is a
  constant and non-negotiable 2 says it does not move, so the field carries the
  measurement and what would have to change first.
- `M24.7` also **withdrew its own acceptance criterion**. It was filed asking
  for "a constructed collision rather than an argument", and constructing an
  FNV-1a-64 collision is a meet-in-the-middle search of roughly 2^32, which is
  not a unit test. No collision was constructed. The criterion would otherwise
  have been quietly met by something weaker, which is the failure mode a written
  acceptance exists to prevent and the one it is most likely to cause.

**The two that were nothing failing.** `M24.5` and `M24.8` are both slow leaks
with no error attached. The grant cache stopped admitting once full, because an
entry left only when its own key was looked up again and a rotating token's key
never is; every connection then made a sidecar RPC, on a declared hot path,
permanently and only under the load that fills it. `LivePool` never forgot a
pool key. Neither would ever have appeared in a log.

**One was a capability two documents asserted and no code implemented.**
`M24.9`: `architecture.md` has credited `pgprox-tls` with cert hot reload since
M-1 and the crate's own `AGENTS.md` repeated it. `server_config` was called once
and its answer never changed. This is `M13`'s subject from the other side: not a
rule with no script, but a capability with no code.

**Test quality came back empty**, which is worth recording as a result rather
than as silence. Seven test functions in the workspace assert nothing, and six
of those are "this input does not panic" and "this input does not hang", where
not panicking is the assertion. The seventh is a `proptest` whose assertion is
`prop_assert_eq!`.

Every fix was run against a build with the fix removed. Eight negative controls
across six tasks, and two of them found the test rather than the code:
`M24.5`'s rate limit and `M24.8`'s waiter guard each had a test that passed
without them until the control said otherwise.

## M25: the query cache against pgpool-II (complete)

```bash
scripts/gates/m25-complete.sh
```

`pgprox-cache` read against pgpool-II's `memqcache`, which is the closest thing
to a reference implementation this feature has.

Most of the comparison came out well, and the parts that did are worth naming
so a later reading does not go over them again. The per-answer cap fires while
the answer is still streaming, so an answer that will not fit is abandoned
mid-flight and falls back to the streaming path rather than being assembled and
then refused. The opt-in is per tenant rather than one global switch. The TTL is
the contract, where `memqcache_expire` defaults to zero and means never. And the
key carries the tenant, the database and the role, which ADR 0024 is about.

**Two places pgpool is genuinely ahead, and both stay open.** It consults
`pg_proc` for a function's volatility, so it catches a tenant's own `VOLATILE`
function; `cacheable.rs` matches a denylist of built-in names and cannot. It
invalidates by table OID from the parse tree; this invalidates a whole tenant.
Both are already written down, in `cacheable.rs`'s own docs and in this crate's
`AGENTS.md`, as the honest limits of a lexical scan. They are limits, not
findings, and this milestone does not pretend otherwise.

**Three findings, all about one constant.** `MAX_RECORDED_ANSWER` is pgpool's
`memqcache_maxcache`, and it behaves unlike every other bound in this cache: it
is invisible when it fires, it cannot be set, and nothing relates it to the
budget it interacts with.

Completion condition: `scripts/gates/m25-complete.sh`, which runs a named test per
finding and reads its exit status, per `M12.8`, and refuses to pass with a
ticked task it does not name, per `M24.0`.

### Where it got to

Three findings, all about `MAX_RECORDED_ANSWER`, and the shape they shared is
worth more than any of them: it was the only bound in the query cache that was
invisible when it fired, unsettable, and unrelated to the limit it interacts
with. Every other bound here is at least two of those three.

| Finding | What it was | What it is |
| --- | --- | --- |
| `M25.1` | an abandoned answer moved no counter, and `get` had already booked a miss | `pgprox_cache_total{result="abandoned"}`, a `SHOW CACHE` column and a JSON field |
| `M25.2` | 1 MiB in a `const`, beside a budget that reloads live | `query_cache.max_entry_bytes`, pushed by the tick loop the way `max_client_conns` reaches the gate |
| `M25.3` | nothing related the two | refused above the budget, and refused at zero the way the budget already was |

`M25.1` is the one worth having. A tenant whose results sat just over a
megabyte saw a hit rate of zero, and every counter agreed nothing was wrong:
the lookup booked a miss before anything knew how big the answer would be, and
`rejected` stayed at zero because `put` was never reached. The two counters say
opposite things about what to do. `rejected` says the budget is too small for
one result; `abandoned` says the results are too big for a cache to be the right
tool, and raising the budget does nothing for it.

**What did not become a task.** Two places pgpool-II is genuinely ahead, and
both stay open:

- It consults `pg_proc` for a function's volatility, so it catches a tenant's
  own `VOLATILE` function. `cacheable.rs` matches a denylist of built-in names
  and cannot.
- It invalidates by table OID from the parse tree. This invalidates a whole
  tenant.

Both were already written down, in `cacheable.rs`'s own docs and in the crate's
`AGENTS.md`, as the honest limits of a lexical scan. ADR 0009 records the same
limit for the classifier. Filing them would have been filing a design, and a
milestone that turned two known limits into two open tasks would have made its
own count of findings look better at the cost of saying something false about
what was discovered.

**And what came out well**, recorded so a later reading does not repeat the
comparison: the per-answer cap fires while the answer is still streaming, so an
answer that will not fit is abandoned mid-flight and falls back to the streaming
path rather than being assembled and then refused, which is the order pgpool
does it in. The opt-in is per tenant rather than one global switch. The TTL is
the contract, where `memqcache_expire` defaults to zero and means never. Bounded
by bytes with LRU eviction, where pgpool carries both a byte total and an entry
count.

One thing the fix had to keep, found by a test rather than by reading: the pair
check in `M25.3` was unconditional at first, and it refused a budget of zero
with the default cap above it. That is exactly the case
`a_budget_of_zero_is_allowed_while_nothing_is_cached` exists to permit, because
an operator may write the section down before deciding who gets it. The check is
conditional on the cache being on, and that test is in the gate for `M25.3` as
well as its own.

## M26: what the query cache costs, measured for the first time (complete)

```bash
scripts/gates/m26-complete.sh
```

`run-2026-07-29-cache.md` measured what the cache is **worth** end to end: 7% of
median latency and 7% of CPU per statement, and no movement in the pool lock
that holds half this proxy's CPU. Nothing has ever measured what it **costs**
per call. `scripts/bench.sh` ran three crates and this was not one of them, and
the store's own module docs promise that if a profile finds its single lock the
answer is to shard by the hash of the key, which is a promise nobody could
have kept: there was no number to compare against.

There is one now, at 4,096 entries across 64 tenants:

| path | instructions |
| --- | --- |
| `serves` | 206 |
| `cache_miss` | 1,605 |
| `cache_hit` | 4,144 |
| `cache_put` | 4,269 |
| `invalidate_one_tenant` | **198,283** |

**The lock is not the problem and invalidation is.** One write costs 48 hits or
124 misses, and it is linear in the whole node's entry count rather than in the
tenant's, because `invalidate_tenant` walks every key and compares an `Arc<str>`
by its contents. `M9.10` counted 10,700 invalidations against 20,000 lookups on
the reference workload; on those numbers invalidation costs roughly thirty-six
times what every lookup on the node costs put together, and 4,096 entries is
twenty-five times smaller than what a 64 MiB budget of point-select answers
holds.

The second number is smaller and points the same way: a hit costs two and a half
times a miss, which is the wrong way round for a structure whose whole argument
is that a hit is the cheap path.

Completion condition: `scripts/gates/m26-complete.sh`, which runs a named test per
finding and reads its exit status, and refuses to pass with a ticked task it
does not name.

### Where it got to

| bench | before | after | |
| --- | --- | --- | --- |
| `cache_hit` | 4,144 | 1,460 | **-65%** |
| `cache_hit_rotating` | n/a | 1,823 | new, and the honest one |
| `cache_miss` | 1,605 | 1,256 | -22% |
| `cache_put` | 4,269 | 3,783 | -11% |
| `invalidate_after_one_put` | 204,255 | 5,689 | **-97%** |

(`M28.2` later renamed that benchmark `invalidate_a_tenants_entries` and gave
it sixteen entries to drop rather than one, because at one entry it moved with
a random seed. The comparison above is the one that was measured.)
| heap blocks per lookup | 2 | **0** | |

**The lock was never the problem.** `store.rs` has promised since M9 that if a
profile ever found its single mutex, the answer was to shard by the hash of the
key. No profile could have: `scripts/bench.sh` ran three crates and this was not
one of them. The first thing a number did was point somewhere else entirely.

**Invalidation was the cost, by two orders of magnitude.** `invalidate_tenant`
filtered `entries.keys()` by `&key.tenant == tenant`, which compares an
`Arc<str>` by its contents, so a write was a string compare against every entry
on the node. One write cost 48 hits. On `M9.10`'s counts, 10,700 invalidations
against 20,000 lookups, that was roughly thirty-six times what every lookup on
the node cost put together.

**Three things the measurement decided rather than judgement did:**

- The tenant index holds sequence numbers, not keys. Holding keys meant hashing
  six fields a second time on every `put`, and `cache_put` went to 6,951
  instead of 5,238.
- `index()` uses `entry()` rather than a `get_mut` that falls back to it. The
  fallback is obviously cheaper and is measurably slower.
- The recency links live in the slab, not in the entries. With them inside
  `Entry`, editing a neighbour means finding it by key, and `cache_put` went the
  wrong way by 48% before the second attempt.

**What the allocation budget found that reading did not.** The crate had none,
while `pgprox-proto` and `pgprox-pool` both did. Adding one showed a *miss* —
which hashes a key and returns `None` — allocating two heap blocks. Neither was
the store's: `QueryCache` was an `#[async_trait]` over a store whose own docs
say "Nothing here waits", and `pgprox-core` also implements the trait for
`Arc<T>`, so a caller holding an `Arc<dyn QueryCache>` boxed twice. ADR 0025
made the contract synchronous, and two `block_on` helpers written to poll a
future that never yields were deleted outright.

**Two corrections to the measurement itself, both mine.** `block_on` used
`Box::pin`, which put a malloc inside every bench iteration and made the budget
test fail its own assertion. And `cache_hit` asks for the same key every time,
so it measured `touch`'s early return rather than `touch`; `cache_hit_rotating`
exists because of that and is the number to watch.

One bench had to change shape rather than improve. `invalidate_one_tenant`
measured a walk; with the walk gone it measured a failed hash lookup, and moved
15% between two runs of the same binary because how many probes a `HashMap` miss
takes depends on a per-process random seed. It stores one entry per iteration
now, is stable to 0.01%, and still guards what matters: against five thousand
instructions, a reintroduced walk is two hundred thousand.

## M27: unsafe becomes a governed exception rather than a closed door (complete)

```bash
scripts/gates/m27-complete.sh
```

The workspace has set `unsafe_code = "forbid"` since M0. `forbid` is not `deny`:
it cannot be overridden by a local `#[allow]` at all, so it is a decision no
measurement can ever reopen. Every other threshold in this repo is a constant
that a commit message and a number can move; this one was the exception, and it
was the exception by accident rather than by argument.

The argument in `standards/security.md` is sound and it is narrower than the
lint: "the failure mode of a decoder bug is a wrong answer or an error, never
memory corruption". That is about the crates parsing bytes an unauthenticated
peer sent. It is not about the query cache's slab or the buffer pool.

**This milestone writes no unsafe code.** It produces the conditions under which
unsafe may be written and the script that refuses it when they are not met. Any
actual use is a later task with a measurement attached, because a rule with no
script is a rule nobody keeps and unsafe with no number is a liability with no
evidence of upside.

Completion condition: `scripts/gates/m27-complete.sh`, which runs a named test per
finding and reads its exit status, and refuses to pass with a ticked task it
does not name.

### Where it got to

`deny` at the workspace root, five conditions on the exception, and
`scripts/check-unsafe.sh` refusing anything that does not meet them.

| Condition | What it stops |
| --- | --- |
| five crates keep `#![forbid]` in their own `lib.rs` | an `#[allow]` reaching a decoder |
| `// SAFETY-POLICY: <benchmark>` on the line above | an exception nobody argued for |
| that benchmark exists in the baseline | unsafe with no evidence of upside |
| the crate is named in the Miri job | unsafe nobody can maintain |
| not from a test, bench or build script | unsafe none of the above governs |

**No unsafe was written.** That was the point: the milestone produces the
conditions and the script, and the first use is a later task that has to satisfy
all five. A policy landed alongside its first exception is a policy shaped to fit
that exception.

**The standard described an arrangement that was not there.**
`standards/rust-style.md` said unsafe was "forbidden at the crate level in every
crate". It was forbidden once at the workspace root and one crate out of sixteen
repeated it. That sentence had survived twenty-six milestones, which is `M13`'s
subject arriving in the document `M13` was written into.

**Writing the negative cases found a defect in the script itself.** An
`#[allow(unsafe_code)]` on the first line of a file made it ask `sed` for line 0,
which is an error rather than an empty answer, and under `set -e` that killed the
run instead of failing the check. A gate written to enforce a rule about care,
dying rather than reporting, which is exactly what `M12` exists to catch. It has
its own case now.

**One thing found and not fixed here.** `m27-complete.sh` passed against a tree
with its own artefacts removed, because with no task ticked its only check was
"nothing is ticked, so nothing is unchecked", which holds whatever else is true.
`tests/gates/negative.sh` caught it. The gate now runs `check-unsafe.sh` and the
negative case and reads both exit codes, so it fails when either is missing. The
same shape is latent in every milestone gate on the commit that files it and
before anything lands, and this is the first time it has been observed rather
than reasoned about.

The tolerance for what comes next is already written down: if an unsafe version
moves its benchmark less than `scripts/bench.sh`'s threshold, it is deleted and
the safe one kept. `M26` is the evidence that the safe route usually wins
anyway, having taken a hit from 4,144 instructions to 1,460 without any.

## M28: the build configuration nobody had measured (complete)

```bash
scripts/gates/m28-complete.sh
```

`M27` closed on the observation that the hot-path procedure puts build
configuration before any unsafe, and that this workspace's release profile had
never been measured against the baseline. I also said it had no
`[profile.release]` block, which was wrong: it has one, and it already carries
`codegen-units = 1` and `panic = "abort"`.

So there is one lever rather than four. `lto` is `"thin"`, and the two that look
available are not: `panic = "abort"` is already taken, and
`-C target-cpu=native` is wrong for a binary shipped as a container image that
runs on hardware the build machine has never seen.

Completion condition: `scripts/gates/m28-complete.sh`.

### Where it got to

**One lever of the four, and the other three were already decided or wrong.**

| Knob | Verdict |
| --- | --- |
| `lto = "thin"` -> `"fat"` | taken, 7 to 15% on the route decision |
| `codegen-units = 1` | already set |
| `panic = "abort"` | already set, and right for a process whose rule is that it does not panic |
| `-C target-cpu=native` | refused: this ships as a container image |

| benchmark | thin | fat | |
| --- | --- | --- | --- |
| `pgprox-route::route_begin` | 1,536 | 1,294 | -15% |
| `pgprox-proto::decode_query` | 460 | 390 | -15% |
| `pgprox-route::route_update` | 7,423 | 6,717 | -9% |
| `pgprox-route::route_point_select` | 6,982 | 6,444 | -7% |

Every cache and pool number is unchanged to within a percent, which is what
inlining across one crate boundary looks like rather than a general uplift. The
cost is a release relink going from 12.98s to 30.43s, and it lands on CI and on
releases and on nobody's edit loop.

**The milestone was filed on a claim that was wrong.** `M27` closed saying this
workspace had no `[profile.release]` block. It has one, and it already carried
two of the four knobs. The milestone's own planning task says so, which is the
only reason the reader is not left with the earlier sentence.

**A benchmark reported a regression that did not exist.** `M28.1` measured
`invalidate_after_one_put` at +6% under fat LTO. A second run put it at -1%. The
benchmark measured one entry stored and dropped, which at that size is small
enough that a `HashMap`'s probe count is a measurable share of it, and probe
counts depend on a per-process random seed. It now drops sixteen entries and
three runs agree to 0.45%.

That is the second time this has happened to the same benchmark, in a different
form. `M26.4` fixed it once, when it measured an early return and moved 15%.
Both times the tell was the same: a number in a gated baseline small enough that
the machinery around it costs more than the thing it measures. The rule that
falls out is worth stating, because it is not written anywhere: a benchmark
under about a thousand instructions is measuring `scripts/bench.sh` as much as
it is measuring the code.

## M29: the first exception the unsafe policy was asked for (complete)

```bash
scripts/gates/m29-complete.sh
```

`M27` produced a policy that lets unsafe in on evidence and deliberately shipped
no exception. `M28` did the safe half of the same procedure and found 7 to 15%
in one line of `Cargo.toml`. This is the unsafe half, and the answer is no.

The candidate was the query cache's recency slab, which is the best one in the
workspace: `Slot` is a private newtype with no public constructor, issued only
by `claim`, so its in-bounds property is a type invariant rather than a runtime
fact. A rotating hit touches five of them.

| benchmark | safe | `get_unchecked` | |
| --- | --- | --- | --- |
| `cache_hit_rotating` | 1,801 | 1,812 | +0.6% |
| `cache_hit` | 1,462 | 1,469 | +0.5% |
| `cache_put` | 3,753 | 3,745 | -0.2% |

**Nothing moved**, and two of the three came out slower with the checks removed,
which is what noise looks like. LLVM had already elided them. The procedure's
second step exists to catch exactly this before anything is written, and it is
the step that is easiest to skip.

The policy worked on its first use and that is worth recording apart from the
result. It did not have to be argued with: the condition it imposes is a
benchmark in `product/perf/baseline.json` that justifies the exception, and
there was no number to name.

Full detail in
[run-2026-08-04-unchecked-slab.md](perf/run-2026-08-04-unchecked-slab.md),
including what this does **not** say: four of the procedure's five patterns are
untested here, and none of them has a candidate with a number behind it yet.

Completion condition: `scripts/gates/m29-complete.sh`.

## M30: the same procedure, applied to every crate (complete)

```bash
scripts/gates/m30-complete.sh
```

`M29` ran the unsafe procedure on one candidate in one crate, found nothing, and
said so in its own closing text: four of the five patterns were untested and
none had a number behind it. This ran the procedure across the workspace, and
started where the procedure says to start, which is a measurement.

The measurement is a callgrind run at N iterations subtracted from one at 2N,
taken per function instead of per binary, so fixture construction cancels the
same way `scripts/bench.sh` already cancels it for the total. Without the
subtraction the cache profile reports its own 4,096-entry fixture as the loop.

| path | before | after | |
| --- | --- | --- | --- |
| `route_point_select` | 6,444 | 3,716 | -42% |
| `route_update` | 6,717 | 3,969 | -41% |
| `route_begin` | 1,294 | 1,165 | -10% |
| `acquire_and_release` | 443 | 278 | -37% |
| `held_read` | 18,669 | 2,263 | -88% |
| `cache_put` | 3,770 | 3,544 | -6% |
| `invalidate_a_tenants_entries` | 86,088 | 83,378 | -3% |
| `decode_query` | 390 | 390 | unchanged, and see below |

**Not one line of unsafe was written, and not one of the four costs was a bounds
check.** That is the first thing worth saying about a procedure whose
best-known pattern is unchecked indexing. Three were work that did not need
doing at all, and the fourth is the one place unsafe would pay.

- **`M30.1`** The router lexed every statement twice to read one word.
  `begins_read_only_transaction` ran beside `classify` over the same text and
  read every token to answer a question the first word fixes.
- **`M30.2`** Every word was compared against every keyword. About 290
  `eq_ignore_ascii_case` calls per point select, to find no match. A filter over
  length, first letter and last letter, computed at compile time from the lists
  themselves, cut `matches_any` by 52% without touching an entry or a comment.
- **`M30.3`** The pool hashed an integer it made up with `SipHash`, at 39% of
  `acquire_and_release`. The rule that fell out is the durable part: who chooses
  the key decides its hasher, and the four peer-chosen keys that keep
  `RandomState` are named in `pgprox_core::hash`.
- **`M30.4`** A 16 KiB memset before every held read, justified by a comment
  saying unsafe was needed and forbidden. `M27` made the second half false; the
  first half was never true, because `AsyncReadExt::read_buf` was imported in
  that file the whole time.
- **`M30.5`** Two thirds of the query decode is a UTF-8 validation that only
  `from_utf8_unchecked` removes, in the first crate on `ADR 0026`'s closed list.
  Refused, correctly, by a script that did not have to be argued with.

## What the sweep says about the tool

Three of the five findings were about something written down that had stopped
being true, and none of them would have been found by reading the code alone.

`M30.4` is the clearest. The comment gave a reason, the reason named a rule, and
the rule had been changed three milestones earlier by this same line of work.
Nobody reread the comments that cited the policy when the policy moved. The
safe API it claimed did not exist had been in the file's imports since it was
written.

`M30.6` is the same shape in the measurement rather than the code. `M28.2` wrote
down that a benchmark under a thousand instructions measures `scripts/bench.sh`
as much as the code, and put it in a roadmap section, where it read as a note
about one milestone. Nobody held the existing benchmarks against it, and
`serves` was 141. It now sits in `standards/testing.md`, with all three
instances named.

## What it says about the procedure

The procedure's own second and third steps did the work. Step two is "look at
the assembly, LLVM has probably already elided the check", which is what `M29`
found. Step three is "try the safe construct first", which is every one of
`M30.1` through `M30.4`. Step four, the unsafe patterns themselves, has now been
reached twice in this repo and turned back twice: once because the bounds checks
were already gone, and once because the crate is closed and should be.

## What was not swept, and why

Six crates were profiled: `pgprox-proto`, `pgprox-core`, `pgprox-route`,
`pgprox-pool`, `pgprox-cache` and `pgprox-session`. The rest were read for the
five patterns and hold none of them. `pgprox-auth`'s only index loop is a 32-byte
xor over a fixed-size array after a length check, and `bin/pgprox`'s is a
three-element array indexed by a match. Neither is on a per-statement path and
neither carries a bounds check LLVM cannot see through.

`pgprox-session` had no benchmark at all before this, which is why `M30.4` went
unseen for twenty-nine milestones: the crates with benchmarks were the sans-I/O
ones, and this one reads a socket. It has one now, over `tokio::io::duplex`, and
it is in the gated baseline.

Completion condition: `scripts/gates/m30-complete.sh`.

## M31: the comments at M30's optimisation sites (complete)

```bash
scripts/gates/m31-complete.sh
```

The procedure `M30` followed sets a bar for the comment at an optimisation site,
and it is the same bar whether or not the optimisation is unsafe. A good comment
answers which invariant, established where, and why it is still true at this
line. Three ways of failing it are named, and `M30` left one of each in the
tree: a comment that refers the reader elsewhere, a comment that describes the
operation rather than justifying it, and a claim with no executable form beside
it.

The last is the one worth stating on its own. A `debug_assert!` is the same
claim written so a test can fail on it, and `M30` wrote none at any of its five
sites. Two of them are one line each and run in every test in the workspace that
reaches those paths.

The two that had one available now carry it. Breaking the filter's finals mask
reports `the filter rejected "SELECT", which is on the list` from the ordinary
classification tests rather than from a test written for it, because the
assertion sits on every call a debug build makes. Dropping the read's reserve
reports `a held read has room for 16377 bytes, not 16384`, where the symptom was
otherwise invisible: the frame still assembled and the buffer still stayed
small, and only the syscall count changed.

The third kind of site is worth naming because it will recur. Some claims have
no executable form at all. `begins_read_only_transaction` stops after the first
word because no later word can change the answer, and that is a fact about the
grammar rather than about state the function can inspect. The comment says so,
and names the test that stands in for it, so a reader meets an argument rather
than a silence they have to interpret.

No code changed, which the gate checks by holding three of `M30`'s figures at
exactly what `M30` left them. A `debug_assert!` compiles out of a release build,
and a figure that moved would mean one of these had reached one.

Completion condition: `scripts/gates/m31-complete.sh`.

## M32: the comparison against pgbouncer and pgcat (complete)

```bash
scripts/compare.sh [connections]
```

Every claim this project makes about pooling is against its own baseline.
`product/perf` holds twenty run documents and not one of them has another
pooler in it, so "absorbs the ratio" means measured against pgprox at a
different connection count, not against what an operator would otherwise
deploy.

Four arms on one machine, one workload, one Postgres: direct, `pgbouncer`,
`pgcat`, `pgprox`. Two questions worth the machinery. Does per-connection
memory beat a C pooler tuned for it since 2007, and what does holding a
fleet-wide cap cost in acquire latency next to a pooler that does not
coordinate at all.

The comparison is deliberately narrowed to pooling. `pgprox` runs with its
query cache off and one upstream rather than three, because a run that let it
answer from cache or spread reads over replicas would be measuring features the
other two do not have and calling it a pooling result.

What cannot be equalised is reported rather than hidden. `pgprox` resolves a
grant through a sidecar on every connect where the other two read a static
password file, so the ramp is not the same work in each arm and is reported
apart from the steady state.

| arm | tx | p50 µs | upstream | peak RSS |
| --- | --- | --- | --- | --- |
| direct | 6,400 | 2,322 | 60 | - |
| pgprox | 17,447 | 4,813 | **50** | 13.9 MB |
| pgbouncer | 17,685 | 3,763 | 60 | **4.5 MB** |
| pgcat | 17,638 | 3,031 | 60 | 26.1 MB |

**Throughput is the same in all three**, and one round said otherwise. A single
run had pgprox 4.7% ahead; three rounds put the medians within 1.4% with ranges
that overlap almost completely. That 4.7% would have been published.

**pgbouncer uses a third of pgprox's memory and a sixth of pgcat's**, and it is
the only one of the three that is flat. That is the first of the two questions
this milestone existed for and the answer is no.

**pgprox multiplexed onto fifty upstream connections where both others used
sixty**, in every round. Same work, same cap, 17% fewer connections held on the
database. It is the one number where pgprox is ahead of both, and it is the
thing pgprox is for.

It is also a millisecond slower per transaction at the median, and the most
consistent of the three by a wide margin.

Full detail, including everything the run does not say, in
[run-2026-08-05-pgbouncer-pgcat.md](perf/run-2026-08-05-pgbouncer-pgcat.md).

## What the milestone cost to make honest

Four of its eight tasks were not the comparison. They were the comparison
becoming trustworthy, and each came from running the thing rather than reading
it.

`M32.1` and `M32.6`: the load client could not authenticate to either pooler. It
spoke trust and cleartext, pgbouncer wanted SCRAM and pgcat wanted MD5, and
there was nothing to compare until it spoke both.

`M32.8`: three runs disagreed by a factor of two, because each rebuilt the
machine underneath itself, and the memory figure was a difference from a
baseline that had stopped being one.

And three near-misses that would each have produced a confident wrong number.
pgcat failing every named `Parse` for want of one configuration line, which
reads exactly like the failure `ADR 0011` predicts. pgbouncer appearing to hold
111 connections against a cap of 60, which was the previous arm's unreaped pool.
A per-connection memory figure of 61 bytes, which was round two measured against
round one's peak.

The pattern is worth keeping: every one was caught by a number being too good or
too bad to believe, and none by a test.

Completion condition: `scripts/gates/m32-complete.sh`.

## M33: what pgbouncer and pgcat do differently (complete)

```bash
scripts/gates/m33-complete.sh
```

`M32` found pgbouncer serving 200 connections in 4.5 MB where pgprox needed 13.9
and pgcat 26.1. A number is not a reason, and both are open source, so this
reads them instead of guessing.

**Three things pgprox found by profiling, pgbouncer has had since 2007.** A
cursor rather than a `drain`, which pgprox added after a profile put 19% of its
time in `memmove` and pgbouncer spells `done_pos`/`parse_pos`. Buffers borrowed
on read and returned when quiet, which is `ADR 0008` and `sbuf_try_resync`.
Reading into uninitialised capacity, which is `M30.4` and an `init_func` that
resets three cursors and not the buffer. The convergence is the finding: these
are not tricks, they are what a pooler needs.

**The obvious answer was wrong, and the experiment is the milestone.** pgbouncer's
buffer is 4 KiB and pgprox's is 16, so pgprox should be paying four times over.
Setting both constants to 4 KiB and re-running moved the per-connection cost by
205 bytes out of 22,835, and `held_read` not at all. A 12 KiB reduction per
buffer showing up as 40 kB across 200 connections means at most three buffers
were outstanding at once.

That is the buffer slab working exactly as `ADR 0008` intended, so completely
that buffer size is not a memory lever. It also means the cheap optimisation is
not available, and a version of the document recommending it existed before the
run.

**What is not the lever anywhere:** pgcat has zero `unsafe` and is the heaviest
of the three by five times. Neither project contains any SIMD. pgbouncer aligns
to `sizeof(long)` and pads nothing. The lever in all three is how much is
allocated per connection and for how long.

**The question it leaves:** 22,835 bytes per connection, of which 5,048 is the
session future and none is the buffers. The other seventeen kilobytes are
unaccounted for, and the cheapest candidate to rule out is glibc's per-thread
allocator arenas, which a one-thread runtime would separate from a real
per-connection cost.

Full detail in
[run-2026-08-05-what-the-others-do.md](perf/run-2026-08-05-what-the-others-do.md).

Completion condition: `scripts/gates/m33-complete.sh`.

## M34: the seventeen kilobytes that are not the buffers (complete)

```bash
scripts/arena.sh 200
```

`M33` accounted for 5,048 of 22,835 bytes per connection, ruled out the read and
write buffers by experiment, and named glibc's per-thread allocator arenas as
the cheapest remaining candidate. This runs it, in three arms, because a
single-threaded runtime moves the thread count and the arena count at once and
answers neither question on its own.

| arm | workers | arenas | B/conn median | range |
| --- | --- | --- | --- | --- |
| baseline | 20 | 160 | 25,272 | 24,104-25,948 |
| one-arena | 20 | 1 | 22,855 | 22,282-**26,972** |
| one-thread | 1 | 160 | **17,797** | 16,179-19,886 |

**The arenas are not it.** `one-arena` covers baseline's range entirely, and in
the third run capping them at one produced a *higher* figure than leaving them
at 160. The first run read as a clean 12% and was written up as one, which is
`M32.8`'s lesson repeating three milestones after it was learned.

**The worker count is, and not because of the arenas.** `one-thread` is 30%
below baseline with no overlap at either end. Cutting the workers moved the
number and cutting the arenas did not, so what the workers cost is tokio's own
per-worker state rather than glibc's.

**This corrects `M33`.** About 30% of its per-connection figure was per-worker
cost divided by a connection count it has nothing to do with, on a twenty-core
machine. Any per-connection memory figure from this project should state its
worker count beside it.

**And it leaves roughly 12.7 KB unexplained**, now known not to be the buffers,
not the arenas, and not per-worker. The next thing to weigh is what
`tokio::spawn` allocates, which is the future plus a header plus what the
allocator rounds up to, against the 5,048 bytes `size_of_val` reports.

Full detail in
[run-2026-08-05-arenas.md](perf/run-2026-08-05-arenas.md).

Completion condition: `scripts/gates/m34-complete.sh`.

## M35: per-connection memory is a curve, not a number (complete)

```bash
scripts/gates/m35-complete.sh
```

`M34` closed naming the spawned task as the next thing to weigh. Weighing it
would have been wrong, because the figure it would be weighed against is not a
per-connection figure.

A cost per connection and a fixed cost look identical at one connection count.
`M32`, `M33` and `M34` each measured at 200 and divided by 200, so each reported
a slope plus an intercept and called the sum a per-connection cost. Measured at
100, 200 and 400, **the reported figure falls as connections rise in every arm**,
which a real per-connection cost cannot do.

Fitting two terms does not rescue it. The same fit against two datasets gives
pgprox 13,253 and 28,259 bytes per connection, from one machine on one day,
because the curve is not a line: pgbouncer's slope between 200 and 400 is a
twentieth of its slope between 100 and 200. Memory in a pooler that pools
buffers is fixed, plus per-connection resident state, plus concurrently-active
times buffer size, and the third term saturates. That is `M33`'s buffer result
seen from the other side.

**Withdrawn:** `M33`'s 22,835 bytes per connection, `M34`'s 17,797, `M34`'s
"12.7 KB unexplained", and a 5.9x slope ratio computed here before the
non-linearity was noticed.

**Standing:** `M32`'s ordering at every count. `M32`'s ratio at its own
operating point, now stated as a point rather than a property. `M33`'s buffer
result and `M34`'s arena result, both of which compared two arms at one
connection count, which is the comparison that stays sound.

**The rule that falls out:** a per-connection memory figure must state the
connection count it was taken at. Comparing arms at one count is sound;
dividing by the count and calling the result bytes per connection flatters
whichever arm has the smaller fixed cost.

The measurement that would answer the original question is the one term that
does not saturate: what an open, quiet connection holds.
`product/perf/workload-idle.yaml` exists for it, and this run failed to get it
by giving a twenty-five second window to a workload whose think time starts at
thirty seconds, so no connection sent anything and the load client correctly
called that an error rather than a result.

Full detail in
[run-2026-08-05-per-connection-is-not-a-number.md](perf/run-2026-08-05-per-connection-is-not-a-number.md).

Completion condition: `scripts/gates/m35-complete.sh`.

## M36: what an open, quiet connection costs (complete)

```bash
WORKLOAD=product/perf/workload-idle.yaml COMPARE_DURATION=90 scripts/compare.sh 800
```

`M35` found per-connection memory under the reference workload to be a curve
rather than a number, and named the one term that does not saturate as the thing
worth measuring. This measures it. Under the idle workload the upstream
connection count during the run was 1 to 3 against a cap of 60, so the buffer
term is out of the picture and what is left is what a quiet connection holds.

Absolute peak resident memory, serving N idle connections:

| arm | 200 | 400 | 800 |
| --- | --- | --- | --- |
| pgprox | 11.8 MB | 17.2 MB | 20.8 MB |
| pgbouncer | **4.2 MB** | **4.9 MB** | **4.1 MB** |
| pgcat | 19.6 MB | 25.9 MB | 40.2 MB |

**pgbouncer's idle connection costs nothing this experiment can measure.**
Quadrupling the count moved it from 4.2 MB to 4.1, a slope of -150 bytes, which
means zero with noise on top. **pgcat's is 36 KB and linear**, which is the two
8 KiB buffers per client its source holds forever. **pgprox needs 20.8 MB where
pgbouncer needs 4.1 for the same 800 idle connections**, measured rather than
extrapolated.

**This is the first thing in the comparison that speaks to the mission.**
`scripts/scale.sh` states the target as under 500 MB at 100,000 connections.

`M38` corrects what this milestone then did with that. It extrapolated its
200-to-800 slope to 1.47 GB at a hundred thousand, and
`run-2026-07-28-100k-hold.md` had already measured that point: **5,726 bytes
each and 546 MB**, nine per cent over the target rather than three times it.
The extrapolation used a slope from where `M35` had just established the fixed
cost still dominates, which is `M35`'s finding unapplied one milestone later.

**What the session holds** is now partly accounted: 5,048 bytes of session
future, measured and guarded by a test. Not the buffers (`M33`), not the arenas
(`M34`), not the statement map. That leaves roughly 10 KB per idle connection
against a 5 KB future, and the next thing to weigh is what `tokio::spawn`
allocates, which no test in this repo has ever measured.

Full detail in
[run-2026-08-05-idle-connection-cost.md](perf/run-2026-08-05-idle-connection-cost.md).

Completion condition: `scripts/gates/m36-complete.sh`.

## M37: what a spawned task costs beyond its future (complete)

```bash
cargo test -p pgprox --test spawn -- --nocapture
```

`M36` accounted for 5,048 bytes of an idle connection's roughly 15 KB and left
one candidate unweighed: the difference between a future and a task.
`tokio::spawn` heap-allocates the future alongside a header holding the waker,
the state, the join handle's channel and the scheduler's links.

| future bytes | held per task | overhead |
| --- | --- | --- |
| 88 | 256 | 168 |
| 1,048 | 1,152 | 104 |
| 4,120 | 4,248 | **128** |
| 16,408 | 16,536 | **128** |

**The overhead is a constant 128 bytes**, flat across a future that grows
sixteenfold. A session task therefore requests about 5,176 bytes, and that
accounts for 128 of `M36`'s ten kilobytes.

**Every candidate any milestone has named is now eliminated.** The buffers
(`M33`, 205 bytes), the allocator's arenas (`M34`, nothing measurable),
per-worker state (`M35`, it is fixed cost rather than per connection), the
prepared statement map (`M36`, under 1 KB), and now the spawn header. Roughly
10 KB per idle connection has no named suspect left.

**The obvious one has never been considered.** `dhat` measures bytes requested;
`M36` measured resident memory, which is a high-water mark. Those differ by
everything the allocator was asked for, freed, and did not return, and glibc
returns very little. A connection's setup resolves a grant, parses a JWT, runs
SCRAM, reads server parameters and takes a pool connection, and all of it
allocates and frees. `malloc_trim(0)` after a ramp is one call and would say
whether the ten kilobytes is state a connection holds or memory the allocator is
sitting on. Everything since `M34` assumed the first without checking.

Full detail in
[run-2026-08-05-spawn-cost.md](perf/run-2026-08-05-spawn-cost.md).

Completion condition: `scripts/gates/m37-complete.sh`.

## M38: the extrapolation M36 did not need to make (complete)

```bash
scripts/gates/m38-complete.sh
```

`M36` fitted a slope over 200 to 800 connections and reported 1.47 GB at a
hundred thousand. `run-2026-07-28-100k-hold.md` had measured that point
directly: **5,726 bytes each and 546 MB**, nine per cent over the target rather
than three times it.

The way it went wrong is `M35`'s own lesson unapplied one milestone later.
`M35` established that the figure is a fixed cost plus a variable term and that
the curve bends; `M36` then took a slope from where the fixed cost still
dominates and extended it by a factor of 167.

The extrapolation is marked superseded rather than deleted, because how it went
wrong is worth more than the figure, and the gate checks both the measured
number and that the wrong one is still visible beside it.

## M39: documentation for people who are not this repo (complete)

```bash
scripts/gates/m39-complete.sh
```

Every document here was written for whoever is building it. There was no README
and no `docs/`, so a person arriving from GitHub could not learn what pgprox is,
run it, configure it, or read what it has been measured at without reading a
roadmap written for somebody else.

Six pages under `docs/`, one Diátaxis quadrant each, plus a README that routes
to them: a tutorial that brings the stack up, a configuration reference, an
operations guide, an architecture explanation and a performance page carrying
only figures traceable to a run document.

The honesty is the requirement rather than the tone. The pages say pgprox has
never been deployed, that the 100k figure is one machine, that every latency
number is loopback and therefore a floor, and that pgbouncer uses a third of its
memory. A doc site reading like a shipped product would be `M13`'s defect on the
outside of the repo.

Three things a gate can check about prose, and it checks them: every relative
link resolves, the configuration reference names every field the document
schema has, and it names every argument the parser accepts. The last one failed
on its first run and the check was wrong rather than the document: `--peer` is
repeatable, so its field is `peers` and deriving the flag from the field name
invented an argument that does not exist. It now reads the parser's own match
arms.

Completion condition: `scripts/gates/m39-complete.sh`.

## M40: a control that only worked where nothing else was broken (complete)

```bash
scripts/gates/m40-complete.sh
```

`tests/gates/negative.sh` has four cases for `m1f-complete.sh`'s scope ADRs, and
all four read that script's exit code. It ends by running the workspace checks,
the coverage gate and `scripts/conformance.sh`, and the last wants a Postgres in
a container.

Without one, the positive case reported **"accepts two ADRs that decided: the
check failed on a good artefact"**. The ADRs were fine. The message named them
anyway and sent a reader to the wrong file.

The three negative cases were worse. `expect_fail` passes on any non-zero exit,
so on the same machine they passed with the ADR check deleted entirely. On a
fully provisioned machine they worked, which is why this was invisible exactly
where it is checked.

All four now assert the message their own check produces. Deleting the ADR check
makes all four fail, which is the test that says they mean something, and the
suite passes with no containers up.

`M12`'s subject was gates that pass by matching a filename. This is the same
defect one level up: a control that passes by matching an exit code somebody
else set.

Completion condition: `scripts/gates/m40-complete.sh`.

## M41: the docs become a site (complete)

```bash
npm ci && npm run build      # published by .github/workflows/docs.yml
```

`M39` wrote six pages that render when somebody browses the repository and are
not a site: no generator, no navigation, no search, nothing that deploys.

Astro Starlight, chosen over `mdBook` and Jekyll by the person who owns the
repo. The trade is written down rather than glossed: it brings a Node toolchain
and 313 packages into a repo that had neither, and `cargo-deny` sees none of
them. The first install arrived with two high-severity advisories, in `astro`
and in `sharp`; the site now runs on the patched versions and without `sharp`
at all, which a docs site with no images does not need. `npm audit` reports
none.

**The Markdown did not move.** Starlight normally reads `src/content/docs/`,
and `src/content.config.ts` points its collection at `docs/` instead, so the
same files serve this site and anybody arriving through GitHub. Two audiences,
one source, no copy that can drift.

That only works because of `src/rewrite-links.mjs`. The pages link the way
GitHub needs, `configuration.md` for a sibling and `../product/perf/x.md` for a
file outside the docs, and neither resolves once the pages are routes. The
first build produced `href="getting-started.md"` in the body of a page whose
sidebar link was correct, which is the shape of bug that looks fine until
somebody clicks. A rehype plugin rewrites both at build time: siblings become
routes, escapes become GitHub URLs, and the source stays correct for the copy
more people read first.

Completion condition: `scripts/gates/m41-complete.sh`.

## M42: the site's toolchain leaves the repository root (complete)

```bash
scripts/gates/m41-complete.sh
```

`M41` put `package.json`, a lockfile, `astro.config.mjs`, `src/` and
`node_modules` at the root of what is otherwise a Rust workspace, where each
read as a top-level concern of the project and none of them was.

They now live in `docsite/`. The split is the useful part: **`docs/` is the
product and `docsite/` is how it is built.** The pages stay where somebody
browsing the repository finds them, and the site's collection points one level
up at them, so there is still one source and no copy that can drift.

The paths that had to follow: the workflow runs `npm ci` and `npm run build`
from `docsite/`, takes its artefact from `docsite/dist`, and tells the Node
cache where the lockfile went, which otherwise caches nothing without saying so.

Completion condition: `scripts/gates/m41-complete.sh`, which now reads the moved
paths.

## M43: what it does, and what one request touches (complete)

```bash
scripts/gates/m43-complete.sh
```

`M39` gave a reader orientation, a tutorial, a reference, an operations guide
and two explanations, and left two questions it could not answer.

**[Features and limits](../../features.md)** is the first: pooling modes
and why statement pooling is absent, the seven things that pin a session,
replica routing with the LSN watermark rule drawn out, the query cache and the
five reasons a statement is never cached, protocol and auth support, and an
explicit list of what is not supported.

That page keeps two lists apart on purpose. **Not supported** is decided
against: sharding, statement pooling, md5, `SCRAM-SHA-256-PLUS`, decoding
replication, read-your-writes from the cache, a cache shared across nodes.
**Not built yet** has no decision against it and simply has not been done. A
reader deciding whether to adopt this needs to know which list a missing thing
is on.

**[Request flow](../../request-flow.md)** is the second: one client frame
from admission to the connection going back to the pool, naming the component at
each step. Establishing a connection, then the loop, then the three things that
run alongside it and change what it does. It ends with a table from symptom to
the step that produces it.

The gate reads the pin reasons out of the enum rather than trusting the page. A
variant added later with no row leaves a reader told a session pins for fewer
reasons than it does, which is `M13`'s subject in a page rather than a standard.

Completion condition: `scripts/gates/m43-complete.sh`.

## M44: the pages a review asks for (complete)

```bash
scripts/gates/m44-complete.sh
```

`M39` and `M43` documented what the proxy does and what one request touches.
Neither answers the questions somebody asks before they are allowed to run it.

Six pages, each for a reader who arrives with a different question.

**[Multitenancy](../../multitenancy.md)** is the one the rest hang off.
A client does not name its own tenant; the token service does. Every shared
structure has a key, and the key is the isolation. The section that matters most
is the last one: the boundary is the database credential, not the tenant name,
so two tenants mapped onto one role are one security domain to Postgres and the
proxy will not invent a boundary the credentials do not have.

**[Clustering and deployment](../../clustering.md)** covers the guaranteed
share, the gossip round, leases, and why a partition under-subscribes. Then the
StatefulSet, and why it is one: gossip addresses a peer by name and a
Deployment's pods get a new name every restart.

**[Admin and management](../../admin.md)** is every `SHOW`, every endpoint,
and every operation that changes a node's state, plus the sentence the API's own
module comment makes and no page did: the admin port has no authentication of
its own.

**[Security](../../security.md)**, **[FIPS builds](../../fips.md)** and
**[Optimizations](../../optimizations.md)** are the remaining three. The
last of those carries the negative results as prominently as the wins, because
an idea that has been measured and refused is worth more written down than
forgotten.

### What the gate found

`M39` documented `SHOW MEM`. The parser has a test rejecting it by name, and it
is the failure that wastes the most time: an operator types it during an
incident and gets an error. Four milestones of documentation work went past it
because nothing compared the page against the enum.

So the gate reads eight lists from the code. `SHOW` in both directions, the
admin API paths from the router declaration, the JWT algorithm allowlist, the
crates on the closed unsafe list, the cache key's component count, the quoted
benchmark figures against `product/perf/baseline.json`, the cluster defaults,
and the FIPS image stage and provider string.

Two of those are worth naming. The optimization figures are checked against the
committed baseline, so rebaselining a hot path makes updating the page part of
the act rather than something to notice later. And the takeover wait is derived
in the check the same way `effective_lease` derives it, so moving the doubt
window fails two rows rather than one.

The closed unsafe list is read from `scripts/check-unsafe.sh` rather than from
every crate carrying the attribute. `pgprox-cache` forbids unsafe too and is
deliberately not on the list, having been the one candidate the policy was asked
about and refused in `M29`.

### The link nobody could see was broken

`M44.1`: every page's edit link pointed at a branch called `docs`.

`M41` put the collection's source in `docs/`, `M42` moved the toolchain into
`docsite/`, and neither noticed that the paths joining them now begin `../`.
Starlight resolves an edit URL with `new URL(path, baseUrl)`, and that `../`
gets spent on the base rather than on the path, so `edit/main/` became `edit/`
and the branch was gone.

It renders correctly in every dev server, the link points at GitHub either way,
and nothing local follows it. It is wrong only in the built output, which is the
only place anyone clicks it. Two settings in two files, correct apart and wrong
together, which is the shape a check has to hold rather than a reading.

Completion condition: `scripts/gates/m44-complete.sh`.

## M45: one directory for the pages and the thing that builds them (complete)

```bash
scripts/gates/m41-complete.sh
scripts/gates/m44-complete.sh
```

`M42` moved the Node toolchain out of the repository root. It was right about
the root and wrong about the split.

Two directories a level apart, one holding the Markdown and one holding the five
files that read it, put a `../` in every path between them. `M44.1` is what that
cost: the content collection's `../docs` and the edit link's base were each
correct alone and wrong together, and the only place it showed was the built
output.

So `docsite/` is gone and its five files sit beside the pages. The root is still
a Rust project, which is what `M42` actually required, and `docs/` meets that as
well as `docsite/` did.

The cost is four entries of noise: somebody browsing `docs/` on GitHub now sees
`package.json`, a lockfile, `astro.config.mjs` and `src/` among the pages. That
is the trade, and it was taken rather than glossed.

Two things followed the move rather than being restated.

**The collection's glob stopped recursing.** `**/*.md` against a directory that
now contains `node_modules` would walk a dependency tree. Every page is a direct
child, which is also what `m41-complete.sh` checks for a title and a navigation
entry, so the two agree by construction.

**The edit-link check became a resolver.** `M44.1` matched the shape of the
settings, which only worked for the arrangement it was written against. It now
resolves a real page's URL the way a browser would and asserts where it lands,
so this move exercised it rather than breaking it. Verified against the pair
that was actually broken: it reports `edit/docs/admin.md` and names both halves.

Completion condition: the two gates above, which now read the moved paths.

## M46: the licence three files have claimed and none granted (complete)

```bash
scripts/check-drift.sh
```

Three files named a licence. None of them was one.

`Cargo.toml` declares `license = "Apache-2.0"`, inherited by every crate. The
README had a Licence section reading "Apache-2.0." and nothing else. An SPDX
identifier is a label, and Apache-2.0 section 4(a) requires that anyone the work
is distributed to receives a copy of the terms. There was no copy. GitHub's
detector reads the file as well, so the repository rendered as unlicensed to
anybody who arrived at it.

`LICENSE` now holds the text verbatim, taken from the system's canonical copy
rather than retyped, with the appendix's copyright line filled in and nothing
else touched. That is checkable rather than asserted: substituting the line back
produces the canonical file byte for byte.

The check went into `check-drift.sh` rather than a milestone gate, because this
is the same failure that script already exists for. One canonical answer, in the
workspace manifest, and every other place that names a licence has to agree with
it. A tree with no `LICENSE` fails, and so does one where the README or the
site's package manifest drifted off it.

No `NOTICE` file. Apache-2.0 requires one only where the work already has one,
and adding it later means every downstream copy carries it from then on.

Completion condition: `scripts/check-drift.sh`.

## M47: the links nothing was checking (complete)

```bash
scripts/check-links.sh
```

`check-drift.sh` checks the links out of `AGENTS.md`. Nothing checked the other
hundred and forty, and fifteen of them were broken.

All fifteen were in `product/roadmap.md` and all broken the same way: one `../`
too many, as if the file sat a directory deeper than it does. Every link it
makes to a run document, and every link it makes to a page. They accumulated
across several milestones, three of them written this week, and each one looked
correct in isolation.

That is what makes it worth a check rather than a proofread. A typo gets
noticed. A consistent misreading of where a file sits produces dozens of wrong
links that all look the same, and nobody finds out until they click one.

It resolves the path and not the fragment. Reproducing the site generator's
heading slugs would be a second implementation of them, and two implementations
of a slug is two chances to disagree. The fragment half is worth doing and is
not done, which is written in the script rather than left to be discovered.

It runs on every commit rather than only when Markdown changes, because a link
breaks when the file it points at moves, and that commit need not touch a
single `.md`.

Completion condition: `scripts/check-links.sh`.

## M48: the design record moves under docs/ (complete)

```bash
scripts/check-links.sh
scripts/check-drift.sh
```

`product/`, `standards/` and `specs/` sat at the repository root beside
`crates/`, `bin/` and `deploy/`, which reads as though they were part of what
ships. They are not. They are how this repository is worked in, and they now
live under `docs/internal/`, which puts every word written for a reader in one
place and leaves the root a Rust project.

**Visible rather than hidden, and that was the decision.** `.sdd/` was the
proposal. `rg` and `fd` skip hidden directories by default, so every future
search of the design record would quietly return nothing, and this repository's
whole arrangement is that an agent is sent to go and read those files. Hidden
directories here hold tool state: `.github`, `.cargo`, `.claude`, `.agents`.
These are content.

The site needed no exclusion. `M45` had already made the collection's glob
top-level only, so `docs/internal/` is invisible to it by construction, and the
build still produces the same fifteen pages.

### What the move found

**A check that narrowed to one link in eighteen without failing.**
`check-drift.sh` matched `\]\((standards|product|\.agents)/` against
`AGENTS.md`. After the move that pattern found one link and reported that every
path AGENTS.md links to exists, having looked at one of them.

That is worse than a broken check, because it reads as a passing one. It is
replaced rather than repaired: `check-links.sh` from `M47` already resolves
every relative link in every Markdown file, so what sits there now is the thing
that check cannot see. Every standard in the directory is named by the index. A
standard that exists and is linked from nowhere is a rule every session must
follow and no session is pointed at.

**240 MB of Node modules in the Docker build context.** `M45` moved the site's
toolchain under `docs/`, and `.dockerignore` named `target/`,
`target-coverage/`, `reference/` and `.git/`. Nothing failed. Every image build
since has simply been slower, which is exactly why nobody noticed.

**Four hundred and sixty references, and the ones a regex missed.** The bulk
rewrite used a negative lookbehind that excluded `-`, so it skipped every
`${PGPROX_BACKLOG:-product/backlog.md}` default in the gate scripts, and
excluded `/`, so it skipped every
`include_str!("../../../product/perf/workload.yaml")` the workspace compiles
against. The first was caught by `check-wired.sh` failing loudly; the second by
the compiler. Neither was caught by reading the diff.

Completion condition: the two checks above, plus every milestone gate.

## M49: one place for what a run leaves behind (complete)

```bash
scripts/check-links.sh
```

`reference/` held 30 MB of upstream proxies cloned for protocol comparison,
gitignored and untracked, sitting at the root beside the code. It is now
`.tmp/reference/`, and `/.tmp` is the one entry covering anything somebody
needs to put somewhere and nobody needs to keep.

### The part that could not be done, and why

The intent was to fold eight `.gitignore` patterns into that one entry. It
cannot be done, and finding that out was most of the milestone.

Every one of the eight guards a tool that writes to the working directory and
gives this repository no say in it. `perf record`, `cargo flamegraph`,
`cargo mutants`, `cargo llvm-cov` and a dhat binary all default to CWD. A
redirect exists for each, and it would have to be typed by the person running
the command, which is the one place it will be forgotten. The pattern is
cheaper than the discipline.

That was checked rather than assumed, because a *script* can be told where to
write and would then need no pattern at all. None of the eight comes from one:
`scripts/bench.sh` writes callgrind output to a mktemp directory,
`scripts/mutants.sh` writes under `target/`, `scripts/profile.sh` writes to
`target/profile`, and the dhat budgets build their profiler with `.testing()`,
which writes no file. Every line is a guard against a hand-run.

So the eight stay, grouped under one comment that says what they are for. The
root is one entry shorter rather than eight, and the reason the other seven did
not move is in the file where somebody will read it before trying again.

Completion condition: `scripts/check-links.sh`, plus the checks that were
already there.

## M50: a README in every crate (complete)

```bash
scripts/check-readmes.sh
```

Every crate carried an `AGENTS.md` and none carried a `README.md`.

Those are different documents for different readers. `AGENTS.md` is rules and
hazards for somebody about to change the crate. A README is orientation for
somebody who has just landed in the directory and wants to know what this is
and how it connects. GitHub renders one at the foot of a directory listing,
which is where a person arrives, and it rendered nothing for any of the sixteen.

Each says what the crate owns, what it is built on, what is built on it, and
the one constraint that shapes it. That last part varies by crate deliberately:
`pgprox-pool` gets the release rule, `pgprox-cluster` gets the invariant,
`pgprox-testkit` gets the bug it exists to hold. A uniform page per crate would
be a template rather than a document, and would read like one.

### What is checkable about a page of prose

The part a reader relies on and cannot verify: which other crates this one is
built on. That is a list in `Cargo.toml`.

`check-readmes.sh` compares the two in both directions, because they fail
differently. A dependency the manifest has and the README does not is a reader
with an incomplete picture. A crate the README names that does not exist is a
reader sent looking for something renamed or deleted, which wastes more of
their time.

It caught one on its first run. `bin/pgprox`'s README said it "composes every
crate in the workspace", which is true, unverifiable, and useless to somebody
trying to see the shape of the thing. It now names all twelve and says what
each is for.

### The crate map had stopped being true in two rows

Writing sixteen READMEs from the code meant reading the map beside them.

`pgprox-cluster` was credited with "SWIM gossip". ADR `0004` was renamed in
`M18.1` for exactly this reason, and says in as many words that no code ever
matched the SWIM description: no `foca` dependency, no `UdpSocket`, a failure
detector at three and ten seconds rather than sub-second. The ADR was corrected
and the table was not.

`pgprox-cache` was "trait stub until M9". M9 closed twenty-five milestones ago.

And the stated exception for `bin/pgload` named two of its four workspace
dependencies, having missed that measuring a TLS deployment means running a
real SCRAM exchange over a real client configuration.

None of the three is load-bearing on its own. Together they are the same shape
as everything else this week: a document that was right when written, describing
a thing that moved, with nothing comparing the two.

Completion condition: `scripts/check-readmes.sh`.

## M51: eighty scripts and no index (complete)

```bash
scripts/check-drift.sh
```

`scripts/` held eighty-two files. Twelve were named anywhere.

Forty-five of them are milestone gates, which is the shape that makes a
directory unreadable: more than half the files are things nobody needs to read,
and nothing says which half. `ls scripts/` sorted `m1f` between `m19` and
`m20`.

The gates now live in `scripts/gates/`, so the listing is thirty-seven entries
of things somebody might actually run, and
[`scripts/README.md`](../../../scripts/README.md) groups those thirty-seven by
what they are for and what they need. Checks are seconds and run on every
commit; measurement is minutes to hours and mostly wants Docker.

`release-check.sh` moved with them. It is `M8`'s completion condition and the
only gate not following the naming convention, so leaving it behind would have
made `scripts/gates/*.sh` mean "most of the gates", which is the kind of nearly
true rule that costs somebody an afternoon later.

### The index is checked

An index is worth exactly its completeness, and this one would rot the first
time somebody added a script in a hurry. `check-drift.sh` now fails on a script
the README does not name, with the gates exempt as a group rather than listed,
because listing forty-five frozen files is the original problem restated.

`AGENTS.md` stopped carrying its own list of eight. Two lists of the same thing
is one list that drifts, and the shorter one was already missing half the
checks.

### What did not need doing

Nothing was deleted. Every script in the directory is referenced by something,
which was worth checking before proposing a cleanup that removed files: the
instinct with eighty-two scripts is that some must be dead, and none was.

### The flake was a bug report

`concurrent_lookups_of_a_cold_key_make_one_call` failed once in a full-suite
run, then passed twenty isolated runs and three more full ones. A one-in-a-few
flake is the shape that gets rerun rather than read.

It was right. `resolve` reads the cache and then claims the key under two
separate locks, so a caller descheduled between them finds that the previous
leader stored and released in the gap, and becomes a second leader for a key
that is already cached. The comment above the claim said "two callers cannot
both decide they are first". That is what the code did not do, and three
separate documents sold the property: the crate's `AGENTS.md`, ADR `0003`'s
consequences, and the request-flow page.

The fix is one more look after taking the claim. It costs nothing on the hot
path, because a cache hit returns before the claim lock is ever touched, and the
extra read lands only where a network call was about to be made anyway. It is
extracted into its own method rather than left inline, because the branch is
only reachable through the race: as a method it has two direct tests, and as a
branch it would have had a coincidence.

### Mutation testing that answers before the merge

3,694 mutants, each a build plus a test run. Nightly, therefore, which means it
reports on Tuesday about a test weakened on Monday.

`MUTANTS_DIFF` narrows a run to the lines a diff touched **and** to the crates
that diff reached. The second half matters as much as the first: `--in-diff`
narrows which mutants are generated, not which crates are visited, and a crate
costs a baseline build whether or not the diff reached it. Sixteen baseline
builds to mutate five lines is the cost that would stop anybody running it on
the commit path.

Measured against the previous commit: two crates instead of sixteen, five
mutants instead of the crate's full set.

The nightly now shards four ways with `fail-fast: false`. It is not replaced by
the diff run and the documentation says so: a change can make a mutant
survivable in code it did not touch, and only the full run sees that.

Completion condition: `scripts/check-drift.sh`, which holds both the index and
the rule that every gate is wired into CI.

## M52: two failures from the CI replay, and what each turned out to be (complete)

```bash
scripts/check-coverage.sh
scripts/gates/m1f-complete.sh
```

A full replay of everything CI runs on a push gave 63 of 65. Both failures were
in the apparatus rather than the proxy, and they failed in opposite ways.

### The one that could not say why

`check-coverage.sh` reported "test run failed" for `pgprox-session` and
`pgprox`, the two crates whose tests are slowest and the only ones that bind
real sockets.

It did not reproduce. The same command passed clean, the same gate passed
clean, the exact CI sequence passed clean, and two concurrent coverage runs
passed clean. Ephemeral port exhaustion was the best hypothesis and was
measured and ruled out: 4,095 ports in the range and `TIME_WAIT` flat at 473
across a whole run.

There was nothing else to look at, because the only copy of which test failed
had gone to `/dev/null`.

That is the finding. An intermittent failure is the one kind that most needs
its evidence kept, and this gate was throwing it away. It now names the failing
tests and prints the path to the full log, which was verified by planting a
failing test and reading it back rather than by waiting for the flake.

**This does not fix the flake**, and the entry says so. It makes the next
occurrence diagnosable, which is all that can honestly be claimed about
something nobody has seen twice.

### The one that had a definite cause

`conformance.sh` started Postgres with `-P`. On Docker Desktop 29.6.2 under
WSL2 the daemon accepts that and allocates nothing: the container is `Up`,
`PublishAllPorts` is true, and `NetworkSettings.Ports` is `{"5432/tcp":[]}`.
`docker port` then prints "no public port '5432/tcp' published", which reads
like the container failed to start and is the opposite of what happened.

Characterised rather than assumed. Every dynamic publish allocates nothing and
every fixed publish works, on any image and any port, with 53 host ports held
and no exhaustion anywhere. Dynamic allocation specifically.

The suite now probes for a free port and asks for it by number. One socket
bind, and it works on both kinds of daemon. `M1F`'s two Postgres versions had
never actually run on this machine, and the gate had been reported as "needs
Docker" for as long as anybody had looked at it, which was wrong: Docker was
there and working.

Completion condition: the two commands above.

## M53: the scripts read as stale, and two of them were (complete)

```bash
scripts/check-drift.sh
```

A survey of all eighty-two scripts for staleness, and the naming that made the
gates directory hard to read.

### What was not stale

Most of it, and that is worth recording because the instinct with forty-four
near-identical filenames is that some must have rotted.

No check names a path that has gone. No gate is vacuous: every one asserts
something, none skips, and the small gaps between `ok` calls in source and `ok`
lines printed are if/else branches where only one side fires. No gate holds a
claim a later milestone corrected, which was checked specifically against `M36`
and `M38`, where `M38` corrected `M36`'s extrapolation and its gate requires the
superseded figure to be *marked* rather than deleted.

`check-unsafe.sh` reads vacuous, reporting "no crate holds unsafe, so none needs
Miri yet". That is correct rather than stale: it is a guard that arms the moment
somebody writes unsafe, and its messages say which case they are in.

### The naming, and why it was not fixed by renaming

Forty-four gates for fifty-six milestones, and nothing said why. There is no
`m1-complete.sh` because `M1` is held by `conformance.sh`, no `m2` because
`M2`'s condition is a `cargo nextest` invocation, no `m8` because it is
`release-check.sh`, and none for `M46` through `M52` because their conditions
are ordinary checks. A missing filename looked like a missing gate.

`ls` compounds it: `m1f` sorts between `m19` and `m20`, `m3` after `m29`.

Zero-padding to `m00`, `m01f`, `m44` would sort correctly and cost about two
hundred and fifty reference updates across CI, globs, the roadmap and the
backlog. That buys a readable `ls` for a directory whose own README now says
nobody should be reading it, and whose prose lives in the roadmap. The index
answers the question people actually have, so the index is what was written.

### The two that were stale

**`cargo fmt` ran twice on every push.** Tier 1 listed `check-fmt.sh` and
`check-crate.sh`, and the second runs the identical check as its first step.
The script stays, because it is the pre-commit hook's fmt entry and
`m0-complete.sh` calls it; that second caller is also why dropping the CI line
loses no coverage if fmt ever leaves `check-crate.sh`.

**`check-wired.sh` announced "everything written to be used is used"** and reads
a watchlist of eight symbols. Its own body was already honest and argues against
a general scanner: nearly every `pub` item in a library legitimately has no
in-tree caller, so a scan would be mostly false positives. Only the summary
oversold it, which is the same shape as the `SHOW MEM` row and the regex that
matched one link in eighteen. The summary is corrected and the argument against
a scanner now sits in the header, where somebody would look before writing one.

Completion condition: `scripts/check-drift.sh`, which holds both indexes.

## M85: eighty-seven milestones and no way to jump to one (complete)

```bash
scripts/check-drift.sh
```

`backlog.md` reached 9,162 lines and 87 milestone headings with no way to
reach one except scrolling or grepping for a task ID you already knew. That is
the same shape `M51` found in `scripts/` and `M12` found in `scripts/gates/`:
a directory, or here a document, big enough that a missing entry looks the
same as a present one until somebody counts.

Archiving old milestones out of the file was the first idea and the wrong one.
`check-commit-msg.sh` enforces the one-task-one-commit rule on every commit by
grepping the exact task ID out of `docs/internal/product/backlog.md`, and
roughly fifteen milestone gates read specific task text from that same path.
Moving content out would have meant updating all of them to search two places
instead of one, which is a change to enforcement scripts this project treats
as load-bearing, not a readability pass. So nothing moved: the file holds
every task at the line its commit put it there, unchanged.

What shipped is a table of contents, one link per milestone heading, and a
`check-drift.sh` rule that fails if a heading exists with no matching line —
the same shape `M51`'s script index and `M12`'s gate index already enforce,
because an index that can silently go stale is the defect this repo keeps
finding in itself.

Completion condition: `scripts/check-drift.sh`, which now reads the table of
contents against the headings it is supposed to list.

## M86: the status table nobody kept adding rows to (complete)

```bash
scripts/check-drift.sh
```

The status table above stopped at `M29`. Every milestone from `M30` on has a
real section and, for `M30` through `M53`, has had one the whole time; nobody
had gone back to add its row. `M54` through `M84` are further behind still:
they have tasks and commit references in `backlog.md` and no roadmap section
at all.

**The table's own checker never saw the gap, because it only reads the
table.** `check-drift.sh`'s "a milestone in the status table can be checked"
rule, `M18.3`'s answer to `M16` and `M17` closing with nothing to run, walks
the table's rows and requires each to name a section with a real command. A
milestone with a section and no row is invisible to it — not exempted,
un-considered. Fifty-six milestones sat outside a rule written to guarantee
every milestone has a way to be checked.

**Backfilling the table is what found two of them failing that rule.** `M35`
and `M42` each had a section and a closing sentence naming their real
completion condition — `scripts/gates/m35-complete.sh` and
`scripts/gates/m41-complete.sh` — but no fenced command block, which is what
the rule actually reads. Both scripts exist and both were already correct;
the milestones were never checked because they were never in the table, the
same shape `M40` found in a different control and `M12` before that. Fixed by
adding the block each section's own prose already named, not by writing a new
one.

Rows added for `M30` through `M53` and for `M85`. `M54` through `M84` stay out
of the table on purpose, with a note saying so, rather than getting rows
pointing at sections that do not exist yet — a row with nothing behind it is
`M18.3`'s defect with the direction reversed. Backfilling their prose is real
work, sized for its own milestone rather than folded into this one.

The stray sentence about `M-1` and `M0` being hard barriers, which had been
sitting between two table rows and breaking the table at that point for
anyone whose renderer takes it literally, moved to prose below the table
where it was clearly meant to read.

Completion condition: `scripts/check-drift.sh`, which now finds every
milestone this table names and would fail again if either gap reopened.

## M87: the mutants nobody has swept since M22

```bash
scripts/gates/m22-complete.sh
```

`M22` closed with the baseline current against sixteen crates. Sixty-two
milestones have landed since, and `M22.4`'s own gate reports each crate's
staleness rather than blocking on it, which is right for a fast local check
and means nobody had looked. Reusing `m22-complete.sh` here rather than a new
script is deliberate: the check this milestone needs already exists, and
writing a second one that reads the same `Sweeps:` markers would be the
two-implementations mistake this project argues against everywhere else.

**Where it stands.** All sixteen crates and binaries are freshly swept,
with every survivor either killed by a new test or accepted in
`docs/internal/product/mutants-baseline.txt` with a written reason.
`pgprox-tls`, `pgprox-auth`, `pgprox` and `pgprox-pool` each found a real
gap; the rest were clean against the baseline outright. Detail below.

**`M87.0`, found by the sweep rather than by reading.** A mutant of
`Lexer::advance` in `pgprox-core` does not fail a test; it takes down the
machine testing it. `next()` returns `Some(token)` for the same unconsumed
character forever when `advance` stops shrinking `self.rest`, and a caller
that collects the iterator — every real one does — grows a `Vec` at whatever
rate the CPU can loop. Thirty gigabytes free to swapping in under ten
seconds, on a mutant `cargo mutants`' own per-test timeout cannot catch
because nothing in the test ever fails or hangs in the way that timeout
watches for: it keeps producing output, just never finishing.

Nothing here is a defect in the shipped code — `cargo nextest run -p
pgprox-core` passes its real 241 tests in 0.14s with memory flat, and
`advance` is correct. The gap is upstream of the code: eleven call sites
across `next` and `skip_trivia` assume `advance` consumes input and nothing
checked that it did, the same shape `M22.5` already found once in the same
function for a different pair of primitives. Fixed with one
`debug_assert!` after the match, comparing `self.rest`'s length before and
after, which guards every arm at once rather than one more assert per call
site — and was reproduced twice under a monitored, single-mutant run before
being trusted as the cause, specifically so a real fix would not be mistaken
for a flaky machine.

**What this means for how the rest of the sweep runs.** A mutant that grows
memory without failing or hanging a test is invisible to the tooling's own
safety net, so the remaining nine crates are swept in small shards under
active memory monitoring rather than as one long unattended run, and any
survivor of this shape gets the same fix — a stated invariant upstream of
every caller — rather than a special case.

**`M87.1` is a second, independent bound rather than the fix.** `nextest`'s
`[profile.mutants]` was capped at `test-threads = 4` before `M87.0` found the
actual cause, on the theory that `nextest`'s own one-process-per-test
parallelism was compounding `MUTANTS_JOBS`. `M87.0`'s mutant crashed the
machine at `MUTANTS_JOBS=1` with `test-threads` never the limiting factor, so
that theory was incomplete rather than wrong. The cap stays, because the two
knobs are orthogonal and a large suite fanning out to twenty processes per
mutant is unwanted pressure with or without a memory-growing mutant in the
run.

**`M87.2` found the same shape once more, in the one branch `M87.0` had not
guarded.** `trim_leading_space` replaced with the fixed literal `"xyzzy"`
does not just fail to shrink `self.rest`; called on an empty `rest`, it
*grows* it back to five bytes, so a caller's loop that had just emptied
`rest` gets it refilled and tokenises the same bytes forever. `M87.0`'s fix
guarded the two branches that call `advance`, on the theory that `advance`
not shrinking `rest` was the whole shape of the risk; it was one instance of
a broader one, that any branch able to replace `rest` needs the same check.
Fixed the same way, one branch over: `debug_assert!(trimmed.len() <
self.rest.len())` before the assignment. `next()`'s own invariant from
`M87.0` did not catch this, because each individual call still nets a
shrink — the growth happens between calls, when `skip_trivia` refills what
the previous call just emptied.

**`M87.3` closed the first batch out.** `Sweeps:` markers updated for the
seven crates fully swept by that point — `pgprox-proto`, `pgprox-route`,
`pgprox-cache`, `pgprox-session`, `pgprox-cluster`, `pgprox-pool` and
`pgprox-core` — plus `pgprox-testkit` and `pgprox-config`, swept clean
earlier in the session while diagnosing the crash before `M87.0` had a name.
Nine crates and binaries remained: `pgprox-admin`, `pgprox-auth`,
`pgprox-observe`, `pgprox-load`, `pgprox-tls`, `pgprox`, and `pgload`.

**`M87.4` found a survivor rather than a hang.** `pgprox-tls`'s sweep turned
up a mutant of a different shape than `M87.0`–`M87.2`: `CertReloader::resolve`
replaced with a body that returns `None` refuses every TLS handshake, and
nothing caught it. Every existing test in the crate read `serving()`, an
accessor that never goes through `resolve`, so a crate can be otherwise
well-tested and still miss the one function rustls itself calls. Fixed by
adding the handshake the crate had never actually run: a real
`rustls::ServerConnection` and `rustls::ClientConnection`, pumped by hand,
using `rustls`'s own synchronous API so no new dependency or runtime was
needed.

**`M87.5` found the same shape once more, in `pgprox-auth`'s `scram.rs`.**
Every free function was individually tested and `ClientExchange`, the
stateful wrapper around them, was not — eleven live mutants across
`client_first`, `client_final` and `verify`, fixed with one end-to-end test
that plays both sides of a real exchange, including a forged server
signature that must be rejected.

**`M87.6` found a third case in the same sweep, in `cache.rs`'s
`Entries::sweep`.** It had never been driven past its capacity trigger with a
clock advanced far enough to actually expire an entry; fixed with a test that
does, which kills two of the three live mutants at its guard. The third, `>`
replaced by `>=`, is accepted in the baseline: the two programs disagree only
when a real clock reads the exact expiry instant, which nothing outside the
module can manufacture without injecting the instant itself, the same
argument already accepted for `Drain<'_>::settled` and
`pgload::one_connection`.

**`M87.7` closed `pgprox-auth` out.** `Sweeps:` marker updated now that both
gaps are fixed. Five crates and binaries remain: `pgprox-admin`,
`pgprox-observe`, `pgprox-load`, `pgprox` and `pgload`.

**`M87.8` swept `pgprox-observe` clean.** 62 mutants, 55 caught, 7 unviable,
none missed or timed out.

**`M87.9` swept `pgprox-admin` clean.** 152 mutants, 74 caught, 78 unviable,
none missed or timed out.

**`M87.10` swept `pgprox-load`.** 218 mutants, 203 caught, 7 unviable, 8
surviving — all eight already carried in the baseline from `M14.43`, six
distinct keys covering boundary and unreachable-fallback equivalences argued
out at the time. No new finding.

**`M87.11` swept `pgload`.** 135 mutants, 90 caught, 40 unviable, 5
surviving, all five matching accepted keys. Two other accepted keys came
back "now caught" in this run; both are documented timing-dependent
mutants, the same class the sibling `run|` entry already describes as
flipping between missed and caught across runs of an unchanged tree, so
neither was removed on the strength of one run. One crate remains:
`pgprox`, the largest.

**`M87.12` is `pgprox`'s sweep, running sharded.** 685 mutants at
`MUTANTS_JOBS=1` is too long for one supervised run, so it runs as eight
shards, each compared against the baseline only once every shard has
reported — a sharded comparison flags baseline entries outside that
shard's own subset as spuriously "now caught", the same artifact `M87.3`
documented for `pgprox-core`. Shard 2/8 found a real gap in
`primary_watch.rs`: `PrimaryWatches::is_empty` was asserted `false` after
`ensure_watched` and never `true` on a fresh registry, and nothing read its
`Debug` output, so a no-op `fmt` and a constant-`false` `is_empty` both
survived. Fixed with one test covering both the empty and non-empty cases
through both `is_empty()` and the `Debug` string.

**`M87.13` found four more, shards 3/8 and 4/8, all the log-only shape
`M17.4` named first.** `refresh`'s `moved` field, `primary_of`'s ambiguity
guard (entirely untested, including the one branch its own doc comment
argues for), `evict_unused`'s grace boundary (two existing tests bracket
it without ever reaching it, fixed with an exact-boundary test `FakeClock`
makes reachable on the nanosecond), and `hold_at_nothing`'s open-pool
filter together with a `ticker` log guard structurally identical to the
already-extracted `something_happened` and `peers_went_unanswered`. One
further survivor, `TICKS_PER_RELOAD`'s `/` replaced with `*`, is
equivalent while `TICK` is one second, and is accepted in the baseline
with the arithmetic written out.

**`M87.14` swept shards 5/8 through 7/8, and found one wiring bug outside
`bin/pgprox`'s own reach to test.** `App::build` drops `retry:
config.retry,` from the `PoolConfig` it hands `LivePool::new`, silently
falling back to "off". Nothing in `bin/pgprox` could see that: the pool's
own configuration was private, so a test could only see what was passed
in, not what a pool was actually built with. Fixed by adding
`LivePool::config`, a test-only accessor in `pgprox-pool` gated behind
its existing `test-fakes` feature, the same shape as `pgprox-tls`'s
`serving()` and `pgprox-core`'s `FakeClock` — introspection that exists
only because a wiring bug needed a seam nothing in production wants.

**`M87.15` found the sweep had missed a whole eighth of `pgprox`,
including its one remaining real gap.** `cargo mutants`'s `--shard` is
zero-indexed, so `1/8` through `8/8` covered the second through eighth
slices and `8/8` itself was rejected outright — an error this session read
as "nothing here" rather than "this shard never ran". Re-run as `0/8`.
Nine of ten survivors matched the baseline already; the tenth was
`SystemJitter`, drawing retry backoff jitter through the same system RNG
`SystemEntropy` already does two functions up, with none of `SystemEntropy`'s
three tests. Fixed the same way: a range check and a distinctness check
over sixty-four draws, the shapes that catch a non-deterministic source
mutated into a constant or scaled by orders of magnitude, which a
specific-value assertion cannot.

`pgprox` is now fully swept: all eight shards, every survivor either
fixed or already accounted for.

Fixing `SystemJitter` touched `pgprox-pool` only through a new test-only
accessor (`M87.14`), which left that crate one line ahead of its own
sweep. Re-swept rather than left stale: `LivePool::config` itself had no
test in the crate that owns it, only an indirect one through
`bin/pgprox`'s wiring test, and was replaceable with
`PoolConfig::default()` undetected. Fixed with a direct test using the
crate's own `pool_with_retry` fixture; the other 218 mutants were
unchanged since `M22`.

Every crate and binary in `scripts/mutants.sh`'s list has now been swept
at least once since `M22`, and every sweep's findings are either a real
fix or a written reason in the baseline.

Completion condition: `scripts/gates/m22-complete.sh` reporting every crate
current, with every survivor either killed by a test or accepted in
`docs/internal/product/mutants-baseline.txt` with a reason.

## M88: a second reading of every crate, and the eighteen things it found

```bash
scripts/gates/m88-complete.sh
```

`M24` was a reading of every crate against correctness, completeness, design,
performance and test quality, and found nine things mutation testing cannot:
missing logic rather than wrong logic. Sixty-four milestones have landed since,
`M87` swept every crate for mutants and found four real gaps, but nothing has
read the crates themselves again in the way `M24` did. This milestone is that
second reading, against the same five questions, over the whole workspace as
it now stands.

Eighteen findings, filed below in the order of what they cost rather than the
order they were found. The costliest is a resource leak on cancellation in
`pgprox-auth`'s singleflight grant resolution; the cheapest are documentation
and test-quality gaps that cost nothing at runtime but leave a gap in what the
suite would catch.

Completion condition: `scripts/gates/m88-complete.sh`, which runs a named test
for each finding and reads its exit status, per `M12.8` and on `M24`'s exact
template.

### Where it stands

Open. Findings land one per commit, each with a test that fails before the fix
and passes after.

**`M88.1`, the costliest.** `CachingResolver::resolve`'s singleflight claim was
released by a line at the end of the leader's turn and by a line inside
`recheck_before_calling`, both on the far side of the `.await` for the sidecar
call. `resolve` is a plain `async fn`, so a client that disconnects while that
call is outstanding drops the future running it, and a dropped future never
reaches either line. The claim then outlives the leader that took it, and
every follower parked on it — this one and every later caller for the same
key — waits forever on a broadcast channel nothing will ever send on again,
until the process restarts. Fixed with an `InflightGuard` whose `Drop` removes
the claim and wakes waiters on every exit from the leader's scope, cancellation
included, which is the one path a line placed after the `.await` cannot cover.

**`M88.2`.** `LeaseLedger`'s pool ceiling was read once, from `or_insert_with`,
which only runs the round a server's ledger is first created. `split_for`
recomputes the free pool from the current cap on every round after that, and
nothing wrote the new answer into a ledger that already existed. A cap raised
after a fleet's first gossip round left every node capped at the old, lower
ceiling, silently under-granting; a cap lowered left the ledger free to keep
leasing above it. Fixed with `LeaseLedger::set_pool`, called from `observe`
every round rather than only at construction. Safe to move either direction:
`available` and `grant` both compute headroom as `pool.saturating_sub(...)`,
so a pool dropped below what is already outstanding reads as no headroom
rather than underflowing, and nothing already granted is revoked.

**`M88.3`.** `pgprox-route`'s `parse_route_assignment` and `begins_transaction`
read SQL with `sql.split_whitespace()`, the exact second-scanner mistake this
crate carries a written rule against. Neither is comment-aware: a leading
comment before the statement, or between `SET` and the parameter name, becomes
the first "word", and the real token behind it is never reached. That is not a
corner case for this crate specifically — its own `hints` module documents a
leading `/* pgprox:replica */` comment as the supported way to send a
per-statement routing hint, so a client following that same convention in
front of a session-scoped `SET pgprox.route = ...` or an explicit `BEGIN` had
the assignment or the transaction go unrecognised, silently. A `BEGIN` missed
this way is the sharper of the two: the target is never fixed, and the second
statement of what the client believes is one transaction is free to land on a
different server, which this crate's own module comment says has no coherent
semantics. Fixed by reading both through `pgprox_core::sql::Lexer`, which
skips comments as trivia before it hands back a token; `begins_transaction`
stays inside its zero-allocation budget, since `Lexer::next` borrows rather
than allocating.

**`M88.4`, the same shape one crate over.** `pgprox-pool`'s `ParsedSet::parse`
and `deallocates_everything` skipped leading trivia once, with the shared
lexer, and then read every later word with `split_whitespace`, which is
comment-blind past that first skip. `SET /* c */ search_path = x` read `/*` as
the parameter name; `DISCARD /* c */ ALL` read it as the second word and never
reached `ALL`. Both silently missed what they were looking for rather than
erroring, on a `SET` that then pinned the session for the wrong reason and a
`DISCARD ALL` whose statement maps went uncleared. Fixed by reading every word
through the lexer, not only the first; a quoted parameter name is
unaffected and still correctly falls through to pinning, which is `M24.2`'s
answer and not this finding's to change.

**`M88.5`.** `pgprox-admin`'s `SHOW CLIENTS`, `SHOW SERVERS` and `SHOW STATS`
each reported a value the module's own doc comment argues against inventing.
`SHOW CLIENTS` wrote `client.tenant` into both the `user` and `database`
columns, which are neither the tenant ID nor each other — a grant's `user`
and `database` differ from the tenant and from each other the same way
`PoolKey`'s do, and `ClientView` does not carry either. `SHOW STATS`'s
`total_query_count` was `stats.transactions.to_string()`, the value already
sitting two columns earlier in `total_xact_count`: nothing in the workspace
counts queries, only transactions, and the copy read as "exactly one query
per transaction". `SHOW SERVERS` emitted one row per pool regardless of how
many connections it held, which is the one of the three fixed in full rather
than blanked: `PoolStats` already carries `active`/`idle` counts, so each pool
now expands into one row per connection it actually holds. The other two are
blanked rather than invented, following this same file's stated policy for
every other column pgprox has no real value for: a value that looks like the
others and is not one is worse than an empty column. Building a real query
counter, or threading the client's actual startup `user`/`database` through
the cluster-scoped `ClientView`, is a contract change this finding did not
take on.

**`M88.6`.** The exporter, not the counters, was where `bin/pgprox` went
wrong. `pgprox_client_conns` was never double-incremented; it was rendered
twice, as a state-only breakdown and a separate tenant-only breakdown, both
covering the same full client count under one metric name — so
`sum(pgprox_client_conns)` with no label filter read twice the real total.
The per-tenant slice itself was not the mistake: it is the allowlist-bounded
feature `pgprox-observe`'s own tests already argue for ("if a tenant label is
ever genuinely needed, it goes behind the allowlist"), and the fix keeps it,
merging the two marginal breakdowns into one joint `(state, tenant)`
breakdown with one sample per pair. `tenant` now sits in the registry as a
declared, bounded label — the allowlist plus `other`, cardinality 17 — rather
than a dimension the exporter invented on its own. Fixing this exposed a
second bug in `pgprox_upstream_conns`, emitted per pool with no `state`
label, folding active and idle into one number, and — where a server had more
than one pool — producing two samples sharing an identical label set, which
is not valid Prometheus exposition. Both are fixed the same way: sum
`active`/`idle` per server from `PoolStats`, already on the `Observatory`
contract, and emit one sample per `(server, state)` pair. Neither fix needed
a contract change; both bugs were entirely in how the exporter read data it
already had.
