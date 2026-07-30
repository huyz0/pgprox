#!/usr/bin/env bash
# Mutation testing against the crates whose logic is a pure state machine.
#
#   scripts/mutants.sh                  # every crate in the list below
#   scripts/mutants.sh pgprox-route     # one of them
#
# Coverage says a line ran. This says the line mattered. `standards/testing.md`
# has claimed since M-1 that this runs nightly and that surviving mutants are
# treated as missing tests, and until `M10.3` nothing ran it.
#
# M9 is the argument for the claim being worth keeping rather than deleting.
# Three of its defects were invisible because a fake answered something Postgres
# refuses, and one fix went in half-applied and green while every gate passed.
# Each of those is a line whose removal changed nothing any test could see,
# which is exactly what a surviving mutant is.
#
# # The baseline
#
# A survivor that is accepted lives in `product/mutants-baseline.txt` with a
# reason. A survivor that is not there fails this script. The file is a list
# nobody may grow without writing down why, which is the same discipline as the
# coverage gate: the number does not move quietly.
#
# Keys carry no line numbers. A survivor is identified by its crate, its file,
# its function and the replacement text, so editing the lines above it does not
# invalidate the entry.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

# The sans-I/O crates. `pgprox-session` is here because its own `AGENTS.md`
# names mutation testing, and it is the most correctness-critical code in the
# project; the other three hold rules a wrong answer would come out of.
CRATES=(pgprox-proto pgprox-route pgprox-cache pgprox-session)
if (( $# > 0 )); then
  CRATES=("$@")
fi

BASELINE="product/mutants-baseline.txt"
# Parallelism and a per-mutant ceiling. Without the ceiling one mutated loop
# that no longer terminates stops the whole run rather than being reported.
JOBS="${MUTANTS_JOBS:-6}"
TIMEOUT="${MUTANTS_TIMEOUT:-60}"

require_tool cargo-mutants "cargo install cargo-mutants --locked" || finish
require_tool cargo-nextest "cargo install cargo-nextest --locked" || finish

# The suite runs under nextest so that a hung test is a failed test.
#
# `cargo mutants` gives the whole suite one budget and reports a timeout when it
# runs out. Under `cargo test` there is no per-test timeout, so one test that
# never returns costs the run its verdict and the mutant is reported as a
# timeout whether or not another test failed it. `M10.13` found that by writing
# assertions that fail six mutants and watching all six come back as timeouts.
# The per-test cap lives in `.config/nextest.toml`, which explains the number.
export NEXTEST_PROFILE=mutants

# Each job gets its own copy of the tree, and the copy includes this repo's
# other build directories: `target-coverage` alone is 6 GB. On a machine whose
# /tmp is a tmpfs that is a run that dies with ENOSPC halfway through, having
# already spent twenty minutes. The copies go on the real disk instead.
export TMPDIR="${TMPDIR_MUTANTS:-$REPO_ROOT/target/mutants-tmp}"
mkdir -p "$TMPDIR"

# Survivors, as `crate|file|function|replacement`, from cargo-mutants' own JSON
# rather than from its human output, which is formatted for reading.
survivors_of() {
  local out="$1"
  python3 - "$out" <<'PY'
import json, sys
outcomes = json.load(open(sys.argv[1] + "/mutants.out/outcomes.json"))
for outcome in outcomes.get("outcomes", []):
    if outcome.get("summary") not in {"MissedMutant", "Timeout"}:
        continue
    mutant = outcome.get("scenario", {}).get("Mutant", {})
    function = mutant.get("function") or {}
    print(
        "{}|{}|{}|{}".format(
            mutant.get("package", "?"),
            mutant.get("file", "?"),
            function.get("function_name", "?"),
            mutant.get("replacement", "?").replace("\n", " "),
        )
    )
PY
}

# Whether every crate reported. A run that measured nothing must not be
# followed by "no surviving mutant", which is true and misleading.
measured=1
found="$(mktemp)"
trap 'rm -f "$found"' EXIT

for crate in "${CRATES[@]}"; do
  out="target/mutants-$crate"
  echo "mutating $crate"
  # The exit code is deliberately ignored: a survivor is not a failure until it
  # has been compared against the baseline, which is the next step.
  cargo mutants -p "$crate" --output "$out" --jobs "$JOBS" --timeout "$TIMEOUT" \
    --test-tool=nextest >"$out.log" 2>&1 || true
  if [[ ! -f "$out/mutants.out/outcomes.json" ]]; then
    fail "$crate produced no outcomes; see $out.log"
    measured=0
    continue
  fi
  survivors_of "$out" >> "$found"
  total="$(python3 -c "
import json
o = json.load(open('$out/mutants.out/outcomes.json'))['outcomes']
print(len(o))
")"
  ok "$crate: $total mutants, $(grep -c "^$crate|" "$found" || true) surviving"
done

# --- against the baseline ----------------------------------------------------
accepted="$(mktemp)"
trap 'rm -f "$found" "$accepted"' EXIT
if [[ -f "$BASELINE" ]]; then
  # Everything before the first tab, comments and blank lines dropped, and only
  # for the crates this run measured. Without that filter a run of one crate
  # reports every other crate's accepted survivors as newly caught, which is a
  # warning about nothing and trains people to ignore the real ones.
  {
    for crate in "${CRATES[@]}"; do
      sed 's/#.*//; s/[[:space:]]*$//' "$BASELINE" | grep -v '^$' | grep "^$crate|" || true
    done
  } | cut -f1 | sort -u > "$accepted"
else
  : > "$accepted"
fi
sort -u "$found" -o "$found"

new=0
while read -r key; do
  [[ -n "$key" ]] || continue
  fail "a new surviving mutant: $key"
  new=1
done < <(comm -23 "$found" "$accepted")

if (( new == 0 && measured == 1 )); then
  ok "no surviving mutant outside $BASELINE"
fi

# A baseline entry that no longer survives is a test somebody wrote. Worth
# saying so it can be removed, and not worth failing over.
while read -r key; do
  [[ -n "$key" ]] || continue
  warn "$BASELINE lists a mutant that is now caught: $key"
done < <(comm -13 "$found" "$accepted")

finish
