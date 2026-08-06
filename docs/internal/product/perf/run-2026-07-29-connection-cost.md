# The CPU is spent per connection, not per statement

Two runs, same statement rate, four times the connections.

```bash
scripts/scale.sh 500
WORKLOAD=product/perf/workload-slow.yaml scripts/scale.sh 2000
```

`workload-slow.yaml` is the reference workload with the think time multiplied
by four and nothing else changed, so 2,000 connections against it offer about
the work 500 connections offer against the reference. Holding the work still
while the connections move is the only way to tell the two costs apart: with
the reference workload at both counts, statements scale with connections and
both hypotheses predict the same cost per statement.

| | Run A | Run B |
| --- | --- | --- |
| Connections | 500 | 2,000 |
| Workload | reference | 4x think time |
| Statements | 31,856 | 36,317 |
| Transactions | 11,189, 0 errors | 12,907, 31 errors |
| **Proxy CPU** | **30,330 ms** | **134,020 ms** |
| CPU per statement | 952 us | 3,690 us |
| **CPU per connection per second** | **2.02 ms** | **2.23 ms** |

## The answer

The work was the same to within 14%. The connections went up four times. The
CPU went up 4.4 times.

Per statement the number is meaningless, moving from 952us to 3,690us for a
workload that did the same amount of it. Per connection per second it is
2.02ms and 2.23ms: the same number twice. **The proxy spends about 2ms of CPU
per connection per second, near enough regardless of what that connection
asks for.**

That is the answer M7.46 was looking for in the wrong place. The memmove was
never going to matter, because the cost does not scale with the thing the
memmove was in.

## What it means for the roadmap

One core holds about 500 of these connections. 100,000 would want 200 cores,
which no single node has.

The 100k hold run in `run-2026-07-28-100k-hold.md` is not a contradiction and
is the useful contrast: those connections were idle by construction, thinking
for ten to fifteen minutes before their first statement, and the node held them
in 546 MB without difficulty. So this is not a floor that every open connection
pays. It is a cost that appears once a fleet of connections is *active*, and
one connection's share of it does not depend much on how active that one
connection is.

That shape points away from the per-statement path and toward something that
runs per connection on a schedule, or toward contention between connections on
something shared. Candidates, none measured:

- The session registry, which every connection touches on every state change,
  and which the admin API and the metrics exporter also walk.
- Timer wheels. Each connection holds a login deadline, and the drain and shed
  paths select on watches; 2,000 tasks parked on timers is 2,000 wakeups
  whenever one fires.
- The tokio scheduler itself, which at 2,000 tasks per worker spends more of
  each poll deciding what to poll.

## What this run does not say

Both runs saturate the database: run B refused 31 transactions with `53300`,
and p99 at 2,000 connections is measured in seconds. So some of the CPU is
being spent on clients that are queued rather than served, and a run against a
database with headroom would separate those. This machine cannot provide one,
which is the same limit `M7` recorded.

The result survives that caveat because it is a comparison: both runs hit the
same database with the same offered load, and the only variable that moved was
the connection count.
