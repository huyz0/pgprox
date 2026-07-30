# Does the cache still help when the database is saturated

`M10.9`. `M9.24` explained a regression with a claim about queueing, and no run
since has tested that claim. This is the run that can contradict it.

## What is being tested

`M9.24` measured the cache against the reference workload at five hundred
connections and found it 7.8% worse on the median. The explanation offered was:
the database is saturated, throughput is pinned by it, and a statement answered
instantly returns its client to the queue sooner, which lengthens the queue for
everybody else. A cache in front of a saturated resource moves work from the
front of the queue to the back of it.

`M10.5` then measured the read-heavy workload and found the cache 24.4% better
on the median. That is consistent with the explanation and is not a test of it,
because that workload saturates nothing: its median is 794us against the
reference workload's 526ms, and its throughput is three times higher. A run with
no queue cannot confirm a claim about queues.

So: raise the connection count on `workload-cached.yaml` until the proxy's
median stops tracking the direct baseline's, which is what saturation looks like
from outside, and take a matched pair there.

## The prediction, written before the numbers

Recorded first, and in a form that can fail, because a claim about queueing is
exactly the kind that absorbs any result if it is written down afterwards.

**The falsification that matters is throughput.** `M9.24` says throughput is
pinned by the database. If the cache on the saturated read-heavy workload raises
transactions completed, then it is not pinned, the cache made the fleet do more
work, and `M9.24`'s explanation is wrong at its root rather than at its edges.
`M9.24` and `M10.5` both found throughput unchanged to within a fraction of a
percent, so this is a real prediction and not a safe one.

**The median is the weaker test, and its two outcomes mean different things.**
This workload addresses 64% of statements at a 57% hit rate, so about 36% of all
statements never reach the database, against 3% on the reference workload.

- If the median regresses here too, at 36% of statements served, `M9.24` stands
  as stated: the redistribution effect dominates whatever load is removed.
- If the median still improves, `M9.24` is **incomplete rather than wrong**. The
  redistribution it describes would have to be bounded by how much load is
  actually removed, with 3% below the crossing point and 36% above it. Saying
  that afterwards would be moving the goalposts, which is the reason it is
  written here first.

**And the count itself is a finding.** `M10.5` ran at five hundred connections
and saturated nothing. If this workload needs several thousand connections to
saturate the same database that the reference workload saturates at five
hundred, that gap is a fact about how much work a read-heavy tenant costs, and
it is worth recording whatever the medians do.

## Method

The walk raises the count with the cache off, since finding the saturation point
is not a question about the cache:

```bash
WORKLOAD=product/perf/workload-cached.yaml scripts/scale.sh <n> --local
```

Saturation is read as the proxy's loaded median leaving the direct baseline's
behind: below it the two track, and above it the proxy's median grows with the
count while the direct client's does not.

## Where it saturates

One run per count, cache off, `workload-cached.yaml`, one local stack, sixty
upstream connections throughout.

| connections | proxy p50 | direct p50 | proxy p99 | transactions | upstream peak |
| --- | --- | --- | --- | --- | --- |
| 500 | 787us | 327us | 11,899us | 53,597 | 30 of 60 |
| 1,000 | 1,068us | 344us | 16,499us | 106,890 | 33 of 60 |
| 2,000 | **154,399us** | 318us | 443,499us | 136,798 | 50 of 60 |
| 3,000 | 378,499us | 322us | 799,199us | 135,705 | 50 of 60 |

Throughput is the clearest reading of it. Doubling from 500 to 1,000 doubles the
transactions, 53,597 to 106,890. Doubling again adds 28%, and adding another
thousand on top adds nothing at all: 136,798 then 135,705. The ceiling is about
4,550 transactions a second and the fleet reaches it between one and two
thousand connections.

The median says the same thing more loudly. It tracks the direct baseline within
a factor of three up to a thousand connections and then leaves it: 144 times
higher at two thousand, 353 times at three. The direct client is the control and
it never moves, 318us to 344us across every count, so the database is answering
a sixty-connection client as fast as it ever did while the queue in front of it
grows without bound.

**Two thousand is where the pair runs.** It is the first count in the walk where
throughput is at the ceiling and the median has left the baseline, and it is the
same regime `M9.24` measured: 154ms here against its 527ms.

That count is itself worth recording. The reference workload saturates this
database at five hundred connections and this one needs four times as many,
which is what a workload of 95% reads and single-statement transactions costs
against one with 30% writes and three statements a transaction.

## The numbers

Three matched pairs at two thousand connections, alternating off and on so a
drift in the machine lands in both arms, zero errors in all six.

```bash
                     WORKLOAD=product/perf/workload-cached.yaml scripts/scale.sh 2000 --local
LOCAL_QUERY_CACHE=5s WORKLOAD=product/perf/workload-cached.yaml scripts/scale.sh 2000 --local
```

| | cache off | cache on | |
| --- | --- | --- | --- |
| p50 | 161,132us | 189,266us | **17.5% worse** |
| p99 | 471,132us | 522,966us | 11.0% worse |
| transactions | 134,490 | 140,112 | 4.2% more |
| RSS per connection | 12,091 B | 12,773 B | 5.6% worse |
| direct baseline p50 | 320us | 321us | unchanged |
| upstream connections | 50 of 60 | 50 of 60 | unchanged |

Each figure is the mean of three runs. Only two of them are results:

- **p50 does not overlap.** Off {153,699, 157,499, 172,199}, on {178,799,
  186,199, 202,799}. The worst control beats the best cache-on run.
- **RSS per connection does not overlap.** Off {11,960, 11,993, 12,320}, on
  {12,513, 12,763, 13,043}.
- **Throughput overlaps**, off {130,710, 136,052, 136,707} against on {136,377,
  140,743, 143,216}, and by only 330 transactions, but it overlaps. It is not
  claimed. The single control that makes the gap look wide, 130,710, is the
  first run of the six and the coldest; drop it and the difference falls to
  2.7% and still overlaps.
- **p99 overlaps** as well, 513,099 against 494,499.

## The predictions, scored

**The falsification did not fire, and it was the one that mattered.** If the
cache had raised throughput, `M9.24`'s claim that the database pins it would
have been wrong at its root. The means point that way, 4.2%, and the sets
overlap, so by the standard `M9.24` itself applied to its own hop measurement
this is not a result and is not claimed as one. Throughput is still pinned as
far as this run can tell.

**The median regressed, so `M9.24` stands as stated rather than as incomplete.**
The prediction laid out two readings in advance: a regression means the
redistribution effect dominates however much load is removed, and an improvement
would have meant the effect is bounded by the share served. It regressed, at 36%
of statements answered from memory, which is twelve times the share `M9.24` was
measuring.

**And the size of it is the part worth keeping.** `M9.24` served 3% of
statements and cost 7.8% of the median. This serves 36% and costs 17.5%. Serving
more made it worse, not better, which is the mechanism's own signature: every
client handed an instant answer comes back sooner, and the queue it rejoins is
the one every other client is waiting in.

**The saturation point was found where it was looked for.** Between one and two
thousand connections, four times what the reference workload needs.

## What this settles

**A cache in front of a saturated database is a latency regression, and the
regression grows with the hit rate.** That is now measured twice on two
workloads, at 3% and 36% of statements served, and it is the same direction both
times.

**Whether the cache helps is not a property of the workload alone. It is a
property of the workload and the load.** The same document, `workload-cached.yaml`,
gives 24.4% better at five hundred connections and 17.5% worse at two thousand.
`M10.5` and this run differ in one number, and the feature changes sign between
them. ADR 0021 makes the cache off by default and opt-in per tenant; this says
the operator turning it on needs to know where their fleet sits against its
database, not just what their queries look like.

**And the throughput question is left open rather than answered.** Three runs
each is not enough to separate a 4% difference from noise. If it were real it
would matter, because it would mean the database is not the only thing pinning
this fleet. Settling it needs more pairs than this run has, and it is not the
question `M10.9` was filed to ask.
