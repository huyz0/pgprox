#!/usr/bin/env bash
# M43: what it does, and what one request touches.
#
#   scripts/gates/m43-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# # What is checkable about these two pages
#
# The features page makes a claim with a list behind it: these are the reasons a
# session pins. That list is a Rust enum, and a variant added to it without a
# row here is a reader told a session pins for six reasons when it pins for
# seven. `M13`'s subject, in a page rather than a standard.
source "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

cd "$REPO_ROOT"

echo "M43: what it does, and what one request touches"
echo

DOCS="${PGPROX_DOCS:-docs}"
FEATURES="$DOCS/features.md"
FLOW="$DOCS/request-flow.md"

for page in "$FEATURES" "$FLOW"; do
  [[ -f "$page" ]] && ok "$(basename "$page") is there" \
    || fail "$page is missing, and the navigation links to it"
done

# Every reason the code can pin for is a row on the page. Read from the enum
# rather than restated, so a variant added later fails here instead of leaving
# the page quietly short.
#
# The variant names are CamelCase and the page uses prose, so this matches on
# the metric label each carries, which is the string an operator actually sees
# in `pgprox_pin_total{reason}` and therefore the one worth being able to look
# up.
missing=""
for label in $(sed -n 's/^            Self::[A-Za-z]* => "\([a-z_]*\)",$/\1/p' \
               crates/pgprox-pool/src/pin.rs); do
  grep -q "\`$label\`" "$FEATURES" "$DOCS/operations.md" || missing+=" $label"
done
if [[ -z "$missing" ]]; then
  ok "every reason a session can pin for is documented"
else
  fail "these pin reasons exist in the code and appear on no page:$missing"
  printf '       a reader is told a session pins for fewer reasons than it does\n'
fi

# The flow page names crates. A crate it names that has gone is a walkthrough
# describing a program that is not this one.
absent=""
for crate in $(grep -ohE 'pgprox-[a-z]+' "$FLOW" | sort -u); do
  [[ -d "crates/$crate" ]] || absent+=" $crate"
done
if [[ -z "$absent" ]]; then
  ok "every crate the walkthrough names exists"
else
  fail "the walkthrough names crates that are not in this workspace:$absent"
fi

finish
