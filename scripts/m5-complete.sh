#!/usr/bin/env bash
# M5 completion condition: transaction pooling holds its release rule, and the
# classifier never calls a DML-bearing statement read-only.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M5: pooling and routing"
echo

POOL=crates/pgprox-pool/src
ROUTE=crates/pgprox-route/src

for c in pgprox-pool pgprox-route; do
  [[ -f "crates/$c/Cargo.toml" ]] && ok "$c exists" \
    || { fail "$c missing"; finish; }
done

# Each module named, so an unrelated file cannot satisfy this.
for m in classify hints replica router; do
  [[ -f "$ROUTE/$m.rs" ]] && ok "pgprox-route::$m" || fail "pgprox-route::$m missing"
done
for m in pin params statements pool reap; do
  [[ -f "$POOL/$m.rs" ]] && ok "pgprox-pool::$m" || fail "pgprox-pool::$m missing"
done

# The routing property, by name. A misclassification is a stale read, which is
# a correctness bug from the tenant's side rather than a slow query.
if grep -rqsE 'no_dml_bearing_statement_is_ever_classified_read_only' "$ROUTE"; then
  ok "the classifier property is asserted by name"
else
  fail "no test named for the classifier property"
fi

# The pooling property, by name.
if grep -rqsE 'a_connection_is_never_released_mid_transaction' "$POOL"; then
  ok "the release rule is asserted by name"
else
  fail "no test named for the release rule"
fi

# The hard cases from ADR 0009, each by name, so a classifier that passes the
# property test by classifying everything as a write is still caught.
for hard in with_cte for_update for_share explain_analyze volatile; do
  grep -rqs "$hard" "$ROUTE" \
    && ok "classified: $hard" || fail "the classifier does not cover: $hard"
done

# The pin triggers from ADR 0001, each by name.
for trigger in listen advisory temp_table with_hold prepare; do
  grep -rqs "$trigger" "$POOL" \
    && ok "pin trigger: $trigger" || fail "no pin trigger for: $trigger"
done

# The classifier parses SQL arriving from the internet, so it is fuzzed.
[[ -f fuzz/fuzz_targets/classify.rs ]] \
  && ok "the classifier has a fuzz target" || fail "no fuzz target for the classifier"

cargo nextest run -p pgprox-pool -p pgprox-route >/dev/null 2>&1 \
  && ok "pool and route suites" \
  || fail "suites (run: cargo nextest run -p pgprox-pool -p pgprox-route)"

for c in pgprox-pool pgprox-route; do
  ./scripts/check-crate.sh "$c" >/dev/null 2>&1 \
    && ok "fmt, clippy, doctests ($c)" || fail "workspace checks ($c)"
  ./scripts/check-coverage.sh "$c" >/dev/null 2>&1 \
    && ok "coverage ($c)" || fail "coverage ($c)"
done

./scripts/check-layering.sh >/dev/null 2>&1 \
  && ok "crate dependency rule" || fail "crate dependency rule"

finish
