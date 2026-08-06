# 0026. Unsafe is a governed exception, not a closed door

Status: accepted

## Context

`[workspace.lints.rust]` has carried `unsafe_code = "forbid"` since M0, with a
comment pointing at `standards/rust-style.md`, which said unsafe was "forbidden
at the crate level in every crate". Two things about that were worth changing
and one was worth correcting.

**The sentence was wrong.** It was forbidden once at the workspace root, and
exactly one crate out of sixteen repeated it in its own `lib.rs`. A standard
describing an arrangement that is not there is `M13`'s subject, and this one had
survived twenty-six milestones.

**`forbid` is not `deny`.** It cannot be overridden by a local `#[allow]` under
any circumstances. Every other threshold in this repo is a constant that a
commit message and a number can move: the coverage gate, the session-future
ceiling, the benchmark tolerance, the SCRAM iteration cap. This one could not be
moved by evidence at all, and it became that way as a default rather than as a
decision anybody defended.

**The argument for it is narrower than the lint.** `standards/security.md` says:

> No `unsafe`, so the failure mode of a decoder bug is a wrong answer or an
> error, never memory corruption.

That is an argument about code an unauthenticated peer's bytes reach. It is a
good argument, and it says nothing about the query cache's recency slab or the
buffer pool, which is where `M26` found the costs that were actually worth
money.

## Decision

The workspace lint becomes `deny`. Five conditions govern the exception, and
`scripts/check-unsafe.sh` enforces all five on every commit.

**1. Five crates stay shut**, with `#![forbid(unsafe_code)]` in their own
`lib.rs` where the workspace lint and any `#[allow]` cannot reach them:

| Crate | Why |
| --- | --- |
| `pgprox-proto` | the wire codec, the primary attack surface in the process |
| `pgprox-core` | `sql::Lexer` decides which untrusted text is SQL; `SecretString` |
| `pgprox-route` | classifies untrusted SQL |
| `pgprox-auth` | a JWT header and a SCRAM exchange, both peer-chosen bytes |
| `pgprox-tls` | the path a client's first bytes take |

The list is a judgement about which code a peer's bytes reach, so it is written
down rather than derived. No rule can infer it.

**2. Every exception names a benchmark.** The line above an
`#[allow(unsafe_code)]` reads `// SAFETY-POLICY: <benchmark>`, and that
benchmark exists in `product/perf/baseline.json`. Unsafe with no number is a
liability with no evidence of upside, and this project already has the
machinery: `scripts/bench.sh` measures instruction counts under callgrind and
holds them against a committed baseline.

The comment goes immediately above the attribute rather than anywhere in the
file, so one justification cannot quietly cover a second exception nobody
argued for.

**3. The hygiene lints are denied workspace-wide**: `unsafe_op_in_unsafe_fn`,
`clippy::undocumented_unsafe_blocks`, `clippy::missing_safety_doc`,
`clippy::multiple_unsafe_ops_per_block`. Each turns something a reviewer would
have to notice into something the compiler refuses.

**4. A crate holding `unsafe` is named in the Miri job.** `scripts/miri.sh` and
the `tier 3 - miri` workflow job exist from this commit, before any crate needs
them, and the check keeps the two in step: a crate that grows an `unsafe` block
and is not named there fails the pre-commit gate.

**5. Tests, benches and build scripts may not take the exception.** Nothing in
the four conditions above governs them: they are not on the closed crates'
paths, they are not what a benchmark justifies, and Miri may never reach them.

## What this does not change

**The default is still safe, and the burden is still on the exception.** Five
conditions is deliberately more friction than a code review. The safe construct
comes first and is measured first: iterators, `assert!` before a loop rather
than `debug_assert!`, `chunks_exact`, `split_at_mut`, `bytemuck`,
`with_capacity`, and the release profile. If the unsafe version moves the
benchmark less than `scripts/bench.sh`'s tolerance, it is deleted and the safe
one kept.

**No unsafe is written by this decision.** `M27` produces the conditions and
the script. The first use is a later task with a measurement attached, and it
will have to satisfy every condition here to land.

## Alternatives rejected

**Leave `forbid` alone.** Defensible, and it was never argued for. It also made
the buffer slab's own comment unactionable: `rust-style.md` said "if the buffer
slab or the codec ever appears to need `unsafe`, that is a design review", and
under `forbid` there was no outcome a design review could reach.

**`deny` with nothing else.** An `#[allow]` is one line and a lint anybody can
switch off in one line is a lint that switches itself off. The five conditions
are the decision; the lint change is just what makes them expressible.

**A crate allowlist instead of conditions.** Simpler, and it governs the wrong
thing. Whether unsafe is acceptable depends on what it buys and whether anything
verifies it, not on which directory it sits in. The closed list exists alongside
the conditions because those five crates have an argument that holds regardless
of the measurement.

**Require a human review sign-off in the commit message.** Unenforceable by a
script, which puts it in the same class as non-negotiable 3, and this project
already carries one rule it cannot check. Adding a second would dilute the one
that is marked.

## Consequences

Unsafe is reachable where it can be justified and measured, and unreachable in
the five crates where the security argument holds regardless. The buffer slab
and the cache's slab are now places a design review can actually conclude
something.

The cost is five conditions and a Miri job that runs nightly and interprets
rather than executes. Today it names no crates and says so, which is a skip
rather than a silent pass: a script printing nothing and exiting zero is
indistinguishable from one that ran and found nothing, and `M12` is about
exactly that difference.

`tests/gates/negative.sh` gains a case per condition, because a check nobody has
seen fail is a check nobody knows the failure mode of. Writing them found one:
an `#[allow(unsafe_code)]` on the first line of a file made the script ask `sed`
for line 0, which is an error rather than an empty answer, and under `set -e`
that killed the run instead of failing the check. A gate written to enforce a
rule about care, dying rather than reporting.
