#!/usr/bin/env bash
# M88: a second reading of every crate, and the eighteen things it found.
#
#   scripts/gates/m88-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code. Every finding below is checked by
# running the test that would fail if it came back, by exact name, and reading
# the exit status.
#
# # How it passes while the milestone is open
#
# The same way M24 did: it checks what has landed rather than what is planned.
# A finding gets its `run_finding` line in the commit that fixes it, never
# before.
#
# That would be a gate anyone could pass by ticking a task and adding nothing,
# so the first check is the one that closes it: every M88 task the backlog
# marks done must be named here. Ticking `M88.3` without adding its test fails
# this script, which is the only reason the arrangement is worth having.
source "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

cd "$REPO_ROOT"

echo "M88: a second reading of every crate, and the eighteen things it found"
echo

BACKLOG="${PGPROX_BACKLOG:-docs/internal/product/backlog.md}"
SELF="${BASH_SOURCE[0]}"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Each finding, named by the test that would fail if it came back. `--exact`
# with a name nothing matches exits non-zero, so this cannot pass by describing
# a test that is no longer there.
#
# No pipeline into `grep -q`: `set -o pipefail` is on and grep exits at its
# first match, which closes the pipe and can kill `cargo test` with SIGPIPE. See
# the same comment in `m15-complete.sh` and `m24-complete.sh`, which is where
# that cost six checks.
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
#
# The check that makes the rest mean anything. Without it, a task ticked in the
# backlog and absent from this script is a finding nothing stands behind, and
# the gate would go on reporting green for the ones that did land.
#
# Two tasks are about the milestone rather than about a finding, and neither has
# a test to run: `M88.0` planned it and `M88.19` closed it. Excluded by name
# rather than by a rule about which tasks have tests, so a third exclusion has
# to be written down here to exist.
finished="$(sed -n '/^## M88:/,/^## /p' "$BACKLOG" \
  | sed -n 's/^- \[x\] `\(M88\.[0-9]*\)`.*/\1/p' \
  | grep -vE '^M88\.(0|19)$' || true)"

if [[ -z "$finished" ]]; then
  ok "no finding has been ticked yet, so none can be unchecked"
else
  unchecked=""
  while read -r task; do
    [[ -z "$task" ]] && continue
    grep -q "^# --- $task:" "$SELF" || unchecked+=" $task"
  done <<<"$finished"

  if [[ -z "$unchecked" ]]; then
    ok "every finished M88 task has checks here ($(wc -w <<<"$finished"))"
  else
    fail "these M88 tasks are ticked and nothing here checks them:$unchecked"
    printf '       a finding reported fixed with no test standing behind it\n'
  fi
fi

# --- the findings that have landed -------------------------------------------

# --- M88.1: a cancelled leader does not leak its singleflight claim ---------
run_finding pgprox-auth \
  cache::tests::a_cancelled_leader_does_not_leak_its_claim \
  "a leader's inflight claim is removed on cancellation, not only on return"

# --- M88.2: a live cap change moves an existing ledger's ceiling ------------
run_finding pgprox-cluster \
  coordinator::tests::a_live_cap_change_moves_an_existing_ledgers_ceiling \
  "a ledger's free pool follows a cap raised after it was created"
run_finding pgprox-cluster \
  lease::tests::set_pool_below_what_is_already_leased_reads_as_no_headroom_rather_than_underflowing \
  "a pool lowered below what is outstanding reads as none, not a u32 wraparound"

# --- M88.3: pgprox-route reads through the shared lexer, not split_whitespace
run_finding pgprox-route \
  hints::tests::a_comment_anywhere_before_the_value_does_not_hide_the_assignment \
  "a comment before a route SET's value does not hide the assignment"
run_finding pgprox-route \
  hints::tests::a_leading_comment_does_not_hide_a_reset_either \
  "a comment before a route RESET does not hide it either"
run_finding pgprox-route \
  router::tests::a_hint_comment_before_begin_does_not_hide_the_transaction \
  "a leading hint comment does not hide an explicit BEGIN"

# --- M88.4: pgprox-pool reads SET and DEALLOCATE through the shared lexer ---
run_finding pgprox-pool \
  params::tests::a_comment_between_set_and_the_value_does_not_hide_the_parameter \
  "a comment inside a SET does not hide the parameter it names"
run_finding pgprox-pool \
  statements::tests::a_comment_between_the_words_does_not_hide_the_deallocation \
  "a comment inside DISCARD/DEALLOCATE ALL does not hide it"

# --- M88.5: SHOW CLIENTS/SERVERS/STATS report real data or none -------------
run_finding pgprox-admin \
  rows::tests::show_clients_does_not_put_the_tenant_in_the_user_and_database_columns \
  "SHOW CLIENTS does not print the tenant into user and database"
run_finding pgprox-admin \
  rows::tests::show_servers_reports_one_row_per_connection_not_one_per_pool \
  "SHOW SERVERS reports one row per connection a pool holds"
run_finding pgprox-admin \
  rows::tests::show_stats_does_not_invent_a_query_count_from_the_transaction_one \
  "SHOW STATS does not copy the transaction count into the query column"

# --- M88.6: pgprox_client_conns is one joint breakdown, not two overlapping
run_finding pgprox \
  metrics::tests::pgprox_client_conns_does_not_double_count_a_bare_sum \
  "pgprox_client_conns's bare sum is the real client count, not double it"
run_finding pgprox \
  metrics::tests::pgprox_upstream_conns_carries_the_state_label \
  "pgprox_upstream_conns carries a state label and sums per server"
run_finding pgprox \
  metrics::tests::clients_are_counted_by_state_and_tenant_together \
  "clients are counted once per (state, tenant) cell, not per dimension"

# --- M88.7: JWT auth without --require-tls refuses to start, not silently --
run_finding pgprox \
  entry::tests::jwt_auth_without_require_tls_refuses_to_start \
  "a node with JWT auth reachable and TLS not required refuses to start"
run_finding pgprox \
  entry::tests::insecure_plaintext_auth_is_the_deliberate_way_out \
  "--insecure-plaintext-auth is the named way to start anyway"

# --- M88.8: FileSource::poll goes through spawn_blocking, not an inline read --
run_finding pgprox-config \
  provider::tests::poll_yields_to_the_runtime_instead_of_blocking_it \
  "poll() hands the read to spawn_blocking rather than blocking the runtime"

# --- M88.9: ParameterCache::ensure says goodbye to its probe connection --
run_finding pgprox-session \
  probe::tests::ensure_says_goodbye_to_its_probe_connection \
  "the probe connection sends Terminate before it is dropped"

# --- M88.10: NoConnection carries the most recent refusal, not the first --
run_finding pgload \
  run::tests::a_run_that_never_connects_reports_the_most_recent_refusal \
  "NoConnection reports the target's current refusal, not a stale first one"

finish
