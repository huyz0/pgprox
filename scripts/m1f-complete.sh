#!/usr/bin/env bash
# M1F completion condition: full protocol coverage, measured rather than claimed.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"
PROTO=crates/pgprox-proto/src

echo "M1F: full protocol coverage"
echo

# --- Group A: message surface -------------------------------------------------
# Anchored on the constant definition, not the name appearing somewhere.
for item in EMPTY_QUERY_RESPONSE PARAMETER_DESCRIPTION FUNCTION_CALL \
            FUNCTION_CALL_RESPONSE; do
  if grep -qsE "pub const $item: Self = Self\(b'.'\)" "$PROTO/frame.rs"; then
    ok "Tag::$item"
  else
    fail "Tag::$item is not defined"
  fi
done
grep -qsE 'Tag::PARAMETER_DESCRIPTION =>' "$PROTO/backend.rs" \
  && ok "ParameterDescription decoded" || fail "ParameterDescription has no decode arm"

# Error fields, matched on the assignment rather than the word.
#
# The first version grepped case-insensitively for the field name, which matches
# the doc comments as readily as the code: deleting `fields.detail = value` left
# five other occurrences of "detail" and the gate still passed. That is the third
# time a gate here has matched prose, so these now anchor on syntax that only
# appears when the field is genuinely wired up.
for field in detail hint position internal_position internal_query context \
             schema table column datatype constraint file line routine \
             severity_nonlocalized; do
  if grep -qsE "fields\.$field = value" "$PROTO/backend.rs"; then
    ok "error field: $field"
  else
    fail "error field not assigned: $field"
  fi
done

# --- Group B: SCRAM -----------------------------------------------------------
if [[ -f "$PROTO/scram.rs" ]] || [[ -f crates/pgprox-auth/src/scram.rs ]]; then
  ok "SCRAM module exists"
else
  fail "no SCRAM module; ADR 0002 chose it and admin tooling cannot connect without it"
fi
# An actual asserted vector, not a mention of the RFC in a comment.
if grep -qsE 'assert_eq!\(\s*$' crates/pgprox-auth/src/scram.rs \
   && grep -qs 'dHzbZapWIk4jUhN' crates/pgprox-auth/src/scram.rs; then
  ok "SCRAM asserted against the RFC 7677 vector"
else
  fail "SCRAM does not assert the published RFC 7677 proof"
fi

# --- Group C: protocol 3.2 ----------------------------------------------------
# ADR 0016 decided to negotiate 3.2 down rather than implement it, so this gate
# asserts that decision holds rather than assuming the opposite. A gate encoding
# a presumed answer quietly forces it.
if compgen -G 'product/decisions/*protocol-3-2*' >/dev/null; then
  ok "protocol 3.2 handling is a recorded decision"
else
  fail "no ADR deciding what to do about protocol 3.2"
fi
# The behaviour the ADR commits to: 3.2 in, 3.0 offered back.
if grep -qsE 'fn version_3_2_is_negotiated_down_to_3_0' "$PROTO/startup.rs"; then
  ok "3.2 down-negotiation is tested"
else
  fail "nothing tests that a 3.2 client is offered 3.0"
fi

# --- the sidecar contract is owned and frozen --------------------------------
if grep -qs 'STATUS: FROZEN' proto/pgprox/auth/v1/auth.proto; then
  ok "sidecar contract is frozen"
else
  fail "sidecar contract still reads as a proposal; ADR 0017 froze it"
fi

# --- Group D: replication scope decision -------------------------------------
if compgen -G 'product/decisions/*replication*' >/dev/null; then
  ok "replication scope recorded as an ADR"
else
  fail "no ADR deciding replication scope (M1F.17 gates the rest of group D)"
fi

# --- Group E: startup and session --------------------------------------------
# The function, not the word, which appears throughout the module's prose.
grep -qsE 'pub fn options\(' "$PROTO/startup.rs" \
  && ok "startup options parsed" || fail "startup options parameter not parsed"

# --- Group F: coverage is measured, not claimed -------------------------------
if [[ -x scripts/message-coverage.sh ]]; then
  ok "message coverage report exists"
else
  fail "scripts/message-coverage.sh missing; coverage is claimed rather than measured"
fi

# --- Group F: the drivers meet the proxy, not only the harness -----------------
#
# conformance.sh answers whether our codec and our harness agree with each
# other. They are the same code. asyncpg could not run a parameterised query
# through the real proxy from M6 until M8 and that suite stayed green the whole
# time, because the harness answered a `Flush` the same wrong way.
if [[ -x scripts/driver-matrix.sh ]]; then
  ok "the drivers can be run against the proxy"
else
  fail "scripts/driver-matrix.sh missing: the drivers have only ever met the harness"
fi

if [[ -f product/conformance/driver-matrix.md ]]; then
  ok "a driver matrix against the proxy is recorded"
else
  fail "no driver matrix recorded: the result exists only in a terminal"
fi

# --- everything still green ---------------------------------------------------
echo
./scripts/check-crate.sh >/dev/null 2>&1 && ok "fmt, clippy, doctests" || fail "workspace checks"
./scripts/check-coverage.sh >/dev/null 2>&1 && ok "coverage gate" || fail "coverage gate"
./scripts/conformance.sh 17 18 >/dev/null 2>&1 \
  && ok "conformance against 17 and 18" || fail "conformance"

finish
