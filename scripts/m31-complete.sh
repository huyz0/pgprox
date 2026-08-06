#!/usr/bin/env bash
# M31: the comments at M30's optimisation sites.
#
#   scripts/m31-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# # What can and cannot be checked
#
# Whether a comment states an invariant is a judgement, and no script makes it.
# What a script can do is run the executable form: `M31.1`'s whole argument is
# that a claim worth writing down is worth writing twice, once in prose and once
# as a `debug_assert!`, and the second one is the half a gate can read.
#
# So the checks below are the tests that fail when a `debug_assert!` fires,
# named by the claim rather than by the assertion.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M31: the comments at M30's optimisation sites"
echo

BACKLOG="${PGPROX_BACKLOG:-docs/internal/product/backlog.md}"
SELF="${BASH_SOURCE[0]}"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

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
finished="$(sed -n '/^## M31:/,/^## /p' "$BACKLOG" \
  | sed -n 's/^- \[x\] `\(M31\.[0-9]*\)`.*/\1/p' \
  | grep -vE '^M31\.(0|2)$' || true)"

if [[ -z "$finished" ]]; then
  ok "no finding has been ticked yet, so none can be unchecked"
else
  unchecked=""
  while read -r task; do
    [[ -z "$task" ]] && continue
    grep -q "^# --- $task:" "$SELF" || unchecked+=" $task"
  done <<<"$finished"

  if [[ -z "$unchecked" ]]; then
    ok "every finished M31 task has checks here ($(wc -w <<<"$finished"))"
  else
    fail "these M31 tasks are ticked and nothing here checks them:$unchecked"
  fi
fi

# A baseline figure held at exactly what it was. Used because this milestone
# changed no code: a `debug_assert!` compiles out of a release build, so a
# figure that moved would mean one of these landed somewhere it costs.
unmoved() {
  local key="$1" expected="$2"
  local measured
  measured="$(python3 -c "import json; print(json.load(open('docs/internal/product/perf/baseline.json')).get('$key', -1))")"
  if [[ "$measured" == "$expected" ]]; then
    ok "$key is still $expected, so nothing here reached a release build"
  else
    fail "$key is $measured and M30 left it at $expected"
    printf '       a comment milestone moved a benchmark, so it changed more than comments\n'
  fi
}

# --- the findings that have landed -------------------------------------------

# --- M31.1: the claims, in the form a test can fail on ------------------------
#
# Whether a comment states an invariant is a judgement and no script makes it.
# What a script reads is the other half: each claim written twice, once in prose
# and once as a `debug_assert!`. These are the tests that fail when one fires.
#
# Neither test is about the assertion. The filter's fires inside `matches_any`
# on any word a debug build classifies, which is why breaking the mask reports
# "the filter rejected \"SELECT\"" from the ordinary classification tests rather
# than from a test written for it.
run_finding pgprox-route \
  classify::properties::the_filter_and_the_scan_agree_on_everything \
  "the filter never rejects a word the scan would accept"
run_finding pgprox-session \
  shell::tests::a_held_read_makes_room_for_a_whole_read_before_it_reads \
  "a reserve leaves room for a whole read"

# And the figures M30 left, unchanged, which is what "comments only" means.
unmoved pgprox-route::route_point_select 3716
unmoved pgprox-pool::acquire_and_release 278
unmoved pgprox-session::held_read 2263

finish
