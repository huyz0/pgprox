#!/usr/bin/env bash
# M6 completion condition: the pieces built against fakes now compose, and the
# end-to-end stack proves it with a real Postgres behind it.
#
# The roadmap names scripts/e2e.sh as the milestone's condition. This script is
# the part that can run without Docker, plus the check that e2e.sh exists and
# asserts the three properties M6 is judged on. Run both.
source "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

cd "$REPO_ROOT"

echo "M6: integration"
echo

SESSION=crates/pgprox-session/src

[[ -f crates/pgprox-session/Cargo.toml ]] \
  && ok "pgprox-session exists" || fail "pgprox-session is documentation only"
[[ -f bin/pgprox/Cargo.toml ]] \
  && ok "bin/pgprox exists" || fail "bin/pgprox missing"

# The session's own modules, each named, so an unrelated file cannot satisfy
# this the way a count would.
for m in state auth relay resume shell connect probe cancel; do
  [[ -f "$SESSION/$m.rs" ]] && ok "pgprox-session::$m" \
    || fail "pgprox-session::$m missing"
done

# The two seams left open on purpose through M1 to M5, because implementing
# either needs a socket and the crates that own them are sans-I/O. A milestone
# that composes everything and leaves these as fakes has composed nothing.
# Named by the implementing type rather than by the trait. "impl Connector
# for" matched a fake in a test module, which is the failure this pair of
# checks exists to catch, so it was catching itself.
grep -rqsE "Connector for PgConnector" "$SESSION" \
  && ok "Connector has a real implementation" \
  || fail "Connector is still only a fake"
grep -rqsE "ReplicaProbe for SqlReplicaProbe" "$SESSION" \
  && ok "ReplicaProbe has a real implementation" \
  || fail "ReplicaProbe is still only a fake"

# The Observatory backing the admin surfaces. M4 built both surfaces over the
# contract and only the fake implemented it.
# pgprox-core is excluded because the fake lives there, and the fake passing
# for the real thing is precisely the failure this line exists to catch.
if grep -rqs --include='*.rs' --exclude-dir=target "impl Observatory for" \
   bin crates/pgprox-session crates/pgprox-admin 2>/dev/null; then
  ok "Observatory has a live implementation"
else
  fail "the admin surfaces still read only the fake"
fi

# Carried from M3, where the transport did not exist yet. Asserted by the name
# of the test rather than by a word that a comment can satisfy.
grep -rqs "a_non_leader_obtains_a_lease_through_the_leader" crates/pgprox-cluster \
  && ok "quota requests reach the leader (M3.12)" \
  || fail "a non-leader still cannot obtain a lease"

# main.rs holds no logic a test cannot call. The composition root is the one
# place concrete types meet, so it is the one place worth this rule.
if [[ -f bin/pgprox/src/main.rs ]]; then
  lines=$(grep -cvE '^\s*(//|$)' bin/pgprox/src/main.rs || true)
  (( lines <= 15 )) && ok "main.rs is wiring only ($lines lines)" \
    || fail "main.rs has $lines lines of logic no test can reach"
fi

# The three properties the milestone is judged on, each asserted by name so a
# green compose run cannot stand in for them.
if [[ -f scripts/e2e.sh ]]; then
  ok "scripts/e2e.sh exists"
  for property in pgbench drain watermark; do
    grep -qs "$property" scripts/e2e.sh \
      && ok "e2e asserts: $property" || fail "e2e does not assert: $property"
  done
else
  fail "scripts/e2e.sh missing"
fi

[[ -f deploy/docker-compose.yml ]] \
  && ok "the e2e stack is described" || fail "deploy/docker-compose.yml missing"

if [[ -f crates/pgprox-session/Cargo.toml ]]; then
  cargo nextest run -p pgprox-session >/dev/null 2>&1 \
    && ok "session suite" || fail "session suite (cargo nextest run -p pgprox-session)"
fi

for c in pgprox-session pgprox; do
  [[ -f "crates/$c/Cargo.toml" || -f "bin/$c/Cargo.toml" ]] || continue
  ./scripts/check-crate.sh "$c" >/dev/null 2>&1 \
    && ok "fmt, clippy, doctests ($c)" || fail "workspace checks ($c)"
  ./scripts/check-coverage.sh "$c" >/dev/null 2>&1 \
    && ok "coverage ($c)" || fail "coverage ($c)"
done

./scripts/check-layering.sh >/dev/null 2>&1 \
  && ok "crate dependency rule" || fail "crate dependency rule"

finish
