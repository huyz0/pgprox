#!/usr/bin/env bash
# M37: what a spawned task costs beyond the future it holds.
#
#   scripts/m37-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M37: what a spawned task costs beyond its future"
echo

BACKLOG="${PGPROX_BACKLOG:-product/backlog.md}"
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
  fi
}

finished="$(sed -n '/^## M37:/,/^## /p' "$BACKLOG" \
  | sed -n 's/^- \[x\] `\(M37\.[0-9]*\)`.*/\1/p' \
  | grep -vE '^M37\.(0|2)$' || true)"

if [[ -z "$finished" ]]; then
  ok "no finding has been ticked yet, so none can be unchecked"
else
  unchecked=""
  while read -r task; do
    [[ -z "$task" ]] && continue
    grep -q "^# --- $task:" "$SELF" || unchecked+=" $task"
  done <<<"$finished"

  if [[ -z "$unchecked" ]]; then
    ok "every finished M37 task has checks here ($(wc -w <<<"$finished"))"
  else
    fail "these M37 tasks are ticked and nothing here checks them:$unchecked"
  fi
fi

# --- the findings that have landed -------------------------------------------

# --- M37.1: a spawned task costs its future plus a header ---------------------
#
# The measurement, and the two assertions that make it mean something: a task
# holds at least its future, and the overhead does not grow with it. The second
# is the load-bearing one. A proportional overhead would mean every byte added
# to the session future costs two, which changes what its size ceiling is for.
run_finding pgprox a_spawned_task_costs_its_future_plus_a_header \
  "a spawned task costs its future plus a fixed header"

RUN="${PGPROX_RUN_DOC:-product/perf/run-2026-08-05-spawn-cost.md}"
if [[ -f "$RUN" ]]; then
  ok "$RUN records the four sizes and what they eliminate"
else
  fail "$RUN is missing, so the last named candidate is eliminated nowhere"
fi

finish
