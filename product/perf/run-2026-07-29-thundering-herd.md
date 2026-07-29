# Waking one waiter instead of all of them: 16x less CPU

`M7.58`. `LivePool::release` called `Notify::notify_waiters`, which wakes every
waiter. At five hundred clients against sixty upstream connections that is
roughly four hundred and forty tasks woken to hand out one connection, and each
one takes the pool's mutex to be told to wait, then twice more building and
dropping a `WaitGuard` on its way back to sleep.

One line, `notify_waiters` to `notify_one`, on the two paths that free exactly
one thing: a released connection, and a reserved slot given back when a connect
fails. `set_limit` keeps `notify_waiters`, because raising a cap can admit many
at once and there is no count to notify.

```bash
LOCAL_PROXY_BIN=<one variant> scripts/scale.sh 500 --local
```

Three matched pairs, alternating, same machine, same binary except for that
line, `workload.yaml` version 3.

## The numbers

| | `notify_waiters` | `notify_one` | |
| --- | --- | --- | --- |
| CPU per statement | 687us | 43.7us | **15.7x less** |
| proxy CPU over the phase | 33.2s | 2.25s | |
| p99 at 500 connections | 2,670ms | 1,273ms | **52% lower** |
| statements | 48,242 | 51,217 | 6.2% more |
| p50 at 500 connections | 453ms | 508ms | 12% higher |
| upstream connections | 50 of 60 | 50 of 60 | unchanged |

Every column is the mean of three runs, and no run of either arm overlaps the
other arm on any of them.

## The profile, which is the same picture from the other side

`perf record -F 499 -g` for twenty seconds under the same load. The headline is
the sample count: **4,119 samples with the herd, 161 without**. The process is
barely on a CPU.

What used to be at the top is gone. `Mutex::lock_contended` was 18.7% and
`LivePool::acquire` 11.7%; neither appears in the top sixteen now. What is
there instead is ordinary work, none of it above 7%: the tokio scheduler,
`malloc`, `memmove`, the codec, `BufferSlab::try_borrow`, `serve::observe`.

That is what this proxy's profile is supposed to look like.

## The one thing that got worse, and why it is the right trade

The median went up 12%, and it is not noise: 453ms to 508ms, consistently.

With the herd, every release put every waiter back in the race, so service was
effectively random and a caller could be served out of turn. `tokio::Notify`
wakes waiters in the order they registered, so `notify_one` is close to
first-come-first-served. FIFO raises the median, because everybody waits their
actual turn, and collapses the tail, because nobody is repeatedly unlucky. The
p99 halving and the p50 rise are the same fact seen twice.

A queue that is 12% slower in the middle and twice as fast at the edge is the
better queue, and the CPU is not a trade at all.

## What this means for M7.55, M7.56 and M7.57

`M7.56` measured 45% of the proxy's CPU in the upstream pool's lock and stopped
there, because the three answers it could see had different consequences.
`M7.57` was framed around choosing between them, with the note that the third,
"the contention is simply what saturation looks like", had to be eliminated
first and needed a machine this repository does not have.

It did not. Most of that 45% was a herd the proxy was inflicting on itself, and
a machine with five hundred connections and sixty upstreams was enough to see
it once the question was "how many wakeups per release" rather than "how much
lock". The two remaining answers, sharding by `PoolKey` and a lock-free free
list, are now both attacking 2.25 seconds of CPU rather than 33, and neither
looks worth its risk against that.

`M7.55` measured roughly 2ms of proxy CPU per connection per second and
concluded that the cost was per connection rather than per statement. It was,
and this is why: contention tracks how many callers are queued, and the herd
made every release cost work proportional to the queue. At 43.7us per statement
that number is now roughly 0.13ms per connection per second on the same
workload, which changes what a hundred thousand active connections would cost
by more than an order of magnitude.

`M9.10` measured the query cache at 7% of CPU and median latency against the
herd. That measurement is not wrong, but it was taken against a proxy spending
94% of its CPU on wasted wakeups, so the cache's share of what is left is now a
different and much larger fraction of a much smaller number. It is not worth
re-running: the cache's value is the round trip it avoids, not the CPU, and the
round trip has not changed.

## What made it testable

The queue length after a release is identical either way: eight waiters, one
release, seven still waiting, whether the other seven were disturbed or not.
The first version of the test asserted on that and passed against the herd.

`LivePool::futile_wakeups` is what made the property visible, counting waiters
that woke, found nothing, and parked again. It reads 7 with `notify_waiters`
and 0 with `notify_one` for one release against eight waiters, which is the
whole finding in two numbers.

## The machine

The same as every run in this directory: `--local` rather than the compose
stack, one node, a Postgres on the same host, and absolute latencies that are a
saturated database on a laptop. The comparison is the measurement. Both arms
were the same binary built twice from the same tree with one line changed, run
alternately within minutes of each other.
