# What pinning costs multiplexing, and why this run does not answer it

`M11.7`. ADR 0001 says a fleet whose tenants use `LISTEN`/`NOTIFY` collapses
back toward session pooling. This was the run meant to measure the curve. It
measured something real and it is not the curve that was asked for, and the
difference is worth more than pretending otherwise.

```bash
scripts/pinning.sh          # four arms, 150 clients, 120s each, one at a time
```

## What it produced

| arm | pinned sessions | upstream peak | p50 | transactions | errors |
| --- | --- | --- | --- | --- | --- |
| none | 0 | 60 | 4,205us | 46,474 | 0 |
| low | 60 | 60 | 4,359us | 42,073 | 57 |
| mid | 60 | 60 | 3,166us | 28,429 | 90 |
| high | 71 | 60 | 1,546us | 24,586 | 270 |

Against the unpinned arm: transactions **-9.5%**, **-38.8%**, **-47.1%**. Every
error is `53300 too many connections, please retry`.

## Why this is not the curve

**The intended y-axis is flat by construction.** The question was how the
upstream connection count moves as the pinned share rises. It does not move: the
peak is 60 in every arm, including the control, because 60 is the pool's cap and
150 clients reach it without any pinning at all. The script's own header says
the run is "deliberately below saturation ... where the fleet uses well under its
cap", and at 150 clients that is false. The parameterisation contradicts the
design note, and the control arm is the evidence.

**The x-axis is compressed too.** Pinned sessions go 0, 60, 60, 71. Once sixty
sessions pin they hold the entire pool for the rest of their lives, so three
documents with different `LISTEN` weights produce nearly the same number of
pins. The axis saturates a little after the pool does.

**And the medians are not comparable, which the harness says itself.** The `high`
arm's p50 is 63% *lower* than the control's, which reads like an improvement and
is the opposite: that arm refused 270 transactions, and a median taken over the
work an arm kept is a median over the faster half. The report prints this
warning next to the numbers rather than leaving a reader to infer it.

## What it did measure, which is worth keeping

**Pinning's cost appears as refused work, not as more connections.** There is no
headroom for it to appear as connections. Sixty pinned sessions own the pool
outright, every other client waits and then gets `53300`, and the fleet completes
between 9.5% and 47.1% fewer transactions. That is ADR 0001's "collapses back to
session pooling" observed directly, and the error code is what an operator would
actually see.

**The errors are the signal, and a clean curve would have none.** Zero in the
control, then 57, 90, 270. The monotonic climb tracks the pinned share better
than any latency number in the table.

## What the real run needs

A connection count low enough that the pool is demand-driven, so that upstream
connections have somewhere to rise from. The control arm has to sit well under
60, which means well under 150 clients, and probably shorter sessions as well so
the pin count does not saturate at the pool size.

Until then ADR 0001's open question stays open, and `M11.7` stays open with it.
The claim this run supports is narrower and still useful: at a connection count
that already saturates the pool, pinning is paid for in refusals.
