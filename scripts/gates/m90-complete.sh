#!/usr/bin/env bash
# M90: a third reading, from several angles at once, and what each one found.
#
#   scripts/gates/m90-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code. Every finding below is checked by
# running the test that would fail if it came back, by exact name, and reading
# the exit status.
#
# # How it passes while the milestone is open
#
# The same way `M24` and `M88` did: it checks what has landed rather than what
# is planned. A finding gets its `run_finding` line in the commit that fixes
# it, never before.
#
# That would be a gate anyone could pass by ticking a task and adding nothing,
# so the first check is the one that closes it: every M90 task the backlog
# marks done must be named here. Ticking one without adding its test fails this
# script, which is the only reason the arrangement is worth having.
source "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

cd "$REPO_ROOT"

echo "M90: a third reading, from several angles at once, and what each one found"
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
# the same comment in `m15-complete.sh`, `m24-complete.sh` and `m88-complete.sh`,
# which is where that cost six checks.
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
# `M90.0` planned it and has no test of its own; excluded by name rather than
# by a rule about which tasks have tests, the same way `M88.0` was. The task
# that eventually closes this milestone gets the same exclusion, added when it
# is filed.
finished="$(sed -n '/^## M90:/,/^## /p' "$BACKLOG" \
  | sed -n 's/^- \[x\] `\(M90\.[0-9]*\)`.*/\1/p' \
  | grep -vE '^M90\.0$' || true)"

if [[ -z "$finished" ]]; then
  ok "no finding has been ticked yet, so none can be unchecked"
else
  unchecked=""
  while read -r task; do
    [[ -z "$task" ]] && continue
    grep -q "^# --- $task:" "$SELF" || unchecked+=" $task"
  done <<<"$finished"

  if [[ -z "$unchecked" ]]; then
    ok "every finished M90 task has checks here ($(wc -w <<<"$finished"))"
  else
    fail "these M90 tasks are ticked and nothing here checks them:$unchecked"
    printf '       a finding reported fixed with no test standing behind it\n'
  fi
fi

# --- the findings that have landed -------------------------------------------

# --- M90.1: SessionRouter::route stops tracking wrote once fixed -------------
run_finding pgprox-route router::tests::a_write_after_begin_is_still_reported \
  "M90.1: a write as a transaction's second statement is still reported"
run_finding pgprox-route router::tests::a_read_only_transaction_never_bothers_classifying_for_wrote \
  "M90.1: a read-only transaction does not bother classifying for wrote"

# --- M90.2: NodeMode wildcard wire conversions --------------------------------
run_finding pgprox-core cluster::tests::as_str_names_every_mode \
  "M90.2: NodeMode::as_str names every current variant"
run_finding pgprox-cluster digest::tests::view_hash_asserts_against_an_unrecognised_mode \
  "M90.2: view_hash's wildcard arm still asserts against an unrecognised mode"
run_finding pgprox gossip::tests::wire_conversions_use_the_exhaustive_as_str \
  "M90.2: gossip.rs's wire conversions use the exhaustive as_str()"

# --- M90.3: grant cache key omits startup_user --------------------------------
run_finding pgprox-auth cache::tests::two_startup_users_on_the_same_token_get_their_own_grant \
  "M90.3: two startup users on the same token get their own grant"
run_finding pgprox-auth cache::tests::the_key_is_a_hash_rather_than_the_token \
  "M90.3: the cache key differs across token, database and user"

# --- M90.4: a rejected route hint is never reported ---------------------------
run_finding pgprox serve::tests::a_rejected_route_hint_is_reported_as_an_error_not_a_bare_ready \
  "M90.4: a rejected route hint is reported as an error, not a bare ready"
run_finding pgprox-core error::tests::mapping_matches_the_documented_table \
  "M90.4: InvalidRouteHint maps to the documented SQLSTATE"

# --- M90.5: the cancel registry leaked on a mid-transaction disconnect -------
run_finding pgprox serve::tests::a_client_that_disconnects_mid_transaction_frees_its_cancel_key \
  "M90.5: a mid-transaction disconnect frees its cancel key"

# --- M90.6: doc drift in the dependency count and ADR 0009 --------------------
run_finding pgprox replicas::tests::neither_document_calls_the_poll_interval_configured \
  "M90.6: neither document calls the poll interval configured"
run_finding pgprox replicas::tests::adr_0009_records_that_bounded_staleness_routing_is_not_built \
  "M90.6: ADR 0009 records that bounded-staleness routing is not built"

# --- M90.7: run_with_peers gossiped and drained against a frozen snapshot ----
run_finding pgprox run::tests::a_peer_published_after_the_tick_loop_started_is_gossiped_with \
  "M90.7: a peer published after the tick loop started is gossiped with"

# --- M90.8: unescaped tenant/server label values in the Prometheus exporter --
run_finding pgprox metrics::tests::a_tenant_name_with_a_quote_does_not_break_the_scrape \
  "M90.8: a tenant name with a quote does not break the scrape"
run_finding pgprox metrics::tests::a_server_name_with_a_backslash_does_not_break_the_scrape \
  "M90.8: a server name with a backslash does not break the scrape"

# --- M90.9: pgload's NoConnection claimed nothing was attempted -------------
run_finding pgload run::tests::a_target_that_only_relocates_says_so_rather_than_claiming_nothing_happened \
  "M90.9: a target that only relocates says so rather than claiming nothing happened"

# --- M90.10: a route hint swallowed whatever statement followed it ----------
run_finding pgprox-route hints::tests::a_reset_followed_by_a_real_statement_is_not_consumed_as_the_hint \
  "M90.10: a reset followed by a real statement is not consumed as the hint"
run_finding pgprox-route hints::tests::a_set_followed_by_a_real_statement_is_not_consumed_as_the_hint \
  "M90.10: a set followed by a real statement is not consumed as the hint"
run_finding pgprox-route router::tests::a_hint_followed_by_a_real_statement_forwards_both_rather_than_dropping_one \
  "M90.10: a hint followed by a real statement forwards both rather than dropping one"

# --- M90.11: normalize's word-folding could expand a codepoint into two -----
run_finding pgprox-cache normalize::tests::folding_never_changes_a_words_codepoint_count \
  "M90.11: folding never changes a word's codepoint count"

# --- M90.12: ReapConfig::max_lifetime and is_expired had no caller ----------
run_finding pgprox-pool pool::tests::a_connection_marked_expired_is_discarded_rather_than_reused \
  "M90.12: a connection marked expired is discarded rather than reused"
run_finding pgprox-pool reap::tests::an_idle_connection_past_its_lifetime_is_reaped_before_its_idle_timeout \
  "M90.12: an idle connection past its lifetime is reaped before its idle timeout"
run_finding pgprox-pool reap::tests::keep_warm_does_not_exempt_a_connection_past_its_lifetime \
  "M90.12: keep_warm does not exempt a connection past its lifetime"
run_finding pgprox-pool live::tests::a_connection_kept_busy_past_its_lifetime_is_discarded_at_its_next_release \
  "M90.12: a connection kept busy past its lifetime is discarded at its next release"

# --- M90.13: the drain's force-close sent no ErrorResponse -------------------
run_finding pgprox serve::tests::a_client_holding_a_connection_is_not_closed_by_the_drain_alone \
  "M90.13: a client holding a connection is not closed by the drain alone, and is told why"

# --- M90.14: a failed post-commit LSN probe never forced the primary --------
run_finding pgprox-core route::tests::an_unconfirmed_write_stays_on_the_primary_even_with_no_watermark \
  "M90.14: an unconfirmed write stays on the primary even with no watermark"
run_finding pgprox-route router::tests::a_write_whose_position_probe_failed_keeps_the_next_read_on_the_primary \
  "M90.14: a write whose position probe failed keeps the next read on the primary"

# --- M90.15: ADR 0009 named the wrong watermark query ------------------------
run_finding pgprox replicas::tests::adr_0009_names_the_watermark_query_the_code_actually_runs \
  "M90.15: ADR 0009 names the watermark query the code actually runs"

# --- M90.16: TenantAllowlist could allowlist a tenant named "other" ---------
run_finding pgprox-observe tenants::tests::a_tenant_named_other_is_refused_rather_than_swallowing_the_aggregate \
  "M90.16: a tenant named other is refused rather than swallowing the aggregate"

# --- M90.17: the gossip version counter reset to 0 on every restart ---------
run_finding pgprox-cluster service::tests::version_floor_reads_milliseconds_since_the_epoch \
  "M90.17: version_floor reads milliseconds since the epoch"
run_finding pgprox-cluster service::tests::version_floor_does_not_panic_before_the_epoch \
  "M90.17: version_floor does not panic before the epoch"
run_finding pgprox-cluster service::tests::a_process_that_restarts_inside_dead_after_is_not_rejected_as_stale \
  "M90.17: a process that restarts inside dead_after is not rejected as stale"

# --- M90.18: a refused probe query dropped its connection with no Terminate -
run_finding pgprox-session probe::tests::a_refused_probe_says_goodbye_before_dropping_the_connection \
  "M90.18: a refused probe says goodbye before dropping the connection"

# --- M90.19: read_header's doc contradicted the code that calls it ----------
# Doc-only: corrected comments in shell.rs and serve.rs, no behavior change
# and nothing to run a test against. Checked by the full existing suite for
# both crates staying green and `cargo doc` finding no broken link.

finish
