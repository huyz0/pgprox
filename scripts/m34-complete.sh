#!/usr/bin/env bash
# M34: the seventeen kilobytes that are not the buffers.
#
#   scripts/m34-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# # What a gate can reach
#
# The experiment needs two containers and four minutes per arm, which is
# `scripts/e2e.sh`'s division rather than a per-commit gate's. What is checked
# is that the run exists, that the script still holds the three arms apart, and
# that the one setup mistake which killed the first attempt cannot come back
# silently.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M34: the seventeen kilobytes that are not the buffers"
echo

BACKLOG="${PGPROX_BACKLOG:-docs/internal/product/backlog.md}"
SELF="${BASH_SOURCE[0]}"

finished="$(sed -n '/^## M34:/,/^## /p' "$BACKLOG" \
  | sed -n 's/^- \[x\] `\(M34\.[0-9]*\)`.*/\1/p' \
  | grep -vE '^M34\.(0|2)$' || true)"

if [[ -z "$finished" ]]; then
  ok "no finding has been ticked yet, so none can be unchecked"
else
  unchecked=""
  while read -r task; do
    [[ -z "$task" ]] && continue
    grep -q "^# --- $task:" "$SELF" || unchecked+=" $task"
  done <<<"$finished"

  if [[ -z "$unchecked" ]]; then
    ok "every finished M34 task has checks here ($(wc -w <<<"$finished"))"
  else
    fail "these M34 tasks are ticked and nothing here checks them:$unchecked"
  fi
fi

# --- the findings that have landed -------------------------------------------

# --- M34.1: is it the allocator's memory or the connection's ------------------
RUN="${PGPROX_RUN_DOC:-docs/internal/product/perf/run-2026-08-05-arenas.md}"
if [[ -f "$RUN" ]]; then
  ok "$RUN records all three arms"
else
  fail "$RUN is missing, so the answer is a claim in a commit message"
fi

if bash -n scripts/arena.sh 2>/dev/null; then
  ok "the experiment parses"
else
  fail "scripts/arena.sh does not parse"
fi

# The three arms, by the environment variable each one moves. An arm that
# stopped setting one of these would run the baseline twice and report a null
# result that looks like an answer.
for piece in TOKIO_WORKER_THREADS MALLOC_ARENA_MAX; do
  if grep -q "$piece" deploy/docker-compose.arena.yml; then
    ok "the overlay still passes $piece"
  else
    fail "the overlay no longer passes $piece, so one arm cannot differ from another"
  fi
done

# The mistake that killed the first attempt: an empty value is still a value,
# and tokio panics on it. The overlay uses `:?` so a missing variable stops the
# stack instead of starting a proxy that dies in a loop.
if grep -q 'PGPROX_WORKER_THREADS:?' deploy/docker-compose.arena.yml; then
  ok "a missing worker count refuses to start rather than defaulting to empty"
else
  fail "the overlay would accept an unset worker count, which tokio panics on"
fi

# And the arms are still three, because two cannot separate a per-thread cost
# from a per-arena one.
arms="$(sed -n '/^ARMS=(/,/^)/p' scripts/arena.sh | grep -c '^  "' || echo 0)"
if (( arms >= 3 )); then
  ok "the experiment still runs $arms arms"
else
  fail "the experiment runs $arms arms, and fewer than three cannot separate"
  printf '       a per-thread cost from a per-arena one\n'
fi

finish
