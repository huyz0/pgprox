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
- `unsafe_code`, forbidden at the crate level in every crate

If the buffer slab or the codec ever appears to need `unsafe`, that is a design
review, not a local `#[allow]`.

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
