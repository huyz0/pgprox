# 0016. Protocol 3.2 is negotiated down, deliberately, until a trigger fires

Status: accepted

## Context

Postgres 18 introduced wire protocol 3.2, the first new minor version since
2003. Its substantive change is the cancel key: 3.0 fixes it at two `int32`s,
3.2 allows up to 256 bits.

This proxy negotiates 3.2 clients down to 3.0 with `NegotiateProtocolVersion`,
which every 3.2-capable driver handles by design. That was written as a
placeholder. This records it as a decision.

Two facts shaped the answer, and both emerged from measuring rather than
guessing.

**The blast radius is small now and grows later.** `ConnId` appears in six files
and 26 non-test call sites, all inside `pgprox-core` and `pgprox-proto`, because
no other crate holds one yet. After M6, `pgprox-session`, `pgprox-admin` and
`bin/pgprox` will each carry one. Doing this later costs more.

**The security argument already got solved another way.** The reason to want a
wider key was that a cancel key is an unauthenticated bearer token and ours was
a counter. ADR 0013's companion fix (M1F.36) makes the 48-bit field a random
secret. Guessing one now means 2^48 attempts, each requiring a TCP connection to
this proxy, to cancel a single query. That is not a real attack.

## Decision

Keep negotiating 3.2 down to 3.0. Do not implement 3.2 yet.

Revisit when any of these fires, and not before:

1. A mainstream driver appears that refuses to negotiate down.
2. Postgres announces deprecation of 3.0.
3. A 3.2-only feature arrives that this proxy needs.

`specs/2026-07-25-protocol-3-2-cancel-keys/spec.md` stays, so the work is
designed and measured when a trigger fires rather than re-derived.

## Consequences

- Nothing a tenant can observe changes. Their driver negotiates down
  transparently, which is what that mechanism exists for.
- `ConnId` stays a fixed 64 bits, so every crate that touches it stays simple.
  That is the real saving: a variable-width key would be carried by every
  downstream crate permanently.
- Supporting 3.2 later means maintaining *both* widths forever, since 3.0
  clients are not going away. Deferring avoids paying that until it buys
  something.
- We accept a higher future cost. The blast radius will be roughly six crates
  rather than two. That is the price of not building something whose only
  beneficiary is a cancel-key entropy question already answered.
- The `m1f-complete.sh` gate now asserts *this decision* rather than assuming
  3.2 support: it checks that 3.2 is negotiated down correctly and that this ADR
  exists. A gate encoding a presumed answer would have quietly forced the
  opposite decision.

## Alternatives rejected

**Implement 3.2 now, because it is cheapest now.** The strongest case, and it
is an argument about cost rather than value. Building something that buys
nothing observable, at permanent complexity cost to every downstream crate, is
worse than paying more later for something we are then sure we want. "Cheap" is
not a reason on its own.

**Implement 3.2 and drop 3.0.** Would avoid carrying two widths. Rejected
outright: the overwhelming majority of clients speak 3.0 and would stop working.

**Leave it undecided and let the gate keep failing.** What was happening. A
permanently red gate trains people to ignore gates, which is worse than either
answer.
