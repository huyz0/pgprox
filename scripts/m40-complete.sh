#!/usr/bin/env bash
# M40: a control that only worked where nothing else was broken.
#
#   scripts/m40-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# The milestone made four controls assert their own check's message instead of a
# script's exit status. What this checks is that they still do, because reading
# an exit code is the easy thing to write and the reason it was wrong is not
# visible from the call site.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M40: a control that only worked where nothing else was broken"
echo

NEGATIVE="${PGPROX_NEGATIVE:-tests/gates/negative.sh}"

if grep -q '^expect_reports()' "$NEGATIVE"; then
  ok "the suite can assert a check's own message"
else
  fail "$NEGATIVE has no expect_reports, so a case can only read an exit status"
fi

# None of the four scope-ADR cases reads an exit code any more. A case that went
# back to `expect_fail` here would pass on any machine where something unrelated
# in the script fails, which is what M40 was.
if sed -n '/the scope ADRs/,/^}/p' "$NEGATIVE" | grep -qE 'expect_(pass|fail) '; then
  fail "a scope-ADR case is back to reading the script's exit status"
  printf '       which passes whenever anything else in that script fails\n'
else
  ok "every scope-ADR case reads the message its own check produces"
fi

# And the suite passes without a stack running, which is the condition it failed
# under. Run rather than asserted: this is the whole point of the milestone.
if timeout 600 "$NEGATIVE" >/dev/null 2>&1; then
  ok "the suite passes with no containers up"
else
  fail "$NEGATIVE does not pass, so the gates are not known to fail on anything"
fi

finish
