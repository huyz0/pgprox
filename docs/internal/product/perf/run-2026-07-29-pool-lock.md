# The 2ms per connection is the pool's lock

A `perf` profile of the proxy under 500 connections, taken after
`run-2026-07-29-connection-cost.md` established that the cost is per connection
rather than per statement. This is the first profile of this process that was
looking at the right thing.

```bash
scripts/scale.sh 500 --local
perf record -F 499 -g -p <pid> -- sleep 20
```

2,156 samples, all on `tokio-rt-worker` threads.

| Overhead | Symbol |
| --- | --- |
| 20.6% | `Mutex::lock_contended` |
| 12.3% | `LivePool::acquire` |
| 5.2% | `syscall` |
| 4.7% | the pool's `HashMap<PoolKey, ...>` entry lookup |
| 4.2% | `tokio::time::Handle::reregister` |
| 4.0% | dropping `LivePool::WaitGuard` |
| 3.6% | `Notify::poll_notified` |
| 2.7% | `Arc<LivePool>::acquire` |
| 2.4% | dropping `TimerEntry` |
| 2.2% | `Notify::notify_waiters` |

## What it is

The upstream pool, and specifically one mutex.

Of the 20.6% spent contending, 12.5 points come from `LivePool::acquire` and 5.1
from dropping a `WaitGuard`, which is the release path. Add `acquire` itself,
the `HashMap` lookup it does while holding the lock, and the `Notify` on both
sides, and roughly **45% of the proxy's CPU is one lock and the wakeups around
it**.

That is the shape `M7.55` measured from the outside. Five hundred connections
share a sixty-connection pool, so every statement's acquire contends with every
other, and every release wakes the waiters. The cost lands per connection
because contention is a function of how many are queued, not of what any one of
them asked for. It is why the per-statement number moved from 952us to 3,690us
for the same amount of work: the work did not change, the queue did.

## What is not the problem

Worth writing down, because two milestones were spent on the wrong candidates.

The frame path does not appear. `Wire`, the codec, the classifier and the relay
are all below the noise floor. `M7.46` replaced the per-frame memmove on the
strength of a profile that put 19% in `__memmove_avx_unaligned_erms`; that
profile was of a different workload and the change, while correct, could not
have moved this number.

Memory is not it either. The session registry, the metrics exporter and the
gossip digest do not appear, which rules out the three candidates `M7.55`
guessed at. Timers are visible but small: `reregister` and `TimerEntry::drop`
are 6.6% together, and they are downstream of the same waiting.

## What to do about it is not settled here

This run names the cost. It does not say which of the obvious answers is right,
and they have different consequences:

- **Sharding the pool by `PoolKey`** removes contention between tenants and
  leaves it within one, which for a fleet where a few tenants are hot is most
  of the win for none of the risk.
- **A lock-free or per-worker free list** removes it altogether and is a
  rewrite of the piece of code the quota invariant depends on.
- **Not queueing in the first place**: at 500 connections against 60 upstreams
  the queue is the design working as intended, and a run against a database
  with headroom might show the contention only appears when saturated.

The third is worth eliminating before either of the others is attempted, and
it needs a machine this one is not.
