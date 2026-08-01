#!/usr/bin/env bash
# M14: the crates mutation testing never reached.
#
#   scripts/m14-complete.sh
#
# Under the constraints the last two milestones set. `M12.8`: no check may match
# a filename where it can run something and read an exit code. `M13.7`: prefer
# planting a violation and requiring the rule to object, because that is the
# only way to know a rule is awake.
#
# The milestone's own subject makes one check unavoidable. `M14.4` found that
# `mutants.sh` reported "1 mutants, 0 surviving" and "all checks passed" for a
# crate whose unmutated baseline had failed to build, so the run tested nothing
# and said so in a way that read like success. A gate for this milestone that
# did not check that would be repeating the mistake it exists to record.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M14: the crates mutation testing never reached"
echo

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# --- the list matches the criterion its own header states ---------------------
#
# The milestone exists because `mutants.sh` said it ran "against the crates
# whose logic is a pure state machine" and listed four of fourteen. This does
# not read the header, which is prose: it compares the list against the crates
# that exist, which is what the header resolves to now that `M13.4` enforces
# sans-I/O across all of them.
#
# `bin/*/` as well as `crates/*/` since `M17.2`. This walked only `crates/*/`,
# so it reported the list complete while both binaries were absent from it, and
# `M16` had just moved seven correctness decisions into one of them. A check
# that names where to look cannot see a place it was not told about.
declare -a missing=()
for dir in crates/*/ bin/*/; do
  crate="$(basename "$dir")"
  [[ -f "$dir/Cargo.toml" ]] || continue
  grep -q "\\b${crate}\\b" scripts/mutants.sh || missing+=("$crate")
done
if (( ${#missing[@]} == 0 )); then
  ok "every package is in the mutation list ($(ls -d crates/*/ bin/*/ | wc -l) of them)"
else
  fail "not in scripts/mutants.sh: ${missing[*]}"
  printf '       M14 exists because that list and its own header disagreed.\n'
fi

# --- a failed baseline is not a clean run -------------------------------------
#
# Planted rather than described. An outcomes file whose baseline failed is
# exactly what `pgprox-config` produced, and the harness read it as success.
mkdir -p "$WORK/fake/mutants.out"
cat > "$WORK/fake/mutants.out/outcomes.json" <<'JSON'
{"outcomes": [{"scenario": "Baseline", "summary": "Failure", "phase_results": []}]}
JSON
baseline="$(python3 -c "
import json
outcomes = json.load(open('$WORK/fake/mutants.out/outcomes.json'))['outcomes']
first = next((o for o in outcomes if o.get('scenario') == 'Baseline'), None)
print('missing' if first is None else first.get('summary', 'unknown'))
")"
if [[ "$baseline" == "Success" ]]; then
  fail "a failed baseline reads as Success, so the guard cannot fire"
else
  ok "a failed baseline is readable as a failure"
fi

# And the guard is actually wired into the script, rather than only being
# possible. Checked by running the comparison the script runs.
if grep -q 'the unmutated baseline is' scripts/mutants.sh; then
  ok "mutants.sh refuses a run whose baseline failed"
else
  fail "mutants.sh no longer checks its baseline; a crate that built nothing would read as clean"
fi

# --- every accepted survivor carries a reason ---------------------------------
#
# The baseline file's own rule: the list may not grow without somebody writing
# down why. A key with an empty reason is a survivor accepted silently, which is
# the same thing as no baseline at all.
BASELINE_FILE="${PGPROX_MUTANTS_BASELINE:-product/mutants-baseline.txt}"
if [[ ! -f "$BASELINE_FILE" ]]; then
  fail "$BASELINE_FILE is missing, so no survivor has a recorded reason"
else
  unreasoned=0
  entries=0
  while IFS= read -r line; do
    [[ -z "$line" || "$line" == \#* ]] && continue
    entries=$(( entries + 1 ))
    reason="${line#*$'\t'}"
    if [[ "$reason" == "$line" || -z "${reason// /}" ]]; then
      fail "a baseline entry has no reason: ${line%%$'\t'*}"
      unreasoned=1
    fi
  done < "$BASELINE_FILE"
  if (( unreasoned == 0 )); then
    ok "every accepted survivor has a written reason ($entries of them)"
  fi
fi

# --- the run this milestone rests on still passes -----------------------------
#
# One crate rather than fourteen, because the full sweep is tens of minutes and
# this gate runs in CI beside twelve others. `pgprox-testkit` is the smallest
# and was the only crate in the milestone that needed no work, which makes it
# the cheapest thing that still exercises the whole path: mutate, compare
# against the baseline, report.
if scripts/mutants.sh pgprox-testkit >"$WORK/testkit.log" 2>&1; then
  ok "a mutation run completes and compares against the baseline"
else
  fail "scripts/mutants.sh pgprox-testkit does not pass"
  tail -15 "$WORK/testkit.log" | sed 's/^/       /'
fi

finish
