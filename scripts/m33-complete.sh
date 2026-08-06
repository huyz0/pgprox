#!/usr/bin/env bash
# M33: what pgbouncer and pgcat do differently.
#
#   scripts/m33-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# # What a gate can check about a study
#
# Not much, and saying so is better than a check that pretends. The milestone's
# output is a document and one experiment, and the experiment refuted its own
# hypothesis, so there is no behaviour to test.
#
# What can be checked is that the two constants the experiment moved are back
# where they were, and that the document's claim about them still matches the
# code. A study saying "16 KiB" beside a constant reading 4 KiB is worse than no
# study.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M33: what pgbouncer and pgcat do differently"
echo

BACKLOG="${PGPROX_BACKLOG:-docs/internal/product/backlog.md}"
SELF="${BASH_SOURCE[0]}"

finished="$(sed -n '/^## M33:/,/^## /p' "$BACKLOG" \
  | sed -n 's/^- \[x\] `\(M33\.[0-9]*\)`.*/\1/p' \
  | grep -vE '^M33\.(0|2)$' || true)"

if [[ -z "$finished" ]]; then
  ok "no finding has been ticked yet, so none can be unchecked"
else
  unchecked=""
  while read -r task; do
    [[ -z "$task" ]] && continue
    grep -q "^# --- $task:" "$SELF" || unchecked+=" $task"
  done <<<"$finished"

  if [[ -z "$unchecked" ]]; then
    ok "every finished M33 task has checks here ($(wc -w <<<"$finished"))"
  else
    fail "these M33 tasks are ticked and nothing here checks them:$unchecked"
  fi
fi

# --- the findings that have landed -------------------------------------------

# --- M33.1: the study, and the experiment that refuted it ---------------------
RUN="${PGPROX_RUN_DOC:-docs/internal/product/perf/run-2026-08-05-what-the-others-do.md}"
if [[ -f "$RUN" ]]; then
  ok "$RUN records what each of the three does with memory"
else
  fail "$RUN is missing, so the reading is a claim in a commit message"
fi

# The experiment set two constants to 4 KiB and put them back. A tree where one
# of them stayed moved is a tree the document describes wrongly, and neither is
# covered by a benchmark: the run showed `held_read` identical at both sizes,
# which is exactly why nothing else would catch it.
for pair in "crates/pgprox-core/src/buf.rs DEFAULT_BUFFER_SIZE" \
            "crates/pgprox-session/src/shell.rs HELD_READ"; do
  set -- $pair
  if grep -qE "$2: usize = 16 \* 1024;" "$1"; then
    ok "$2 is back at the 16 KiB the study measured against"
  else
    fail "$2 is not 16 KiB, so the study describes a tree that is not this one"
  fi
done

finish
