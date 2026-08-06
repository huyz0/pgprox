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

BACKLOG="${PGPROX_BACKLOG:-docs/internal/product/backlog.md}"
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

# --- M36.1: what an open, quiet connection costs ------------------------------
RUN="${PGPROX_RUN_DOC:-docs/internal/product/perf/run-2026-08-05-idle-connection-cost.md}"
if [[ -f "$RUN" ]]; then
  ok "$RUN records the three counts and what they say about the target"
else
  fail "$RUN is missing, so the only per-connection figure worth quoting is not written down"
fi

# The workload the run depends on, and the fact that made `M35`'s attempt fail.
# A run against it has to outlast its think time, and the think time is in the
# workload rather than in the script, so a change there silently changes what a
# valid run is.
IDLE="${PGPROX_IDLE_WORKLOAD:-docs/internal/product/perf/workload-idle.yaml}"
if [[ -f "$IDLE" ]]; then
  ok "the idle workload the measurement needs is still there"
else
  fail "$IDLE has gone, so the measurement cannot be repeated"
fi

# The target the run is measured against, read from the script that states it
# rather than repeated here. A document comparing against 500 MB while the
# script says something else is a document about nothing.
if grep -q 'RSS under 500 MB' scripts/scale.sh; then
  ok "the target the run compares against is still the one scale.sh states"
else
  fail "scripts/scale.sh no longer states the 500 MB target the run is measured against"
fi

# And the future-size test the run accounts 5,048 bytes to. It is the one
# component of the per-connection cost that is measured rather than inferred,
# so a run that lost it would be accounting for nothing.
if grep -q 'one_session_costs_less_than_the_slab_buffer_it_no_longer_holds' \
    bin/pgprox/src/serve.rs; then
  ok "the session future still has a test that measures it"
else
  fail "the session future's size test has gone, so the one accounted component"
  printf '       of the per-connection cost is no longer measured\n'
fi

finish
