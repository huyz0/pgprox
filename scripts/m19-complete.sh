#!/usr/bin/env bash
# M19: a seam for peer discovery.
#
#   scripts/m19-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# # This gate exists from the milestone's first commit and grows with it
#
# `M18.3` made a milestone with nothing to run a failure, and `M18.0` said a
# completion condition is part of the milestone rather than an afterthought.
# Taken together that means an open milestone's gate cannot wait until the end,
# so this one starts by checking what `M18.2` already produced and gains a check
# as each task lands. It passes at every point, which is what lets CI run it
# while the milestone is open.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M19: a seam for peer discovery"
echo

SPEC="specs/2026-08-02-peer-discovery-seam"

# --- the spec this milestone is built from -----------------------------------
#
# Checked because the tasks are written against it. A spec that lost the rule
# would be a spec permitting the change it was written to forbid, and the tasks
# would still look done.
flat() { sed 's/^[[:space:]]*>[[:space:]]*/ /' "$1" | tr '\n' ' ' | tr -s ' '; }
if [[ -f "$SPEC/spec.md" ]] \
   && flat "$SPEC/spec.md" | grep -q "never cause a node to be counted alive that gossip has not heard from"; then
  ok "the spec states the rule that keeps the seam safe"
else
  fail "$SPEC/spec.md is missing or no longer states what an external source may not do"
fi

for part in contracts.md tests.md tasks.md; do
  if [[ -s "$SPEC/$part" ]]; then
    ok "the spec has its $part"
  else
    fail "$SPEC/$part is missing or empty"
  fi
done

finish
