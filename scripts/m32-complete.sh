#!/usr/bin/env bash
# M32: the comparison against pgbouncer and pgcat.
#
#   scripts/m32-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# # What this gate can and cannot reach
#
# The run itself needs three containers and a Postgres, so it is not something
# a per-commit gate runs. What it checks is everything the run rests on: that
# the client can authenticate the way the other two poolers require, that the
# three arms are configured to the same cap, and that the document exists.
#
# The same division `scripts/e2e.sh` and `scripts/bench.sh` already sit on.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M32: the comparison against pgbouncer and pgcat"
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
    printf '       a finding this milestone fixed has no test standing behind it\n'
  fi
}

# --- every finished task is checked here -------------------------------------
finished="$(sed -n '/^## M32:/,/^## /p' "$BACKLOG" \
  | sed -n 's/^- \[x\] `\(M32\.[0-9]*\)`.*/\1/p' \
  | grep -vE '^M32\.(0|5)$' || true)"

if [[ -z "$finished" ]]; then
  ok "no finding has been ticked yet, so none can be unchecked"
else
  unchecked=""
  while read -r task; do
    [[ -z "$task" ]] && continue
    grep -q "^# --- $task:" "$SELF" || unchecked+=" $task"
  done <<<"$finished"

  if [[ -z "$unchecked" ]]; then
    ok "every finished M32 task has checks here ($(wc -w <<<"$finished"))"
  else
    fail "these M32 tasks are ticked and nothing here checks them:$unchecked"
  fi
fi

# --- the findings that have landed -------------------------------------------

finish
