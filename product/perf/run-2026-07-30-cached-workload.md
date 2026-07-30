# What the cache is worth on a workload it is for

`M10.5`. `M9.24` measured the cache against the reference workload and found it
7.8% worse on the median, and said in as many words that the result was about a
workload the feature is not for. This is the other measurement.

```bash
                     WORKLOAD=product/perf/workload-cached.yaml scripts/scale.sh 500 --local
LOCAL_QUERY_CACHE=5s WORKLOAD=product/perf/workload-cached.yaml scripts/scale.sh 500 --local
```

`workload-cached.yaml`, committed before this run: 95% reads against the
reference document's 70%, and 90% single-statement transactions against its
80/15/5 mix. Everything else is identical, so the difference between the two
runs is the difference between the two documents.

## The prediction, written before the numbers

Recorded first so the run can contradict it, which is the only reason to write
one down.

**The addressable share roughly triples, to something near 80% of statements.**
`M9.24` reached a lookup on 27% of statements, and the two things that kept it
there are both changed here: writes drop from 30% to 5%, and the share of
statements inside a `BEGIN` drops from roughly two thirds to about a third.

**The hit rate rises well above 26%.** That figure was a cache emptied roughly
every other lookup by the write rate. At a sixth of the writes, entries should
live closer to their 5s TTL than to the next `UPDATE`.

**And the median at five hundred connections improves rather than regresses.**
This is the prediction worth being wrong about. `M9.24`'s explanation is that
the database is saturated, that throughput is pinned by it, and that answering
3% of statements instantly returns those clients to the queue sooner and
lengthens it for everyone else. If that explanation is right, then serving a
much larger share should reduce what the database is actually asked to do by
enough to shorten the queue rather than redistribute it, and the median should
improve. If the median regresses here too, at four fifths of statements served,
then the queueing explanation is wrong and the cost is somewhere else, and that
is worth more than the improvement would have been.

## The numbers

Three matched pairs, alternating, zero errors in all six. The
five-hundred-connection phase.

| | cache off | cache on | |
| --- | --- | --- | --- |
| p50 | 794us | 600us | **24.4% better** |
| p99 | 11,132us | 10,866us | 2.4% better |
| added p50 at matched load | 430us | 25us | **94% better** |
| transactions | 53,608 | 53,655 | unchanged |
| statements | 80,681 | 80,768 | unchanged |
| CPU per statement | 59.3us | 49.7us | **16% better** |
| RSS per connection | 12,945 B | 13,393 B | 3.5% worse |
| upstream connections | 7 of 60 | 7 of 60 | unchanged |

Each figure is the mean of three runs. Three of the four columns that moved have
sets that do not overlap: the slowest cache-on p50, 609us, beats the fastest
control, 780us; the worst cache-on hop, 36us, beats the best control, 388us; and
the same holds for CPU per statement.

The cache, consistent to within half a percent across all three runs:

| | |
| --- | --- |
| hit rate | 57% of lookups |
| share of all statements | 27% |
| lookups | 51,447 against 80,771 statements, so 64% |
| invalidations | 14,090 per run |

## The prediction, scored

**Addressable share: predicted near 80%, actual 64%.** Wrong in the direction of
optimism, and the arithmetic says why. One transaction in ten is a
four-statement one wrapped in a `BEGIN` and a `COMMIT`, which is six statements
none of which may be cached. Per hundred transactions that is ninety cacheable
candidates against sixty that are not, so a tenth of the transactions eats a
third of the statements. `M9.18` is not a small tax.

**Hit rate: predicted well above 26%, actual 57%.** Right, and the mechanism is
the one predicted: at a sixth of the write rate the entries live closer to their
5s TTL than to the next `UPDATE`.

**The median: predicted to improve, and it improved by a quarter.** Right, and
this is the one that was worth being wrong about.

## What this does and does not settle

**It settles what the feature is worth to a tenant it is for.** A quarter off the
median, a sixth off the CPU per statement, and the hop through the proxy all but
gone at matched load: 25us against 430us, which is a cached statement not
crossing the network at all. Against `M9.24`'s 7.8% *worse* on the reference
workload, the two runs together say the thing ADR 0021 says, in numbers: this is
opt-in per tenant because whether it helps is a property of the tenant.

**It does not settle `M9.24`'s explanation for why the other run regressed.**
That explanation was queueing: the database is saturated, throughput is pinned by
it, and answering a statement instantly returns that client to the queue sooner.
This workload does not saturate anything. Its median is 794us against the
reference workload's 526,599us, three orders of magnitude apart, and its
throughput is three times higher on the same hardware. So the queue this
explanation is about does not exist here, and a run without a queue cannot
confirm or refute a claim about queues. It is consistent with it and no more.

Testing it directly needs this workload at a connection count that does saturate,
which is `M10.9`.

**And the memory is worse, by 3.5% per connection.** That is the entries
themselves plus the withheld sequences, it is the cost the feature is supposed to
have, and at 13,393 bytes per connection it is well inside what M7 measures
against.
