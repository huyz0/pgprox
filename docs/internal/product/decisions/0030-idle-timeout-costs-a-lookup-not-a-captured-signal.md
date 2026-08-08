# 0030. The idle timeout costs a lookup, not a captured future

Status: accepted

## Context

An authenticated client that sends nothing for a long time is holding a slot
under `max_client_conns` for no reason an operator asked for. The request was
a configurable timeout: close it, the same way a demoted node's shed already
closes a client that belongs elsewhere.

`Sessions` already had the mechanism this needed. `shed` fires a per-session
`Shutdown`, watched in the relay loop's own `select!` and gated `if idle`, so
it never fires mid-transaction. Reusing that shape for idle timeout looked
free, and it was not.

`bin/pgprox/src/serve.rs`'s per-connection session future carries an exact,
gated size: `one_session_costs_less_than_the_slab_buffer_it_no_longer_holds`
asserts under 5 KiB, and `M9.23` left it at 5,048. A future is the union of
everything alive across its `await`s, and the relay loop is nothing but
`await`s, so anything a `select!` branch captures is paid by every connection
whether or not that connection ever uses the branch.

## What was measured, in order

**A second `tokio::sync::watch`-based `Shutdown`, one for shed and one for the
idle timeout, each its own `select!` branch.** 5,288 bytes. 240 more than the
baseline, against 72 bytes of headroom before the ceiling. Worse than either
alternative below, because a `watch::Receiver`'s `changed()` future is not a
small type and this held two.

**`tokio::time::sleep(timeout)` as a branch, timer held directly, no second
`Shutdown`.** 5,224 bytes. 176 more. Cheaper than two signals, still over the
ceiling.

**One shared `Shutdown`, already there for `shed`, plus a small
`Arc<AtomicBool>` flag read synchronously the moment it fires, still passed
into the relay loop as a captured reference.** 5,136 bytes. 88 more. Under the
ceiling by 16 bytes, closer than either prior attempt to failing again on the
next unrelated change to this loop.

**The same `Shutdown`, the same flag, but the flag reached through
`Sessions::was_idle_timeout(conn)` — a lookup at the moment `shed` wakes,
reading `context` and `conn`, both already part of the loop's captured state —
rather than a reference threaded into `relay` and held across every
subsequent `await`.** 5,112 bytes. 64 more than the baseline, with a real
margin rather than a bare pass.

## Decision

One signal. `Sessions::shed` and `Sessions::close_idle` both fire the same
per-session `Shutdown`; a client closed either way is told 57P01 or 57P05
through the identical `select!` branch, gated `if idle` for the identical
reason drain and shed already are — closing a client mid-transaction is the
one thing this proxy does not do for a reason under its own control.

Which of the two reasons fired is answered by
`Sessions::was_idle_timeout(conn)`, a registry lookup made once, at the moment
the branch wakes, rather than by anything the relay loop carries across its
own suspension points. `conn: ConnId` and `context: &Context` were both
already part of that state before this existed; nothing new joins it.

The actual timer is not in the relay loop at all. `client_idle_timeout` is a
document field, read once at node startup into `Context` the same way
`retry` is (`M73.0`, and not hot-reloaded for the same stated reason), and
acted on by `idle_timeout_pass`, a function the existing tick loop calls
alongside `shed_pass` once a second. It walks `Sessions::views`, the same list
`shed_pass` already walks, and calls `close_idle` on whichever clients have
been idle at least that long. One `Instant` comparison per idle client per
second, in a walk that already happens, replaces a per-connection timer that
every connection would have paid for whether or not it was configured.

## What was rejected

**A second `Shutdown`.** Measured first, above, and it was the worst of the
three real attempts.

**A `tokio::time::Sleep` held in the loop.** Measured second, and it does not
explain itself in a way that helps the next person reading the `select!`: a
raw timer next to two `Notify`-backed signals reads as an inconsistency
without the history in this ADR.

**A flag threaded as a parameter into `relay`.** The version that measured
5,136. It works and it very nearly failed the budget again on the very next
change to this function, which is a fragile place to leave a passing test.
The lookup version costs less and is not one accidental capture away from
failing.

## Consequences

- `Sessions::register` takes one more argument, `idle: Arc<AtomicBool>`,
  alongside the `close: Shutdown` it already took. Every call site updated in
  the same commit.
- `pgprox_core::error::ClientError` gains `IdleTimeout`, mapped to Postgres's
  own `57P05`/`idle_session_timeout` and its own message, on the theory that a
  driver already handling that GUC from a real Postgres server should not need
  to learn a second code to handle the same closure from a proxy in front of
  it.
- The default is off (`client_idle_timeout: None`): a connection pool sitting
  on a deliberately idle connection is ordinary, and this must not close it
  out from under an operator who never asked for the behaviour.
- The session future is 5,112 bytes, up from 5,048, against a 5,120-byte
  ceiling. The margin is 8 bytes. The next change to `relay`'s `select!`
  should expect to measure it.
