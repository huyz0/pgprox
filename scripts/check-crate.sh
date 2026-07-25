#!/usr/bin/env bash
# fmt and clippy for one crate, or the whole workspace with no argument.
#
# This is the script agent hooks call after editing a .rs file, so it is scoped
# to a single crate on purpose: clippy over the whole workspace on every edit is
# too slow to be useful as in-session feedback.
#
# Usage: check-crate.sh [crate-name]
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

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

finish
