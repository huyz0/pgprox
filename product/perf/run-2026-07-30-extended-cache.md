# The extended protocol reaches the cache, and the cache now costs 8% of the median

`M9.24`. What `M9.17` through `M9.23` were worth, measured the way `M9.10`
measured the simple protocol, against the same reference workload.

It is 8% worse on the median at five hundred connections. Two thirds of that is
not the cache's bookkeeping but the hits themselves, which is the part worth
reading: against a saturated database, answering a statement instantly puts that
client back in the queue sooner and the other ninety-seven percent wait longer.

```bash
                                            scripts/scale.sh 500 --local  # control
LOCAL_QUERY_CACHE=5s                        scripts/scale.sh 500 --local  # test
LOCAL_QUERY_CACHE=5s LOCAL_QUERY_CACHE_BYTES=64B  ...                     # neither
```

Three matched pairs for the first two, alternating, on one machine of twenty
cores, `workload.yaml` version 3 throughout, zero errors in all six. The third
configuration is one run, and it exists to separate two costs that the pair
cannot: the tenant is opted in, so every lookup happens and every sequence is
held back, and the byte budget is 64 bytes, so nothing can ever be stored.

## The numbers

The five-hundred-connection phase: sixty upstream connections, five hundred
clients, and a queue.

| | off | held, never served | on |
| --- | --- | --- | --- |
| p50 | 526,599us | 531,699us | 567,666us |
| | | +1.0% | **+7.8%** |
| p99 | 1,283,332us | 1,279,999us | 1,329,999us |
| transactions | 17,798 | 17,773 | 17,794 |
| statements | 50,281 | 50,173 | 50,391 |
| CPU per statement | 45.0us | 48us | 47.3us |
| RSS per connection | 17,670 B | 19,341 B | 19,303 B |
| lookups | 0 | 13,385 | 13,401 |
| hits | 0 | 0 | 3,502 (26%) |
| statements served | 0 | 0 | 1,598 (3%) |
| upstream connections | 50 of 60 | 50 of 60 | 50 of 60 |

The off and on columns are means of three runs each and their p50 sets do not
overlap: the slowest control, 527,599us, beats the fastest cache-on run,
564,499us. The middle column is a single run and is quoted as one.

At matched load, sixty connections through the proxy against sixty direct, the
added hop is 295us off and 266us on. Those sets overlap, {327, 280, 277} against
{278, 237, 284}, so that is not a result and is not claimed as one.

## The two costs, separated

**Holding sequences back and looking statements up costs 1% of the median.** All
of the CPU and all of the memory, and almost none of the latency: 48us per
statement against 45.0us, 19,341 bytes per connection against 17,670, and
531,699us against 526,599us. That is the bookkeeping, and it is what it looks
like: a normalised SQL string, an `Arc` and a `Box` per key, a lookup that finds
nothing, and one buffered copy of each withheld frame.

**Serving costs another 7%.** Same bookkeeping, same withholding, and the only
difference is that 3% of statements are answered from memory instead of from the
database. The median gets worse by 36,000us.

That is not a paradox, it is queueing. Throughput is pinned by the database in
all three columns: 17,773 to 17,798 transactions, a spread of a tenth of a
percent. So the cache cannot make the fleet do more work. What it does is hand
1,598 clients their answer immediately, and each of them comes back with its next
statement sooner than it would have. The arrival rate at the sixty upstream
connections rises until it matches what they can serve, which it does at a longer
queue, and the queue is what the other 97% of statements are waiting in.

A cache in front of a saturated resource does not shorten the queue. It moves
work from the front of the queue to the back of it.

## Why M9.10 measured the opposite, which is a fact about M7.58

`M9.10` ran this comparison on the simple protocol alone and found the cache 7.1%
*better* on the median. Both numbers are right, and what changed between them is
not the cache.

That run was taken when `LivePool::release` woke every waiter. CPU per statement
was 673us and `M7.56`'s profile put 45% of it in the pool's lock, so the proxy
itself was a bottleneck sitting in front of the database, and a statement that
never acquired a connection skipped past it. `M7.58` then took CPU per statement
to 43.7us, a 15.7x reduction, and removed that bottleneck. What is left in front
of the database is the database, and the arithmetic above is what a cache does
there.

The cache's own cost per statement did not change between the two runs. What it
was buying shrank by a factor of fifteen underneath it.

## What this does not say

**It is not an argument that the feature is wrong.** ADR 0021 makes it off by
default and opt-in per tenant, and this workload is not one a tenant would opt in
for. Thirty percent of its statements are writes, each dropping that tenant's
entries, and two thirds are inside a `BEGIN`, which `M9.18` refuses to serve or
store. Between them only 27% of statements reach a lookup at all. The feature is
for a read-heavy tenant running single-statement transactions, and this run says
nothing about that tenant.

**It is not a measurement of the extended protocol's own cost.** The extended
half does now reach the cache, which is what `M9.17` set out to do: lookups went
from roughly 10% of statements to 27%. The share *served* did not rise, because
`M9.18` removed in-transaction statements from the addressable set in the same
milestone. Two changes in opposite directions, and this is their sum.

**And the hit rate falling from 39% to 26% is a correctness fix, not a
regression.** `M9.27` made the simple protocol refuse an entry stored by a
sequence that asked for no row description, because serving it sends rows with
nothing describing them. Those refusals count as lookups and not as hits, which
is what they are.

## What would change the answer

None of these are measured here, and they are in the order this run suggests
rather than the order they look interesting:

1. **A workload the feature is for.** The cheapest and the most honest. A
   read-heavy tenant whose served share is large enough to reduce what the
   database is asked to do, rather than 3%, is where the arithmetic above
   reverses: below saturation there is no queue to move work to the back of.
2. **Invalidate by table rather than by tenant.** 6,829 invalidations against
   13,401 lookups is the cache being emptied roughly every other lookup. ADR 0021
   already names table-dependency tracking as a later refinement on the same
   bound.
3. **Serve inside a transaction that has not written.** `M9.18` refuses the class
   because a stored answer carries a transaction status, and `M9.22` has since
   made the payload carry no status at all. A `'T'` could be generated the way
   `'I'` now is, which would roughly triple the addressable share of this
   workload.
