#!/usr/bin/env bash
# M10 completion condition: the three claims this repo made about itself are
# enforced by something that fails, rather than by the sentences that made them.
#
# The milestone exists because each of the three was true as a statement and
# false as a fact: eleven milestone gates of which CI ran three, a codec
# described as "fuzzed, not assumed" with nothing running the fuzzer, and a
# `docs/internal/standards/testing.md` claiming a nightly mutation run against a tool that was
# not installed.
#
# So this gate checks that each claim now has a mechanism, and it deliberately
# does not re-check the mechanisms themselves. `scripts/check-drift.sh` already
# asserts that every gate in `scripts/gates/`, `fuzz.sh` and
# `mutants.sh` is named in `.github/workflows/ci.yml`; repeating that here would
# be a second place to update when the list changes. What this adds is the part
# drift cannot see: that the jobs are on a schedule, that the mutation baseline
# is a list of reasons rather than a list of names, and that the standard
# describes the run that exists.
#
# No Docker, seconds, like every other gate in this family.
source "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

cd "$REPO_ROOT"

CI=".github/workflows/ci.yml"

echo "M10: the claims nothing enforces"
echo

# --- claim one: the milestone gates run ---------------------------------------
#
# Eight of the eleven fired once, on the commit that closed their milestone, and
# nothing checked them again. `check-drift.sh` owns the "named in CI" half. What
# it cannot see is whether one of them was quietly dropped from the job that
# runs them, so this counts.
gates=(scripts/gates/m*-complete.sh)
if (( ${#gates[@]} > 0 )); then
  ok "${#gates[@]} milestone gates exist"
else
  fail "no milestone gates: the roadmap's completion conditions are prose"
fi

missing=()
for gate in "${gates[@]}"; do
  grep -qs "$(basename "$gate")" "$CI" || missing+=("$(basename "$gate")")
done
if (( ${#missing[@]} == 0 )); then
  ok "every milestone gate is named in CI"
else
  fail "gates absent from CI: ${missing[*]}"
fi

# The gate for this milestone, which is the one that was missing when every
# other M10 task was done. A milestone cannot check itself into existence.
[[ -f scripts/gates/m10-complete.sh ]] \
  && ok "M10 has a gate of its own" \
  || fail "scripts/gates/m10-complete.sh is missing, which is this milestone's own subject"

# --- claim two: the codec is fuzzed -------------------------------------------
#
# `pgprox-proto/AGENTS.md` says a malformed frame must not take down a node
# serving 100k connections, and that this is fuzzed rather than assumed. A
# fuzzer nobody runs makes the second half of that sentence false.
[[ -x scripts/fuzz.sh ]] \
  && ok "scripts/fuzz.sh exists and is executable" \
  || fail "no runnable scripts/fuzz.sh: the codec is assumed, not fuzzed"

[[ -d fuzz/fuzz_targets ]] && (( $(find fuzz/fuzz_targets -name '*.rs' | wc -l) > 0 )) \
  && ok "fuzz targets exist" \
  || fail "no fuzz targets under fuzz/fuzz_targets"

# On a schedule rather than only by hand, which is the whole difference between
# the claim and the practice.
grep -qs "schedule" "$CI" \
  && ok "CI has a schedule for tier 3" \
  || fail "no schedule in $CI: the nightly jobs are nightly in name only"

# --- claim three: mutation testing runs ---------------------------------------
#
# `docs/internal/standards/testing.md` said this ran nightly for three milestones before
# anything ran it once.
[[ -x scripts/mutants.sh ]] \
  && ok "scripts/mutants.sh exists and is executable" \
  || fail "no runnable scripts/mutants.sh"

baseline="docs/internal/product/mutants-baseline.txt"
if [[ -f $baseline ]]; then
  ok "a mutation baseline is recorded"
else
  fail "$baseline missing: a surviving mutant has nowhere to be accepted"
fi

# The baseline is only worth having if every line says why. `untriaged` was
# allowed while `M10.4` through `M10.8` worked through the first run's output
# and is not allowed after them, which is a rule that needs a check or it is
# another sentence.
# Entries only. The header explains what `untriaged` was and why it is no longer
# allowed, and a check that read its own documentation as a violation would be
# unfixable without deleting the explanation.
if [[ -f $baseline ]] && grep -v '^#' "$baseline" | grep -qs 'untriaged'; then
  fail "the mutation baseline still says 'untriaged': that is not a reason"
else
  ok "every accepted mutant carries a reason"
fi

# A hung test has to be a failed test, or a timeout is a run nobody read rather
# than a mutant the suite caught. `M10.13` found that the hard way.
if grep -qs 'test-tool=nextest' scripts/mutants.sh \
   && grep -qs 'terminate-after' .config/nextest.toml; then
  ok "the mutation run kills a hung test rather than timing out on it"
else
  fail "no per-test timeout for the mutation run: a hang would be read as a survivor"
fi

# --- and the sentence that started it -----------------------------------------
#
# The standard has to describe the run that exists. This is the weakest check
# here, because prose cannot be asserted, so it checks the two things that were
# actually wrong: that the script is named, and that the timeout meaning is
# written down where somebody reading a survivor list will find it.
if grep -qs 'scripts/mutants.sh' docs/internal/standards/testing.md \
   && grep -qs 'timeout' docs/internal/standards/testing.md; then
  ok "docs/internal/standards/testing.md describes the run that exists"
else
  fail "docs/internal/standards/testing.md does not describe the mutation run as it is"
fi

finish
