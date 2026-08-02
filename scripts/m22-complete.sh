#!/usr/bin/env bash
# M22: the mutants nobody has swept since M17.
#
#   scripts/m22-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# # What this gate does not do
#
# It does not sweep. `pgprox` alone was 571 mutants and eighty minutes at
# `M17.4`, and this sits beside twenty-one other gates in one CI job whose whole
# argument is that together they cost less than the coverage job. The sweep is
# the nightly `mutants` job, running `scripts/mutants.sh`.
#
# What it can do is say which tree the baseline describes, per crate, which is
# the thing that was missing: four gates read this file's contents and none
# asked whether the contents were current, so it went four milestones out of
# date with every one of them green.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M22: the mutants nobody has swept since M17"
echo

# Overridable so the checks can be run against a planted baseline, which is how
# `m18-complete.sh` proves its own rules fire. A check that has only ever been
# seen to pass is a check nobody knows the failure mode of.
BASELINE="${PGPROX_MUTANTS_BASELINE:-product/mutants-baseline.txt}"
SWEEP="${PGPROX_MUTANTS_SCRIPT:-scripts/mutants.sh}"

if [[ ! -f "$BASELINE" ]]; then
  fail "$BASELINE is missing"
  finish
fi

# Where a crate's code lives, since the sweep names packages and git wants
# paths. The two binaries are packages too and are held to the same gate.
crate_path() {
  case "$1" in
    pgprox)  echo "bin/pgprox" ;;
    pgload)  echo "bin/pgload" ;;
    *)       echo "crates/$1" ;;
  esac
}

# --- every accepted survivor still carries a reason --------------------------
#
# `untriaged` is what `M10.3` used for the first sweep, before anyone had read
# the output, and `M10.4` through `M10.8` are the tasks that removed it. A
# re-baseline is exactly the moment it would come back.
if grep -qE $'\tuntriaged' "$BASELINE"; then
  fail "$BASELINE has an untriaged entry: that is not a reason"
else
  ok "every accepted survivor carries an argument rather than 'untriaged'"
fi

# --- and every crate says which tree it was swept against --------------------
# `|| true` because `grep -v` on an empty stream exits 1, and `lib.sh` sets
# `-e`, so an unreadable list killed this script with no verdict rather than
# reporting one. That is the shape `M19`'s gate had and the reason it exited 101
# with four checks printed and nothing after them.
listed="$(sed -n '/^CRATES=(/,/^)/p' "$SWEEP" \
  | sed 's/CRATES=(//; s/^)$//' | tr -s ' \n' '\n' | grep -v '^$' | sort -u || true)"

if [[ -z "$listed" ]]; then
  fail "could not read the crate list out of $SWEEP"
  finish
fi

unswept="" stale=0
while read -r crate; do
  commit="$(sed -n "s/^# Sweeps: $crate \([0-9a-f]\{7,\}\)\$/\1/p" "$BASELINE" | head -1)"
  if [[ -z "$commit" ]]; then
    unswept+=" $crate"
    continue
  fi
  if ! git cat-file -e "$commit^{commit}" 2>/dev/null; then
    fail "$BASELINE says $crate was swept against a commit this repository does not have"
    continue
  fi
  behind="$(git rev-list --count "$commit..HEAD" -- "$(crate_path "$crate")" 2>/dev/null || echo 0)"
  if (( behind > 0 )); then
    warn "$crate is $behind commit(s) past its last sweep"
    stale=$((stale + 1))
  fi
done <<<"$listed"

if [[ -n "$unswept" ]]; then
  # Reported, not failed, while `M22` is open: the crates are being swept one
  # commit at a time and a gate that failed until the last one landed could not
  # run while the milestone it belongs to was in progress, which is the defect
  # `M18.3` fixed.
  warn "no sweep recorded for:$unswept"
else
  ok "every crate in $SWEEP records which tree it was swept against"
fi

if (( stale == 0 )) && [[ -z "$unswept" ]]; then
  ok "every crate's baseline describes the code as it stands"
fi

finish
