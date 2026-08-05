# Per-connection memory is not a number, and three milestones reported it as one

Date: 2026-08-05. `M35.1`. No code changed.

`M34` closed saying roughly 12.7 KB per connection was unexplained and named the
spawned task as the next thing to weigh. Weighing it would have been wrong,
because the figure it would be weighed against is not a per-connection figure.

## The defect

A cost per connection and a fixed cost look identical at one connection count.
They separate at two. `M32`, `M33` and `M34` each measured at 200 connections and
divided by 200, so each reported a slope plus an intercept and called the sum a
per-connection cost.

Measured at 100, 200 and 400, same load, same stack, delta being peak resident
memory under load minus idle before it:

| arm | 100 | 200 | 400 |
| --- | --- | --- | --- |
| pgprox | 33,873 | 24,924 | 18,544 |
| pgbouncer | 11,755 | 11,919 | 6,922 |
| pgcat | 84,459 | 69,919 | 53,299 |

**The reported figure falls as connections rise, in every arm.** A real
per-connection cost cannot do that. Every one of these tables was measuring a
fixed cost divided by whatever connection count the run happened to use.

## The correction that does not work either

The obvious fix is to fit two terms and report the slope. For pgprox that gives
13,253 bytes per connection over a 2,120 kB fixed cost, and the model predicts
all three points within 3%, which is convincing.

It is also wrong, and a second dataset is what showed it. The same fit run
against the four-arm comparison stack gives pgprox 28,259 bytes per connection.
Two estimates of the same quantity, from the same machine on the same day,
differing by more than two times.

The reason is in the pairwise slopes:

| arm | 100 to 200 | 200 to 400 |
| --- | --- | --- |
| pgprox | 20,111 | 31,519 |
| pgbouncer | 12,083 | **1,925** |
| pgcat | 56,607 | 36,803 |

If memory were linear in connections these would agree. They do not, in any arm,
and pgbouncer's second slope is a twentieth of its first.

**Memory in a pooler that pools buffers is not linear in connections.** It is
roughly fixed, plus per-connection resident state, plus concurrently-active
times buffer size. The third term saturates: at 400 connections on a workload
with think time, no more are active at once than at 200, so pgbouncer's curve
flattens. This is the same fact `M33` found from the other side, when quartering
pgprox's buffer moved its figure by 205 bytes.

A linear fit to that curve produces a number with a confident shape and no
meaning. The 5.9x pgprox-to-pgbouncer slope ratio computed from it is withdrawn.

## What stands

**`M32`'s ordering.** pgbouncer below pgprox below pgcat, at every connection
count measured. Nothing here disturbs that.

**`M32`'s ratio, at its own operating point.** "pgbouncer uses a third of
pgprox's memory" is true at 200 connections on the reference workload. It is not
a property of the two programs and does not carry to another point: at 400 the
same arithmetic gives 2.7x, at 100 it gives 2.9x.

**`M33`'s buffer result.** Quartering the read buffer changed nothing, and the
non-linearity above is why: buffers track concurrency, and concurrency was never
the binding term at these sizes.

**`M34`'s arena result.** Capping the arenas changed nothing. That was measured
as a ratio between two arms at one connection count, which is exactly the
comparison this document says is safe.

## What is withdrawn

`M33`'s "22,835 bytes per connection" and `M34`'s "17,797 at one worker" are not
per-connection costs. They are the value of a curve at one point on it.
`M34`'s "roughly 12.7 KB unexplained" is a remainder of a quantity that was not
what it was labelled.

`M34`'s finding that the worker count matters survives as a statement about
fixed cost, which is what it should have said: fewer workers, less fixed
overhead. Its slope did not move.

## The measurement that would answer the question

What matters at a hundred thousand connections is not this curve. It is the one
term that does not saturate: what an open, quiet connection holds when nothing
is active. `product/perf/workload-idle.yaml` exists for exactly that and this
run did not manage it.

It was attempted and failed for a reason worth writing down. The idle workload
sends a transaction every thirty seconds to five minutes, and the run was given
twenty-five seconds, so no connection sent anything and `bin/pgload` reported
"nothing was attempted" in all four arms. That is the load client behaving
correctly: a run with no transactions is an error rather than a report.

**A run against the idle workload needs a duration longer than its longest think
time.** Three hundred seconds is what `run-2026-07-28-100k.md` used. Nothing in
the tooling says so, which is why this was found by running it.

## What this changes about how to report

A per-connection memory figure from this project should state the connection
count it was taken at, because it is a point on a curve rather than a constant.
Comparing two arms at the same count is sound and is what `M32`, `M33` and `M34`
each actually did. Dividing by the count and presenting the result as bytes per
connection is what was wrong, and it is wrong in the direction of flattering
whichever arm has the smaller fixed cost.
