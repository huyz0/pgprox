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

finish
