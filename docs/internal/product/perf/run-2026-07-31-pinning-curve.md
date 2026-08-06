# The pinning curve

`M11.7`. The second run. The [first](run-2026-07-31-pinning.md) measured
something real and it was not the curve: at 150 clients the control arm already
peaked at the sixty-connection cap, so upstream had nowhere to rise and the
y-axis was flat by construction.

```bash
scripts/pinning.sh          # four arms, 40 clients, 120s each, one at a time
```

## What changed, and why forty

The old parameterisation read the mean, 37 of 60, and called that "below
saturation". The mean is not the condition. A pinned session takes an upstream
connection out of circulation permanently, so the quantity that has to have room
is the *peak*, and the peak was already at the cap.

Forty comes from the calibration the old note itself quoted and then argued
past: forty clients hold twelve of the sixty, four hundred hold all sixty.
Twelve leaves the entire cap above it. It fixes the x-axis at the same time,
because at most forty sessions can pin and forty is under the cap, where at a
hundred and fifty the pin count saturated at the pool size and three documents
with different weights produced 60, 60 and 71.

The control arm's peak is now a guard rather than a paragraph. A control at the
cap fails the run and tells the operator to lower the connection count.

## Predictions, written before the run

Recorded first so the run can contradict them.

1. **Upstream rises with the pinned share, from about 12 to about 40.** This is
   the y-axis the first run could not produce. The high arm should approach the
   client count, because a session that pins holds its connection for the rest of
   its life and nearly every session in that arm pins early.
2. **No `53300` in any arm.** The first run's errors were the pool refusing a
   queue it could not serve. With forty clients under a cap of sixty there is no
   queue to refuse. Any error that does appear is a finding, not the cost of
   pinning.
3. **The pin counts spread**, roughly 0, 16, 30, 40. The documents differ in one
   weight, `watch_channel` at 1, 2 and 20 against about 1,000 for everything
   else, which places the first `LISTEN` near statement 1,001, 501 and 51 of a
   session that executes on the order of 500 to 600 statements inside the run.
   The low arm's sessions mostly never reach their first pin; the high arm's
   nearly all pin in the first tenth of their life.
4. **The median moves much less than the first run's did**, because with no
   refusals every arm's median is taken over comparable work. Some rise is
   expected as fewer connections are left to share.

The interesting outcome is where clients per upstream connection approaches one,
which is the point ADR 0001 describes as collapsing back to session pooling.

## What it produced

| arm | pinned | upstream peak | clients per connection | p50 us | p99 us | transactions | errors |
| --- | --- | --- | --- | --- | --- | --- | --- |
| none | 0 | 14 | 2.9 | 1,455 | 36,099 | 17,041 | 0 |
| low | 24 | 30 | 1.3 | 2,260 | 30,499 | 17,111 | 0 |
| mid | 36 | 39 | 1.0 | 2,181 | 23,799 | 17,170 | 0 |
| high | 40 | 40 | 1.0 | 1,806 | 87,199 | 16,894 | 0 |

Counts in [curve-2026-07-31-pinning.tsv](curve-2026-07-31-pinning.tsv).

Predictions 1 and 2 held. Upstream rose 14 to 40 and no arm saw a single error.
Prediction 3 was directionally right and numerically over on two arms: pins came
in at 0, 24, 36, 40 against 0, 16, 30, 40. Prediction 4 is discussed below and
is the part of this run not to trust.

## The answer: there is no crossing point, because the cost is linear

The question was where multiplexing stops paying for itself. The honest answer
is that it does not stop anywhere in particular. It degrades linearly, from the
first pinned session.

Fit the obvious model, in which a pinned session takes one upstream connection
for good but gives back the share it was already consuming:

```
upstream(pins) = 14 + (1 - 1/2.857) * pins  =  14 + 0.650 * pins
```

Both constants come from the control arm alone, so the model has **zero free
parameters** with respect to the three pinned arms.

| pins | measured | model | residual |
| --- | --- | --- | --- |
| 0 | 14 | 14.0 | +0.0 |
| 24 | 30 | 29.6 | +0.4 |
| 36 | 39 | 37.4 | +1.6 |
| 40 | 40 | 40.0 | +0.0 |

R^2 = 0.9937.

So each pinned session costs **0.650 upstream connections**, and there is no
knee, no threshold, and nothing that behaves like a safe pinned share below
which multiplexing is unaffected. Half the multiplexing benefit is gone at 54%
pinned, which is a point on a line rather than a transition.

The `high` arm is the degenerate case stated exactly: 40 clients, 40 pinned
sessions, 40 upstream connections, peak equal to mean equal to the client count,
clients per connection 1.000. ADR 0001 says such a fleet "collapses back to
session pooling". It is not an analogy. One upstream connection per client is
the definition of session pooling, and that is what the database reported.

## What this run says that the first one could not

The [first run](run-2026-07-31-pinning.md) concluded that pinning is paid for in
refused work. That is true at saturation and it is not the general case. It was
a statement about a pool with no headroom, not about pinning.

With headroom, **throughput is flat**: 17,041, 17,111, 17,170, 16,894
transactions, a spread of 1.6% with no ordering, while upstream connections
nearly tripled. Pinning does not cost throughput. It costs the resource that
lets one node serve many clients, and it costs it linearly. Those are different
claims and the first run could not separate them because at 150 clients the pool
was already at its cap, so the only way the cost could appear was as `53300`.

The two runs compose: pinning consumes connections at 0.65 each, and the
refusals the first run measured are what happens after that consumption reaches
the cap.

## What not to take from this

**The latency columns.** One run per arm, no repetition, and they do not order:
p50 goes 1,455, 2,260, 2,181, 1,806, so the fully pinned arm reads *better* than
the two mixed ones, and p99 goes 36,099, 30,499, 23,799, 87,199, where two
pinned arms beat the control. There is a plausible story here, that a mixed arm
is worst because pinned sessions hold connections while unpinned clients queue
behind them, and it is a story rather than a finding: it does not explain the
p99 ordering and one sample per arm cannot separate any of it from noise.
Prediction 4 guessed the median would move less than in the first run and it
did, but that prediction should not be counted as confirmed by numbers this
unreplicated.

The connection columns are the measurement. They are structural, they fit a
zero-parameter model to R^2 = 0.994, and they are what the question asked for.

## The guard this run added

The control arm's peak is now checked against the cap. A control at the cap
fails the run with the instruction to lower the connection count, because from
there every arm reports the cap and the curve is flat for a reason that has
nothing to do with pinning. The first run passed every other check in this
script and produced exactly that table.
