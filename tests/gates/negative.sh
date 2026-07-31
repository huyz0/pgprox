#!/usr/bin/env bash
# Proof that the gates can fail. `M12`.
#
#   tests/gates/negative.sh            every case
#   tests/gates/negative.sh commit-msg one case
#
# A gate is trusted to say a milestone is done, and on a healthy tree every
# gate says yes. That is exactly the condition under which a broken gate is
# invisible. So each case here breaks an artefact on purpose, runs the check,
# and asserts a **non-zero exit**.
#
# The exit code and not the output, which is the whole reason this file exists.
# `M11.7`'s replacement check piped `awk` into a block that called `fail`. It
# printed `FAIL` in red, with the right message, and exited 0, because the
# right-hand side of a pipeline is a subshell and `_fail_count` lives in the
# parent. Reading the output would have confirmed it worked.
#
# `M12.7` extends this to every `mN-complete.sh`. `M12.1` starts it with the
# commit message hook, because that is the gate that let `M11.11` through.
source "$(dirname "${BASH_SOURCE[0]}")/../../scripts/lib.sh"

cd "$REPO_ROOT"

WANTED="${1:-}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Run a command, expect it to fail. The message says what was broken, so a
# regression reads as "this no longer objects to X" rather than "case 4 failed".
expect_fail() {
  local what="$1"; shift
  if "$@" >"$WORK/out" 2>&1; then
    fail "$what: the check passed on a broken artefact"
    sed 's/^/        /' "$WORK/out"
    return
  fi
  ok "$what"
}

expect_pass() {
  local what="$1"; shift
  if "$@" >"$WORK/out" 2>&1; then
    ok "$what"
    return
  fi
  fail "$what: the check failed on a good artefact"
  sed 's/^/        /' "$WORK/out"
}

msg() { printf '%s\n' "$1" > "$WORK/msg"; printf '%s' "$WORK/msg"; }

# --- check-commit-msg.sh, M12.1 ----------------------------------------------
#
# The three ways this can be wrong, and the two ways it must stay right.
case_commit_msg() {
  echo
  echo "  check-commit-msg.sh"

  expect_fail "refuses a subject with no task ID" \
    scripts/check-commit-msg.sh "$(msg 'fix the thing')"

  # The regression that motivated the task. `M99.99` is well formed and refers
  # to nothing, which is how `M11.11` was committed before its entry existed.
  expect_fail "refuses a well-formed ID that is not a task" \
    scripts/check-commit-msg.sh "$(msg 'M99.99: a task that does not exist')"

  expect_fail "refuses a missing commit message file" \
    scripts/check-commit-msg.sh "$WORK/no-such-file"

  # And the cases it must not break. A real task, and the mechanical subjects
  # git writes itself, which have no task and never will.
  expect_pass "accepts a task that exists" \
    scripts/check-commit-msg.sh "$(msg 'M12.1: resolve the ID against the backlog')"

  expect_pass "accepts a merge subject" \
    scripts/check-commit-msg.sh "$(msg "Merge branch 'main'")"

  expect_pass "accepts a revert subject" \
    scripts/check-commit-msg.sh "$(msg 'Revert "M12.1: something"')"
}

# --- m7-complete.sh, the scale run, M12.2 ------------------------------------
#
# The gate reads `$PGPROX_PERF_DIR`, so these cases hand it a directory built
# for the purpose instead of moving the real documents aside. A test that
# mutates the tree it is testing leaves the tree broken when it is interrupted,
# and this suite runs in CI.
scale_doc() {
  local dir="$1" name="$2" title="$3" count="$4"
  # The `|| true` is load-bearing under `set -euo pipefail`: without it the
  # `[[ ]] &&` is the group's last command and returns 1 for a document with no
  # connection count, which kills the suite rather than writing the file.
  { printf '# %s\n\n| | |\n| --- | --- |\n' "$title"
    if [[ -n "$count" ]]; then printf '| Connections | %s |\n' "$count"; fi
  } > "$dir/$name" || true
}

case_m7_scale() {
  echo
  echo "  m7-complete.sh, the scale run"

  local dir="$WORK/perf"

  # Nothing recorded at all.
  rm -rf "$dir"; mkdir -p "$dir"
  expect_fail "refuses an empty perf directory" \
    env PGPROX_PERF_DIR="$dir" scripts/m7-complete.sh

  # The regression. Documents that match `run-*.md` and are not scale runs, which
  # is what eleven of the sixteen in `product/perf` are. The old check reported
  # them as scale runs and counted them.
  rm -rf "$dir"; mkdir -p "$dir"
  scale_doc "$dir" run-2026-01-01-cache.md "The cache helps by about seven percent" ""
  scale_doc "$dir" run-2026-01-02-pinning.md "What pinning costs multiplexing" ""
  expect_fail "refuses run documents that are not scale runs" \
    env PGPROX_PERF_DIR="$dir" scripts/m7-complete.sh

  # A scale run, but at a connection count that proves nothing. The old check
  # could not see the number at all.
  rm -rf "$dir"; mkdir -p "$dir"
  scale_doc "$dir" run-2026-01-03-tiny.md "Scale run: 8 connections" 8
  expect_fail "refuses a scale run below the connection count M7 asks for" \
    env PGPROX_PERF_DIR="$dir" scripts/m7-complete.sh

  # And the shape that must pass, so the check is not merely strict.
  rm -rf "$dir"; mkdir -p "$dir"
  scale_doc "$dir" run-2026-01-04-1000.md "Scale run: 1000 connections, compose stack" 1000
  expect_pass "accepts a scale run at the stated connection count" \
    env PGPROX_PERF_DIR="$dir" scripts/m7-complete.sh
}

# --- m9-complete.sh, the cache figure, M12.3 ---------------------------------
#
# M9's claim is a number with a sign, so the check ties the roadmap's figure to
# a run that records it. These cases break that tie in each direction.
case_m9_cache() {
  echo
  echo "  m9-complete.sh, the cache figure"

  local dir="$WORK/perf9" road="$WORK/roadmap.md"
  rm -rf "$dir"; mkdir -p "$dir"
  printf '| M9 | Query cache | complete; it costs 7.8%% of the median |\n' > "$road"

  # A run document exists and matches the glob, and does not contain the number
  # the roadmap claims. This is the regression: the old check counted the file.
  printf '# The cache run\n\nIt was faster, roughly.\n' > "$dir/run-2026-01-01-cache.md"
  expect_fail "refuses a cache run that does not record the claimed figure" \
    env PGPROX_PERF_DIR="$dir" PGPROX_ROADMAP="$road" scripts/m9-complete.sh

  # The other direction: the run says 9.9%, the roadmap says 7.8%. A number
  # that drifted from its evidence.
  printf '# The cache run\n\nThe cache costs 9.9%% of the median.\n' > "$dir/run-2026-01-01-cache.md"
  expect_fail "refuses a roadmap figure its runs no longer support" \
    env PGPROX_PERF_DIR="$dir" PGPROX_ROADMAP="$road" scripts/m9-complete.sh

  # A roadmap row with no figure at all: nothing to hold the milestone to.
  printf '| M9 | Query cache | complete; the cache is good |\n' > "$road"
  expect_fail "refuses a roadmap row that states no figure" \
    env PGPROX_PERF_DIR="$dir" PGPROX_ROADMAP="$road" scripts/m9-complete.sh

  # And the tie intact.
  printf '| M9 | Query cache | complete; it costs 7.8%% of the median |\n' > "$road"
  printf '# The cache run\n\nThe cache costs 7.8%% of the median.\n' > "$dir/run-2026-01-01-cache.md"
  expect_pass "accepts a figure a run records" \
    env PGPROX_PERF_DIR="$dir" PGPROX_ROADMAP="$road" scripts/m9-complete.sh
}

# -----------------------------------------------------------------------------

echo "gates: proof that they can fail"

ran=0
if [[ -z "$WANTED" || "$WANTED" == commit-msg ]]; then case_commit_msg; ran=1; fi
if [[ -z "$WANTED" || "$WANTED" == m7-scale ]]; then case_m7_scale; ran=1; fi
if [[ -z "$WANTED" || "$WANTED" == m9-cache ]]; then case_m9_cache; ran=1; fi

if (( ! ran )); then
  fail "no such case: $WANTED"
fi

finish
