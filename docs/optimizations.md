---
title: Optimizations
description: "The performance work that has actually been done, what each change was worth, and the candidates that were measured and refused."
---

What has been optimized, what each change bought, and what was tried and thrown
away. [Performance](performance.md) has the end-to-end numbers; this is the work
behind them.

Every figure here is instructions per operation under callgrind, or bytes
requested from the allocator, both of which are deterministic. They are not
times.

## What holds the numbers in place

Optimization work that nothing measures decays back. Three mechanisms keep these
results from drifting.

**A committed baseline.** `docs/internal/product/perf/baseline.json` holds instruction counts
for sixteen declared hot paths, and `scripts/bench.sh` fails CI on a 5% drift in
either direction. Rewriting the baseline is a deliberate act with a reason in
the commit message.

**Allocation budgets.** Several hot paths assert an exact allocation count, most
of them zero: frame scanning, backend message decoding, the relay step, the
route decision, the pool release decision, a warm acquire, and a cache hit. The
count is per thread rather than per process, so the test harness's own
bookkeeping cannot land inside a window budgeted at zero, and each test runs a
positive control first, so a counter that stopped counting fails the harness
before it fails a budget.

**Callgrind per function, not per binary.** Each benchmark runs at N iterations
and at 2N and reports the difference, so fixture construction cancels. Without
the subtraction the cache profile reports its own 4,096-entry fixture as the
loop.

## Build configuration

Free instructions, and this workspace had one lever left of the four.

| Knob | Verdict |
| --- | --- |
| `lto = "thin"` to `"fat"` | Taken. 7 to 15% on the route decision |
| `codegen-units = 1` | Already set |
| `panic = "abort"` | Already set, and right for a process whose rule is that it does not panic |
| `-C target-cpu=native` | Refused. This ships as a container image that runs on hardware the build machine has never seen |

| Benchmark | thin | fat | |
| --- | --- | --- | --- |
| `route_begin` | 1,536 | 1,294 | -15% |
| `decode_query` | 460 | 390 | -15% |
| `route_update` | 7,423 | 6,717 | -9% |
| `route_point_select` | 6,982 | 6,444 | -7% |

Every cache and pool number was unchanged to within a percent, which is what
inlining across one crate boundary looks like rather than a general uplift. The
cost is a release relink going from 13 to 30 seconds, which lands on CI and on
releases and on nobody's edit loop.

## The four that mattered

A sweep across every crate, starting from a profile rather than from a reading.

| Path | Before | After | |
| --- | --- | --- | --- |
| `route_point_select` | 6,444 | 3,716 | **-42%** |
| `route_update` | 6,717 | 3,969 | **-41%** |
| `held_read` | 18,669 | 2,263 | **-88%** |
| `acquire_and_release` | 443 | 278 | **-37%** |
| `route_begin` | 1,294 | 1,165 | -10% |
| `cache_put` | 3,770 | 3,695 | -6% (see below) |
| `invalidate_a_tenants_entries` | 86,088 | 85,633 | -3% (see below) |

The "after" column is the committed baseline, so a rebaselined hot path that
leaves this table alone fails a check rather than leaving the page quietly
stale.

The last two rows do not subtract, and the reason is worth more than a tidier
table. Their before and after were measured on the same machine and the cuts
were real, -6% and -3%. The after column then moved when the six `pgprox-cache`
benchmarks were rebaselined onto CI, which reads that crate 3 to 4% higher than
a developer machine does while agreeing to the instruction on every other hot
path here. Subtracting the two columns now compares two machines and understates
what changed; the percentages are the same-machine figures, which are the ones
that describe the work. `cache_put` also benchmarks slightly different code than
it did then. See
[`run-2026-08-06-ci-baseline.md`](internal/product/perf/run-2026-08-06-ci-baseline.md).

Not one line of unsafe was written, and not one of those four costs was a bounds
check. Three were work that did not need doing at all.

### The router lexed every statement twice

`begins_read_only_transaction` ran beside the classifier over the same text and
read every token to answer a question the first word settles. It now exits after
the first word. That is most of the 42%.

### Every word was compared against every keyword

A point select was making about 290 case-insensitive string comparisons to find
no match. The keyword lists now carry masks over word length, first letter and
last letter, computed at compile time from the lists themselves, so a word that
cannot possibly be on a list is rejected by three integer tests.

That cut the matching function by 52% without touching an entry or a comment. A
debug assertion checks the filter against the full scan on every call, so a
filter that rejected a word actually on the list fails a test run rather than
silently routing a write to a replica.

### The pool hashed an integer it made up

`SipHash` on a connection id this process had just issued, at 39% of the acquire
and release path. It now uses a cheap unseeded hasher.

The durable part is the rule that fell out, which is a security rule as much as
a performance one: **who chooses a map key decides its hasher**. A key a peer
chooses keeps the seeded default, because that seed is what stops a client
sending a thousand keys that land in one bucket. Only keys this process hands
out get the fast one, and the maps that keep the default are named in code so
the next profile does not quietly move one.

### A 16 KiB memset before every read

Every held read zeroed a buffer before filling it, and the comment above it
justified this by saying the safe alternative needed unsafe and unsafe was
forbidden. The safe API had been imported in that same file since it was
written.

That is an 88% cut on the read path and it is the most instructive finding of
the sweep. The comment gave a reason, the reason named a rule, and the rule had
been changed three milestones earlier by this same line of work. Nobody reread
the comments that cited the policy when the policy moved.

## Streaming instead of buffering

The relay reads a five byte header on its own and then decides what to do with
the body from the tag. A `Query` or a `Parse` is read whole, because the SQL
decides everything downstream. A `DataRow` or a `CopyData` has nothing to
inspect, so when nothing is recording it for the cache the body moves from one
socket to the other and never lands.

One 16 MiB row, the same bytes down both paths:

| | held |
| --- | --- |
| Read and buffer | 16,777,216 bytes |
| Stream | 0 bytes |

The frame limit is 1 GiB, deliberately, because a `SELECT` of a 100 MB `bytea`
is a real query that real Postgres answers. Buffering every body would mean
holding that twice: once on the way in, once in the write buffer it is copied
into.

## Memory per connection

The design point is a long tail of connections that exist and rarely do
anything, so the number that matters is what an idle one holds.

| | |
| --- | --- |
| Idle connection, resident | 5,726 bytes |
| Session state alive across awaits | 5,048 bytes |
| Spawned task overhead above its future | 128 bytes, constant |
| Read and write buffers while active | 16 KiB, borrowed from a pool |
| Buffers while idle | none |

Four things produced that.

**Buffers are borrowed, not owned.** A connection takes buffers from a slab when
its socket has something to say and returns them when it goes quiet. This is the
single largest reason 100,000 connections is affordable.

**Identifiers are shared, not cloned.** Tenant and server identifiers wrap
`Arc<str>`, because they are cloned once per connection and a string copy per
connection at that scale is a real allocation cost on a path that should not
allocate.

**Startup state is boxed and dropped.** Authentication costs a couple of
kilobytes of state. Without boxing it into its own future, that state would sit
inside the connection's future for as long as the connection lives: hundreds of
megabytes, at scale, of work that finished in the first milliseconds.

**The task header is a constant.** A spawned task costs its future plus 128
bytes, flat across futures that differ by a factor of sixteen. A proportional
overhead would have meant every byte added to the session future cost two, which
would change what the size ceiling is for. The test asserts the overhead at
16 KB is within 512 bytes of the overhead at 88.

## Measured and refused

Negative results, kept because they are what stops the same idea being tried
again.

**Unchecked indexing into the cache's recency slab.** The best unsafe candidate
in the workspace: the index type is a private newtype with no public
constructor, so being in bounds is a type invariant rather than a runtime fact.
Removing the checks moved nothing, and two of the three benchmarks came out
slower, which is what noise looks like. LLVM had already elided them.

| Benchmark | safe | unchecked | |
| --- | --- | --- | --- |
| `cache_hit_rotating` | 1,801 | 1,812 | +0.6% |
| `cache_hit` | 1,462 | 1,469 | +0.5% |
| `cache_put` | 3,753 | 3,745 | -0.2% |

**Skipping UTF-8 validation in the query decoder.** Two thirds of the decode,
and only `from_utf8_unchecked` removes it. Refused: that crate reads bytes a
peer chose, and it carries `#![forbid(unsafe_code)]` in its own source precisely
so that a decoder bug's worst outcome is a wrong answer rather than memory
corruption. The refusal came from a script rather than from an argument.

**glibc allocator arenas.** Named as the cheapest remaining candidate for
per-connection memory, and measured with three arms that move the worker count
and the arena count independently. It was not it. A preliminary reading
suggested 12% and the third run came out higher than baseline.

**`-C target-cpu=native`.** See the build table.

## Two rules that came out of the work

**A benchmark under about a thousand instructions is measuring the harness.**
The same cache benchmark reported a false 15% once and a false 6% regression
later, both times because the machinery around it cost more than the thing it
measured. Hash map probe counts depend on a per-process seed, and at small
enough sizes that is a measurable share of the total. It now drops sixteen
entries and three runs agree to within half a percent.

**A number that is too good to believe usually is.** Several figures published
here were corrections of earlier ones: a per-connection cost that disappeared
when measured at a second connection count, an arena result that was noise, and
an extrapolation to 100,000 connections made when the repository already held a
direct measurement at that scale. Every one was caught by the number looking
wrong, and none by a test.
