---
name: hot-path
description: Measure before optimizing, and prove the optimization worked. Use when touching the relay loop, frame scanning, pool acquire, route decisions, the grant cache, or gossip encoding, or when asked to make something faster or investigate a performance regression.
---

# Hot path work

Line coverage tells you a line was executed. It says nothing about whether it
runs a billion times a day, and a proxy lives on that distinction.

## The declared hot paths

From `standards/testing.md`. If the change touches one of these, this procedure
applies:

1. The steady-state relay loop, both directions
2. Frame boundary scanning (type byte plus length)
3. `ReadyForQuery` status handling and the pool release decision
4. Warm-pool acquire
5. Route decision: classification plus replica eligibility
6. Grant cache lookup on connect
7. Gossip digest encode and decode

## Measure first

Never optimize from intuition. In a proxy the bottleneck is reliably somewhere
other than where it looks.

```bash
scripts/bench.sh                     # instruction counts against the baseline
scripts/bench.sh --update            # re-record the baseline, deliberately
scripts/profile.sh                   # replay the workload, semantic coverage
scripts/scale.sh <connections>       # RSS, added latency, upstream count
```

Record the baseline before touching anything. An optimization with no baseline
is a story.

## Gate on counts, never wall clock

Wall-clock timing on shared CI runners is noise. Two things are deterministic
and both are used here:

**Allocation counts**, via `dhat-rs` in an ordinary test:

```rust
#[test]
fn relay_of_one_data_row_does_not_allocate() {
    let _p = dhat::Profiler::builder().testing().build();
    let stats_before = dhat::HeapStats::get();
    relay_one_frame(&mut state, DATA_ROW_1KIB);
    let stats_after = dhat::HeapStats::get();
    assert_eq!(stats_after.total_blocks, stats_before.total_blocks);
}
```

**Instruction counts**, via `callgrind` directly. Each bench binary takes a
name and an iteration count; `scripts/bench.sh` runs it at N and at 2N and
divides the difference, so startup and fixtures cancel exactly. A few per cent
is a real change here where a timing would report it as noise. Not
`iai-callgrind`: it pulls two crates under unmaintained advisories and
`cargo deny` fails the workspace on them.

The baseline is `product/perf/baseline.json`. Rewriting it is a deliberate act
with a reason in the commit message, which is why the script only does it when
asked.

## The three lists

Replaying the reference workload against an instrumented binary, keeping
execution *counts* rather than hit/miss, produces a cost profile. Cross-reference
it into three lists, each implying a different action:

- **Hot and under-tested**: high count, low assertion density or surviving
  mutants. The highest-risk code in the repo. Write tests here first. This beats
  uncovered-line count as a prioritization signal, since that mostly points at
  error paths nobody hits.
- **Hot and expensive**: count times per-call cost. The optimization queue,
  ordered by total contribution rather than by what looks interesting.
- **Cold and complex**: near-zero count, high complexity or visible
  hand-optimization. Candidates for deletion. Speculative optimization is
  endemic in proxies and this is how it gets found.

## The reference workload

`product/perf/workload.yaml`. Everything measures against it: tenant mix (a
few hot, a long tail idle), query shape distribution, connection churn rate,
transaction size distribution, replica read fraction.

Do not change it to make a number look better. Without a fixed reference,
profiles are not comparable week to week and the whole exercise becomes
anecdote. If the workload is genuinely unrepresentative, change it in its own
commit with the reasoning, and re-baseline everything.

## After the change

- [ ] Baseline recorded before
- [ ] Allocation budget test still passes, or was tightened
- [ ] The instruction count improved, and by how much
- [ ] The improvement is stated as a number in the commit message
- [ ] No correctness test was weakened to get it

An optimization that cannot be stated as a number against a baseline did not
happen.
