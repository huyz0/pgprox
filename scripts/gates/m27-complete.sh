#!/usr/bin/env bash
# M27: unsafe becomes a governed exception rather than a closed door.
#
#   scripts/gates/m25-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code. Every finding below is checked by
# running the test that would fail if it came back, by exact name, and reading
# the exit status.
#
# # How it passes while the milestone is open
#
# The same way M19 through M26 did: it checks what has landed rather than what
# is planned. A finding gets its `run_finding` line in the commit that fixes it,
# never before.
#
# That would be a gate anyone could pass by ticking a task and adding nothing,
# so the first check is the one that closes it: every M27 task the backlog marks
# done must be named here. Ticking `M27.1` without adding its test fails this
# script, which is the only reason the arrangement is worth having.
source "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

cd "$REPO_ROOT"

echo "M27: unsafe becomes a governed exception rather than a closed door"
echo

BACKLOG="${PGPROX_BACKLOG:-docs/internal/product/backlog.md}"
SELF="${BASH_SOURCE[0]}"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Each finding, named by the test that would fail if it came back. `--exact`
# with a name nothing matches exits non-zero, so this cannot pass by describing
# a test that is no longer there.
#
# No pipeline into `grep -q`: `set -o pipefail` is on and grep exits at its
# first match, which closes the pipe and can kill `cargo test` with SIGPIPE. See
# the same comment in `m15-complete.sh`, which is where that cost six checks.
run_finding() {
  local crate="$1" name="$2" finding="$3"
  local out="$WORK/$crate-$RANDOM.out"

  cargo test -p "$crate" --all-targets -- --exact "$name" >"$out" 2>>"$WORK/log"
  if grep -q "^test $name \.\.\. ok$" "$out"; then
    ok "$finding"
  else
    fail "$finding: $crate $name did not run and pass"
    printf '       a finding this milestone fixed has no test standing behind it\n'
  fi
}

# --- every finished task is checked here -------------------------------------
#
# The check that makes the rest mean anything. Without it, a task ticked in the
# backlog and absent from this script is a finding nothing stands behind, and
# the gate would go on reporting green for the ones that did land.
#
# Two tasks are about the milestone rather than about a finding, and neither has
# a test to run: `M27.0` planned it and `M27.2` closed it. Excluded by name
# rather than by a rule about which tasks have tests, so a third exclusion has
# to be written down here to exist.
finished="$(sed -n '/^## M27:/,/^## /p' "$BACKLOG" \
  | sed -n 's/^- \[x\] `\(M27\.[0-9]*\)`.*/\1/p' \
  | grep -vE '^M27\.(0|2)$' || true)"

if [[ -z "$finished" ]]; then
  ok "no finding has been ticked yet, so none can be unchecked"
else
  unchecked=""
  while read -r task; do
    [[ -z "$task" ]] && continue
    grep -q "^# --- $task:" "$SELF" || unchecked+=" $task"
  done <<<"$finished"

  if [[ -z "$unchecked" ]]; then
    ok "every finished M27 task has checks here ($(wc -w <<<"$finished"))"
  else
    fail "these M27 tasks are ticked and nothing here checks them:$unchecked"
    printf '       a finding reported fixed with no test standing behind it\n'
  fi
fi

# --- the findings that have landed -------------------------------------------

# --- M27.1: the policy, and the script that enforces it -----------------------
#
# Not `run_finding`, because what this milestone produced is a script and two
# standards rather than a Rust test. The check is the same shape all the same:
# run the thing and read its exit code, per `M12.8`.

# The conditions hold on this tree right now.
if scripts/check-unsafe.sh >/dev/null 2>&1; then
  ok "the five conditions hold on this tree"
else
  fail "scripts/check-unsafe.sh does not pass on the tree it governs"
fi

# And every one of them can fail, which is the half that makes the first mean
# anything. `M12`: a check nobody has seen fail is a check nobody knows the
# failure mode of.
if tests/gates/negative.sh unsafe >/dev/null 2>&1; then
  ok "every condition is proven able to fail"
else
  fail "tests/gates/negative.sh unsafe does not pass"
  printf '       the conditions are not proven able to fail, so they are decoration\n'
fi

# The verification duty has somewhere to run. A policy that requires Miri and
# has no Miri job is the shape `M13` audited seven of.
if [[ -x scripts/miri.sh ]] && grep -q 'name: tier 3 - miri' .github/workflows/ci.yml; then
  ok "the Miri job exists for the crates that will need it"
else
  fail "the policy requires Miri and there is no job to run it"
fi

finish
