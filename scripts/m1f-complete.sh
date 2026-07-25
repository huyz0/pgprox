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
# Mentioning the constant is not supporting it. negotiate_version must return
# Accept for 3.2, which today it does not: it answers Negotiate { minor: 0 }.
# The first version of this check grepped for the name and passed on a test
# import, which is the same false positive the M1R gate had.
if grep -qs 'fn negotiate_version' "$PROTO/startup.rs" \
   && grep -A20 'fn negotiate_version' "$PROTO/startup.rs" | grep -qs 'minor == 2\|SUPPORTED_MINOR'; then
  ok "3.2 is accepted, not negotiated down"
else
  fail "3.2 still only negotiated down (mentioning the constant is not support)"
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

# --- everything still green ---------------------------------------------------
echo
./scripts/check-crate.sh >/dev/null 2>&1 && ok "fmt, clippy, doctests" || fail "workspace checks"
./scripts/check-coverage.sh >/dev/null 2>&1 && ok "coverage gate" || fail "coverage gate"
./scripts/conformance.sh 17 18 >/dev/null 2>&1 \
  && ok "conformance against 17 and 18" || fail "conformance"

finish
