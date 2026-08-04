# Rust style

Enforced by `cargo fmt` and the workspace clippy config. Where a rule here is
not machine-checkable it says so, and those are the ones worth reading.

## Formatting

`rustfmt.toml` is checked in and non-negotiable. `scripts/check-fmt.sh` runs it
in check mode. Do not argue with the formatter and do not add `#[rustfmt::skip]`
without a comment saying why.

## Lints

Workspace lints live in the root `Cargo.toml` under `[workspace.lints.rust]` and
`[workspace.lints.clippy]`, and every crate opts in with `[lints] workspace =
true`. Denied beyond the defaults:

- `clippy::pedantic`, with a short allowlist for the lints that fight async code.
  Every entry in that allowlist carries a comment explaining the case.
- `clippy::unwrap_used` and `clippy::expect_used` outside `#[cfg(test)]`
- `clippy::todo`, `clippy::dbg_macro`, `clippy::print_stdout`,
  `clippy::print_stderr`
- `unsafe_code`, denied at the workspace root

## Unsafe is a governed exception

This said "`unsafe_code`, forbidden at the crate level in every crate", which
described an arrangement that was not there: it was forbidden once in
`[workspace.lints.rust]` and exactly one crate repeated it in its own `lib.rs`.
`M27.1` corrected the sentence and changed what it says.

`forbid` cannot be overridden by a local `#[allow]` at all, which made it the
one threshold in this repo that no measurement could reopen. It is now `deny`,
and an exception has to get past five conditions that
[`scripts/check-unsafe.sh`](../scripts/check-unsafe.sh) enforces:

1. **Five crates stay shut**, with `#![forbid(unsafe_code)]` in their own
   `lib.rs` where no `#[allow]` reaches them: `pgprox-proto`, `pgprox-core`,
   `pgprox-route`, `pgprox-auth`, `pgprox-tls`. Each of them reads bytes a peer
   chose. See [security.md](security.md).
2. **Every `#[allow(unsafe_code)]` names a benchmark** on the line above it, as
   `// SAFETY-POLICY: <benchmark>`, and that benchmark exists in
   `product/perf/baseline.json`. Unsafe with no number is a liability with no
   evidence of upside.
3. **The hygiene lints are on** and denied workspace-wide:
   `unsafe_op_in_unsafe_fn`, `clippy::undocumented_unsafe_blocks`,
   `clippy::missing_safety_doc`, `clippy::multiple_unsafe_ops_per_block`.
4. **A crate holding `unsafe` is named in the Miri job.** Unsafe without Miri
   in CI is unsafe nobody can maintain.
5. **Tests, benches and build scripts may not take the exception.** Nothing in
   the four conditions above governs them.

Try the safe construct first, and measure both. Iterators, `assert!` before a
loop rather than `debug_assert!`, `chunks_exact`, `split_at_mut`, `bytemuck`,
`with_capacity`, and the release profile itself all reach unsafe-level speed
often enough that reaching for `unsafe` first is usually the slower route to the
same number. If the unsafe version moves the benchmark less than the tolerance
`scripts/bench.sh` holds, delete it and keep the safe one.

See ADR [0026](../product/decisions/0026-unsafe-is-a-governed-exception.md).

## Module layout

- `lib.rs` holds the crate's public surface and its module declarations. It
  contains no logic beyond re-exports.
- One concept per module. A module that needs a table of contents comment to
  navigate is two modules.
- Public items carry doc comments. `#![warn(missing_docs)]` in every crate.
- Tests live in a `#[cfg(test)] mod tests` at the bottom of the file they test,
  except integration tests, which live in `tests/` behind the `integration`
  feature.

## Naming

- Crates are `pgprox-<area>`, modules and functions are `snake_case`, types are
  `UpperCamelCase`. Nothing surprising.
- Avoid abbreviations that are not already Postgres or networking vocabulary.
  `lsn`, `tls`, `swim`, `tx` are fine. `mgr`, `cfg`, `hdlr` are not.
- A type named `FooManager` or `FooHandler` usually means the responsibility was
  not identified. Name what it does.
- Boolean parameters are banned in public functions. Use a two-variant enum, so
  the call site reads `Pool::acquire(key, Blocking::No)` rather than
  `Pool::acquire(key, false)`.

## Types

- Newtype anything that would otherwise be a bare `String` or `u64` crossing a
  module boundary: `TenantId`, `NodeId`, `PoolKey`, `Lsn`. The compiler catching
  a swapped pair of arguments is worth the ten lines.
- Prefer `&str` and `&[u8]` in arguments, owned types in returns.
- Do not derive `Debug` on anything holding a credential. See
  [security.md](security.md).
- `#[non_exhaustive]` on public enums that describe external state, so adding a
  variant is not a breaking change.

## Comments

Comment why, not what. A comment restating the code is noise that goes stale.
The comments worth writing are the ones explaining a non-obvious constraint: why
this lock order, why this buffer size, why this apparently redundant check is
load-bearing.

Wire-protocol code is the exception. There, cite the Postgres documentation
section or message name for anything a reader would otherwise have to reverse
engineer.
