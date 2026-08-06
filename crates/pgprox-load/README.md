# pgprox-load

The reference workload, the sampler that turns it into work, and the run
report. No I/O.

## Why not pgbench

pgbench opens one connection per client and one thread per few clients, so
100,000 connections is not a matter of raising a flag.

It also cannot express the two things this proxy is judged on: a tenant mix
where most connections are idle most of the time, and connection churn.

So the workload lives in a committed document under
[`docs/internal/product/perf/`](../../docs/internal/product/perf/), this crate
turns it into a deterministic stream of work, and `bin/pgload` is the thin part
that puts that stream on a socket.

## Everything here is a pure function

Sampling, distribution and report are all functions of a workload and a seed.
That is what makes a run reproducible, and what makes this crate testable
without a database.

It is also why the workload documents are embedded at compile time rather than
read at runtime in tests: the committed document is what gets tested, so a
fixture invented in a test cannot drift away from the schema and prove only
that the invention parses.

## Where it sits

Depends on nothing in the workspace, not even `pgprox-core`. Used by
`bin/pgload`, and by `bin/pgprox` as a dev dependency only, where the gossip
allocation budget reads the cluster size the workload declares so it measures
at the membership the workload describes.

It is never a runtime dependency of the proxy. The proxy has no business
knowing about a load generator.
