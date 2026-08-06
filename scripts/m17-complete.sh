#!/usr/bin/env bash
# M17: the binaries mutation testing never reached.
#
#   scripts/m17-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# # What this gate does not do, and where that check lives
#
# It does not run a mutation sweep. `pgprox` alone is 571 mutants and eighty
# minutes, and this gate sits beside fifteen others. `M14`'s and `M15`'s gates
# made the same trade and said so; the sweep is the nightly `mutants` job in CI,
# against `docs/internal/product/mutants-baseline.txt`.
#
# What it can check is the thing that made the sweep miss the binaries for
# three milestones: they were not in the list. `scripts/mutants.sh` decides what
# is measured, and a binary quietly dropped from `CRATES` would take the nightly
# green while measuring nothing, which is exactly the shape `M12` spent a
# milestone on. So that is checked by running the script's own selection rather
# than by grepping it.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M17: the binaries mutation testing never reached"
echo

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

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

# --- both binaries are in the sweep ------------------------------------------
#
# Asked of the script rather than of its source. `--list` runs the same
# selection a real sweep would and prints what it would mutate, so a binary
# removed from `CRATES` fails here whatever the file looks like.
#
# The `--list` half needs `cargo-mutants`, which CI installs for the nightly
# `mutants` job and not for this one. That is deliberate: the milestone job's
# whole argument is that sixteen gates together cost less than the coverage
# job, and a tool install for one listing works against it. So the listing is
# reported as unchecked when the tool is absent rather than failing, and the
# tokenised check below runs either way. `M16`'s gate makes the same trade for
# its blocked half.
have_mutants=0
have cargo-mutants && have_mutants=1

for crate in pgprox pgload; do
  if (( have_mutants )); then
    if cargo mutants -p "$crate" --list --exclude '**/src/bin/**' >"$WORK/$crate.list" 2>/dev/null \
       && [[ -s "$WORK/$crate.list" ]]; then
      ok "$crate has mutants to test ($(wc -l < "$WORK/$crate.list"))"
    else
      fail "$crate produced no mutants: the sweep would measure nothing and say nothing"
    fi
  fi

  # Tokenised rather than matched as a pattern. The list puts several crates on
  # a line, so an anchored regex misses every one that is not first, and a
  # word-boundary match would find `pgprox` inside `pgprox-proto`.
  if sed -n '/^CRATES=(/,/^)/p' scripts/mutants.sh | tr -s ' \n' '\n' | grep -Fxq "$crate"; then
    ok "$crate is in scripts/mutants.sh's list"
  else
    fail "$crate is not in scripts/mutants.sh: the nightly would skip it"
  fi
done

if (( ! have_mutants )); then
  warn "cargo-mutants is not installed, so what each binary would mutate was not listed"
  printf '       the nightly mutants job installs it and runs the sweep itself\n'
fi

# --- every accepted survivor carries a reason --------------------------------
#
# `untriaged` is not a reason. `M10.3` used it for the first sweep and said so,
# and `M10.4` through `M10.8` are the tasks that removed it. It may not come
# back: an entry with no argument is a survivor nobody read.
if grep -qE $'\tuntriaged' docs/internal/product/mutants-baseline.txt; then
  fail "docs/internal/product/mutants-baseline.txt has an untriaged entry: that is not a reason"
else
  ok "every accepted survivor carries an argument rather than 'untriaged'"
fi

# --- the two defects this milestone found, each by its test ------------------
run_test pgprox "sessions::tests::updates_for_a_client_that_has_gone_are_ignored" \
  "a pin is not counted for a client that has gone"
run_test pgprox "run::tests::a_servers_pools_are_its_own_and_its_count_includes_the_waiters" \
  "a server's pools are its own and waiters count toward its demand"

# --- and the three tests that did not test their names -----------------------
run_test pgprox "serve::tests::a_cancel_for_a_held_query_reaches_the_server" \
  "a cancel is asserted to reach the server rather than to finish"
run_test pgprox "dial::tests::a_server_name_tls_cannot_verify_is_refused_before_dialling" \
  "a name TLS cannot verify is refused before the socket opens"
run_test pgprox "dial::tests::a_verified_backend_is_dialled_over_tls" \
  "an upstream TLS connection is proved to succeed and not only to fail"

# --- the timeout constants `M17.7` re-derived --------------------------------
#
# The numbers themselves are in the two files that set them, with the
# measurement beside each. What is checkable here is that the per-test cap is
# still below the whole-suite budget, which is the invariant `M10.13` built and
# `M17.7` re-derived: a hang has to become a failed test before it becomes an
# abandoned run.
cap="$(grep -oE 'period = "([0-9]+)s".*terminate-after = ([0-9]+)' .config/nextest.toml \
  | sed -E 's/period = "([0-9]+)s".*terminate-after = ([0-9]+)/\1 \2/')"
budget="$(grep -oE 'MUTANTS_TIMEOUT:-([0-9]+)' scripts/mutants.sh | grep -oE '[0-9]+')"
if [[ -n "$cap" && -n "$budget" ]]; then
  # shellcheck disable=SC2086
  set -- $cap
  if (( $1 * $2 < budget )); then
    ok "the per-test cap ($(($1 * $2))s) is inside the suite budget (${budget}s)"
  else
    fail "the per-test cap ($(($1 * $2))s) is not inside the suite budget (${budget}s)"
    printf '       a hung test would exhaust the run before it became a failure\n'
  fi
else
  fail "could not read the per-test cap or the suite budget"
fi

finish
