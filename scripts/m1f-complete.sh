#!/usr/bin/env bash
# M1F completion condition: full protocol coverage, measured rather than claimed.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"
PROTO=crates/pgprox-proto/src

echo "M1F: full protocol coverage"
echo

# --- Group A: message surface -------------------------------------------------
for item in EMPTY_QUERY_RESPONSE PARAMETER_DESCRIPTION FUNCTION_CALL; do
  grep -qs "$item" "$PROTO/frame.rs" && ok "Tag::$item" || fail "Tag::$item missing"
done
grep -qs 'ParameterDescription' "$PROTO/backend.rs" \
  && ok "ParameterDescription decoded" || fail "ParameterDescription not decoded"

# Error fields: three of about twenty today. Named, not counted, so adding an
# unrelated field cannot satisfy this.
for field in detail hint position schema table column constraint routine; do
  grep -qsi "$field" "$PROTO/backend.rs" \
    && ok "error field: $field" || fail "error field not extracted: $field"
done

# --- Group B: SCRAM -----------------------------------------------------------
if [[ -f "$PROTO/scram.rs" ]] || [[ -f crates/pgprox-auth/src/scram.rs ]]; then
  ok "SCRAM module exists"
else
  fail "no SCRAM module; ADR 0002 chose it and admin tooling cannot connect without it"
fi
grep -rqs 'RFC 5802\|rfc5802\|7677' crates/ \
  && ok "SCRAM tested against published vectors" \
  || fail "SCRAM not tested against RFC 5802/7677 vectors"

# --- Group C: protocol 3.2 ----------------------------------------------------
grep -qs 'PROTOCOL_3_2' "$PROTO/startup.rs" && grep -qs 'Accept' "$PROTO/startup.rs" \
  && ok "3.2 referenced in negotiation" || fail "3.2 still only negotiated down"

# --- Group D: replication scope decision -------------------------------------
if compgen -G 'product/decisions/*replication*' >/dev/null; then
  ok "replication scope recorded as an ADR"
else
  fail "no ADR deciding replication scope (M1F.17 gates the rest of group D)"
fi

# --- Group E: startup and session --------------------------------------------
grep -qs 'options' "$PROTO/startup.rs" \
  && ok "startup options parsed" || fail "startup options parameter not parsed"

# --- Group F: coverage is measured, not claimed -------------------------------
if [[ -x scripts/message-coverage.sh ]]; then
  ok "message coverage report exists"
else
  fail "scripts/message-coverage.sh missing; coverage is claimed rather than measured"
fi

# --- everything still green ---------------------------------------------------
echo
./scripts/check-crate.sh >/dev/null 2>&1 && ok "fmt, clippy, doctests" || fail "workspace checks"
./scripts/check-coverage.sh >/dev/null 2>&1 && ok "coverage gate" || fail "coverage gate"
./scripts/conformance.sh 17 18 >/dev/null 2>&1 \
  && ok "conformance against 17 and 18" || fail "conformance"

finish
