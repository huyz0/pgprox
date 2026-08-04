# Two thirds of the query decode is a check the policy will not let anyone skip

Date: 2026-08-05. `M30.5`. No code changed.

`M30` ran the unsafe procedure across the workspace and found three costs worth
removing, none of them a bounds check and none of them needing unsafe. This is
the fourth, and it is the one where unsafe is the only answer and the answer is
still no.

## The measurement

A callgrind run at N iterations subtracted from one at 2N, per function, on the
same binary `scripts/bench.sh` uses.

| path | total | `str::from_utf8` | share |
| --- | --- | --- | --- |
| `decode_query` | 390 | 262 | 67% |
| `decode_error_response` | 1,884 | 802 | 43% |

Both go through one line, `Reader::cstr` in `crates/pgprox-proto/src/read.rs`:

```rust
let end = memchr::memchr(0, rest).ok_or(FieldError::Unterminated)?;
let text = std::str::from_utf8(&rest[..end]).map_err(|_| FieldError::NotUtf8 { what })?;
```

The `memchr` is the crate this project already took a dependency on for exactly
this scan, and it is the smaller half. The validation is the larger one.

The rate is about 1.06 instructions per byte on 256 bytes of generated SQL,
which is std's ASCII fast path working. It is not a slow implementation being
caught out; it is a linear pass over every byte of every statement, and the
statement is the largest thing on the wire in the client-to-server direction.

## What would remove it

`std::str::from_utf8_unchecked`. That is the whole change: the bytes are already
there and the `&str` is the same bytes with a promise attached.

It is refused, and `scripts/check-unsafe.sh` refuses it without anybody having
to argue. `pgprox-proto` is the first entry on `ADR 0026`'s closed list and
carries `#![forbid(unsafe_code)]` in its own `lib.rs`, where neither the
workspace lint nor an `#[allow]` can reach it.

The argument for that entry is in `standards/security.md`:

> No `unsafe` **in the crates that read bytes a peer chose**, so the failure
> mode of a decoder bug is a wrong answer or an error, never memory corruption.

This is precisely the case it describes, and the failure mode is worse than the
general form of that sentence suggests. A `&str` holding invalid UTF-8 is
undefined behaviour immediately, not on use: every later slice of it may split
a character it believes exists, and the peer chooses the bytes. It would be a
memory-safety bug reachable by an unauthenticated client sending one malformed
`Query`.

## What would reduce it without unsafe

Nothing that is on the table, and the two candidates were measured rather than
dismissed.

**A vectorised pre-check does not help.** The obvious idea is that most SQL is
ASCII, so a cheap `is_ascii()` could take a fast path. On the same 256 bytes:

| | instructions |
| --- | --- |
| `str::from_utf8` | 271 |
| `[u8]::is_ascii` | 307 |
| neither | 3 |

`is_ascii` is *slower* than the full validation it was meant to avoid, so there
is no fast path here to take. std's validator is already the better of the two
things std offers.

**A SIMD validator is a dependency.** `simdutf8` would plausibly cut this
severalfold, and it is a new crate on the path that reads untrusted bytes, which
is where `scripts/check-deps.sh` is strictest. That is a trade worth someone
proposing on its own terms with a number attached; it is not a thing to slip in
under a performance milestone.

**The validation is not duplicated.** Worth checking rather than assuming, since
a redundant second pass would have been a safe win. `cstr` validates once, and
everything downstream takes `&str`: `sql::Lexer`, the classifier, and the
cache's normalizer all operate on the validated text and none of them revalidate
it.

## What this is

The closed list was justified in the abstract, in `M27`, before anything had
been measured against it. This is the number it was bought with: about 262
instructions per `Query` and 802 per `ErrorResponse`, on every statement of
every connection.

That is a real price and it is the right trade. It is worth writing down anyway,
for two reasons. A policy whose cost is unknown is a policy nobody can revisit
honestly, and this one should be revisitable: if a SIMD validator ever clears
the supply-chain gate, this page says what it would be worth.

And `M29` closed by saying four of the procedure's five patterns were untested
here. This tests the fifth, zero-copy reinterpretation, and finds the workspace's
best candidate for it sitting inside the one place the workspace has decided not
to look.

## What this does not say

It does not say `pgprox-proto` should be reopened. Every condition in `ADR 0026`
held, the script refused the exception without being asked, and the crate on the
other side of the refusal is the one an unauthenticated peer's first bytes reach.

It does not say the remaining 128 instructions of `decode_query` are optimal.
They were not the subject; 106 of them are `memchr`, which is already SIMD, and
the rest is the frame arithmetic around it.
