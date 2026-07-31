#!/usr/bin/env bash
# fmt and clippy for one crate, or the whole workspace with no argument.
#
# This is the script agent hooks call after editing a .rs file, so it is scoped
# to a single crate on purpose: clippy over the whole workspace on every edit is
# too slow to be useful as in-session feedback.
#
# Usage: check-crate.sh [crate-name]
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

# Skipped only by `tests/gates/negative.sh`, which invokes a gate once per
# broken artefact to prove the gate fails. Those cases exercise a gate's own
# logic; re-running cargo for each of them adds nothing and costs 84 seconds an
# invocation, which put the suite past any budget CI would accept. `M12.11`.
#
# CI runs this script in tier 1 regardless, so nothing goes unchecked, and
# `check-drift.sh` fails if `ci.yml` or the pre-commit config ever sets the
# variable. It announces itself on stderr because a knob that turns off the
# coverage gate should never fire quietly.
if [[ -n "${PGPROX_SKIP_DELEGATED_CHECKS:-}" ]]; then
  printf 'PGPROX_SKIP_DELEGATED_CHECKS is set: %s did not run\n' "$(basename "${BASH_SOURCE[0]}")" >&2
  exit 0
fi

CRATE="${1:-}"

if ! has_rust; then
  skip "clippy (no Cargo.toml yet)"
  finish
fi

require_tool cargo || finish

if [[ -n "$CRATE" ]]; then
  scope=(-p "$CRATE")
  label="$CRATE"
else
  scope=(--workspace)
  label="workspace"
fi

if cargo fmt "${scope[@]}" --check 2>/dev/null || cargo fmt --all --check; then
  ok "fmt ($label)"
else
  fail "fmt ($label): run 'cargo fmt --all' to fix"
fi

if cargo clippy "${scope[@]}" --all-targets --all-features -- -D warnings; then
  ok "clippy ($label)"
else
  fail "clippy ($label)"
fi

# And again with the features a release build actually has. `--all-features`
# turns on `test-fakes`, so an import used only by a fake reads as used and a
# plain `cargo build` warns where this said nothing. That is how an unused
# import sat in pgprox-core from M4 until the M6 review found it.
if cargo clippy "${scope[@]}" --all-targets -- -D warnings >/dev/null 2>&1; then
  ok "clippy, default features ($label)"
else
  fail "clippy, default features ($label): run 'cargo clippy${scope[*]:+ ${scope[*]}} --all-targets'"
fi

# Doctests are not part of the nextest run that the coverage gate uses, so
# without this they are never executed by anything. That matters here because
# compile_fail doctests are how type-level guarantees are proven: an ID newtype
# that silently became interchangeable with another would go unnoticed.
if cargo test --doc "${scope[@]}" >/dev/null 2>&1; then
  ok "doctests ($label)"
else
  fail "doctests ($label): run 'cargo test --doc' for detail"
fi

# Rustdoc warnings are not clippy warnings and nothing else catches them. A
# broken intra-doc link is a reference that silently stops resolving, which is
# how documentation rots: the prose still names the thing, but the reader can no
# longer get to it. Found when a link to another crate's type compiled and
# tested clean while pointing at nothing.
if RUSTDOCFLAGS="-D warnings" cargo doc "${scope[@]}" --no-deps --all-features >/dev/null 2>&1; then
  ok "rustdoc ($label)"
else
  fail "rustdoc ($label): run 'cargo doc --no-deps' for detail"
fi

finish
