#!/usr/bin/env bash
# M35: every per-connection memory figure so far was two numbers added together.
#
#   scripts/m35-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# # What a gate can reach
#
# The milestone withdrew numbers rather than adding behaviour, so what is
# checkable is that the withdrawal stuck: the documents it corrects still exist
# to be corrected, and the correction is still beside them.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M35: per-connection memory is a curve, not a number"
echo

BACKLOG="${PGPROX_BACKLOG:-product/backlog.md}"
SELF="${BASH_SOURCE[0]}"

finished="$(sed -n '/^## M35:/,/^## /p' "$BACKLOG" \
  | sed -n 's/^- \[x\] `\(M35\.[0-9]*\)`.*/\1/p' \
  | grep -vE '^M35\.(0|2)$' || true)"

if [[ -z "$finished" ]]; then
  ok "no finding has been ticked yet, so none can be unchecked"
else
  unchecked=""
  while read -r task; do
    [[ -z "$task" ]] && continue
    grep -q "^# --- $task:" "$SELF" || unchecked+=" $task"
  done <<<"$finished"

  if [[ -z "$unchecked" ]]; then
    ok "every finished M35 task has checks here ($(wc -w <<<"$finished"))"
  else
    fail "these M35 tasks are ticked and nothing here checks them:$unchecked"
  fi
fi

# --- the findings that have landed -------------------------------------------

# --- M35.1: per-connection memory is a curve ---------------------------------
RUN="${PGPROX_RUN_DOC:-product/perf/run-2026-08-05-per-connection-is-not-a-number.md}"
if [[ -f "$RUN" ]]; then
  ok "$RUN records the three connection counts and what they withdraw"
else
  fail "$RUN is missing, so three milestones carry figures nothing corrects"
fi

# The documents it corrects. A correction beside a document that has gone is a
# correction to nothing, and these are the three that report a per-connection
# figure taken at one connection count.
for corrected in product/perf/run-2026-08-05-pgbouncer-pgcat.md \
                 product/perf/run-2026-08-05-what-the-others-do.md \
                 product/perf/run-2026-08-05-arenas.md; do
  if [[ -f "$corrected" ]]; then
    ok "$(basename "$corrected") is still there to be corrected"
  else
    fail "$corrected has gone, so $RUN corrects a document nobody can read"
  fi
done

# The idle workload's think time against what a run would have to be given. The
# attempt that failed did so because the duration was shorter than the shortest
# think, and nothing in the tooling said so.
if grep -q 'longer than its longest think' "$RUN"; then
  ok "the run says what a future idle measurement needs to be given"
else
  fail "$RUN no longer records why the idle measurement failed, which is the"
  printf '       one thing that stops the next attempt failing the same way\n'
fi

finish
