#!/usr/bin/env bash
# M26: what the query cache costs, measured for the first time.
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
# The same way M19 through M25 did: it checks what has landed rather than what
# is planned. A finding gets its `run_finding` line in the commit that fixes it,
# never before.
#
# That would be a gate anyone could pass by ticking a task and adding nothing,
# so the first check is the one that closes it: every M26 task the backlog marks
# done must be named here. Ticking `M26.1` without adding its test fails this
# script, which is the only reason the arrangement is worth having.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M26: what the query cache costs, measured for the first time"
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
# a test to run: `M26.0` records the baseline and `M26.5` closed it. Excluded by name
# rather than by a rule about which tasks have tests, so a third exclusion has
# to be written down here to exist.
finished="$(sed -n '/^## M26:/,/^## /p' "$BACKLOG" \
  | sed -n 's/^- \[x\] `\(M26\.[0-9]*\)`.*/\1/p' \
  | grep -vE '^M26\.(0|5)$' || true)"

if [[ -z "$finished" ]]; then
  ok "no finding has been ticked yet, so none can be unchecked"
else
  unchecked=""
  while read -r task; do
    [[ -z "$task" ]] && continue
    grep -q "^# --- $task:" "$SELF" || unchecked+=" $task"
  done <<<"$finished"

  if [[ -z "$unchecked" ]]; then
    ok "every finished M26 task has checks here ($(wc -w <<<"$finished"))"
  else
    fail "these M26 tasks are ticked and nothing here checks them:$unchecked"
    printf '       a finding reported fixed with no test standing behind it\n'
  fi
fi

# --- the findings that have landed -------------------------------------------

# --- M26.1: a write stops walking the whole node ------------------------------
#
# The number is in product/perf/baseline.json and scripts/bench.sh is what
# holds it. What a test can hold is the invariant a second index introduces:
# every path that removes an entry has to remove it from both, and the ones
# that do it least visibly are eviction, expiry on read, and a tenant dropped
# by a reconfigure.
run_finding pgprox-cache \
  store::tests::the_tenant_index_holds_exactly_what_the_entry_map_holds \
  "the tenant index and the entry map cannot drift apart"
# And the behaviour the index exists to make cheap, which would still have to
# be right if it were free.
run_finding pgprox-cache \
  store::tests::invalidating_a_tenant_leaves_every_other_tenant_alone \
  "invalidating one tenant leaves the others alone"
run_finding pgprox-cache \
  store::tests::invalidating_a_tenant_gives_its_bytes_back \
  "the byte total still follows what invalidation removed"

# --- M26.2: a hit stops paying for a second lookup ----------------------------
#
# The number is in the baseline. What a test holds is that the recency order is
# still an order after a hit stopped writing the key into it by hand, and that
# eviction still takes the least recently used.
run_finding pgprox-cache \
  store::tests::a_hit_makes_an_entry_the_last_one_evicted \
  "a hit still moves an entry to the back of the eviction queue"
run_finding pgprox-cache \
  store::tests::the_recency_index_holds_one_place_per_entry \
  "every entry still has exactly one place in the recency order"
run_finding pgprox-cache \
  store::tests::eviction_takes_the_least_recently_used \
  "eviction still takes the least recently used"
# And the budget, which is where the blocks a lookup costs are written down.
# It found what M26.3 and M26.4 are for: a miss that touches nothing allocated
# twice, and it was the trait rather than the store.
run_finding pgprox-cache \
  a_hit_serves_an_answer_without_building_anything \
  "a lookup allocates what the trait costs and nothing more"

# --- M26.3: a lookup allocates nothing ----------------------------------------
#
# The budget is the check, and it says zero where it used to describe where two
# blocks came from. The instruction counts moved with it and are in the
# baseline; scripts/bench.sh is what holds those.
run_finding pgprox-cache \
  a_hit_serves_an_answer_without_building_anything \
  "a miss allocates nothing, through the Arc and through the store alike"
# The trait's own fake, because a contract change that left the fake behaving
# differently would be the one thing worse than the boxing.
run_finding pgprox-core \
  cache::tests::the_fake_reports_emptiness_from_its_contents \
  "the fake still behaves like the store it stands in for"
# And the blanket impl the forwarding cost came from, which still forwards.
run_finding pgprox-core \
  cache::tests::a_cache_behind_an_arc_is_the_same_cache \
  "an Arc around a cache is still that cache"

# --- M26.4: the recency order is a list rather than a tree --------------------
#
# The numbers are in the baseline. What the tests hold is that a list has two
# ways to be wrong where a tree had one, and only one of them is visible from
# the front.
run_finding pgprox-cache \
  store::tests::the_recency_order_is_walkable_from_both_ends \
  "every older link is the mirror of a newer one"
run_finding pgprox-cache \
  store::tests::the_recency_index_holds_one_place_per_entry \
  "the walk reaches every entry exactly once and never cycles"
run_finding pgprox-cache \
  store::tests::eviction_takes_the_least_recently_used \
  "eviction still takes the least recently used"
run_finding pgprox-cache \
  store::tests::a_hit_makes_an_entry_the_last_one_evicted \
  "a hit still moves an entry to the back of the eviction queue"

finish
