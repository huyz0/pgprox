#!/usr/bin/env bash
# M4 completion condition: config providers, the metric registry, the admin API
# and the SHOW pseudo-database.
source "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

cd "$REPO_ROOT"

echo "M4: operations"
echo

CONFIG=crates/pgprox-config/src
OBSERVE=crates/pgprox-observe/src
ADMIN=crates/pgprox-admin/src

for c in pgprox-config pgprox-observe pgprox-admin; do
  [[ -f "crates/$c/Cargo.toml" ]] && ok "$c exists" \
    || { fail "$c missing"; finish; }
done

# The contract the admin API reads, which lives in core because pgprox-admin
# may not depend on the crates that hold the data. See the M4 backlog note.
[[ -f crates/pgprox-core/src/admin.rs ]] \
  && ok "pgprox-core::admin" || fail "pgprox-core::admin missing"

for m in document provider drain; do
  [[ -f "$CONFIG/$m.rs" ]] && ok "pgprox-config::$m" || fail "pgprox-config::$m missing"
done
for m in metrics spans health tenants; do
  [[ -f "$OBSERVE/$m.rs" ]] && ok "pgprox-observe::$m" || fail "pgprox-observe::$m missing"
done
for m in api openapi show; do
  [[ -f "$ADMIN/$m.rs" ]] && ok "pgprox-admin::$m" || fail "pgprox-admin::$m missing"
done

# The symlink swap is the case an event watcher pointed at the file misses, and
# it is the reason hot reload appears to work in testing and fails in a cluster.
grep -rqs "symlink" "$CONFIG" \
  && ok "the symlink swap is covered" || fail "no test for a ConfigMap symlink swap"

# An unbounded label takes down a Prometheus, so this is a review blocker
# expressed as a test rather than as a preference.
if grep -rqsE 'no_metric_has_an_unbounded_label' "$OBSERVE"; then
  ok "unbounded labels are rejected by name"
else
  fail "no test named for the unbounded label rule"
fi

# Readiness that flaps under load causes the connection storm the whole design
# exists to prevent.
grep -rqs "readyz" "$OBSERVE" \
  && ok "readiness is covered" || fail "no readiness endpoint"

# Every SHOW command in ADR 0007, by name, so a parser that accepts one and
# silently ignores the rest is caught.
for cmd in pools servers clients peers quota tenants config stats; do
  grep -rqsi "\"$cmd\"\|show_$cmd\|Show::$cmd" "$ADMIN" \
    && ok "SHOW ${cmd^^}" || fail "SHOW ${cmd^^} missing"
done

# The generated document is the contract an agent reads, so a broken one is a
# broken contract rather than a cosmetic problem.
if grep -rqsE 'the_openapi_document_validates' "$ADMIN"; then
  ok "the OpenAPI document is validated by name"
else
  fail "no test named for OpenAPI validation"
fi

cargo nextest run -p pgprox-config -p pgprox-observe -p pgprox-admin >/dev/null 2>&1 \
  && ok "operations suites" \
  || fail "suites (run: cargo nextest run -p pgprox-config -p pgprox-observe -p pgprox-admin)"

for c in pgprox-core pgprox-config pgprox-observe pgprox-admin; do
  ./scripts/check-crate.sh "$c" >/dev/null 2>&1 \
    && ok "fmt, clippy, doctests, rustdoc ($c)" || fail "workspace checks ($c)"
  ./scripts/check-coverage.sh "$c" >/dev/null 2>&1 \
    && ok "coverage ($c)" || fail "coverage ($c)"
done

./scripts/check-layering.sh >/dev/null 2>&1 \
  && ok "crate dependency rule" || fail "crate dependency rule"
./scripts/check-deps.sh >/dev/null 2>&1 \
  && ok "supply chain" || fail "supply chain (run: ./scripts/check-deps.sh)"

finish
