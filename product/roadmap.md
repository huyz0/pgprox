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
and `scripts/m9-complete.sh` passes either way because none of this changes what
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
scripts/m10-complete.sh
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
scripts/m11-complete.sh
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
scripts/m12-complete.sh
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
scripts/m13-complete.sh
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
scripts/m14-complete.sh
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
