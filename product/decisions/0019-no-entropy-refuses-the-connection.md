# 0019. A node with no entropy refuses connections rather than degrading

Status: accepted

## Context

A cancel key is a bearer token. Anything holding one can cancel the query it
names, and protocol 3.0 gives it 32 bits of secret and no authentication at
all. ADR 0016 deferred the 256-bit keys of protocol 3.2, and `M1F.36` recorded
what follows from that: the part of a `ConnId` that is not the node number must
be unguessable, because a counter lets one tenant cancel another's queries by
trying numbers near its own.

`pgprox-session` gets those bits from an `Entropy` trait so the crate stays
testable without a random number generator, and the composition root fills it
in. `M6.27` wrote the first real implementation, over the `aws-lc-rs` system
source that rustls and SCRAM already use, and ran straight into the gap: the
trait returned a `u64`, so a source that could not produce bits had three
options and no good one.

- **Panic.** On a connection path, in a proxy holding a hundred thousand of
  them. The worst place in the process for one.
- **Return a fixed value.** Every connection gets the same cancel key, which is
  both guessable and colliding, and the client cannot tell.
- **Return a counter.** Exactly what `M1F.36` says must not happen.

The common thread is that all three keep serving, and the client has no way to
know it was handed a token that is not secret.

## Decision

`Entropy::next` returns `Option<u64>` and `Registry::issue` returns
`Option<ConnId>`. A node whose entropy source has nothing refuses the
connection with a new `ClientError::Internal`, which maps to SQLSTATE `XX000`.

The client is told "internal error" and nothing more, like every other client
message in the taxonomy. Which internal condition failed is an operator's
business, and a prober's gift.

### Rejected: keep the signature and log loudly

A log line does not stop the connection being served with a guessable key, and
the property at stake is one that fails silently for as long as nobody is
attacking and then fails completely once somebody is.

### Rejected: a new error variant per internal condition

`XX000` is the code for "this is ours and no client action fixes it". A second
variant would only be worth it if a client should act differently, and by
definition it should not. `Internal` carries a `&'static str` for the operator,
which is where the specificity belongs.

## Consequences

- `ClientError` gains a variant. The enum is `#[non_exhaustive]`, so nothing
  matching on it breaks, and the SQLSTATE table in
  `standards/error-handling.md` gains the row.
- `Entropy` and `Registry::issue` change signature. Both live in
  `pgprox-session`, whose only consumer is the binary, so the blast radius is
  two crates rather than the workspace.
- A machine with a broken entropy source now fails visibly, at the first
  connection, with a code an operator can search for. It previously would have
  served every client a cancel key of zero.
- `XX000` exists in the taxonomy now, which is a thing to watch: an error with
  a real code that gets reported as `XX000` sends the operator to the wrong
  place. Adding a second use of it deserves the same argument this one had.
