# Ten of the sixteen hot paths are bit-identical on both machines. Six are not

Date: 2026-08-06. `M59.1`. No code changed.

The instruction-count baseline was measured on a twenty-core developer machine
and CI gates against it to 5%. Six of the sixteen benchmarks read higher on a
GitHub runner than they do here, one of them by 4.4%, which spent most of the
budget before any real change could use it. This rebaselines those six to the
runner and records what the other ten did, because "no difference" is a
measurement too and it is the one that says the difference is real.

## Where the numbers come from

Six consecutive CI runs, `M59.0` through `M64.0`, read out of the archived logs
of the `instruction counts` job rather than re-run for this. `M59.0` is the
lower bound because it changed what `cache_put` benchmarks; before it the
comparison would be against different code.

```bash
gh run view <id> --job <instruction counts> --log | grep instructions
```

## The ten that do not move

`scan_frame`, `decode_backend_message`, `relay_frame`, `decode_query`,
`decode_error_response`, `route_point_select`, `route_update`, `route_begin`,
`acquire_and_release` and `held_read` each returned the same number in all six
runs, and that number is the committed baseline to the instruction. Not close:
identical. Their entries are untouched.

That is what makes the other six a finding rather than noise. Callgrind is
deterministic for a given binary, so a benchmark that reads differently in two
places is running different instructions, not the same instructions measured
badly.

## The six that do

| benchmark | was | now | drift | spread across six runs |
| --- | --- | --- | --- | --- |
| `cache_put` | 3,540 | 3,695 | +4.4% | 0.43% |
| `invalidate_a_tenants_entries` | 83,378 | 85,633 | +3.0% | 1.44% |
| `cache_hit_rotating` | 1,799 | 1,810 | +0.6% | 0.22% |
| `cache_miss` | 1,237 | 1,239 | +0.2% | 0.57% |
| `serves_a_mix_of_tenants` | 38,262 | 38,319 | +0.2% | 0.02% |
| `cache_hit` | 1,461 | 1,461 | 0% | 0.82% |

Every one is `pgprox-cache`, and every one of that crate's six is here. Nothing
in another crate moved at all.

Each new figure is the lower median of the six readings, so it is a number some
run actually produced rather than an average of numbers none did.

## Why the cache crate and nothing else

Not established, and worth saying rather than guessing well. What separates
these six from the other ten is that they are the benchmarks whose work is
dominated by a `HashMap` over owned keys and by the allocator underneath it.
The runner ships a different glibc, and `malloc` is code the count includes.
That fits the shape of the evidence and is not the same as having shown it: a
distribution-versioned build of the two would settle it, and nothing here needs
it settled.

## What this costs

A developer running `scripts/bench.sh` now reads about 4% *below* the baseline
for `cache_put` rather than CI reading 4% above it. The budget is spent in the
other direction, not saved.

That is the right way round. CI is where the count gates a build; a local run is
a measurement someone asked for and is looking at. A gate should be centred on
the environment that enforces it.

## `M59.0`'s acceptance condition, which this also settles

`M59.0` cycled `cache_put` over sixteen keys after the same code read 3,668 and
3,838 on the same runner, a 4.63% spread against a 5% gate, and broke CI on a
commit that did not touch the crate. Its acceptance was that the spread across
CI runs falls below the tolerance, measured rather than assumed.

Six runs: 3,686, 3,689, 3,695, 3,696, 3,701, 3,702. A spread of **0.43%**
against 5%, from 4.63% before. Met.
