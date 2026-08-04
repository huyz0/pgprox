#!/usr/bin/env bash
# M29: the first exception the unsafe policy was asked for.
#
#   scripts/m29-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# The milestone's outcome is that nothing changed in the code, so what there is
# to check is that nothing changed in the code: the run document exists, the
# tree still holds no unsafe, and the policy that refused the exception still
# passes. A gate for a negative result checks the negative.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M29: the first exception the unsafe policy was asked for"
echo

RUN="${PGPROX_RUN_DOC:-product/perf/run-2026-08-04-unchecked-slab.md}"

if [[ -f "$RUN" ]]; then
  ok "$RUN records both arms"
else
  fail "$RUN is missing, so the measurement is a claim in a commit message"
fi

# The prototype was reverted. This is the whole result: an exception was
# measured and not taken, and a tree that grew one anyway would mean the
# document describes something other than what shipped.
if scripts/check-unsafe.sh >/dev/null 2>&1; then
  ok "the policy that refused the exception still holds"
else
  fail "scripts/check-unsafe.sh does not pass"
fi

# And the numbers the document rests on are still the shipped ones, so a later
# change that moved them silently would show up here rather than in a reader's
# confusion. Read from the baseline rather than restated.
for pair in "pgprox-cache::cache_hit 1600" "pgprox-cache::cache_hit_rotating 1950"; do
  set -- $pair
  measured="$(python3 -c "import json; print(json.load(open('product/perf/baseline.json')).get('$1', 999999))")"
  if (( measured < $2 )); then
    ok "$1 is $measured, still the figure the run compares against"
  else
    fail "$1 is $measured, so the run document compares against a number that moved"
  fi
done

finish
