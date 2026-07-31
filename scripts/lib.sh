#!/usr/bin/env bash
# Shared helpers. Sourced by every check script so output and exit behaviour
# are identical whether a check runs from a git hook, from CI, or from an
# agent hook.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export REPO_ROOT

# Coverage runs use their own target dir. Sharing one with the normal dev build
# makes every instrumented run invalidate the cache and rebuild the world,
# which is the difference between a 90 second hook and a 6 minute one.
export COVERAGE_TARGET_DIR="${COVERAGE_TARGET_DIR:-$REPO_ROOT/target-coverage}"

# A constant, deliberately not `${COVERAGE_MIN:-95}`. Non-negotiable 4 in
# AGENTS.md is the 95% gate and non-negotiable 2 is that a threshold is never
# lowered to make a check pass, and a settable default is a threshold anyone can
# lower. `COVERAGE_MIN=10 scripts/check-coverage.sh pgprox-route` used to print
# `ok coverage (pgprox-route): 99.65% >= 10%` and exit 0: the gate announced its
# own weakened bar and passed anyway. `M13.1`.
#
# Nothing is lost by fixing it. The script already prints the measured
# percentage, so anyone who wants to know where a crate stands reads that
# number rather than moving the line it is compared against.
COVERAGE_MIN=95
export COVERAGE_MIN

_fail_count=0

if [[ -t 1 ]]; then
  _red=$'\033[31m'; _green=$'\033[32m'; _yellow=$'\033[33m'; _dim=$'\033[2m'; _off=$'\033[0m'
else
  _red=''; _green=''; _yellow=''; _dim=''; _off=''
fi

ok()   { printf '%s  ok%s  %s\n' "$_green" "$_off" "$*"; }
skip() { printf '%s  --%s  %s %s(skipped)%s\n' "$_dim" "$_off" "$*" "$_dim" "$_off"; }
warn() { printf '%s  !!%s  %s\n' "$_yellow" "$_off" "$*"; }
fail() { printf '%sFAIL%s  %s\n' "$_red" "$_off" "$*"; _fail_count=$((_fail_count + 1)); }

# Report and exit with the accumulated failure count. Scripts that check many
# things call this at the end so one run reports every problem rather than
# stopping at the first, which matters when an agent is reading the output.
finish() {
  if (( _fail_count > 0 )); then
    printf '\n%s%d check(s) failed%s\n' "$_red" "$_fail_count" "$_off"
    exit 1
  fi
  printf '\n%sall checks passed%s\n' "$_green" "$_off"
  exit 0
}

have() { command -v "$1" >/dev/null 2>&1; }

# True once the workspace actually has Rust in it. Before M0 the tree is
# documentation only, and the Rust checks must no-op cleanly rather than fail.
has_rust() { [[ -f "$REPO_ROOT/Cargo.toml" ]]; }

require_tool() {
  if ! have "$1"; then
    fail "missing tool: $1${2:+  (install: $2)}"
    return 1
  fi
  return 0
}
