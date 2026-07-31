# What a full fleet tells the clients a dead node displaces

`M11.6`. `M8`'s rolling-upgrade rehearsal killed a node and lost 22 of 21,088
transactions, and said in its own write-up that it does not say what happens
when the survivors are already at their cap. `M11.3` then found that the
mechanism that sentence named, shedding, is refused at the cap by design. What
was left untested is admission, and this is the run for it.

**The answer is nothing. They are served, in about a seventh of a second, at
every point across the kill.** Neither `53300` nor `57014` reaches a client at
any moment of the run. The two refusals the pool is careful to distinguish are
both unreached.

What the run does find is elsewhere, and it is about capacity and about what an
operator sees, not about what a client is told.

## The stack, and the one thing it needed that the e2e stack lacks

Three nodes, a primary capped at 60 connections with `guaranteed_fraction: 0.5`,
two replicas, the mock sidecar. `deploy/docker-compose.fleet.yml` over the
e2e stack, adding exactly two things: the client cap raised so the *upstream*
quota is the binding constraint rather than the door, and one DNS alias that all
three nodes answer to.

The alias is the part without which the question cannot be asked. A displaced
client in production reconnects through a Service and lands on a survivor. A
client pointed at `pgprox-3:6432` reconnects to a socket that is not there and
measures its own retry loop. Docker's embedded DNS returns every running
container holding an alias and drops the ones that are not, which is a Service
in the one respect this needs, and it was checked rather than assumed: with the
victim killed, `getent hosts pgprox` returns the two survivors and not the
corpse.

A thousand clients on the reference workload, 120 seconds, the victim killed at
60. Saturation is asserted before the kill rather than assumed: the run refuses
to proceed unless the fleet holds all 60 upstream connections *and* has callers
queued behind them. Both arms reached it. A quota with no headroom and nothing
waiting would be sixty idle connections, and a client displaced onto that would
be served from one of them for uninteresting reasons.

## Which node to kill, and why it is chosen rather than named

Every node is guaranteed a third of half the cap, ten connections here, and the
other thirty go to whichever node leases them first. Across five runs of this
script that lease landed in three different places and split three different
ways: 11/14/35, 40/10/10, 10/39/11. Killing a node holding its guaranteed ten
would be killing a sixth of the fleet's upstream capacity while calling it a
third.

So the victim is whichever node holds the most upstream capacity at the instant
of the kill. In the run recorded here that was `pgprox-3`, holding 39 of 60.

## What a client is told

One `psql` through the alias, at known offsets from the kill, doing exactly what
a displaced client does. The `-1s` row is the control: a saturated fleet that
has lost nobody.

| when | outcome | seconds |
| --- | --- | --- |
| -1s | served | 0.15 |
| +2s | served | 0.16 |
| +5s | served | 0.15 |
| +10s | served | 0.12 |
| +20s | served | 0.13 |
| +29s | served | 0.13 |

Two seconds after the node holding 39 of the fleet's 60 upstream connections was
killed outright, with 740 callers queued across the two survivors, a new client
connects and gets an answer in 0.16 seconds. The kill is invisible to it.

The mechanism is the one `M11.3` described from the other side. There is no
admission decision to make: the client cap is not binding, so the survivor
accepts. The statement then waits for an upstream connection, and the wait is
short because a queue of 400 in front of 39 connections still drains in
milliseconds when each transaction is milliseconds long. The 30-second acquire
deadline is two orders of magnitude away from being reached.

## What it costs, next to `M8`'s figure

| | transactions | errors | share |
| --- | --- | --- | --- |
| `M8`, three nodes, one killed, not at the cap | 21,088 | 22 | 0.10% |
| this run, at the cap, one killed | 47,743 | 454 | 0.95% |
| this run's control, at the cap, nobody killed | 43,297 | 1 | 0.002% |

Nine times `M8`'s share, and the reason is not that admission behaves worse at
the cap. It is that a node at the cap is carrying in-flight work when it dies:
363 of the 454 are connections that went down with the node, reported as a TLS
peer closing without `close_notify`. The remaining 91 could not finish a startup
within 60 seconds.

Those 91 are not the proxy refusing. The probe is the control that settles it: a
client arriving at the same instant is served in 0.13 seconds. They are the load
generator's own reconnect storm, 340-odd clients retrying at once through one
resolver against two nodes that are each already relaying for five hundred
others. A real fleet behind a Service has the same storm and the same shape; what
this run can say is that the node is not what makes them wait.

The control's single `53300` in 43,298 transactions is worth keeping in view: a
saturated fleet that loses nobody refuses one client in forty thousand.

## The finding that is not about clients: the fleet runs at 50 of 60

`pg_stat_activity` on the primary, which is the only view that cannot be stale.

| t | connections |
| --- | --- |
| -2.0 | 60 |
| +1.2 | 21 |
| +3.5 | 21 |
| +5.7 | 50 |
| +8.1 to +29.8 | 50 |

The kill takes the database from 60 to 21. The survivors lease back up within
about five seconds, and then stop at 50 and stay there for the rest of the run.

Fifty is not a coincidence and it is not a limit anything measured. It is
arithmetic: three nodes times ten guaranteed is thirty reserved, leaving thirty
leasable, and the survivors take all thirty. Ten of the sixty are the dead node's
guaranteed share, held for a node that no longer exists. The run's own view
confirms the split exactly, `pgprox-1` at 10+1 and `pgprox-2` at 10+29.

So a three-node fleet that loses a third of itself runs the database at 83% of
its allowed capacity, for as long as the node stays dead, in exchange for the
guarantee that a returning node finds its share waiting. That is a real
trade-off and it is the design working. It is worth writing down because the
fleet cannot report it: `/v1/servers` says headroom is zero, which is true of
the leasable pool and not of the cap.

## The finding an operator would trip over: the reported view exceeds the cap

The same window, as the fleet reports it against what the database has.

| t | `/v1/servers` reports | database has | cap |
| --- | --- | --- | --- |
| -2.0 | 60 | 60 | 60 |
| +1.2 | 60 | 21 | 60 |
| +5.7 | 60 | 50 | 60 |
| +8.1 onward | **89** | 50 | 60 |

The cluster view is assembled from what every node last gossiped, and a node
that has been killed never gossips again, so its last reading stays in the sum
forever. Eight seconds after the kill the survivors have leased up, their new
numbers arrive, and the dead node's 39 are still being added to them: 89 against
a cap of 60.

**No cap is breached.** The database is holding 50 and never exceeds 60 at any
sample in any run. But an operator watching `/v1/servers` during a node loss
sees a fleet reporting 48% more connections than its cap allows, and stays
seeing it. `/v1/stats` has the same shape, reporting 1,326 clients where there
were 1,000.

This is a reporting defect rather than a quota defect, and the distinction is
only available because the run asked the database. The first version of it did
not, watched the fleet report 81 against 60, and could not say whether that was
a stale view or a breached cap. That is why `primary_conns` is in the sampler.

## What the run got wrong twice before it got this

Both mistakes are the same mistake, and it is worth naming because it is cheap
to make again.

**The client gave up when the server did.** The load client's default connect
timeout is 30 seconds and the proxy's `ACQUIRE_TIMEOUT` is also 30 seconds. The
first run recorded 112 clients saying "startup did not finish within 30s" and
not one SQLSTATE, which reads as the proxy failing to answer and is the client
declining to wait for the answer. A client that gives up when the server does
measures its own patience. The run now gives it twice the server's deadline.

**And the aggregate cannot answer a question about one client.** Even with the
timeout doubled, a thousand connections all reconnecting at once tell you about
the queue behind them rather than about the node in front. The probe is one
client at a time, and it turned a run that looked like "the fleet stops
answering" into a run that says "a client is served in 0.13 seconds". Everything
above rests on that distinction.

## Reproducing

```bash
scripts/admission.sh --no-kill    # the control
scripts/admission.sh              # the run
```

Artefacts land in `target/admission/`: the load report, the sampled fleet views
as JSONL, the probes, and each node's log.
