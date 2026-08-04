# pgprox against pgbouncer and pgcat

Date: 2026-08-05. `M32.4`.

```bash
COMPARE_ROUNDS=3 scripts/compare.sh 200
```

Every claim this project has made about pooling was against its own baseline.
`product/perf` held twenty run documents before this one and not one of them had
another pooler in it, so "absorbs the ratio" meant measured against pgprox at a
different connection count rather than against what an operator would otherwise
deploy.

## What was run

| | |
| --- | --- |
| Arms | direct, pgprox, pgbouncer 1.25.2, pgcat 1.2.0 |
| Clients | 200 per pooled arm, 60 direct |
| Duration | 30s per arm, three rounds, one stack |
| Workload | `product/perf/workload.yaml`, seed 1 |
| Upstream cap | 60, in all three, read from each arm's own file |
| Machine | 20 cores, 31 GB, WSL2, everything in containers on one box |

The direct arm runs at 60 rather than 200 because that is all a database with no
pooler in front of it would accept. It is the floor, not a competitor.

## The numbers

Median across three rounds, with the low and high beside it where they differ.

| arm | tx | errors | p50 µs | p99 µs | upstream | peak RSS | B/conn cold |
| --- | --- | --- | --- | --- | --- | --- | --- |
| direct | 6,400 (6,387-6,401) | 0 | 2,322 | 27,899 | 60 | - | - |
| pgprox | 17,447 (17,378-17,649) | 0 | 4,813 (4,717-5,199) | 688,199 | **50** | 13.9 MB (12.5-15.3) | 22,835 |
| pgbouncer | 17,685 (16,479-17,908) | 0 | 3,763 (2,650-4,647) | 735,299 | 60 | **4.5 MB (4.52-4.55)** | 7,618 |
| pgcat | 17,638 (15,223-17,893) | 0 (0-1) | 3,031 (2,690-16,499) | 732,499 | 60 | 26.1 MB (25.4-26.4) | 60,456 |

## What it says

**Throughput is the same in all three, and the first run said otherwise.** A
single round had pgprox 4.7% ahead of pgbouncer, and three rounds put the
medians within 1.4% of each other with ranges that overlap almost completely.
The 4.7% was noise and would have been published as a result. This is the whole
reason the run does three rounds.

**pgbouncer uses a third of pgprox's memory and a sixth of pgcat's.** 4.5 MB
serving 200 connections, against 13.9 and 26.1. It is also the only one of the
three that is flat: pgbouncer moved 32 KB across three rounds, pgprox grew from
12.5 MB to 15.3 MB, and pgcat sat around 26 MB. Twenty years of being deployed
everywhere shows up exactly where it should.

That answers the first of the two questions this run existed for, and the answer
is no. pgprox does not beat a C pooler tuned for memory since 2007, at this
connection count, on this workload.

**pgprox multiplexed onto fifty upstream connections where both others used
sixty.** Same work, same cap, 17% fewer connections held on the database, in
every round. That is the thing pgprox is for and it is the one number here where
it is ahead of both.

**pgprox is about a millisecond slower per transaction at the median.** 4,813 µs
against pgbouncer's 3,763 and pgcat's 3,031, and pgprox's range does not overlap
either median. It is also by far the most consistent of the three: a 10% spread
against pgbouncer's 76% and pgcat's 513%.

**p99 is the cap, not the pooler.** All three sit near 700 ms because 200 clients
are queueing for 60 upstream connections. Nothing in that number is about pooling
implementation.

## What it does not say

**It is not a scale result.** 200 connections is two orders of magnitude below
where pgprox is aimed, and the memory question gets more interesting, not less,
as connections rise. pgbouncer's per-connection cost is famously flat; pgprox's
buffer slab is designed to be sublinear because buffers are borrowed rather than
held. Neither claim is tested by this run. The interesting comparison is at
10,000 and it needs a machine that `M16` has not found.

**It does not measure what pgprox is for.** The one thing pgprox does that
neither of the others can is hold a cap across nodes that do not share memory.
This run points one arm at one node. The coordination cost is unmeasured, and so
is the benefit.

**It does not measure the connect path.** pgprox resolves a grant through a
sidecar on every connect where the other two read a static password file. The
run holds connections open for its duration, so that cost is in the ramp and the
ramp is not in these numbers.

**The features are turned off.** pgprox ran with its query cache off and one
upstream rather than three. A run that let it answer from cache or spread reads
over replicas would be measuring things the other two do not have.

**pgcat threw one error in one round.** Not investigated. One in roughly 50,000
transactions.

## What nearly went wrong

Three things, and all three would have produced a confident wrong number.

**pgcat failed every named `Parse`** until the pool was given
`prepared_statements_cache_size`. The failure reads exactly like the one ADR 0011
predicts for a pooler with no statement mapping, which is a finding this project
would have been pleased to report. It was a missing line of configuration. Both
of the other two arms now have their statement settings checked before a run
starts.

**pgbouncer appeared to hold 111 upstream connections against a cap of 60.**
Counting by role rather than by client address attributed the previous arm's
unreaped pool to whichever arm was running. 111 was pgbouncer's 60 plus pgprox's
51 still sitting there.

**The memory figure was a difference from a baseline that had stopped being
one.** `peak - idle` is a per-connection cost only while `idle` means idle, and
round two starts at round one's peak. pgbouncer read 7,618 bytes per connection
cold and 61 in the round after it.

## Worth a look

pgprox's resident memory grew every round, 12.5 to 13.9 to 15.3 MB, while the
other two were flat. It may be allocator retention and it may be something that
does not stop. Three rounds is not enough to tell which, and the difference
matters at a node meant to stay up for weeks.
