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
