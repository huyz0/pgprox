# A spawned task costs its future plus 128 bytes

Date: 2026-08-05. `M37.1`. No code changed.

`M36` measured an idle connection at roughly 15 KB and accounted for 5,048 of it
as the session future. What remained unweighed was the difference between a
future and a task: `size_of_val` reports the future, and `tokio::spawn` puts it
in a heap allocation alongside a header holding the waker, the state, the join
handle's channel and the scheduler's links.

```bash
cargo test -p pgprox --test spawn -- --nocapture
```

## The measurement

`dhat`, so these are bytes requested from the allocator. 256 tasks per point,
each parked on a channel nobody sends to, measured after they have all reached
their await.

| future bytes | held per task | overhead |
| --- | --- | --- |
| 88 | 256 | 168 |
| 1,048 | 1,152 | 104 |
| 4,120 | 4,248 | **128** |
| 16,408 | 16,536 | **128** |

**The overhead is a constant, not a proportion.** It sits at 128 bytes across a
future that grows by a factor of sixteen, which is what a header looks like. The
88-byte row reads high because a request that small is rounded rather than
because the header is bigger there.

The test asserts both halves of that: a task holds at least its future, and the
overhead at 16 KB is within 512 bytes of the overhead at 88. The second is the
one worth having, because a proportional overhead would mean every byte added to
the session future costs two, and that would change what the future-size ceiling
is for.

## What it accounts for

The session future is 5,048 bytes, so a spawned session task requests roughly
5,176.

**That accounts for 128 bytes of `M36`'s ten kilobytes.** The last candidate any
of the previous four milestones named is eliminated.

## Everything now ruled out

| candidate | ruled out by | what it was worth |
| --- | --- | --- |
| the read and write buffers | `M33`, quartering them | 205 bytes |
| the allocator's arenas | `M34`, capping at one | nothing measurable |
| per-worker runtime state | `M35`, it is fixed cost, not per connection | n/a |
| the prepared statement map | `M36`, four statements and 250 bytes of SQL | under 1 KB |
| the spawn header | this | 128 bytes |

Roughly 10 KB per idle connection is still unexplained, and there is no longer a
named suspect for it.

## What has not been considered, and is now the obvious one

**`dhat` measures what was requested. `M36` measured resident memory, which is a
high-water mark.** Those differ by everything the allocator was asked for,
freed, and did not return to the operating system, and glibc returns very little:
it trims the top of the heap only, and only past a threshold.

A connection's setup is not free. It resolves a grant through the sidecar, parses
a JWT, runs a SCRAM exchange, reads server parameters and takes a pool
connection, and every one of those allocates and frees. If those allocations
land in the middle of the heap rather than at its top, they stay resident for
the process's life and `M36` counts them against whichever connection count the
run used.

That is testable in two ways and neither needs a container:

- `dhat` inside the proxy would report requested bytes for a live session, which
  put beside `M36`'s resident figure gives the gap directly.
- `malloc_trim(0)` after a ramp would say how much of the resident memory glibc
  is willing to give back at all.

The second is one call and would settle whether the ten kilobytes is state a
connection holds or memory the allocator is sitting on. They are different
problems with different fixes, and everything since `M34` has assumed the first
without checking.

## A smaller thing the mutations found

The test builds its futures by declaring an array before the await and touching
it after, and the comment said the touch afterwards was what kept the array in
the future. Moving the touch to before the await, so nothing reads it later,
changes nothing: `rustc` keeps a local declared before an await in the state
machine either way.

The comment claimed a mechanism and the mutation says it is not the mechanism.
The assertion on `size_of_val` is therefore not a check that a trick worked, it
is the only thing that knows the future is the size the test asked for. That is
a stronger reason to keep it than the one originally written down.

## What is not claimed

This measures spawn overhead in a test binary with a two-worker runtime and
futures made of padding. A real session's future is not padding and the runtime
has twenty workers. Neither should change a header size, and the flat 128 across
four sizes is the evidence that it does not, but it is a test-shaped measurement
rather than a measurement of the proxy.
