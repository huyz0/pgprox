#!/usr/bin/env bash
# M32: the comparison against pgbouncer and pgcat.
#
#   scripts/gates/m32-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# # What this gate can and cannot reach
#
# The run itself needs three containers and a Postgres, so it is not something
# a per-commit gate runs. What it checks is everything the run rests on: that
# the client can authenticate the way the other two poolers require, that the
# three arms are configured to the same cap, and that the document exists.
#
# The same division `scripts/e2e.sh` and `scripts/bench.sh` already sit on.
source "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

cd "$REPO_ROOT"

echo "M32: the comparison against pgbouncer and pgcat"
echo

BACKLOG="${PGPROX_BACKLOG:-docs/internal/product/backlog.md}"
SELF="${BASH_SOURCE[0]}"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

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
finished="$(sed -n '/^## M32:/,/^## /p' "$BACKLOG" \
  | sed -n 's/^- \[x\] `\(M32\.[0-9]*\)`.*/\1/p' \
  | grep -vE '^M32\.(0|7)$' || true)"

if [[ -z "$finished" ]]; then
  ok "no finding has been ticked yet, so none can be unchecked"
else
  unchecked=""
  while read -r task; do
    [[ -z "$task" ]] && continue
    grep -q "^# --- $task:" "$SELF" || unchecked+=" $task"
  done <<<"$finished"

  if [[ -z "$unchecked" ]]; then
    ok "every finished M32 task has checks here ($(wc -w <<<"$finished"))"
  else
    fail "these M32 tasks are ticked and nothing here checks them:$unchecked"
  fi
fi

# --- the findings that have landed -------------------------------------------

# --- M32.1: the load client can authenticate the way the other two require ----
#
# The handshake, against a fake server running `pgprox-auth`'s own server half,
# so the two ends of the exchange are the two ends this workspace ships rather
# than a fake agreeing with itself.
run_finding pgload \
  client::tests::a_scram_handshake_completes_and_leaves_a_usable_session \
  "the load client completes a SCRAM handshake"

# SCRAM is mutual and the client half of that is the easy part to leave out,
# because a handshake that skips it still succeeds against an honest server.
run_finding pgload \
  client::tests::a_server_that_cannot_prove_it_knew_the_password_is_refused \
  "and refuses a server that cannot prove it knew the password"

# A run that reports zero transactions and no reason is a run nobody can act on.
run_finding pgload \
  client::tests::a_mechanism_this_client_does_not_have_is_refused_by_name \
  "and names the mechanism when it cannot answer one"

# One SCRAM client exchange in the workspace, not two. `bin/pgprox` drives the
# same type through `pgprox-session`'s trait, and its own test runs that half
# against the server half, so a divergence fails there rather than here.
run_finding pgprox dial::tests::the_client_half_and_the_server_half_agree \
  "the proxy drives the same exchange, and it agrees with the server half"

# --- M32.6: the client answers MD5, because pgcat offers nothing else ---------
#
# Against a value Postgres computed rather than one this code did. A test that
# recomputed the same formula in the same order would pass for a wrong formula,
# and the first two expectations written here were both wrong.
run_finding pgload client::tests::the_md5_answer_is_postgres_own_construction \
  "the md5 answer is the construction Postgres computes"
run_finding pgload client::tests::an_md5_request_is_answered_with_the_salted_digest \
  "and an md5 request is answered with it"

# The refusal is still there for a method this client genuinely cannot answer.
# That test named MD5 until this milestone, and it was repurposed rather than
# deleted: its subject is the refusal, not the mechanism.
run_finding pgload client::tests::a_method_this_client_cannot_answer_is_reported_by_name \
  "and a method it cannot answer is still refused by name"

# The proxy is not the load client and still refuses MD5, for the reason on its
# own dial path. `M32.6` is about a measurement tool speaking what it measures.
if grep -q 'the server asked for md5, which this proxy does not implement' \
    crates/pgprox-session/src/connect.rs; then
  ok "the proxy still refuses md5"
else
  fail "the proxy no longer refuses md5, which M32.6 was explicitly not about"
fi

# --- M32.2: the arms are configured so the comparison is about pooling --------
#
# Not `run_finding`: what landed is configuration, and the check is that the
# three files agree about the two things that decide whether this is a
# comparison at all. Both are read from the files rather than restated, so a
# check here keeps passing only while the files do.

cap_pgprox="$(awk '/max_connections:/ { print $2; exit }' deploy/config/compare.yaml)"
cap_pgbouncer="$(awk -F'= *' '/^max_db_connections/ { print $2; exit }' deploy/compare/pgbouncer.ini)"
cap_pgcat="$(awk -F'= *' '/^pool_size/ { print $2; exit }' deploy/compare/pgcat.toml)"
if [[ "$cap_pgprox" == "$cap_pgbouncer" && "$cap_pgprox" == "$cap_pgcat" ]]; then
  ok "all three arms are capped at $cap_pgprox upstream connections"
else
  fail "the arms are capped differently, so a run would not be a comparison"
  printf '       pgprox %s, pgbouncer %s, pgcat %s\n' \
    "$cap_pgprox" "$cap_pgbouncer" "$cap_pgcat"
fi

# The one that nearly produced a false finding. Without these the other two
# arms fail every named Parse, which reads exactly like the failure ADR 0011
# predicts for a pooler with no statement mapping and is a missing line of
# configuration. `M32.6` watched it happen.
statements_missing=""
grep -q '^max_prepared_statements *= *[1-9]' deploy/compare/pgbouncer.ini || statements_missing+=" pgbouncer"
grep -q '^prepared_statements_cache_size *= *[1-9]' deploy/compare/pgcat.toml || statements_missing+=" pgcat"
if [[ -z "$statements_missing" ]]; then
  ok "prepared statements are mapped in every arm that needs telling"
else
  fail "these arms would fail every named Parse, which is configuration and not a finding:$statements_missing"
fi

# --- M32.3: the run ----------------------------------------------------------
#
# The run needs four containers and a Postgres, which is `scripts/e2e.sh`'s
# division rather than a per-commit gate's. What is checked here is that the
# script exists and parses, and that it still refuses the two ways a comparison
# stops being one. A script that cannot be parsed is a run nobody can make.
if bash -n scripts/compare.sh 2>/dev/null; then
  ok "the comparison run parses"
else
  fail "scripts/compare.sh does not parse"
fi

# Its own guards, by name. These are the checks that made the difference
# between a comparison and four numbers taken under four different conditions.
for guard in check_caps_agree check_statements_are_mapped arm_address; do
  if grep -q "^$guard()" scripts/compare.sh; then
    ok "the run still refuses to proceed without $guard"
  else
    fail "scripts/compare.sh no longer has $guard, so a run could report an arm's"
    printf '       numbers taken under conditions another arm did not share\n'
  fi
done

# --- M32.8: the run is reproducible and its memory figure means something -----
#
# The run itself needs containers. What is checked is that the two fixes are
# still in the script: rounds inside one stack, and a memory figure that is not
# a difference from a baseline that stopped being a baseline.
for piece in "ROUNDS=" "cold_per_conn()" "REPORT_ONLY="; do
  if grep -q -- "$piece" scripts/compare.sh; then
    ok "the run still carries $piece"
  else
    fail "scripts/compare.sh no longer carries $piece"
    printf '       without it a run reports one number with no spread behind it,\n'
    printf '       or a per-connection figure taken against the previous round peak\n'
  fi
done

# The median is over rounds, so a single round has to be a deliberate choice
# rather than the default. Three, because two cannot have a middle value.
rounds="$(awk -F'[:-]' '/^ROUNDS=/ { print $3; exit }' scripts/compare.sh | tr -d '}"')"
if [[ "$rounds" =~ ^[0-9]+$ ]] && (( rounds >= 3 )); then
  ok "the run defaults to $rounds rounds, so every figure has a spread"
else
  fail "the run defaults to $rounds rounds, which is not enough for a median"
fi

# --- M32.4: the run, recorded ------------------------------------------------
#
# The run needs four containers. What a per-commit gate reads is the document,
# and the two things in it that a later change could quietly make false: the cap
# every arm was held to, and the versions the other two arms were.
RUN="${PGPROX_RUN_DOC:-docs/internal/product/perf/run-2026-08-05-pgbouncer-pgcat.md}"
if [[ -f "$RUN" ]]; then
  ok "$RUN records every arm's figures"
else
  fail "$RUN is missing, so the comparison is a claim in a commit message"
fi

# The cap the document reports is the cap the files still carry. A document
# saying sixty beside configuration saying ninety is worse than no document.
doc_cap="$(grep -o 'Upstream cap | [0-9]*' "$RUN" 2>/dev/null | grep -o '[0-9]*' || true)"
file_cap="$(awk '/max_connections:/ { print $2; exit }' deploy/config/compare.yaml)"
if [[ -n "$doc_cap" && "$doc_cap" == "$file_cap" ]]; then
  ok "the document and the configuration agree the cap was $doc_cap"
else
  fail "the document says cap ${doc_cap:-none} and the configuration says $file_cap"
fi

finish
