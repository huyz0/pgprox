#!/usr/bin/env bash
# M20: the protocol layer against pgbouncer, pgcat and odyssey.
#
#   scripts/gates/m20-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# # This gate exists from the milestone's first commit and grows with it
#
# `M19`'s gate made the argument and this one inherits it: a milestone whose
# completion condition arrives last is a milestone that was unmeasurable while
# it was open. It passes at every point, which is what lets CI run it now.
source "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

cd "$REPO_ROOT"

echo "M20: the protocol layer against pgbouncer, pgcat and odyssey"
echo

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

run_test() {
  local crate="$1" name="$2" claim="$3"
  local out="$WORK/$crate-$RANDOM.out"

  # `|| true` so that cargo failing outright becomes a reported failure rather
  # than a dead script: `lib.sh` sets `-e`. `M19`'s gate exited 101 with four
  # checks printed and no verdict, which reads exactly like one that passed.
  cargo test -p "$crate" --all-targets -- --exact "$name" >"$out" 2>>"$WORK/log" || true
  if grep -q "^test $name \.\.\. ok$" "$out"; then
    ok "$claim"
  else
    fail "$claim: $crate $name did not run and pass"
    printf '       a claim this milestone made has no test standing behind it\n'
  fi
}

# --- M20.0: the half of the rule that already holds --------------------------
#
# This milestone's first finding is `M15.3`'s, through the door that fix left
# open: `DISCARD ALL` reaches both maps and a protocol `Close` does not. So the
# gate starts by holding the half that works, which is what makes the missing
# half a gap rather than an opinion, and gains the other checks as the tasks
# land.
run_test pgprox-session "resume::tests::discard_all_makes_both_maps_forget" \
  "DISCARD ALL is still forgotten by the session and by the connection"
run_test pgprox-session "resume::tests::a_re_parse_after_discard_all_prepares_again_rather_than_assuming" \
  "a re-parse after DISCARD ALL prepares rather than assuming"

# --- M20.1: and the half that did not ----------------------------------------
#
# Two tests for one defect, deliberately. The first is the rule in the layer
# that owns it, with no socket. The second is the whole path, against a fake
# that had to learn `Close` before it could show anything: it answered one from
# its simple-query arm for four milestones, which is why four readings of this
# code found nothing here.
run_test pgprox-session "resume::tests::a_protocol_close_makes_both_maps_forget" \
  "a closed statement is forgotten by the session and by the connection"
run_test pgprox-session "resume::tests::closing_a_statement_this_session_never_parsed_forgets_nothing" \
  "a Close of an unknown name drops nothing else"
run_test pgprox "serve::tests::a_statement_the_client_closed_is_prepared_again_before_the_next_bind" \
  "a client that closes and re-prepares a statement is not answered 26000"

# --- M20.2: the connection string's settings reach the connection ------------
#
# The end-to-end one is the claim: the client sends no `SET` at all, so anything
# on the wire naming `search_path` got there from the startup packet. The
# sans-I/O one is the branch it cannot show, which is a setting the allowlist
# will not replay pinning the session instead of being lost.
run_test pgprox "serve::tests::a_search_path_from_the_connection_string_reaches_the_server" \
  "a search_path from the connection string reaches the server"
run_test pgprox-session "relay::tests::a_startup_setting_outside_the_allowlist_pins_rather_than_being_lost" \
  "a startup setting that cannot be replayed pins rather than being dropped"
run_test pgprox-session "relay::tests::one_pin_is_reported_once_however_many_settings_caused_it" \
  "two unreplayable settings are one pin, counted once"

# --- M20.3: an extension nobody implements is declined out loud --------------
#
# The wire test is the one that matters, and it is the whole path on purpose:
# the two halves that were wrong lived in different crates, and either one left
# alone still tells a client its extension was accepted.
run_test pgprox-session "shell::tests::a_protocol_extension_is_declined_on_the_wire_by_name" \
  "an unimplemented _pq_ extension is declined by name on the wire"
run_test pgprox-proto "startup::tests::an_extension_makes_an_answer_owed_at_a_version_that_needs_none" \
  "an extension makes an answer owed even at a version that needs none"
run_test pgprox-proto "startup::tests::an_extension_is_recognised_by_its_prefix_and_nothing_else" \
  "an ordinary parameter is not reported as an unrecognised option"

# --- M20.4: a reaped connection says goodbye ---------------------------------
#
# End to end, against a fake that had to learn to notice one: a `Terminate` has
# an empty body, so a fake recording bodies records the empty string and no
# assertion can tell it from anything else.
run_test pgprox "serve::tests::a_reaped_connection_says_goodbye_rather_than_vanishing" \
  "a reaped connection sends Terminate before its socket goes"

# --- M20.5: a connection nobody watched is checked before it is lent ---------
#
# The fake dies while the connection is idle, which is what a
# `pg_terminate_backend`, an `idle_session_timeout` or a restart leaves behind.
# Without the check the client does not get an error, it gets its socket closed,
# which is the shape this was reported as being better than.
run_test pgprox "serve::tests::a_connection_that_died_while_idle_is_not_handed_to_a_client" \
  "a connection that died while idle is not handed to a client"

# --- M20.6: the unnamed statement is still the unnamed statement -------------
#
# The assertion is about which name left this process, which is why it is a unit
# test on the rewrite rather than a sequence: both behaviours produce a working
# sequence, and only one of them produces a statement the server keeps.
run_test pgprox "serve::tests::an_unnamed_parse_keeps_its_name_and_a_named_one_does_not" \
  "an unnamed Parse is not renamed and a named one still is"
run_test pgprox-pool "statements::tests::the_unnamed_statement_is_tracked_apart_from_the_ones_that_are_held" \
  "the unnamed statement does not occupy a slot under the per-connection cap"

# --- M20.7: the other half of what a client asks for at connect --------------
#
# The precedence test is the one worth keeping. Both forms carry the same
# settings, so the only thing that decides a disagreement is the order they are
# replayed in, and that order is a field's contents rather than a rule anyone
# can read from the call site.
run_test pgprox "serve::tests::a_plain_startup_parameter_reaches_the_server_too" \
  "a plain startup parameter reaches the server"
run_test pgprox-session "state::tests::options_wins_where_a_client_asked_for_the_same_setting_both_ways" \
  "options wins over a plain parameter naming the same setting"
run_test pgprox-proto "startup::tests::a_plain_startup_parameter_is_a_setting_and_the_four_special_ones_are_not" \
  "user, database, options and replication are not runtime settings"

# --- M20.8: a connection this proxy cannot serve is refused, not ignored ------
#
# Both halves, because the parameter being present is not the question and its
# value is: a client that said `replication=false` is an ordinary client and
# must not be refused for having mentioned it.
run_test pgprox-session "state::tests::a_replication_connection_is_refused_at_connect_and_told_why" \
  "a replication connection is refused at connect and told why"
run_test pgprox-session "state::tests::a_client_that_said_replication_is_off_is_an_ordinary_client" \
  "replication=false is an ordinary client"
run_test pgprox-proto "startup::tests::replication_is_read_the_way_postgres_reads_it" \
  "replication is read the way Postgres reads it"

finish
