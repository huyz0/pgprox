# The allocator's arenas are not it. The worker count is, partly.

Date: 2026-08-05. `M34.1`. No code changed.

`M33` measured 22,835 bytes per connection, accounted for 5,048 of them as the
session future, ruled out the read and write buffers by experiment, and named
glibc's per-thread allocator arenas as the cheapest remaining candidate. This
runs that.

```bash
scripts/arena.sh 200
```

## Why three arms

A single-threaded runtime moves the thread count and the arena count at once, so
on its own it answers neither question. glibc gives each thread its own arena up
to a cap, and `MALLOC_ARENA_MAX` moves the cap without moving the threads.

| arm | workers | arenas |
| --- | --- | --- |
| baseline | 20 | 160, the glibc default of 8 per core |
| one-arena | 20 | 1 |
| one-thread | 1 | 160, which 6 threads cannot exceed |

Every arm states both numbers. `TOKIO_WORKER_THREADS` set to the empty string is
still set, and tokio parses it and panics, which killed the first attempt in all
three arms at once. An arm on an unstated default is also not reproducible on a
machine with a different core count.

## The numbers

Bytes per connection, three invocations, arms compared within one stack.

| arm | run 1 | run 2 | run 3 | median | range |
| --- | --- | --- | --- | --- | --- |
| baseline | 25,948 | 24,104 | 25,272 | 25,272 | 24,104-25,948 |
| one-arena | 22,855 | 22,282 | 26,972 | 22,855 | 22,282-**26,972** |
| one-thread | 16,179 | 19,886 | 17,797 | 17,797 | 16,179-19,886 |

## What it says

**The arenas are not it.** `one-arena`'s median is 10% below baseline and its
range covers baseline's entirely: in the third run, capping the arenas at one
produced a *higher* per-connection figure than leaving them at 160. There is no
effect here that this experiment can distinguish from noise.

The first run alone read as a clean 12% and it was written up as one. It is in
this document because `M32.8` is three milestones old and its lesson is that a
single run of this kind is a coin toss with a plausible face. The same mistake,
caught by the same discipline, in the milestone that came after the one that
learned it.

**The worker count is real.** `one-thread`'s median is 30% below baseline and its
range does not touch baseline's at either end. Nineteen fewer workers took 7,475
bytes per connection off a figure that has nothing to do with connections.

**It is not the arenas that make it real.** Those are the same two facts placed
together: cutting the workers moved the number and cutting the arenas did not, so
what the workers cost is tokio's own per-worker state rather than glibc's. Run
queues, timer wheels, and I/O driver registrations, none of them measured here.

**Most of it survives both.** 17,797 bytes per connection at one worker, of which
5,048 is the session future. **Roughly 12.7 KB per connection is still
unexplained**, and it is now known not to be the buffers, not to be the arenas,
and not to be per-worker.

## What this corrects

`M33` reported 22,835 bytes per connection as a per-connection cost. About 30%
of it is per-worker cost divided by a connection count it has nothing to do
with, on a twenty-core machine. On a four-core node the same code would report a
smaller number for the same reason, which makes the figure a property of the
machine as much as of the proxy.

Any future per-connection memory figure from this project should state its
worker count beside it, the way this table does.

## What is left to look at

In the order they are worth trying, and none of them measured:

- **The spawned task's own allocation.** `size_of_val` on the future is 5,048
  bytes and the test that guards it measures exactly that. What `tokio::spawn`
  allocates is the future plus a header plus whatever the allocator rounds up
  to, and nothing here has weighed it.
- **The per-session `body: Vec<u8>`.** It grows to the largest frame the session
  has seen and never shrinks, and it is not borrowed from the slab.
- **The upstream side.** A client connection's share of a pooled upstream
  connection is not free, and this experiment attributes all of it to the client.

The first is one line to measure and would either close most of the gap or rule
itself out.
