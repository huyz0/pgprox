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

200 connections each, same tenants, same transaction shapes, same think time,
same churn. `scripts/m23-complete.sh` checks that the two documents differ in
their statements and nowhere else, so a difference between the runs has one
cause.

| workload | row | rss per connection |
| --- | --- | --- |
| `workload.yaml` | pgbench's | 26,112 bytes |
| `workload-large.yaml` | 1 MiB | 34,693 bytes |

A megabyte of result costs **8,581 more bytes per connection, which is 0.82% of
the row**. A relay that held the row entire would cost 1,048,576 more.

That is `M16.1`'s finding under concurrency rather than in isolation: 200
sessions each pulling a megabyte through the real proxy, a real pool and a real
Postgres, and the bytes are not accumulating anywhere.

## What this does not say

**Not the 100k target.** 34,693 bytes extrapolates to 3,308 MB at 100k against
a 500 MB target, and the extrapolation is worthless here: the reference
workload extrapolates to 2,490 MB on this machine, where `M7` measured 546 MB
on three. A one-node local stack sharing twenty cores with its own load client
is not the deployment shape, and the per-connection constant is dominated by
fixed cost at these counts. The same large workload at 50 connections reported
69,959 bytes per connection, twice the figure at 200, for that reason.

The comparison is the result. The absolute number is not portable and the pair
is, because both halves carry the same fixed cost.

**The latency figures are Postgres.** 1,114,400us added p99 on the large run is
the database generating megabyte rows while `pgload` competes for the same
cores. Nothing about the proxy's added latency can be read from it, and it is
recorded only because a run that failed is not a run.

**The 100k half of `M16` stays blocked.** This narrows what is unknown rather
than closing it: what remains unmeasured is whether the same holds at a hundred
thousand connections on real network hardware, which is the part that needs
three machines.
