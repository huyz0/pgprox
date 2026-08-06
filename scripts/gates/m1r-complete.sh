#!/usr/bin/env bash
# M1R completion condition: the codec streams, the cap bug is gone, and the
# conformance suite exercises more than SELECT 1.
source "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

cd "$REPO_ROOT"

echo "M1R: protocol revision"
echo

# --- the streaming API exists -------------------------------------------------
PROTO=crates/pgprox-proto/src
grep -qs 'pub fn decode_header' "$PROTO/frame.rs" \
  && ok "decode_header exists" || fail "no header-only decode"
[[ -f "$PROTO/relay.rs" ]] && ok "relay state machine exists" || fail "$PROTO/relay.rs missing"

# --- the two caps are separate ------------------------------------------------
if grep -qs 'MAX_INSPECT' "$PROTO/frame.rs" || grep -qs 'MAX_INSPECT' "$PROTO/relay.rs"; then
  ok "inspect cap is distinct from the passthrough cap"
else
  fail "no separate inspect cap; a large DataRow will still be refused"
fi

# --- tests --------------------------------------------------------------------
cargo nextest run -p pgprox-proto >/dev/null 2>&1 \
  && ok "pgprox-proto unit tests" || fail "pgprox-proto unit tests"

./scripts/check-coverage.sh pgprox-proto >/dev/null 2>&1 \
  && ok "coverage" || fail "coverage (run: scripts/check-coverage.sh pgprox-proto)"

# --- breadth ------------------------------------------------------------------
# Named cases rather than a count, so adding an unrelated test cannot make this
# pass. Each is a gap the review found.
SUITE=crates/pgprox-proto/tests/conformance_client.rs
for case in large_value null_value multi_statement empty_query copy_in \
            binary_parameter error_mid pipelin listen_notify; do
  if grep -qs "$case" "$SUITE"; then
    ok "conformance covers: $case"
  else
    fail "conformance does not cover: $case"
  fi
done

# A distinctive marker rather than the word "prepared", which already appears in
# a comment. A gate that passes on prose is worse than no gate.
missing_depth=0
for driver in psql pgx asyncpg jdbc npgsql; do
  grep -qs 'PGPROX_DEPTH_PREPARED_REUSE' "tests/conformance/drivers/$driver.sh" \
    || { fail "driver $driver does not exercise prepared statement reuse"; missing_depth=1; }
  grep -qs 'PGPROX_DEPTH_LARGE_RESULT' "tests/conformance/drivers/$driver.sh" \
    || { fail "driver $driver does not exercise a large result"; missing_depth=1; }
done
(( missing_depth == 0 )) && ok "every driver exercises reuse and a large result"

finish
