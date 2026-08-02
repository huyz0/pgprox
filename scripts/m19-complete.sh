#!/usr/bin/env bash
# M19: a seam for peer discovery.
#
#   scripts/m19-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# # This gate exists from the milestone's first commit and grows with it
#
# `M18.3` made a milestone with nothing to run a failure, and `M18.0` said a
# completion condition is part of the milestone rather than an afterthought.
# Taken together that means an open milestone's gate cannot wait until the end,
# so this one starts by checking what `M18.2` already produced and gains a check
# as each task lands. It passes at every point, which is what lets CI run it
# while the milestone is open.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M19: a seam for peer discovery"
echo

SPEC="specs/2026-08-02-peer-discovery-seam"

# --- the spec this milestone is built from -----------------------------------
#
# Checked because the tasks are written against it. A spec that lost the rule
# would be a spec permitting the change it was written to forbid, and the tasks
# would still look done.
flat() { sed 's/^[[:space:]]*>[[:space:]]*/ /' "$1" | tr '\n' ' ' | tr -s ' '; }
if [[ -f "$SPEC/spec.md" ]] \
   && flat "$SPEC/spec.md" | grep -q "never cause a node to be counted alive that gossip has not heard from"; then
  ok "the spec states the rule that keeps the seam safe"
else
  fail "$SPEC/spec.md is missing or no longer states what an external source may not do"
fi

for part in contracts.md tests.md tasks.md; do
  if [[ -s "$SPEC/$part" ]]; then
    ok "the spec has its $part"
  else
    fail "$SPEC/$part is missing or empty"
  fi
done

# --- M19.1: the seam, and the rule it exists to hold -------------------------
#
# Each by the test that would fail if the claim came back. `M12.8`: the gate
# runs the test rather than looking for the file it lives in.
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

run_test() {
  local crate="$1" name="$2" claim="$3"
  local out="$WORK/$crate-$RANDOM.out"

  # `|| true` so that cargo failing outright becomes a reported failure rather
  # than a dead script: `lib.sh` sets `-e`, and the first version of this gate
  # exited 101 with four checks printed and no verdict, which reads exactly like
  # a gate that passed and stopped early.
  cargo test -p "$crate" --all-targets -- --exact "$name" >"$out" 2>>"$WORK/log" || true
  if grep -q "^test $name \.\.\. ok$" "$out"; then
    ok "$claim"
  else
    fail "$claim: $crate $name did not run and pass"
    printf '       a claim this milestone made has no test standing behind it\n'
  fi
}

run_test pgprox-core "cluster::tests::a_static_source_serves_what_it_was_built_with" \
  "a static source serves the table it was built with"
run_test pgprox-core "cluster::tests::a_published_table_reaches_a_receiver_taken_before_it" \
  "a table published later reaches a receiver taken at startup"
run_test pgprox-core "cluster::tests::a_source_that_has_gone_stale_says_so_through_an_arc" \
  "a stale source is still stale through an Arc"
run_test pgprox-core "cluster::tests::the_default_loop_never_returns" \
  "a source with no loop does not look like one that finished"

# --- M19.3: the three consumers read the current table -----------------------
#
# One test each, and each publishes *after* the consumer was built. That is the
# only shape that can tell a source from a copy: a test that published first
# would pass against either.
run_test pgprox "serve::tests::a_cancel_for_a_node_added_after_startup_is_forwarded_to_it" \
  "a cancel reaches a node that joined after this one started"
run_test pgprox "observatory::tests::the_fan_out_reaches_a_peer_added_after_construction" \
  "the client fan-out asks a peer that joined after construction"
run_test pgprox "gossip::tests::a_quota_request_goes_to_a_leader_whose_address_arrived_late" \
  "a quota request reaches a leader whose address arrived late"

# --- M19.4 and M19.5: the simulation, and the claim it corrected -------------
#
# The second of these was a reduction of a cap breach until the transport was
# read. It asserts the opposite now, and it was checked against the one-way
# model it came from: both it and the property fail there, so neither is
# passing vacuously.
run_test pgprox-cluster "coordinator::tests::the_cap_holds_while_peer_tables_change_underneath_it" \
  "the cap holds while peer tables change under the fleet"
run_test pgprox-cluster "coordinator::tests::a_peer_table_cannot_make_liveness_one_way" \
  "a peer table cannot make liveness one-way, because an exchange is not"
run_test pgprox-cluster "coordinator::tests::a_peer_table_a_node_never_hears_from_moves_no_quorum" \
  "a peer nothing has gossiped with moves no quorum"

# --- M19.6: the drain fake refuses where a drain refuses ---------------------
#
# Run once, which is what this gate can honestly claim. The failure it fixed
# showed about one run in twenty-five, so one pass here is not evidence that it
# is gone and this line does not say it is: what it catches is the fake losing
# its boundary rule outright, which is the regression that would put the flake
# back. The evidence for the flake being gone is in `product/backlog.md` and is
# eight runs out of eight at five times the exposure, against zero out of eight
# without the rule, plus twenty consecutive runs at the committed duration.
# `M16.8` set that shape and added no gate at all.
run_test pgload "run::tests::a_drain_mid_run_is_a_relocation_rather_than_an_error" \
  "a drain between transactions is a relocation and not a lost transaction"
run_test pgload "client::tests::a_shutdown_after_a_statement_has_run_is_a_loss_rather_than_a_relocation" \
  "the same code after a statement has run is the loss it used to produce by luck"

finish
