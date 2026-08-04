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

# --- the findings that have landed -------------------------------------------

finish
