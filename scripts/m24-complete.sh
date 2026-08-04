#!/usr/bin/env bash
# M24: a reading of every crate, and the nine things it found.
#
#   scripts/m24-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code. Every finding below is checked by
# running the test that would fail if it came back, by exact name, and reading
# the exit status.
#
# # How it passes while the milestone is open
#
# The same way M19 through M23 did: it checks what has landed rather than what
# is planned. A finding gets its `run_finding` line in the commit that fixes it,
# never before.
#
# That would be a gate anyone could pass by ticking a task and adding nothing,
# so the first check is the one that closes it: every M24 task the backlog marks
# done must be named here. Ticking `M24.3` without adding its test fails this
# script, which is the only reason the arrangement is worth having.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M24: a reading of every crate, and the nine things it found"
echo

BACKLOG="${PGPROX_BACKLOG:-product/backlog.md}"
SELF="${BASH_SOURCE[0]}"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Each finding, named by the test that would fail if it came back. `--exact`
# with a name nothing matches exits non-zero, so this cannot pass by describing
# a test that is no longer there.
#
# No pipeline into `grep -q`: `set -o pipefail` is on and grep exits at its
# first match, which closes the pipe and can kill `cargo test` with SIGPIPE. See
# the same comment in `m15-complete.sh`, which is where that cost six checks.
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
# `M24.0` is the planning task and has no test; it is excluded by name rather
# than by a rule about which tasks have tests, so a second exclusion has to be
# written down here to exist.
finished="$(sed -n '/^## M24:/,/^## /p' "$BACKLOG" \
  | sed -n 's/^- \[x\] `\(M24\.[0-9]*\)`.*/\1/p' \
  | grep -v '^M24\.0$' || true)"

if [[ -z "$finished" ]]; then
  ok "no finding has been ticked yet, so none can be unchecked"
else
  unchecked=""
  while read -r task; do
    [[ -z "$task" ]] && continue
    grep -q "^# --- $task:" "$SELF" || unchecked+=" $task"
  done <<<"$finished"

  if [[ -z "$unchecked" ]]; then
    ok "every finished M24 task has checks here ($(wc -w <<<"$finished"))"
  else
    fail "these M24 tasks are ticked and nothing here checks them:$unchecked"
    printf '       a finding reported fixed with no test standing behind it\n'
  fi
fi

# --- the findings that have landed -------------------------------------------

# --- M24.1: a SET after a semicolon is recorded ------------------------------
run_finding pgprox-pool \
  params::tests::a_set_after_a_semicolon_is_recorded_too \
  "a SET after a semicolon reaches the session's parameters"
run_finding pgprox-pool \
  params::tests::every_replayable_set_in_a_string_is_recorded_wherever_it_sits \
  "no replayable SET is left both unrecorded and unpinned"
run_finding pgprox-pool \
  params::tests::a_reset_after_a_semicolon_is_heard_too \
  "a RESET after a semicolon is heard rather than replayed over"
# The split itself, in the crate that owns it. `M22.7`: a crate's decisions are
# tested in that crate, and where a statement ends is `pgprox-core`'s decision.
run_finding pgprox-core \
  sql::tests::a_statement_is_split_on_a_separator_and_not_on_data \
  "a semicolon inside quoted text does not split a statement"

# --- M24.2: a SET with a quoted parameter name pins --------------------------
run_finding pgprox-pool \
  pin::tests::a_set_whose_parameter_name_is_quoted_pins \
  "a SET naming its parameter in quotes pins rather than being lost"
run_finding pgprox-pool \
  pin::tests::a_quoted_name_does_not_make_set_local_pin \
  "quoting the parameter does not make SET LOCAL pin"
run_finding pgprox-pool \
  params::tests::a_quoted_parameter_name_pins_instead_of_being_recorded \
  "the recorded half and the pinned half agree about a quoted name"

# --- M24.3: a schema-qualified advisory lock pins ----------------------------
run_finding pgprox-pool \
  pin::tests::a_schema_qualified_advisory_lock_pins \
  "pg_catalog.pg_advisory_lock pins, and the _xact_ forms still do not"

# --- M24.4: the cache key names the database and the role --------------------
run_finding pgprox-cache \
  store::tests::the_same_sql_against_a_different_database_is_a_different_entry \
  "one tenant's two databases do not share a cache entry"
run_finding pgprox-cache \
  store::tests::the_same_sql_under_a_different_role_is_a_different_entry \
  "two roles of one tenant do not share a cache entry"
# The wiring half, in the crate that builds the key. The two above would pass
# for a proxy that filled both fields with a constant.
run_finding pgprox \
  serve::tests::the_cache_key_carries_the_database_and_the_role_the_grant_resolved_to \
  "the key is filled from the grant rather than invented"

# --- M24.5: a full grant cache recovers --------------------------------------
run_finding pgprox-auth \
  cache::tests::a_full_cache_of_dead_entries_admits_a_live_one \
  "a grant cache full of expired entries admits a new one"
run_finding pgprox-auth \
  cache::tests::a_full_cache_of_live_entries_still_refuses \
  "the sweep drops the dead and never a live entry"
run_finding pgprox-auth \
  cache::tests::a_full_cache_does_not_sweep_on_every_miss \
  "the sweep is rate limited rather than run per connection"

# --- M24.6: the SCRAM iteration count has a ceiling --------------------------
run_finding pgprox-auth \
  scram::tests::an_absurd_iteration_count_is_refused_at_the_ceiling \
  "a peer cannot ask for four billion PBKDF2 rounds"
run_finding pgprox-auth \
  scram::tests::the_ceiling_admits_a_hardened_server \
  "the ceiling still admits a server that raised scram_iterations"
# The RFC vectors, because a ceiling set below a real exchange would pass both
# of the checks above and break every SCRAM dial.
run_finding pgprox-auth \
  scram::tests::the_client_proof_matches_rfc_7677 \
  "the RFC 7677 exchange at 4,096 rounds still derives"

finish
