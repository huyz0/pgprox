# 0017. This repository owns the sidecar contract, and it is frozen at v1

Status: accepted

## Context

`proto/pgprox/auth/v1/auth.proto` defines credential resolution. It was written
marked "PROPOSAL, NOT FROZEN" because `standards/contracts.md` requires
agreement from the sidecar owners before the Rust side depends on it, and no
sidecar team exists.

That caveat has now outlived its usefulness. Nothing external implements the
contract, `pgprox-auth` depends on it, and a file that has said "not frozen" for
a while starts reading as "nobody is responsible for this".

## Decision

This repository owns `pgprox.auth.v1`. It is frozen as of now.

The rules that were conditional become unconditional:

- Field numbers are never reused, including for removed fields.
- Fields are never removed, only deprecated.
- Anything optional in the proto is genuinely optional in the Rust type.
- A breaking change means `pgprox.auth.v2`, not an edit to v1.

Freezing costs nothing today, because no external implementation exists to
break. Unfreezing later is impossible once one does, which is the asymmetry
that decides it.

If a sidecar team forms, ownership transfers with the versioning rules already
in force, rather than being negotiated after someone has built against a moving
target.

## Consequences

- `pgprox-auth` can depend on the contract without a caveat, and a sidecar
  implementer in any language has a stable target.
- We give up the ability to renumber fields for tidiness. That is the point.
- A field added carelessly now is permanent. The mock sidecar and the
  round-trip tests are the mitigation: a field with no test is a field nobody
  has checked crosses the wire correctly.
- The `contract-change` skill applies to this file from now on, which means a
  change is a spec change first.

## Alternatives rejected

**Keep it a proposal until a sidecar team exists.** The status quo. Rejected
because the team may never exist, the proxy already depends on the contract, and
a permanent "not frozen" notice is indistinguishable from an unmaintained file.

**Move the proto to its own repository now.** The eventual shape if a separate
team owns the sidecar. Rejected as premature: a second repository with one file
and no second consumer adds release coordination for no benefit. The package
name already carries versioning, so extracting it later is a move rather than a
redesign.

**Version it v0 to signal instability.** Honest about maturity. Rejected because
it invites exactly the careless changes freezing is meant to prevent, and the
proxy is already shipping against it.
