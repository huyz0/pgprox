#!/usr/bin/env bash
# M3 completion condition: the cluster layer holds its invariant under a
# simulation that can partition, delay, drop and restart.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M3: cluster"
echo

CLUSTER=crates/pgprox-cluster/src
[[ -f crates/pgprox-cluster/Cargo.toml ]] && ok "pgprox-cluster exists" \
  || { fail "pgprox-cluster missing"; finish; }

# Each module named, so an unrelated file cannot satisfy this.
for m in quota lease membership reservation shed digest sim; do
  [[ -f "$CLUSTER/$m.rs" ]] && ok "pgprox-cluster::$m" || fail "pgprox-cluster::$m missing"
done

# The simulation must be deterministic, or a failing seed is not reproducible
# and the property tests are anecdotes.
grep -qsE 'fn seed|seed:' "$CLUSTER/sim.rs" 2>/dev/null \
  && ok "the simulation is seeded" || fail "no seed in the simulation"

# The invariant, by name. This is the milestone.
if grep -rqsE 'guaranteed_plus_leased_never_exceeds_the_cap' "$CLUSTER"; then
  ok "the cap invariant is asserted by name"
else
  fail "no test named for the cap invariant"
fi

for scenario in partition leader_loss restart; do
  grep -rqs "$scenario" "$CLUSTER" \
    && ok "simulated: $scenario" || fail "the simulation does not cover: $scenario"
done

cargo nextest run -p pgprox-cluster --features sim >/dev/null 2>&1 \
  && ok "simulation suite" || fail "simulation suite (run: cargo nextest run -p pgprox-cluster --features sim)"

./scripts/check-crate.sh pgprox-cluster >/dev/null 2>&1 \
  && ok "fmt, clippy, doctests" || fail "workspace checks"
./scripts/check-coverage.sh pgprox-cluster >/dev/null 2>&1 \
  && ok "coverage" || fail "coverage"

finish
