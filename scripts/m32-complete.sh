#!/usr/bin/env bash
# M32: the comparison against pgbouncer and pgcat.
#
#   scripts/m32-complete.sh
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
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M32: the comparison against pgbouncer and pgcat"
echo

BACKLOG="${PGPROX_BACKLOG:-product/backlog.md}"
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
  | grep -vE '^M32\.(0|5)$' || true)"

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

finish
