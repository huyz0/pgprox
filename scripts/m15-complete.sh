#!/usr/bin/env bash
# M15: the protocol crate under a second reading.
#
#   scripts/m15-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code. Every check below runs a named
# test and reads its exit status, so a test that is renamed, deleted or made to
# pass vacuously fails this gate rather than quietly stopping being evidence.
#
# # What this gate does not do
#
# It does not run a mutation sweep. `M15.12` exists because a mutation run found
# a survivor in code this milestone wrote, so the honest thing is to say where
# that check lives: the nightly `mutants` job in CI, against
# `product/mutants-baseline.txt`. A targeted run over `pgprox-proto` is twelve
# minutes, and this gate sits beside fourteen others. `M14`'s gate made the same
# trade and said so.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M15: the protocol crate under a second reading"
echo

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Each finding, named by the test that would fail if it came back. Running one
# test by exact name is the check: `--exact` with a name nothing matches exits
# non-zero, so this cannot pass by describing a test that is no longer there.
# No pipeline. `set -o pipefail` is on, and `grep -q` exits at its first match,
# which closes the pipe and kills `cargo test` with SIGPIPE, so the pipeline
# reports 141 for a test that passed. It did that here for six of thirteen
# checks and not for the other seven, because whether the write lands before
# grep exits depends on how much the run printed. `M12.6` is the same family:
# a check whose verdict came from the shell rather than from the thing checked.
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

# --- M15.1: the inspect cap, and the retention that makes it worth having ----
run_finding pgprox-proto \
  relay::tests::a_whole_inspected_message_cannot_buffer_past_the_inspect_cap \
  "a client cannot buffer past the inspect cap"
run_finding pgprox-proto \
  relay::tests::the_buffer_a_large_message_needed_does_not_outlive_it \
  "a large inspection buffer is released once its message is over"
# The constant the two above are measured against, pinned to something that is
# not itself. `M15.12`: a mutation run turned `8 * 1024` into `8 + 1024` and
# every test in the file stayed green.
run_finding pgprox-proto \
  relay::tests::the_retained_size_is_the_size_it_says \
  "the retained size is pinned against the requirement that chose it"

# --- M15.2: a failed COPY gives the connection back --------------------------
run_finding pgprox-proto \
  session::tests::a_failed_copy_gives_the_connection_back \
  "a failed COPY does not hold the connection for the session's life"

# --- M15.3: DISCARD ALL reaches both statement maps --------------------------
run_finding pgprox-session \
  resume::tests::discard_all_makes_both_maps_forget \
  "DISCARD ALL is heard by both statement maps"
run_finding pgprox-session \
  resume::tests::a_re_parse_after_discard_all_prepares_again_rather_than_assuming \
  "a re-parse after DISCARD ALL prepares rather than assuming"

# --- M15.5: the header fast path is a fast path ------------------------------
run_finding pgprox-proto \
  relay::tests::a_contiguous_header_is_read_where_it_lies \
  "a contiguous header is not copied"
# And the boundary case the fast path must not have broken.
run_finding pgprox-proto \
  relay::tests::a_message_split_at_every_boundary_relays_identically \
  "a split header still relays byte for byte"

# --- M15.6: the preference rule is stated against a list that has an order ---
run_finding pgprox-proto \
  backend::tests::our_preference_order_decides_and_not_just_our_membership \
  "the SASL preference rule is falsifiable"

# --- M15.7: a client-controlled length cannot overflow the cursor ------------
run_finding pgprox-proto \
  read::tests::a_length_that_would_overflow_the_cursor_is_refused \
  "a wire length cannot overflow the read cursor"

# --- M15.9: the handshake is not read against the relay cap ------------------
run_finding pgprox-session \
  shell::tests::an_oversized_handshake_message_is_refused_before_it_is_read \
  "an unauthenticated client is not read against the gigabyte cap"

# --- M15.10: a count never disagrees with the list it counts -----------------
run_finding pgprox-proto \
  encode::answered_tests::a_count_never_disagrees_with_the_list_it_counts \
  "an encoded count matches the number of items behind it"

# --- M15.11: the parameter that pinned where it could replay -----------------
run_finding pgprox-pool \
  pin::tests::a_parameter_that_replays_is_also_recorded_for_replay \
  "a replayable parameter is recorded as well as unpinned"

# --- M15.13: nothing reserves on a count it has not read ---------------------
run_finding pgprox-session \
  probe::tests::a_count_with_nothing_behind_it_is_refused_rather_than_reserved \
  "a wire count does not reserve before it is read"

# --- every accepted survivor still carries a reason --------------------------
#
# `M15.12` removed two entries whose functions had been rewritten. The rule that
# makes the file worth having is the one `M14` set: an entry is an argument, and
# an entry with no argument is a survivor accepted silently.
BASELINE_FILE="${PGPROX_MUTANTS_BASELINE:-product/mutants-baseline.txt}"
unreasoned=0
entries=0
while IFS= read -r line; do
  [[ -z "$line" || "$line" == \#* ]] && continue
  entries=$(( entries + 1 ))
  reason="${line#*$'\t'}"
  if [[ "$reason" == "$line" || -z "${reason// /}" ]]; then
    fail "a baseline entry has no reason: ${line%%$'\t'*}"
    unreasoned=1
  fi
done < "$BASELINE_FILE"
if (( unreasoned == 0 )); then
  ok "every accepted survivor has a written reason ($entries of them)"
fi

finish
