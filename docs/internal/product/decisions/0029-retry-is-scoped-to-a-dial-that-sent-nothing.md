# 0029. Retry is scoped to a dial that sent nothing, and stops there

Status: accepted

## Context

A client asked pgprox to retry a transient failure rather than surface it: a
connection refused, a database mid-restart, a brief network blip. The instinct
is reasonable and the request as stated — "retry any transient error, whether
reading or writing" — is not something this proxy can grant safely everywhere
it happens.

The distinction that decides it: whether anything has been sent to a server
that the server might have already acted on. A statement mid-flight cannot be
retried without knowing whether it committed. A connection that failed to
*open* has sent nothing to anyone, on any attempt, because opening is the
whole of what happened. The first case needs to know something this process
cannot always know. The second needs nothing extra at all: the emptiness is
structural, not tracked.

## Decision

Retry applies to exactly one place: `pgprox-pool`'s `LivePool::open`, which
wraps `Connector::connect`. Nowhere else. A statement already sent, a read
already started, a transaction already open — none of these retry, and this
ADR does not propose a design for making them safe to. That is deliberately
left as future work rather than attempted here, because the difficulty is
real: it needs to track, per connection, whether any byte has reached the
server since the last safe point, and get that tracking right on every path
through the relay loop or not attempt it.

**The policy is `pgprox_core::retry::RetryConfig`: attempts, a base delay, a
cap.** Off by default (`attempts: 0`), because retrying is a decision about
how hard to try again on an operator's behalf, and this proxy does not make
that decision unasked. Read from a `retry:` section in the configuration
document, the same way `drain_grace` and `grant_ttl_cap` are.

**Backoff is full jitter**, computed by a pure function,
`pgprox_core::retry::backoff(config, attempt, roll)`, that takes the random
draw as a parameter rather than drawing it. The function is deterministic and
tested exhaustively without a socket or a clock; the roll is supplied by a
`pgprox_pool::jitter::Jitter` trait, implemented in `bin/pgprox` by
`SystemJitter` over the same `aws-lc-rs` provider the cancel-key entropy
source uses, so a FIPS build carries one validated randomness source rather
than two.

**`Jitter` is not `pgprox_session::cancel::Entropy` reused.** That trait's
contract is about a cancel key's unguessability: a bearer token, refused
outright rather than given a predictable fallback. A retry delay defends
nothing; its only job is keeping two callers backing off together from staying
synchronised, which any variation source does. Giving it the cancel-key
trait's security narrative would mislead a reader into thinking jitter needs
that guarantee. It also runs the wrong way across the dependency graph:
`pgprox-session` depends on `pgprox-pool`, so `pgprox-pool` cannot reach
upward for a trait `pgprox-session` defines.

**Retrying needs no "was anything sent" bookkeeping, because the answer is
always no.** `open` either succeeds, and a connection exists, or it does not,
and nothing does. There is no partial state to reason about, which is what
makes this the one place a retry policy can be applied unconditionally rather
than behind a runtime check.

## What was rejected

**Retrying inside the relay loop, for a read or a write with no response byte
yet relayed and no transaction open.** This is the harder, more valuable case
the original request actually asked for, and it needs correctness machinery
this change does not build: tracking, on the live connection, whether the
current statement has had any byte cross the wire since the connection was
last known idle. Attempting it inside this change would have coupled a
well-scoped, easily-proven safe mechanism to a much larger and riskier one.
Left as a named follow-up rather than built partially.

**A metric for retries.** `pgprox-pool` takes no `tracing` dependency today,
and a retry succeeding is meant to be invisible to an operator; what should be
visible is a rate, `pgprox_pool_retry_total` or similar, through
`pgprox-observe`. Reasonable, and a small enough addition that folding it in
without a use for it yet would have been guessing at the metric's shape.

**Hot-reloading the retry policy.** `PoolConfig` is built once, at node
startup, from the document as it stood then. `max_client_conns` and each
server's cap reload without a restart because `M70.0` wired them through the
tick loop; `retry` was not threaded through that path. A document change to
`retry:` takes effect on the next restart, which is worth stating rather than
leaving an operator to discover by watching it not take effect.

## Consequences

- A demoted primary that also refuses new connections outright — as opposed to
  the case ADR 0027 and ADR 0028 already handle, where it answers but reports
  itself in recovery — now has a bounded, configurable number of chances to
  recover before pgprox reports the failure, rather than exactly one.
- The default changes nothing: `attempts: 0` is silent and behaves exactly as
  before this existed.
- `pgprox-pool` gains one new runtime dependency, `Arc<dyn Jitter>`, on every
  `LivePool::new` call site.
