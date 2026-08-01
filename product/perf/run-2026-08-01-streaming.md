# What one large row costs

`M16.1`. `pgprox-proto` contains a streaming relay written so that a large
message is forwarded as it arrives rather than held. Nothing calls it.

```bash
cargo test -p pgprox-session --test streaming -- --nocapture
```

## The number

One 16 MiB `DataRow`, the same bytes down both paths, offered in 16 KiB chunks
the way a socket delivers them.

| path | held |
| --- | --- |
| `Wire::read_tagged`, which the proxy uses | 16,777,216 bytes |
| `FrameRelay`, which it does not | 0 bytes |

Zero rather than five, because a `DataRow` is `Inspect::None`: the relay reads
the header, learns it has nothing to inspect, and forwards every byte without
copying one.

16 MiB is a fraction of what a client may legitimately ask for.
`DEFAULT_MAX_FRAME` is 1 GiB, and it is 1 GiB deliberately, because a `SELECT`
of a 100 MB `bytea` is a real query that real Postgres answers and an earlier
64 MiB cap here refused it.

## Why the reading was not enough

The relay module's own header states the alternative it exists to prevent: a
relay built on `decode` "must accumulate an entire body before forwarding a
byte", and "a single large `DataRow` would then hold up to a gigabyte, and ADR
0008's whole premise is that an idle connection costs roughly 200 bytes".

The proxy's relay loop is built on `decode`. The paragraph describes the code
that shipped.

## What this does not say

`M7` held 100k connections at 546 MB. That run used small rows, so it is not
contradicted by this and does not answer it. The question this raises is what
the same run does with a result set large enough for the difference to show,
and that needs the three machines `M7`'s full run needed. It is named in the
roadmap as the completion condition rather than claimed here.

Nor does this say the proxy is slow. It says it holds the whole of a large
message, once per direction, per connection carrying one. A single client doing
that is nothing; the design's premise is a hundred thousand of them.

## After (`M16.3`)

The pump now reads the header, and reads the body only when something needs it.
For an uninspected tag on a session that is not recording for the cache, the
body is moved from one socket to the other a chunk at a time.

| | held for one 16 MiB `DataRow` |
| --- | --- |
| before, `Wire::read_tagged` | 16,777,216 bytes |
| after, `read_header` then `take_body` | 512 bytes |

512 is `FIRST_READ`, the stack chunk a quiet connection reads into, so the
largest piece the pump ever holds is a read rather than a message. A busy
connection reads into the 16 KiB borrowed buffer instead, which is the same
answer with a different constant: bounded by what the slab lends, not by what
the peer sent.

There is a second copy that goes with it. `forward` re-encoded the tag, the
length and the body into the write buffer, so a 16 MiB row was held twice. The
streaming path queues the header from `body_len` and then moves the body
through, flushing per chunk, so the write side does not become the buffer the
read side stopped being.

### What still buffers, on purpose

Everything `inspect_policy` marks `Whole` or `Prefix`, which is every message
the proxy acts on and none of the bulk, and every message on a session
recording for the query cache, because `belongs_in_payload` includes `DataRow`
and a cache entry is the bytes. The cache has its own bound.

### The end-to-end numbers

`scripts/e2e.sh`, three databases and three proxy nodes, before and after:

| | tps |
| --- | --- |
| before | 160.5 |
| after | 178.2 |

Worth almost nothing as a performance claim, and `M17.6` later showed it is
worth less than that. Two things were wrong with the machine: `conformance.sh`
had leaked 548 Postgres containers, and the host was running unrelated work at
about half its CPU. Later runs of the same unchanged code reported 101 and 102.

So these two numbers are not a before and after. They are two runs that
completed, and the checks either side of them are what the run is for: pgbench
clean with prepared statements, a drain with zero failed transactions, 25
write-then-read rounds with none served stale, and no token in any log.

Wall-clock throughput from `e2e.sh` on a developer machine is not a
measurement. That is the argument `scripts/bench.sh` opens with, and it is why
the claim this milestone makes is counted rather than timed.

The claim this milestone makes is about memory, and it is 16,777,216 against
512.
