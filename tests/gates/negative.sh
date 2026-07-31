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

# -----------------------------------------------------------------------------

echo "gates: proof that they can fail"

ran=0
if [[ -z "$WANTED" || "$WANTED" == commit-msg ]]; then case_commit_msg; ran=1; fi

if (( ! ran )); then
  fail "no such case: $WANTED"
fi

finish
