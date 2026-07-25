# 0008. Connections hold no I/O buffer while idle

Status: accepted

## Context

Target is 50k to 100k client connections per node. Most of them are idle most of
the time, which is the entire reason the proxy is worth building.

The arithmetic that forces this decision: 100k connections with a 16 KiB read
buffer and a 16 KiB write buffer each is 3.2 GB of memory that is doing nothing.
Per-connection buffers are the obvious design and they do not survive contact
with the target.

## Decision

Task-per-connection on Tokio, but a connection holds no I/O buffer while idle.
It borrows a buffer from a sharded slab when its socket becomes readable and
returns it once quiescent.

An idle client then costs a socket plus a small state struct, on the order of
200 bytes, rather than 32 KiB.

The borrow and return points are the two ends of the relay loop, so this is a
small amount of machinery rather than an architectural change.

## Consequences

- Userspace memory at 100k connections drops from roughly 3 GB to a few hundred
  MB. The target of under 500 MB userspace RSS is set on this basis.
- Kernel socket memory does not go away. At 100k sockets, expect 1 to 3 GB
  depending on `net.ipv4.tcp_rmem` and `tcp_wmem` minimums, which must be tuned
  in the pod. This is worth stating plainly because it is the number people are
  surprised by after optimizing userspace.
- File descriptors need `ulimit -n` around 262144.
- The slab is a shared structure on the hottest path in the process, so it is
  sharded rather than a single pool, and its allocation behaviour is asserted by
  `dhat` tests rather than assumed.
- Buffer exhaustion under a synchronized burst becomes a real failure mode. The
  slab blocks rather than allocating without bound, which converts a memory
  spike into latency. That is the correct direction.
- Outbound ephemeral ports are not a constraint at 5k connections per upstream
  host, since the limit applies per destination tuple.

## Alternatives rejected

**Fixed per-connection buffers, larger instances.** Simplest and most
predictable. Rejected because several GB of idle buffer is pure waste that grows
linearly with the product's main selling point.

**Adaptive buffers starting small and growing.** No shared slab needed.
Rejected because it still has a per-connection allocation floor, and the floor
times 100k is the problem.

**Ship fixed buffers, add reclaim when measured.** Tempting, and the reason it
was rejected is that retrofitting buffer ownership changes the relay loop's
lifetimes, which is exactly the code that is hardest to change once it has
tests. Doing it first costs a day; doing it later costs a rewrite of the hot
path.
