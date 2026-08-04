# What pgbouncer and pgcat do differently, and where pgprox's memory actually goes

Date: 2026-08-05. `M33.1`. No code changed.

`M32` measured the three poolers and found pgbouncer serving 200 connections in
4.5 MB where pgprox needed 13.9 MB and pgcat 26.1 MB. A number is not a reason.
Both are open source, so this reads them.

Versions: pgbouncer at `13a344f`, 2026-07-10. pgcat at `5b03881`, 2025-02-27,
which is the 1.2.0 the comparison ran.

## The three designs, in one paragraph each

**pgbouncer** gives each socket one `IOBuf`: a twelve-byte header of three
cursors and a flexible array member holding `pkt_buf` bytes, default **4096**,
in the same allocation. The cursors are `done_pos`, `parse_pos` and `recv_pos`,
splitting the buffer into sent, parsed-and-pending, and received-unparsed. It is
taken from a slab when a socket has something to say and freed back when
`iobuf_empty`. Writes go out of the *peer's* read buffer straight to the
destination socket, so a relayed frame is never copied between a read buffer and
a write buffer. There is no write buffer.

**pgcat** gives each client two `BytesMut::with_capacity(8196)`, held for the
life of the connection, cleared but never released. There is no buffer pool of
any kind in its source. 16 KB per client, always, whether the client is running
a query or asleep.

**pgprox** gives each wire a read buffer and a write buffer, both borrowed from
`BufferSlab` when needed and returned when quiet, both `DEFAULT_BUFFER_SIZE` at
**16 KiB**. Plus a per-connection session future, currently 5,048 bytes, which
is resident whether the connection is busy or idle.

## Three things pgprox found by profiling that pgbouncer has had since 2007

Worth recording because they were arrived at independently, and two of them were
found only after a measurement went wrong.

| pgprox | pgbouncer | |
| --- | --- | --- |
| `Wire::read_at`, a cursor rather than a `drain` | `done_pos`/`parse_pos` | pgprox's comment says a profile put 19% of its time in `__memmove_avx_unaligned_erms` |
| `BufferSlab`, borrowed on read and returned when quiet, `ADR 0008` | `sbuf_try_resync(release)` freeing the `IOBuf` when empty | |
| `read_buf` into uninitialised capacity, `M30.4` | `do_iobuf_reset` as the slab's init function, resetting three cursors and not the 4 KiB | pgprox zeroed 16 KiB per read for twenty-nine milestones |

The convergence is the point. These are not clever tricks, they are what a
connection pooler turns out to need, and a design that does not have them will
find out by profile eventually.

## Where the easy answer said pgprox's memory goes

pgbouncer's buffer is 4 KiB and pgprox's is 16 KiB, and pgprox holds two of
them per active wire against pgbouncer's one. That is 8x on paper, against a
measured 3x, so the arithmetic looked close enough to be the answer.

It is not the answer.

`DEFAULT_BUFFER_SIZE` and `HELD_READ` were both set to 4 KiB and the comparison
re-run:

| | 16 KiB | 4 KiB | |
| --- | --- | --- | --- |
| pgprox peak RSS | 12,540 kB | 12,160 kB | -3% |
| pgprox idle RSS | 8,080 kB | 7,740 kB | -4% |
| per connection | 22,835 B | 22,630 B | **-0.9%** |
| `held_read` | 2,263 | 2,263 | unchanged |

Quartering the buffer moved the per-connection cost by 205 bytes. A 12 KiB
reduction per buffer showing up as 40 kB across 200 connections means at most
three buffers were outstanding at once.

**That is the buffer slab working exactly as designed.** `ADR 0008`'s premise is
that connections are busy for milliseconds and idle for hundreds of them, so
buffers should be pooled against concurrency rather than against connection
count. The experiment says the premise holds so completely that the buffer size
is not a memory lever at all.

It also says the obvious optimisation is not available. There was a version of
this document that recommended shrinking the buffer to match pgbouncer, written
before the run.

## So where does it go

22,835 bytes per connection, of which the session future is 5,048. The other
17,787 are unaccounted for, and this document does not know what they are.

What they are not: the read and write buffers, by the experiment above.

Candidates, none of them measured:

- The spawned task's own allocation, which is the future plus tokio's header,
  and which the future-size test does not cover.
- The per-connection `body: Vec<u8>` each session holds, which grows to the
  largest frame it has seen and never shrinks.
- The upstream wire's own two buffers, for the connection's share of a pooled
  upstream.
- Allocator arenas. glibc gives each thread its own, and a twenty-core runtime
  has twenty. That would appear as a step rather than as a per-connection cost,
  which is testable by running the same load on a one-thread runtime.

The last one is the cheapest to rule out and should be first.

## What is not the lever

**Not unsafe.** pgcat contains zero `unsafe` in its source and is the heaviest
of the three by a factor of five. pgbouncer is C, where the question does not
arise, and its memory advantage comes from allocating less rather than from
reaching past a check. `M29` and `M30` already found the same thing inside
pgprox: nothing moved.

**Not SIMD.** Neither project contains any. pgprox uses `memchr`, which is
vectorised, on the one scan that warranted it, and that is already more than
either of the others does.

**Not alignment.** pgbouncer's slab aligns to `sizeof(long)` and no further, and
does not pad objects to cache lines. The one packing decision it has made is 26
one-bit bitfields in `PgSocket`, which saves 25 bytes per connection out of
several hundred.

The lever in all three is how much is allocated per connection and for how long.
pgcat allocates 16 KB per client and holds it forever. pgbouncer allocates 4 KB
per active socket and gives it back. pgprox gives its buffers back and then
pays 22 KB for something else.

## What this changes

Nothing yet, which is the honest outcome of an experiment that refuted its
hypothesis. What it produces is a better question than the one it started with:
not "should the buffer be smaller" but "what are the seventeen kilobytes that
are not the buffer", and a first move for answering it.
