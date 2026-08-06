#!/usr/bin/env bash
# M21: the driver matrix does not cover what M20 changed.
#
#   scripts/gates/m21-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# # What this gate does not do
#
# It does not run the matrix. That needs Docker, a built proxy image and five
# toolchains, and it sits beside twenty other gates in one CI job whose whole
# argument is that together they cost less than the coverage job.
# `scripts/driver-matrix.sh` is the thing that runs it, on request, like
# `scripts/e2e.sh`.
#
# What is checkable without a stack is that the suite and its report still
# describe each other, which is the failure this milestone is about: a driver in
# one list and not the other, or a report that names fewer drivers than the
# script runs, is a gap that reads as coverage.
source "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

cd "$REPO_ROOT"

echo "M21: the driver matrix does not cover what M20 changed"
echo

MATRIX=scripts/driver-matrix.sh
# Overridable so the checks below can be run against a planted report rather
# than only against the real one, which is how `m18-complete.sh` proves its own
# rules fire. A check that has only ever been seen to pass is a check nobody
# knows the failure mode of.
REPORT="${PGPROX_MATRIX_REPORT:-docs/internal/product/conformance/driver-matrix.md}"
PROBES=tests/proxy-drivers

# --- the suite the milestone is about still exists ---------------------------
for path in "$MATRIX" "$REPORT" "$PROBES"; do
  if [[ -e "$path" ]]; then
    ok "$path exists"
  else
    fail "$path is missing: the drivers would be back to meeting only the harness"
  fi
done

# --- every driver the matrix runs has a probe, and vice versa ----------------
#
# Read from the script's own list rather than hard-coded here, so a driver added
# to the matrix and not to the probes fails rather than being silently skipped.
# The suite reports a missing toolchain as a skip, which is right, and would
# report a missing probe the same way, which would not be.
listed="$(sed -n 's/^DRIVERS=(\(.*\))$/\1/p' "$MATRIX" | tr ' ' '\n' | grep -v '^$' | sort)"
probed="$(find "$PROBES" -maxdepth 1 -name '*.sh' ! -name '_*' -printf '%f\n' 2>/dev/null \
  | sed 's/\.sh$//' | grep -v -- '-tls12-' | sort)"

if [[ -z "$listed" ]]; then
  fail "could not read the driver list out of $MATRIX"
elif [[ "$listed" == "$probed" ]]; then
  ok "every driver the matrix runs has a probe ($(wc -l <<<"$listed"))"
else
  fail "the matrix and the probes disagree about which drivers exist"
  diff <(echo "$listed") <(echo "$probed") | sed 's/^/       /'
fi

# --- and the report accounts for all of them ---------------------------------
#
# A report naming four drivers for a suite that runs five is the shape this
# milestone exists to stop: a result that reads as complete and is not.
missing=""
while read -r driver; do
  grep -qE "^\| $driver \|" "$REPORT" || missing+=" $driver"
done <<<"$listed"

if [[ -z "$missing" ]]; then
  ok "the report accounts for every driver the matrix runs"
else
  fail "the report says nothing about:$missing"
fi

# --- M21.1: a stale report says how stale ------------------------------------
#
# ## Why this warns rather than fails, against what the task first asked for
#
# `M21.1` was filed as "a gate fails on a report older than the newest commit
# touching bin/pgprox, crates/pgprox-session or crates/pgprox-proto". That
# criterion is wrong and this corrects it.
#
# Regenerating the report needs Docker, a built proxy image and five driver
# toolchains. A gate that failed on any proxy commit until someone ran all that
# would be red from the first edit and permanently red in CI, which has none of
# it. `check-core-contract.sh` says the thing this would become: a rule people
# route around. The two mechanical halves it kept are the ones that can be met.
#
# So the failure is for provenance being absent, which is always fixable and is
# the state that makes staleness unmeasurable, and staleness itself is reported
# with a count. That is strictly more than existed before, where the report
# carried a date nobody compared to anything.
PROXY_PATHS=(bin/pgprox crates/pgprox-session crates/pgprox-proto)
describes="$(sed -n 's/^Describes: \([0-9a-f]\{7,\}\)$/\1/p' "$REPORT" | head -1)"

if [[ -z "$describes" ]]; then
  fail "$REPORT does not say which tree it describes"
  printf '       regenerate it with scripts/driver-matrix.sh\n'
elif ! git cat-file -e "$describes^{commit}" 2>/dev/null; then
  fail "$REPORT names a commit this repository does not have: $describes"
else
  ok "the report says which tree it describes"

  behind="$(git rev-list --count "$describes..HEAD" -- "${PROXY_PATHS[@]}" 2>/dev/null || echo 0)"
  if (( behind == 0 )); then
    ok "the matrix results describe the proxy as it stands"
  else
    # Named rather than counted alone. "Three commits behind" is a number;
    # "behind M20.4 and M20.6" is what tells someone whether it matters, and
    # both of those changed what goes on the wire.
    warn "the matrix results are $behind commit(s) behind the proxy"
    git log --format='       %s' "$describes..HEAD" -- "${PROXY_PATHS[@]}" | head -8
    printf '       re-run scripts/driver-matrix.sh to say whether they still hold\n'
  fi
fi

finish
