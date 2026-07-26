#!/usr/bin/env bash
# The coverage gate. Per crate, never workspace-wide: a single global number
# lets a 99% crate mask a 70% one.
#
# Usage: check-coverage.sh [crate-name]
#   With a crate name, gates that crate only (what agent hooks call).
#   With no argument, gates every workspace member independently.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

CRATE="${1:-}"

if ! has_rust; then
  skip "coverage (no Cargo.toml yet)"
  finish
fi

require_tool cargo || finish
if ! cargo llvm-cov --version >/dev/null 2>&1; then
  fail "missing cargo-llvm-cov  (install: cargo install cargo-llvm-cov)"
  finish
fi
if ! cargo nextest --version >/dev/null 2>&1; then
  fail "missing cargo-nextest  (install: cargo install cargo-nextest)"
  finish
fi

# Generated code and the composition root's main.rs are the only exclusions.
# Adding another needs a reason in the commit message. See standards/testing.md.
IGNORE_RE='(target-coverage|/OUT_DIR/|/out/|\.pb\.rs|bin/pgprox/src/main\.rs)'

run_gate() {
  local crate="$1"
  local out

  # Stale profile data from a previous crate's run is discarded first.
  # Coverage attributes a generic function to the file it is written in, so a
  # crate that instantiates another crate's generics and does not exercise them
  # lands in that crate's report. bin/pgprox instantiating LivePool did exactly
  # that, and pgprox-pool read 94% while its own tests covered 99%. Left alone
  # this understates and, with the runs in the other order, overstates.
  # --profraw-only keeps the instrumented build, so this costs nothing.
  CARGO_TARGET_DIR="$COVERAGE_TARGET_DIR" cargo llvm-cov clean \
    --workspace --profraw-only >/dev/null 2>&1 || true

  if ! out="$(CARGO_TARGET_DIR="$COVERAGE_TARGET_DIR" cargo llvm-cov nextest \
        -p "$crate" --lib --bins \
        --ignore-filename-regex "$IGNORE_RE" \
        --summary-only --json 2>/dev/null)"; then
    fail "coverage ($crate): test run failed"
    return
  fi

  local pct
  pct="$(printf '%s' "$out" | python3 -c '
import json,sys
d=json.load(sys.stdin)
print(round(d["data"][0]["totals"]["lines"]["percent"], 2))
' 2>/dev/null)" || { fail "coverage ($crate): could not parse llvm-cov output"; return; }

  if python3 -c "import sys; sys.exit(0 if float('$pct') >= float('$COVERAGE_MIN') else 1)"; then
    ok "coverage ($crate): ${pct}% >= ${COVERAGE_MIN}%"
  else
    fail "coverage ($crate): ${pct}% < ${COVERAGE_MIN}%  (write tests, do not lower the gate)"
  fi
}

if [[ -n "$CRATE" ]]; then
  run_gate "$CRATE"
else
  members="$(cargo metadata --no-deps --format-version 1 2>/dev/null \
    | python3 -c 'import json,sys; print("\n".join(p["name"] for p in json.load(sys.stdin)["packages"]))')"
  if [[ -z "$members" ]]; then
    skip "coverage (workspace has no members yet)"
    finish
  fi
  while read -r m; do
    [[ -n "$m" ]] && run_gate "$m"
  done <<< "$members"
fi

finish
