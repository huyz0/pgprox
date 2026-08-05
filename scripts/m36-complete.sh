#!/usr/bin/env bash
# M36: what an open, quiet connection costs.
#
#   scripts/m36-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# # What a gate can reach
#
# The run needs four containers and a workload whose think time is measured in
# minutes, which is `scripts/e2e.sh`'s division rather than a per-commit gate's.
# What is checked is the document and the one setup fact that made `M35`'s
# attempt fail: a run against the idle workload has to outlast its think time,
# and the workload is where that number lives.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M36: what an open, quiet connection costs"
echo

BACKLOG="${PGPROX_BACKLOG:-product/backlog.md}"
SELF="${BASH_SOURCE[0]}"

finished="$(sed -n '/^## M36:/,/^## /p' "$BACKLOG" \
  | sed -n 's/^- \[x\] `\(M36\.[0-9]*\)`.*/\1/p' \
  | grep -vE '^M36\.(0|2)$' || true)"

if [[ -z "$finished" ]]; then
  ok "no finding has been ticked yet, so none can be unchecked"
else
  unchecked=""
  while read -r task; do
    [[ -z "$task" ]] && continue
    grep -q "^# --- $task:" "$SELF" || unchecked+=" $task"
  done <<<"$finished"

  if [[ -z "$unchecked" ]]; then
    ok "every finished M36 task has checks here ($(wc -w <<<"$finished"))"
  else
    fail "these M36 tasks are ticked and nothing here checks them:$unchecked"
  fi
fi

# --- the findings that have landed -------------------------------------------

finish
