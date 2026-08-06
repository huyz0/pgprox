# Protocol 3.2 and 256-bit cancel keys

Status: **proposed, not started.** `standards/contracts.md` requires stopping
before a change that crosses tracks, so this exists to make the blast radius
visible before anyone edits.

## Problem

Postgres 18 introduced wire protocol 3.2, the first new minor version since
2003. Its substantive change is the cancel key: 3.0 fixes it at two `int32`s,
and 3.2 allows up to 256 bits.

This proxy currently negotiates 3.2 clients *down* to 3.0
(`startup::negotiate_version`), which every 3.2-capable driver accepts by
design. That works and is not a bug. It does mean the proxy never benefits from
the longer key.

The 3.0 key is 64 bits, of which we spend 16 on the node ID, leaving 48 for the
counter. A cancel key is a bearer token: anyone holding it can cancel that
query. 48 bits of counter is not a secret, it is a sequence number, so today's
cancel keys are guessable by design.

**That is the real reason to want 3.2, and it is a security improvement rather
than a compatibility one.** The current scheme is only acceptable because a
cancel request must also reach the proxy and can at worst cancel a query.

## Scope

In:

- `ConnId` widened to carry a 256-bit key alongside its node ID.
- `BackendKeyData` encoding for both 3.0 and 3.2.
- `CancelRequest` decoding for both.
- `negotiate_version` accepting 3.2 rather than negotiating down.
- The 3.0 path kept intact, because most clients still speak it.

Out:

- `_pq_.` protocol extension parameters. Related but separable; M1F.16.
- Any change to how a cancel is routed between nodes. The node ID stays encoded
  in the key; only its width changes.

## Blast radius

Measured, not estimated: `ConnId` appears in six files and 26 non-test call
sites, all within `pgprox-core` and `pgprox-proto`.

That is smaller than the plan assumed, because no other crate holds a `ConnId`
yet. **Doing this before M6 is therefore much cheaper than after**, when
`pgprox-session`, `pgprox-admin` and `bin/pgprox` will all carry one.

| Crate | What changes |
| --- | --- |
| `pgprox-core` | `ConnId` representation, constructors, accessors, tests |
| `pgprox-proto` | `key_from_conn_id`, `conn_id_from_key`, `backend_key_data`, `CancelRequest` decode, `negotiate_version` |

## Acceptance criteria

Given a client requesting protocol 3.0
When it authenticates
Then it receives an 8-byte `BackendKeyData` and its `CancelRequest` cancels its
query.

Given a client requesting protocol 3.2
When it authenticates
Then it receives a longer `BackendKeyData` and its `CancelRequest` cancels its
query.

Given a cancel key issued by any node at either version
When a `CancelRequest` arrives at a different node
Then that node decodes the owning node ID and forwards it.

Given two connections on the same node
Then their cancel keys differ in more than a counter, so one cannot be derived
from the other.

## Open question for a human

**Is the 3.0 key's guessability acceptable in the meantime?**

Widening it is possible without 3.2 by filling the 48 counter bits with
randomness instead of a sequence, which costs nothing and is strictly better
than today. That is a smaller change than this spec and could ship first.

I would do that first and treat 3.2 as the follow-up, but it changes what a
cancel key means and is worth a decision rather than an assumption.

## Tasks, once approved

1. Make the 3.0 counter random rather than sequential, keeping the 64-bit shape.
2. `ConnId` carries a variable-width key; 3.0 remains a special case of it.
3. Encode and decode `BackendKeyData` at both widths.
4. Decode `CancelRequest` at both widths, dispatching on message length.
5. `negotiate_version` accepts 3.2.
6. Conformance: both versions against real Postgres 18, cancel working in each.
