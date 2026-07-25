#!/usr/bin/env bash
# Workspace formatting. No-ops cleanly before M0 when there is no Rust yet.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

if ! has_rust; then
  skip "cargo fmt (no Cargo.toml yet)"
  finish
fi

require_tool cargo || finish

if cargo fmt --all --check; then
  ok "cargo fmt"
else
  fail "cargo fmt: run 'cargo fmt --all' to fix"
fi

finish
