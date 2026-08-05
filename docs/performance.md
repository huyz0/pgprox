---
title: Performance
description: "What has been measured, on what hardware, and what the numbers do not say."
---

Every figure here comes from a run document in
[`product/perf/`](../product/perf/), taken on one 20-core developer machine with
everything in containers. That machine is the largest caveat on this page and it
applies to all of it.

## Against the targets

| Target | Measured | |
| --- | --- | --- |
| Under 500 MB at 100,000 connections | **546 MB** | 9% over |
| Added p99 under 1ms | **348µs at p50, 4,300µs at p99**, at 1,000 connections | p50 inside, p99 four times over |
| Upstream cap held | Held, across three nodes | Asserted by the end-to-end suite |

The memory figure is 100,000 connections held open and mostly quiet, which is
the design point: a long tail of tenants whose connections exist and rarely do
anything. 5,726 bytes each, flat from the moment the ramp finished, no leak and
no churn.

The latency figure is 1,000 connections, not 100,000, and it is loopback. There
is no network between the client, the proxy and the database, so the added
latency is a floor rather than a measurement. A real deployment adds a network
hop and this number does not predict it.

## What one connection costs

| | |
| --- | --- |
| Idle, resident | 5,726 bytes |
| Session state alive across awaits | 5,048 bytes |
| Buffers while active | Borrowed from a pool, 16 KiB, returned when quiet |
| CPU while active | ~2ms per connection per second |

That CPU figure is the binding constraint rather than memory. One core holds
about 500 active connections, near enough regardless of what each asks for. Two
runs at the same statement rate and four times apart in connection count gave
2.02ms and 2.23ms.

The pair is the useful part: 100,000 idle connections cost 546 MB and almost no
CPU, while 2,000 active ones cost five cores. The 100k target is reachable for
connections that are mostly idle and is a different question for connections
that are mostly busy.

## Against other poolers

Three poolers, one machine, one workload, 200 connections against a
60-connection cap, three rounds. Full method and caveats in
[`run-2026-08-05-pgbouncer-pgcat.md`](../product/perf/run-2026-08-05-pgbouncer-pgcat.md).

| | transactions | p50 | upstream held | peak RSS |
| --- | --- | --- | --- | --- |
| pgprox | 17,447 | 4,813µs | **50** | 13.9 MB |
| pgbouncer 1.25.2 | 17,685 | 3,763µs | 60 | **4.5 MB** |
| pgcat 1.2.0 | 17,638 | 3,031µs | 60 | 26.1 MB |

**Throughput is the same in all three.** The medians sit within 1.4% of each
other with ranges that overlap almost completely. An earlier single run had
pgprox 4.7% ahead and that was noise.

**pgbouncer uses a third of pgprox's memory here**, and at 800 idle connections
the gap is five times: 4.1 MB against 20.8. Twenty years of being deployed
everywhere shows up where it should. pgprox does not beat it on memory and this
page is not going to pretend otherwise.

**pgprox multiplexed onto 50 upstream connections where both others used 60**,
in every round. Same work, same cap, 17% fewer connections held on the database.
That is the one number where it leads, and it is the thing it is for.

**pgprox is about a millisecond slower per transaction at the median**, and by
far the most consistent of the three: a 10% spread across rounds against
pgbouncer's 76% and pgcat's 513%.

None of this measures what pgprox exists for, which is a cap held across nodes.
The comparison points one arm at one node, so the coordination cost is
unmeasured and so is its benefit.

## What pinning costs

Transaction pooling degrades toward session pooling as sessions pin. Four arms
at rising pin rates, 150 clients each:

| Pinned sessions | Transactions against the unpinned arm |
| --- | --- |
| 0 | baseline |
| 60 | -9.5% |
| 60, heavier | -38.8% |
| 71 | -47.1% |

Every error in those arms was `53300 too many connections`, which is the cap
converting into refusals as the pool runs out of movable connections. Auto-pin
is a correctness feature that buys a working application, not free
multiplexing. The difference from a pooler without it is that pgprox tells you,
through `pgprox_pin_total{reason}`.

## What the query cache is worth

About 7% of median latency and of CPU per statement on the reference workload,
and 4% more transactions at saturation. It does not move the p99, because the
tail is the pool lock rather than the database.

It is off by default and off per tenant unless that tenant opts in. See
[`run-2026-07-29-cache.md`](../product/perf/run-2026-07-29-cache.md).

## Hot paths

Instruction counts per operation, measured under callgrind against a committed
baseline that CI enforces to 5%. These are not times; they are deterministic
and comparable across machines, which is why they gate.

| | instructions |
| --- | --- |
| Frame boundary scan | 20 |
| Backend message decode | 159 |
| Relay one frame | 167 |
| Decode a `Query` | 390 |
| Route a point select | 3,716 |
| Pool acquire and release | 278 |
| Query cache hit | 1,461 |
| Held socket read | 2,263 |

The declared hot paths also carry allocation budgets, most of them zero: the
steady-state relay loop, frame scanning, the pool release decision, warm-pool
acquire and the route decision all allocate nothing per operation.

## What has not been measured

**A 100,000-connection run that also serves.** Holding that many has been
demonstrated. Serving them needs load generators on their own machines, a
database that can absorb the offered load, and a real network. Until then the
latency story stops at 1,000 connections on loopback.

**Anything on real hardware.** One 20-core box, everything in containers, no
network between the client, the proxy and the database.

## How to reproduce

```bash
scripts/bench.sh                 # instruction counts against the baseline
scripts/scale.sh 1000            # RSS, added latency, upstream connections
scripts/compare.sh 200           # against pgbouncer and pgcat
scripts/e2e.sh                   # the stack and the properties it must hold
```

Each writes what it measured and refuses to report a number it did not get.
`scripts/bench.sh` fails CI on a 5% drift, and the baseline is rewritten only
as a deliberate act with a reason in the commit message.
