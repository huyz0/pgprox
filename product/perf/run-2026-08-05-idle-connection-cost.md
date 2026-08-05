# What an open, quiet connection costs, and what that says about 100k

Date: 2026-08-05. `M36.1`. No code changed.

`M35` found that per-connection memory under the reference workload is a curve
rather than a number: the buffer term tracks concurrency, concurrency saturates,
and dividing by connection count produces a figure that falls as connections
rise. It named the one term that does not saturate as the thing worth measuring
and failed to measure it.

This measures it. Under `workload-idle.yaml` a connection sends a transaction
every thirty seconds to five minutes, so almost none are active at once: the
upstream connection count during these runs was **1 to 3**, against a cap of 60.
The buffer term is out of the picture by construction, and what is left is the
state a connection holds while it is doing nothing.

```bash
WORKLOAD=product/perf/workload-idle.yaml COMPARE_DURATION=90 scripts/compare.sh 800
```

## The numbers

Absolute peak resident memory, serving N idle connections.

| arm | 200 | 400 | 800 |
| --- | --- | --- | --- |
| pgprox | 11.8 MB | 17.2 MB | 20.8 MB |
| pgbouncer | **4.2 MB** | **4.9 MB** | **4.1 MB** |
| pgcat | 19.6 MB | 25.9 MB | 40.2 MB |

Bytes per additional idle connection, from the absolute peak:

| arm | 200 to 400 | 400 to 800 | 200 to 800 |
| --- | --- | --- | --- |
| pgprox | 28,631 | 9,318 | 15,756 |
| pgbouncer | 3,727 | -2,089 | **-150** |
| pgcat | 33,444 | 37,294 | 36,011 |

## What it says

**pgbouncer's idle connection costs nothing this experiment can measure.**
Quadrupling the connections moved its resident memory from 4.2 MB to 4.1 MB. Its
slope is negative, which means it is zero and the noise is larger than the
signal. That is the design doing exactly what `M33` read in its source: no
buffer is held while a socket is quiet, and `PgSocket` is small enough to
disappear into the measurement floor.

**pgcat's is large and linear**, 36 KB per idle connection across both
intervals. Its source says why and `M33` recorded it: two `BytesMut` of 8 KiB
each, allocated per client and held for the life of the connection whether or
not anything is happening. 16 KB of that is the buffers and the rest is the task
around them.

**pgprox is between them and its cost is real.** At 800 idle connections it
holds 20.8 MB against pgbouncer's 4.1, which is **five times**, measured rather
than extrapolated.

## What this says about the roadmap's target

`scripts/scale.sh` states the target: under 500 MB at 100,000 connections.

At pgprox's 200-to-800 slope of 15,756 bytes, a hundred thousand idle
connections is **1.47 GB**. Three times the target.

That extrapolation deserves its caveat in the same breath. It is 167 times the
largest count measured, and pgprox's slope is not constant: 28,631 bytes between
200 and 400, then 9,318 between 400 and 800. If it keeps falling the number
comes down; if it flattens at the 9,318 figure the answer is 0.87 GB, still
above target. Nothing here says which.

What does not need extrapolating is the comparison. pgbouncer serves 800 idle
connections in 4.1 MB and pgprox needs 20.8 MB for the same thing, on the same
machine, in the same minute.

## What the session is holding

The question `M34` and `M35` were circling. Some of it is now accounted for:

- **5,048 bytes** is the session future, measured directly by
  `one_session_costs_less_than_the_slab_buffer_it_no_longer_holds` and guarded
  at a 5 KiB ceiling.
- **Not the read and write buffers.** `M33` quartered them and moved the figure
  by 205 bytes, and this run confirms it from the other direction: at 1 to 3
  upstream connections almost no buffers are out, and the cost is still there.
- **Not the allocator arenas.** `M34`, and its result stands because it compared
  two arms at one connection count.
- **Not the prepared statement map.** The workload prepares four statements
  totalling 250 bytes of SQL.

That leaves roughly 10 KB per idle connection unaccounted for, against a 5,048
byte future. The next thing to weigh is what `tokio::spawn` allocates, which is
the future plus a task header plus what the allocator rounds up to, and which no
test in this repo has ever measured.

## What is not claimed

Single run per point, three points, one machine. The ordering and the 800-count
comparison are solid; the slope is not, and the extrapolation to 100k is
arithmetic on a slope that is visibly still moving.

`M16`'s 100k run remains the only thing that settles the target, and it is still
blocked on hardware.
