# The bounds checks were already gone

`M29.1`. The first exception `M27`'s unsafe policy was asked for, and the
policy refused it on the evidence it asks for.

The candidate was the query cache's recency slab. `M26.4` replaced a `BTreeMap`
with a doubly linked list held in a `Vec` indexed by `Slot`, and `Slot` is a
private newtype with no public constructor, issued only by `claim`. That is
exactly the shape the unsafe-optimisation procedure names as its best case: a
trusted index whose in-bounds property is a type invariant rather than a runtime
fact, so `get_unchecked` has a contract the compiler could not have derived.

A rotating hit touches five of them: one read in `unlink`, up to two neighbour
writes, one write in `link_newest` and one to the slot itself.

## The measurement

Both arms on the same machine, same toolchain, same `[profile.release]` with
`lto = "fat"`, N and 2N under callgrind.

| benchmark | safe | `get_unchecked` | |
| --- | --- | --- | --- |
| `cache_hit_rotating` | 1,801 | 1,812 | +0.6% |
| `cache_hit` | 1,462 | 1,469 | +0.5% |
| `cache_put` | 3,753 | 3,745 | -0.2% |

Nothing moved. The spread between the two arms is smaller than the spread
between two runs of either, and two of the three came out slower with the
bounds checks removed, which is what noise looks like rather than a
regression.

## Why

LLVM had already elided the checks. The procedure's own second step says to
look before writing anything: "LLVM elides most bounds checks it can prove. If
the check is already gone, unsafe buys nothing but risk." It could prove these,
and the reason is visible in the code: `claim` pushes a `Placed` and returns the
index it pushed to, `slots` is never shortened, and every access is `slot.0 as
usize` against a `Vec` in the same function the optimizer can see the whole of.

`M28.1` probably helped. Fat LTO gives LLVM the whole program, and a bounds
check it can only elide by inlining across a crate boundary is one thin LTO
might have left in. That is not measured here and this document does not claim
it.

## What this decides

No unsafe in the cache slab. The procedure's rule is that an optimisation which
moves less than noise gets deleted and the safe version kept, and `M27.1`'s
policy says the same thing in its own terms: an exception has to name a
benchmark in `product/perf/baseline.json` that justifies it, and there is no
number here to name.

The policy worked as intended on its first use, which is worth recording
separately from the result. It did not have to be argued with; the condition it
imposes is the measurement, and the measurement said no.

## What this does not say

It does not say unsafe is worthless everywhere in this workspace. It says the
bounds checks on one trusted-index slab were already gone. The other patterns
the procedure lists are untested here: uninitialized buffers, raw pointers
shared across threads, trusted iterator lengths and zero-copy reinterpretation.
None of them has a candidate with a number behind it yet, which is the same
reason this one was tried first and the only reason it was tried at all.
