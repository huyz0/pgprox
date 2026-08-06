#!/usr/bin/env bash
# M28: the build configuration nobody had measured.
#
#   scripts/m25-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code. Every finding below is checked by
# running the test that would fail if it came back, by exact name, and reading
# the exit status.
#
# # How it passes while the milestone is open
#
# The same way M19 through M27 did: it checks what has landed rather than what
# is planned. A finding gets its `run_finding` line in the commit that fixes it,
# never before.
#
# That would be a gate anyone could pass by ticking a task and adding nothing,
# so the first check is the one that closes it: every M28 task the backlog marks
# done must be named here. Ticking `M28.1` without adding its test fails this
# script, which is the only reason the arrangement is worth having.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M28: the build configuration nobody had measured"
echo

BACKLOG="${PGPROX_BACKLOG:-docs/internal/product/backlog.md}"
SELF="${BASH_SOURCE[0]}"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Each finding, named by the test that would fail if it came back. `--exact`
# with a name nothing matches exits non-zero, so this cannot pass by describing
# a test that is no longer there.
#
# No pipeline into `grep -q`: `set -o pipefail` is on and grep exits at its
# first match, which closes the pipe and can kill `cargo test` with SIGPIPE. See
# the same comment in `m15-complete.sh`, which is where that cost six checks.
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

# --- every finished task is checked here -------------------------------------
#
# The check that makes the rest mean anything. Without it, a task ticked in the
# backlog and absent from this script is a finding nothing stands behind, and
# the gate would go on reporting green for the ones that did land.
#
# Two tasks are about the milestone rather than about a finding, and neither has
# a test to run: `M28.0` planned it and `M28.3` closed it. Excluded by name
# rather than by a rule about which tasks have tests, so a third exclusion has
# to be written down here to exist.
finished="$(sed -n '/^## M28:/,/^## /p' "$BACKLOG" \
  | sed -n 's/^- \[x\] `\(M28\.[0-9]*\)`.*/\1/p' \
  | grep -vE '^M28\.(0|3)$' || true)"

if [[ -z "$finished" ]]; then
  ok "no finding has been ticked yet, so none can be unchecked"
else
  unchecked=""
  while read -r task; do
    [[ -z "$task" ]] && continue
    grep -q "^# --- $task:" "$SELF" || unchecked+=" $task"
  done <<<"$finished"

  if [[ -z "$unchecked" ]]; then
    ok "every finished M28 task has checks here ($(wc -w <<<"$finished"))"
  else
    fail "these M28 tasks are ticked and nothing here checks them:$unchecked"
    printf '       a finding reported fixed with no test standing behind it\n'
  fi
fi

# --- the findings that have landed -------------------------------------------

# --- M28.1: the release profile is measured -----------------------------------
#
# Not `run_finding`: what landed is three lines of Cargo.toml and a baseline,
# not a Rust test. The check is the same shape, per `M12.8`: read the artefact
# and compare it against the thing it claims.

# The profile says what the baseline was measured under. A baseline taken at
# fat and a profile that says thin is a set of numbers nobody can reproduce.
if grep -q '^lto = "fat"$' Cargo.toml; then
  ok "the release profile is the one the baseline was measured under"
else
  fail "Cargo.toml no longer sets lto = \"fat\", so the baseline is unreproducible"
fi

# And the numbers it bought, pinned so a later profile change that quietly
# gives them back is caught. Thin put route_begin at 1,536 and decode_query at
# 460; these are the fat figures with room for a toolchain bump.
for pair in "pgprox-route::route_begin 1400" "pgprox-proto::decode_query 420"; do
  set -- $pair
  measured="$(python3 -c "import json,sys; print(json.load(open('docs/internal/product/perf/baseline.json'))['$1'])" 2>/dev/null || echo 999999)"
  if (( measured < $2 )); then
    ok "$1 is $measured, under the $2 thin left it near"
  else
    fail "$1 is $measured, which is back where thin had it"
  fi
done

# --- M28.2: a benchmark that moved with a random seed -------------------------
#
# The check is that the benchmark still measures enough work for the seed to be
# noise. It is named here rather than its stability re-measured, because
# measuring it three times is what `scripts/bench.sh` does and this gate is not
# the place to do it again: a benchmark under a thousand instructions is where
# the problem starts, and the baseline is where that is visible.
held="$(python3 -c "import json; print(json.load(open('docs/internal/product/perf/baseline.json')).get('pgprox-cache::invalidate_a_tenants_entries', 0))")"
if (( held > 10000 )); then
  ok "the invalidation benchmark measures $held instructions, well past seed noise"
else
  fail "the invalidation benchmark is $held instructions, back in the range where"
  printf '       a HashMap probe count decides whether it passes\n'
fi

# And the benchmark it replaced is gone rather than both being carried, which
# would leave the unstable one still gating CI.
if grep -q 'invalidate_after_one_put' docs/internal/product/perf/baseline.json; then
  fail "the baseline still carries invalidate_after_one_put, which is the unstable one"
else
  ok "the unstable benchmark is gone rather than kept beside its replacement"
fi

finish
