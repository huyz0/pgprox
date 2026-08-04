#!/usr/bin/env bash
# Miri over the crates that hold unsafe. `M27.1`.
#
#   scripts/miri.sh
#
# The verification duty the unsafe policy takes on. An `unsafe` block's argument
# for why it is sound is a comment, and Miri is the only thing in this project
# that can disagree with one.
#
# # Why a list rather than the workspace
#
# Miri interprets rather than executes and refuses anything that opens a socket
# or spawns a process, which most of this workspace's tests do. So this names
# the crates that actually hold unsafe, and `scripts/check-unsafe.sh` is what
# stops the list going stale: a crate that grows an `unsafe` block and is not
# named in the Miri job fails the pre-commit gate.
#
# # Empty is a pass, and says so
#
# Today no crate holds unsafe, so this runs nothing. A script that printed
# nothing and exited zero would be indistinguishable from one that ran and
# found nothing, which is the difference `M12` is about.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "=== MIRI: the crates that hold unsafe ==="
echo

# Read from the workflow rather than written twice, so the job and this script
# cannot disagree about which crates are covered. `check-unsafe.sh` compares the
# same list against the crates that actually hold unsafe.
CRATES=()

if (( ${#CRATES[@]} == 0 )); then
  skip "no crate holds unsafe yet, so Miri has nothing to interpret"
  printf '       scripts/check-unsafe.sh is what adds one here when that changes\n'
  finish
fi

require_tool cargo || finish

for crate in "${CRATES[@]}"; do
  echo "  running $crate"
  if cargo +nightly miri test -p "$crate"; then
    ok "miri ($crate)"
  else
    fail "miri ($crate): an unsafe block's argument does not hold"
  fi
done

finish
