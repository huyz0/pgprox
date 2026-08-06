#!/usr/bin/env bash
# M38: the extrapolation M36 did not need to make.
#
#   scripts/gates/m38-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# The milestone is a correction, so the check is that the correction is still
# there. A wrong number that came back because somebody reverted a paragraph is
# exactly what this is for.
source "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

cd "$REPO_ROOT"

echo "M38: the extrapolation M36 did not need to make"
echo

CORRECTED="${PGPROX_RUN_DOC:-docs/internal/product/perf/run-2026-08-05-idle-connection-cost.md}"
MEASURED="${PGPROX_MEASURED_DOC:-docs/internal/product/perf/run-2026-07-28-100k-hold.md}"

# The measured run the correction points at. Without it the correction cites
# nothing and the extrapolation is the only figure a reader finds.
if [[ -f "$MEASURED" ]]; then
  ok "the measured 100k run is still there to be cited"
else
  fail "$MEASURED has gone, so the correction cites a document nobody can read"
fi

# The correction itself, in the document that carried the wrong figure and in
# the roadmap. Both, because a reader arrives at either.
for doc in "$CORRECTED" docs/internal/product/roadmap.md; do
  if grep -q '5,726 bytes' "$doc"; then
    ok "$(basename "$doc") carries the measured figure beside the extrapolation"
  else
    fail "$(basename "$doc") no longer carries the measured figure, so the"
    printf '       extrapolated 1.47 GB reads as the answer again\n'
  fi
done

# And the extrapolation is still there rather than deleted. How it went wrong is
# the part worth keeping, and a document that quietly loses its wrong number
# teaches nothing.
if grep -q '1.47 GB' "$CORRECTED"; then
  ok "the extrapolation is marked rather than removed"
else
  fail "the extrapolation has been deleted from $CORRECTED rather than corrected"
fi

finish
