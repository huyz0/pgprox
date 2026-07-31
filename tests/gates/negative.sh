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

# The gates delegate fmt, clippy and coverage to `check-crate.sh` and
# `check-coverage.sh`. This suite runs a gate once per broken artefact, and
# those delegated checks return the same answer every time while costing 84
# seconds an invocation. Skipping them is what makes the suite affordable; CI
# runs both in tier 1 anyway, and `check-drift.sh` fails if this ever appears
# in `ci.yml` or the pre-commit config. `M12.11`.
export PGPROX_SKIP_DELEGATED_CHECKS=1

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

# --- m11-complete.sh, the admission run, M12.4 -------------------------------
#
# M11.6's result is which of two SQLSTATEs a displaced client sees, and the
# answer is neither. A run that does not name both has not addressed it.
case_m11_admission() {
  echo
  echo "  m11-complete.sh, the admission run"

  local dir="$WORK/perf11"
  # Only the admission check reads `$PGPROX_PERF_DIR`; the gate's other checks
  # keep reading the real committed artefacts and keep passing. So a failure
  # here is the admission check and nothing else.
  rm -rf "$dir"; mkdir -p "$dir"

  # No admission run at all.
  expect_fail "refuses a perf directory with no admission run" \
    env PGPROX_PERF_DIR="$dir" scripts/m11-complete.sh

  # The regression: a file whose name matches and whose content does not
  # address the question. The old check reported the claim from the filename.
  printf '# Admission\n\nThe fleet was fine.\n' > "$dir/run-2026-01-01-admission.md"
  expect_fail "refuses an admission run that names no SQLSTATE" \
    env PGPROX_PERF_DIR="$dir" scripts/m11-complete.sh

  # Half the question. 53300 without 57014 does not distinguish the two
  # refusals the pool is careful to keep apart.
  printf '# Admission\n\nNo client saw 53300.\n' > "$dir/run-2026-01-01-admission.md"
  expect_fail "refuses an admission run that names only one of the two codes" \
    env PGPROX_PERF_DIR="$dir" scripts/m11-complete.sh
}

# --- m1f-complete.sh, the scope ADRs, M12.5 ----------------------------------
#
# "A recorded decision rather than an omission" is a claim about an ADR's
# status. The old checks matched a filename, which an empty file satisfies.
case_m1f_adr() {
  echo
  echo "  m1f-complete.sh, the scope ADRs"

  local dir="$WORK/decisions"
  rm -rf "$dir"; mkdir -p "$dir"

  # No ADR at all.
  expect_fail "refuses a decisions directory with neither scope ADR" \
    env PGPROX_DECISIONS="$dir" scripts/m1f-complete.sh

  # The regression: files with the right names and nothing in them.
  : > "$dir/0016-protocol-3-2-deferred.md"
  : > "$dir/0015-replication-is-out-of-scope.md"
  expect_fail "refuses an empty file with the right name" \
    env PGPROX_DECISIONS="$dir" scripts/m1f-complete.sh

  # An ADR that has not decided yet. A gate that reports a recorded decision
  # here is reporting the filename.
  printf '# 0016. Protocol 3.2\n\nStatus: proposed\n' > "$dir/0016-protocol-3-2-deferred.md"
  printf '# 0015. Replication\n\nStatus: accepted\n' > "$dir/0015-replication-is-out-of-scope.md"
  expect_fail "refuses an ADR still marked proposed" \
    env PGPROX_DECISIONS="$dir" scripts/m1f-complete.sh

  # And both decided, which must pass.
  printf '# 0016. Protocol 3.2\n\nStatus: accepted\n' > "$dir/0016-protocol-3-2-deferred.md"
  expect_pass "accepts two ADRs that decided" \
    env PGPROX_DECISIONS="$dir" scripts/m1f-complete.sh
}

# --- check-drift.sh, the gate that cannot fail, M12.6 ------------------------
#
# `fail` counts into the parent shell, so calling it from a pipeline's
# right-hand side prints FAIL and exits 0. These cases plant the shape in a
# temp directory rather than in `scripts/`, via `PGPROX_SHELL_ROOTS`.
case_drift_subshell() {
  echo
  echo "  check-drift.sh, the gate that cannot fail"

  local dir="$WORK/shell"
  rm -rf "$dir"; mkdir -p "$dir"

  # The exact shape M11.7 shipped for one commit.
  cat > "$dir/planted.sh" <<'PLANT'
#!/usr/bin/env bash
printf 'a\tb\n' | {
  IFS=$'\t' read -r verdict rest
  case "$verdict" in
    a) fail "this prints FAIL and exits 0" ;;
  esac
}
PLANT
  expect_fail "flags fail called from a pipeline subshell" \
    env PGPROX_SHELL_ROOTS="$dir/*.sh" scripts/check-drift.sh

  # A `while read` loop on the right of a pipe: same subshell, same problem.
  cat > "$dir/planted.sh" <<'PLANT'
#!/usr/bin/env bash
find . -name '*.rs' | while read -r f; do
  fail "no test for $f"
done
PLANT
  expect_fail "flags fail inside a piped while-read loop" \
    env PGPROX_SHELL_ROOTS="$dir/*.sh" scripts/check-drift.sh

  # And the idiom that must not be flagged. `|| { ...; }` is a brace group in
  # the current shell, and it is how most of scripts/ reports failure. A lint
  # that flags this would be turned off within a day.
  cat > "$dir/planted.sh" <<'PLANT'
#!/usr/bin/env bash
command -v cargo >/dev/null || { fail "no cargo"; return 1; }
grep -q x file || { fail "missing x"; exit 1; }
PLANT
  expect_pass "does not flag the || { fail ...; } idiom" \
    env PGPROX_SHELL_ROOTS="$dir/*.sh" scripts/check-drift.sh
}

# --- every gate, M12.7 -------------------------------------------------------
#
# The cases above each break one artefact and prove one check objects. This one
# is the floor under all of them: every gate, run against a tree that has none
# of what it looks for, must exit non-zero.
#
# The method is to copy `scripts/` into an empty directory. `lib.sh` derives
# `REPO_ROOT` from its own location, so the copy looks out at a tree with no
# crates, no product/, no deploy/, and every check has something to object to.
# Nothing in the real tree is touched.
#
# A warning about writing this loop, because the first version of it reported
# all thirteen gates exiting 0 and they were all exiting 1:
#
#     printf '%s exit=%s\n' "$(basename "$g")" "$?"    # wrong
#
# The command substitution runs before `$?` is expanded and replaces it with
# basename's status. The exit code has to be captured into a variable before
# anything else runs. That is the same mistake as M11.7's, one level up: a
# harness that reported success for a failure it had measured correctly.
case_every_gate() {
  echo
  echo "  every gate, against a tree holding none of its artefacts"

  local root="$WORK/bare"
  rm -rf "$root"; mkdir -p "$root/scripts"
  cp scripts/*.sh "$root/scripts/"

  local g
  for g in "$root"/scripts/m*-complete.sh "$root"/scripts/release-check.sh; do
    [[ -f "$g" ]] || continue
    expect_fail "$(basename "$g") fails when its artefacts are absent" bash "$g"
  done
}

# --- the thresholds, M13.1 ---------------------------------------------------
#
# Non-negotiable 2 is that a threshold is never lowered to make a check pass. A
# settable default is a threshold anyone can lower, and the gate then reports
# its own weakened bar as a pass.
case_thresholds() {
  echo
  echo "  pass/fail thresholds"

  # The property itself, directly: an exported COVERAGE_MIN must not reach the
  # gate. This is the behaviour, not the source text.
  if COVERAGE_MIN=10 bash -c 'source scripts/lib.sh >/dev/null 2>&1; [[ "$COVERAGE_MIN" == 95 ]]'; then
    ok "an exported COVERAGE_MIN does not move the coverage gate"
  else
    fail "COVERAGE_MIN=10 reaches the coverage gate: the 95% bar can be lowered from the environment"
  fi

  local dir="$WORK/thresh"
  rm -rf "$dir"; mkdir -p "$dir"

  # Reintroducing one must be refused.
  printf '#!/usr/bin/env bash\nCOVERAGE_MIN="${COVERAGE_MIN:-50}"\n' > "$dir/plant.sh"
  expect_fail "refuses a pass/fail threshold reintroduced as a settable default" \
    env PGPROX_SHELL_ROOTS="$dir/*.sh" scripts/check-drift.sh

  # And a run parameter must stay settable. Overriding a duration, a seed or a
  # port is what those defaults are for, and a rule that flagged them would be
  # turned off rather than obeyed.
  printf '#!/usr/bin/env bash\nCOVERAGE_MIN=95\nDURATION="${SCALE_DURATION:-30}"\n' > "$dir/plant.sh"
  expect_pass "leaves run parameters settable" \
    env PGPROX_SHELL_ROOTS="$dir/*.sh" scripts/check-drift.sh
}

# --- check-tests-kept.sh, M13.2 ----------------------------------------------
#
# The other half of non-negotiable 2. The coverage gate does not notice a
# deleted test, because the tests that remain still cover the lines.
#
# Each case builds a throwaway git repository with `scripts/` copied in, so the
# script's own REPO_ROOT points at it and the real tree is never staged against.
tests_kept_repo() {
  local repo="$WORK/kept"
  rm -rf "$repo"; mkdir -p "$repo/scripts" "$repo/s"
  cp scripts/*.sh "$repo/scripts/"
  git -C "$repo" init -q .
  git -C "$repo" config user.email t@example.com
  git -C "$repo" config user.name t
  printf '#[test]\nfn a_first_test() {}\n#[tokio::test]\nasync fn a_second_test() {}\n' > "$repo/s/a.rs"
  git -C "$repo" add -A
  git -C "$repo" commit -qm "M0.1: seed"
  printf '%s' "$repo"
}

case_tests_kept() {
  echo
  echo "  check-tests-kept.sh"

  local repo; repo="$(tests_kept_repo)"

  # A test deleted and not declared.
  printf '#[test]\nfn a_first_test() {}\n' > "$repo/s/a.rs"
  git -C "$repo" add -A
  printf 'M0.2: drop a test\n' > "$repo/msg"
  expect_fail "refuses a removed test that is not declared" \
    bash "$repo/scripts/check-tests-kept.sh" "$repo/msg"

  # Declared, which must pass. Deleting a test is ordinary work; deleting it
  # silently is what the rule is against.
  printf 'M0.2: drop a test\n\nRemoves-test: a_second_test\n' > "$repo/msg"
  expect_pass "accepts a removed test that is declared" \
    bash "$repo/scripts/check-tests-kept.sh" "$repo/msg"

  # A rename is a removal and an addition, and the count does not move. This is
  # the case a "test count did not go down" check passes and should not.
  repo="$(tests_kept_repo)"
  printf '#[test]\nfn a_renamed_test() {}\n#[tokio::test]\nasync fn a_second_test() {}\n' > "$repo/s/a.rs"
  git -C "$repo" add -A
  printf 'M0.2: rename a test\n' > "$repo/msg"
  expect_fail "refuses a rename, which a count would not see" \
    bash "$repo/scripts/check-tests-kept.sh" "$repo/msg"

  # A deleted file takes every test in it.
  repo="$(tests_kept_repo)"
  git -C "$repo" rm -q s/a.rs
  printf 'M0.2: drop the file\n' > "$repo/msg"
  expect_fail "refuses a deleted file that held tests" \
    bash "$repo/scripts/check-tests-kept.sh" "$repo/msg"

  # Adding tests changes nothing, and must not be objected to.
  repo="$(tests_kept_repo)"
  printf '#[test]\nfn a_first_test() {}\n#[tokio::test]\nasync fn a_second_test() {}\n#[test]\nfn a_third_test() {}\n' > "$repo/s/a.rs"
  git -C "$repo" add -A
  printf 'M0.2: add a test\n' > "$repo/msg"
  expect_pass "accepts added tests" \
    bash "$repo/scripts/check-tests-kept.sh" "$repo/msg"
}

# -----------------------------------------------------------------------------

echo "gates: proof that they can fail"

ran=0
if [[ -z "$WANTED" || "$WANTED" == commit-msg ]]; then case_commit_msg; ran=1; fi
if [[ -z "$WANTED" || "$WANTED" == m7-scale ]]; then case_m7_scale; ran=1; fi
if [[ -z "$WANTED" || "$WANTED" == m9-cache ]]; then case_m9_cache; ran=1; fi
if [[ -z "$WANTED" || "$WANTED" == m11-admission ]]; then case_m11_admission; ran=1; fi
if [[ -z "$WANTED" || "$WANTED" == m1f-adr ]]; then case_m1f_adr; ran=1; fi
if [[ -z "$WANTED" || "$WANTED" == drift-subshell ]]; then case_drift_subshell; ran=1; fi
if [[ -z "$WANTED" || "$WANTED" == every-gate ]]; then case_every_gate; ran=1; fi
if [[ -z "$WANTED" || "$WANTED" == thresholds ]]; then case_thresholds; ran=1; fi
if [[ -z "$WANTED" || "$WANTED" == tests-kept ]]; then case_tests_kept; ran=1; fi

if (( ! ran )); then
  fail "no such case: $WANTED"
fi

finish
