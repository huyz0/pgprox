#!/usr/bin/env bash
# M16: the streaming relay nothing streams through.
#
#   scripts/m16-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code. Every check runs a named test and
# reads its exit status, so a test that is renamed, deleted or made to pass
# vacuously fails this gate rather than quietly stopping being evidence.
#
# # This milestone is not fully complete and this gate says so
#
# `M16`'s completion condition is "a measurement first, then the two directions,
# then the same 100k run with a result set large enough that the difference
# would show". The first two are done. The third needs the three machines and
# the real network `M7`'s full run needed, and it is recorded in
# `docs/internal/product/backlog.md` as blocked rather than filed.
#
# So this gate checks what was built and **reports the blocked half rather than
# asserting it passes**. A gate that quietly dropped the part nobody can run
# here would turn a milestone that is two thirds done into one that reports
# green, which is the failure `M10.17` and `M12` are both about. `M13`'s gate
# does the same for the one non-negotiable that has no script.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M16: the streaming relay nothing streams through"
echo

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# One test by exact name, and its exit status is the check. No pipeline:
# `set -o pipefail` is on and `grep -q` exits at its first match, which closes
# the pipe and kills `cargo test` with SIGPIPE, so a passing test reports 141.
# `M15`'s gate hit that for six of thirteen checks and not the other seven,
# because whether the write lands before grep exits depends on how much the run
# printed.
run_test() {
  local crate="$1" name="$2" claim="$3"
  local out="$WORK/$crate-$RANDOM.out"

  cargo test -p "$crate" --all-targets -- --exact "$name" >"$out" 2>>"$WORK/log"
  if grep -q "^test $name \.\.\. ok$" "$out"; then
    ok "$claim"
  else
    fail "$claim: $crate $name did not run and pass"
    printf '       a claim this milestone made has no test standing behind it\n'
  fi
}

# --- the measurement, which is the milestone's headline number ---------------
#
# `M16.1` measured what the data path holds and `M16.5` re-measured it. The test
# asserts the ratio rather than the two numbers, for the reason its own header
# gives: a number that only prints is a number nobody notices changing.
run_test pgprox-session \
  "one_large_row_through_the_buffering_path_and_the_streaming_one" \
  "a 16 MiB row streams in orders of magnitude less than it buffers"

# --- direction one: server to client -----------------------------------------
run_test pgprox "serve::tests::a_body_streams_across_without_being_held" \
  "a server body crosses without being held"
run_test pgprox "serve::tests::a_streamed_body_reaches_the_peer_while_it_is_still_streaming" \
  "the peer sees bytes before the body has finished arriving"

# --- direction two: client to server -----------------------------------------
#
# `M16.4` did the COPY loop and `M16.6` did the prefix-inspected messages. The
# `Bind` tests are the second: a large parameter must leave its tail on the wire
# and the forwarded header must still describe the whole message.
run_test pgprox "serve::tests::a_bind_whose_names_fit_keeps_its_tail_on_the_wire" \
  "a large Bind reads its prefix and leaves the rest"
run_test pgprox "serve::tests::a_rewritten_prefix_and_its_tail_announce_one_length" \
  "a rewritten prefix and its tail announce one length"
run_test pgprox "serve::tests::a_name_past_the_prefix_is_topped_up_rather_than_refused" \
  "a name running past the prefix is read rather than refused"

# --- and what the streaming bought, per connection ---------------------------
run_test pgprox "serve::tests::one_session_costs_less_than_the_slab_buffer_it_no_longer_holds" \
  "a session costs less than the buffer it no longer holds"

# --- the half that cannot run here -------------------------------------------
#
# Reported, not asserted, and not silently skipped either. `warn` counts toward
# nothing, which is the point: this is a statement about what this machine can
# check, and the reader is the one who decides what it means.
echo
warn "the 100k run with a large result set is not checked here"
printf '       it needs the load generators on their own machines, a database\n'
printf '       that can absorb the offered load, and a real network between the\n'
printf '       three. Every latency number in this repo is loopback and is\n'
printf '       therefore a floor; docs/internal/product/backlog.md records it as blocked\n'
printf '       rather than filed, because a task nobody can start is not a plan.\n'

finish
