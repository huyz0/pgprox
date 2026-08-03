# What a large row costs with many connections pulling one

`M23.1`. `M16.1` measured one 16 MiB `DataRow` on one connection in a unit
test: 16,777,216 bytes held on the path the proxy used, zero on the streaming
relay it did not. `M16`'s completion condition asks for something that
measurement cannot give, "the same 100k run with a result set large enough that
the difference would show", and the 100k half is blocked on three machines.

**The connection count is not what makes the difference visible. The row size
is.** `M7`'s 100k run used pgbench's rows, so a proxy that held every row entire
would have produced the same numbers. A pair of runs at one connection count,
differing in nothing but the statements, answers the memory question on the
machine that is here.

```bash
WORKLOAD=product/perf/workload.yaml       scripts/scale.sh 200 --local
WORKLOAD=product/perf/workload-large.yaml scripts/scale.sh 200 --local
```

## The number

Two connection counts, same tenants, same transaction shapes, same think time,
same churn. `scripts/m23-complete.sh` checks that the two documents differ in
their statements and nowhere else, so a difference between the runs has one
cause.

| connections | `workload.yaml` | `workload-large.yaml` | difference |
| --- | --- | --- | --- |
| 200 | 26,112 bytes/conn | 34,693 bytes/conn | +8,581 |
| 600 | 17,674 bytes/conn | 17,271 bytes/conn | **-403** |

A relay that held each row entire would show +1,048,576.

**The second pair is the one that answers the question, and it corrects the
first.** With only the run at 200 this document said a megabyte of result costs
8,581 more bytes per connection. It does not. At 600 the difference is negative,
which is to say the two runs differ by less than the measurement's own
variability, and a cost that disappears when you look harder was never a cost.
What 8,581 measured was fixed overhead landing differently across two runs at a
count where fixed overhead still dominates.

That distinction is the whole reason for a second pair. One pair cannot tell a
per-connection cost from a constant, and the two have opposite meanings here: a
per-connection cost that grows with the count is something accumulating, which
is exactly what streaming exists to prevent.

So the finding is stronger than the one pair suggested. At 600 sessions each
pulling a megabyte through the real proxy, a real pool and a real Postgres,
**there is no measurable per-connection cost to the row being a megabyte
instead of a few bytes.**

## What this does not say

**Not the 100k target.** 17,271 bytes extrapolates to 1,647 MB at 100k against
a 500 MB target, and the extrapolation is worthless here: the reference
workload extrapolates to 1,685 MB on this machine, where `M7` measured 546 MB
on three. A one-node local stack sharing twenty cores with its own load client
is not the deployment shape.

The per-connection constant is dominated by fixed cost until the count is large
enough to amortise it, which is visible in the numbers themselves: the large
workload reported 69,959 bytes per connection at 50, 34,693 at 200 and 17,271
at 600, on the same machine against the same database. The comparison at one
count is the result. The absolute number is not portable, and the pair is,
because both halves carry the same fixed cost.

**The latency figures are Postgres.** 1,114,400us added p99 on the large run is
the database generating megabyte rows while `pgload` competes for the same
cores. Nothing about the proxy's added latency can be read from it, and it is
recorded only because a run that failed is not a run.

**The 100k half of `M16` stays blocked.** This narrows what is unknown rather
than closing it: what remains unmeasured is whether the same holds at a hundred
thousand connections on real network hardware, which is the part that needs
three machines.
