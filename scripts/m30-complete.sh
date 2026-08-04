#!/usr/bin/env bash
# M30: the same procedure, applied to every crate.
#
#   scripts/m30-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# # How it passes while the milestone is open
#
# The same way M19 through M29 did: it checks what has landed rather than what
# is planned, and a finding gets its check in the commit that fixes it.
#
# That would be a gate anyone could pass by ticking a task and adding nothing,
# so the first check is the one that closes it: every M30 task the backlog marks
# done must be named here.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M30: the same procedure, applied to every crate"
echo

BACKLOG="${PGPROX_BACKLOG:-product/backlog.md}"
SELF="${BASH_SOURCE[0]}"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Each finding, named by the test that would fail if it came back. `--exact`
# with a name nothing matches exits non-zero, so this cannot pass by describing
# a test that is no longer there.
#
# No pipeline into `grep -q`: `set -o pipefail` is on and grep exits at its
# first match, which closes the pipe and can kill `cargo test` with SIGPIPE.
run_finding() {
  local crate="$1" name="$2" finding="$3"
  local out="$WORK/$crate-$RANDOM.out"

  cargo test -p "$crate" --all-targets -- --exact "$name" >"$out" 2>>"$WORK/log"
  if grep -q "^test $name \.\.\. ok$" "$out"; then
    ok "$finding"
  else
    fail "$finding: $crate $name did not run and pass"
    printf '       a finding this milestone fixed has no test standing behind it\n'
  fi
}

# A figure from the gated baseline, held under a ceiling. Used where what landed
# is a measurement rather than a behaviour: the test says the code is still
# right, and this says it is still fast, which are different claims.
under() {
  local key="$1" ceiling="$2"
  local measured
  measured="$(python3 -c "import json; print(json.load(open('product/perf/baseline.json')).get('$key', 999999))")"
  if (( measured < ceiling )); then
    ok "$key is $measured, under the $ceiling it was before"
  else
    fail "$key is $measured, back at or above the $ceiling this milestone moved it from"
  fi
}

# --- every finished task is checked here -------------------------------------
#
# Without this, a task ticked in the backlog and absent from this script is a
# finding nothing stands behind, and the gate would go on reporting green for
# the ones that did land.
#
# Two tasks are about the milestone rather than about a finding: `M30.0`
# planned it and `M30.7` closes it. Excluded by name rather than by a rule, so
# a third exclusion has to be written down here to exist.
finished="$(sed -n '/^## M30:/,/^## /p' "$BACKLOG" \
  | sed -n 's/^- \[x\] `\(M30\.[0-9]*\)`.*/\1/p' \
  | grep -vE '^M30\.(0|7)$' || true)"

if [[ -z "$finished" ]]; then
  ok "no finding has been ticked yet, so none can be unchecked"
else
  unchecked=""
  while read -r task; do
    [[ -z "$task" ]] && continue
    grep -q "^# --- $task:" "$SELF" || unchecked+=" $task"
  done <<<"$finished"

  if [[ -z "$unchecked" ]]; then
    ok "every finished M30 task has checks here ($(wc -w <<<"$finished"))"
  else
    fail "these M30 tasks are ticked and nothing here checks them:$unchecked"
    printf '       a finding reported fixed with no test standing behind it\n'
  fi
fi

# --- the findings that have landed -------------------------------------------

# --- M30.1: a statement lexed twice to read one word --------------------------
#
# The behaviour, by the test that fails if the early exit stops being sound.
# Two mutations were run against it before it was trusted: dropping the `SET`
# second-word guard, and dropping the opener check. Both fail it.
run_finding pgprox-route \
  classify::tests::a_statement_that_cannot_open_a_transaction_is_answered_by_its_first_words \
  "a statement that cannot open a transaction is answered by its first words"

# And the number, so a later change that quietly puts the second pass back is
# caught here rather than in a profile nobody runs. The figures before were
# 6,444 and 6,717.
under pgprox-route::route_point_select 5200
under pgprox-route::route_update 4800

# --- M30.2: every word compared against every keyword -------------------------
#
# Three checks rather than one, because the filter can be wrong in two
# directions and only one of them is visible in a benchmark. Letting a keyword
# through is a write classified as a read; rejecting nothing is a filter that
# costs and buys nothing, and every other test in the crate passes either way
# because the scan behind it still runs.
run_finding pgprox-route \
  classify::properties::the_filter_lets_every_word_on_every_list_through \
  "the filter lets every word on every list through"
run_finding pgprox-route \
  classify::properties::the_filter_is_a_filter_and_not_an_answer \
  "the filter rejects something, and does not answer for the scan"
run_finding pgprox-route \
  classify::properties::the_filter_and_the_scan_agree_on_everything \
  "the filter and the scan agree on words generated next to the lists"

# The route decision after both findings. It was 6,444 and 6,717.
under pgprox-route::route_point_select 4000
under pgprox-route::route_update 4200

# --- M30.6: a second benchmark that moved with a random seed ------------------
#
# Not `run_finding`: what landed is the shape of a measurement, and the place
# that is visible is the baseline. The check is `M28.2`'s, against the rule
# `standards/testing.md` now states: a gated benchmark measures at least a
# thousand instructions, because below that a `HashMap` probe count decides
# whether it passes.
served="$(python3 -c "import json; print(json.load(open('product/perf/baseline.json')).get('pgprox-cache::serves_a_mix_of_tenants', 0))")"
if (( served > 10000 )); then
  ok "the serves benchmark measures $served instructions, well past seed noise"
else
  fail "the serves benchmark is $served instructions, back in the range where"
  printf '       a HashMap probe count decides whether it passes\n'
fi

# And the unstable one is gone rather than carried beside its replacement, which
# would leave it still gating CI.
if grep -q '"pgprox-cache::serves"' product/perf/baseline.json; then
  fail "the baseline still carries pgprox-cache::serves, which is the unstable one"
else
  ok "the unstable benchmark is gone rather than kept beside its replacement"
fi

finish
