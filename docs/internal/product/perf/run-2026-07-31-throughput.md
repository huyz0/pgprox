# Does the cache raise throughput at saturation, or was that noise

`M11.1`. `M10.9` found the cache 4.2% ahead on transactions at saturation and
declined to claim it, because the sets overlapped by 330 out of 136,000. This
run settles it.

## Why it matters more than 4%

`M9.24`'s explanation for the cache regressing the median rests on one premise:
throughput is pinned by the database, so the cache cannot make the fleet do more
work, and all it can do is change who waits. Every conclusion drawn from that
run and from `M10.9` inherits the premise.

If throughput rises with the cache on, the premise is false. Something other
than the database is limiting this fleet, the redistribution story is at best
incomplete, and two recorded runs need rereading. That is worth more than the
median figures either run produced.

## How many pairs, argued before running them

`M10.9`'s six runs give the spread this needs.

| | mean | sd |
| --- | --- | --- |
| cache off | 134,490 | 3,290 |
| cache on | 140,112 | 3,463 |

Difference 5,622 transactions, 4.2%, pooled sd 3,377, so Cohen's d is 1.66. For
a two-sided test at the conventional 5% with 80% power that needs 6 runs per
arm; 90% needs 8; 95% needs 10.

**Eight per arm.** Three reasons for taking 90% rather than 80%: d is itself
estimated from three runs and is therefore the least reliable number here, so
the true effect is as likely to be smaller as larger; the cost is five more
pairs on a machine that is otherwise idle; and a null result at 80% power is
much weaker than a null result at 90%, which matters because a null is the
outcome that leaves `M9.24` standing.

`M10.9`'s three pairs are the first three. Five more, same workload, same two
thousand connections, same machine, same alternating order.

**Written down before the runs**: the verdict is whatever eight pairs say, not
whatever the first significant result says. No stopping early, no dropping the
cold first run, and if the difference lands inside the noise the answer is a
bound rather than a shrug.

## The numbers

Eight pairs at two thousand connections, alternating, zero errors in sixteen
runs. The first three are `M10.9`'s, unchanged and not re-run.

| pair | off | on | difference |
| --- | --- | --- | --- |
| 1 | 130,710 | 143,216 | +12,506 |
| 2 | 136,052 | 136,377 | +325 |
| 3 | 136,707 | 140,743 | +4,036 |
| 4 | 133,476 | 143,559 | +10,083 |
| 5 | 136,180 | 138,387 | +2,207 |
| 6 | 137,544 | 138,045 | +501 |
| 7 | 131,805 | 142,153 | +10,348 |
| 8 | 135,270 | 139,552 | +4,282 |

**Throughput rises. Eight pairs out of eight.**

Mean 134,718 against 140,254, a difference of 5,536 transactions or 4.11%. The
paired t is 3.28 on 7 degrees of freedom, and the 95% confidence interval on the
difference is +1.14% to +7.08%, which excludes zero. The sign test alone, eight
positives out of eight, is p = 0.008.

The unpaired sets still overlap, 137,544 against 136,377, which is why `M10.9`
declined to claim this and why eight pairs were needed rather than eight more
runs. The pairing is what removes the drift: every pair is an off run and an on
run on the same machine minutes apart, and every one of the eight moves the same
way.

**And the median is still worse, by 16.6%**, 161,074us against 187,874us, with
sets that do not overlap: the worst control at 172,499us beats the best cache-on
run at 176,699us. That reproduces `M10.9`'s 17.5% on eight pairs rather than
three.

### The confound, and what argues against it

Within every pair the control runs first and the cache-on run second, so a
machine that got faster as it warmed would produce exactly this result.

The direct baseline is the control for that. It is measured inside every run,
sixty connections against the database with the proxy not in the path, and it
averages 314us in the off arm and 315us in the on arm. If the second run of each
pair were sitting on a warmer machine, that number would move too. It does not.

That is evidence rather than proof. A stronger design alternates which arm goes
first, and if this result is ever load-bearing for a decision it should be
re-run that way.

## What this changes

**`M9.24`'s premise is false as stated.** It said throughput is pinned by the
database, so the cache cannot make the fleet do more work and can only change
who waits. The fleet does more work: 4% more transactions in the same thirty
seconds, consistently, on a database that is saturated by every other measure.

**Its mechanism survives, and it explains both numbers at once.** Serving 36% of
statements from memory does return those clients to the queue sooner, and the
64% that still reach the database do wait longer for it. What `M9.24` missed is
that the served statements are nearly free rather than merely reordered, so they
add completions on top. The result is a workload split in two: a third of it
answered instantly, two thirds answered more slowly than before. Total
throughput goes up because of the first part. The median goes down because the
median statement is in the second part.

So the sentence "a cache in front of a saturated resource moves work from the
front of the queue to the back of it" is right about where the latency goes and
wrong to conclude that nothing is gained. Both were in the same paragraph and
only one of them was measured.

**What it does not change.** The cache is still a latency regression at
saturation, by 16.6% here and 17.5% in `M10.9`, and still an improvement below
it, by 24.4% in `M10.5`. An operator choosing to turn it on is choosing more
throughput and a worse median, which is a real trade and a different one from
what `M9.24` described.

`M9.24`'s document is left as it was written. A run is a record of what was true
when it was taken; the correction belongs here and in the roadmap.
