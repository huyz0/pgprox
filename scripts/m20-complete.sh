#!/usr/bin/env bash
# M20: the protocol layer against pgbouncer, pgcat and odyssey.
#
#   scripts/m20-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# # This gate exists from the milestone's first commit and grows with it
#
# `M19`'s gate made the argument and this one inherits it: a milestone whose
# completion condition arrives last is a milestone that was unmeasurable while
# it was open. It passes at every point, which is what lets CI run it now.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M20: the protocol layer against pgbouncer, pgcat and odyssey"
echo

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

run_test() {
  local crate="$1" name="$2" claim="$3"
  local out="$WORK/$crate-$RANDOM.out"

  # `|| true` so that cargo failing outright becomes a reported failure rather
  # than a dead script: `lib.sh` sets `-e`. `M19`'s gate exited 101 with four
  # checks printed and no verdict, which reads exactly like one that passed.
  cargo test -p "$crate" --all-targets -- --exact "$name" >"$out" 2>>"$WORK/log" || true
  if grep -q "^test $name \.\.\. ok$" "$out"; then
    ok "$claim"
  else
    fail "$claim: $crate $name did not run and pass"
    printf '       a claim this milestone made has no test standing behind it\n'
  fi
}

# --- M20.0: the half of the rule that already holds --------------------------
#
# This milestone's first finding is `M15.3`'s, through the door that fix left
# open: `DISCARD ALL` reaches both maps and a protocol `Close` does not. So the
# gate starts by holding the half that works, which is what makes the missing
# half a gap rather than an opinion, and gains the other checks as the tasks
# land.
run_test pgprox-session "resume::tests::discard_all_makes_both_maps_forget" \
  "DISCARD ALL is still forgotten by the session and by the connection"
run_test pgprox-session "resume::tests::a_re_parse_after_discard_all_prepares_again_rather_than_assuming" \
  "a re-parse after DISCARD ALL prepares rather than assuming"

finish
