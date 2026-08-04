#!/usr/bin/env bash
# Instruction counts for the declared hot paths, against a committed baseline.
#
#   scripts/bench.sh            run every bench and compare
#   scripts/bench.sh --update   run every bench and rewrite the baseline
#
# Counts rather than wall clock. `callgrind` returns the same number on a busy
# machine as on an idle one, so a few per cent is a real change here where a
# timing would report it as noise. That is why this exists alongside the
# allocation budgets: a budget catches a new copy, a count catches work that
# got more expensive without allocating.
#
# # How one iteration is isolated
#
# Each bench binary takes a name and an iteration count. This runs it at N and
# at 2N and divides the difference by N, so process startup, the loader, the
# fixtures and the loop itself all cancel exactly. That is also why there is no
# benchmark harness crate here: the two obvious ones pull dependencies this
# project's supply-chain gate refuses, and subtraction is a smaller thing to
# own than an exception to that gate.
#
# Updating the baseline is a deliberate act with a commit message attached. A
# script that rewrote it on every run would turn a regression into a new normal.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

BASELINE="product/perf/baseline.json"
CRATES=(pgprox-proto pgprox-route pgprox-pool pgprox-cache pgprox-session)

# Enough iterations that the per-iteration number is stable to the unit, few
# enough that callgrind finishes in seconds.
ITERATIONS="${BENCH_ITERATIONS:-2000}"

# How far a count may drift before it is called a change. Callgrind is
# deterministic for a given binary, but a compiler release moves everything a
# little, so zero tolerance would fail on every toolchain bump.
# A constant for the reason in lib.sh beside COVERAGE_MIN: this decides whether
# a benchmark run passes, and a threshold that can be raised from the
# environment is not a threshold. `M13.1`.
TOLERANCE_PERCENT=5

UPDATE=""
[[ "${1:-}" == "--update" ]] && UPDATE=1

require_tool valgrind "apt install valgrind" || finish
require_tool cargo || finish

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
MEASURED="$WORK/measured"

# Total instructions for one run of a bench binary.
instructions() {
  local binary="$1" name="$2" iterations="$3"
  valgrind --tool=callgrind --callgrind-out-file="$WORK/callgrind.out" \
    "$binary" "$name" "$iterations" 2>&1 \
    | awk '/refs:/ { gsub(/,/, "", $NF); print $NF; exit }'
}

# Where cargo put a crate's bench binary. Named rather than globbed for a
# hash, so a stale binary from an earlier build cannot be measured instead.
bench_binary() {
  local crate="$1"
  cargo build --release --benches -p "$crate" >/dev/null 2>&1 || return 1
  find target/release/deps -maxdepth 1 -name 'hot_paths-*' -type f -newermt '-1 day' \
    -printf '%T@ %p\n' 2>/dev/null | sort -rn | while read -r _ path; do
      # The right binary is the one that knows the crate's bench names, which
      # is cheaper to ask than to derive from cargo's metadata.
      if "$path" 2>/dev/null | grep -q .; then
        case "$crate" in
          pgprox-proto) "$path" | grep -q '^scan_frame$' && { echo "$path"; return; } ;;
          pgprox-route) "$path" | grep -q '^route_point_select$' && { echo "$path"; return; } ;;
          pgprox-pool) "$path" | grep -q '^acquire_and_release$' && { echo "$path"; return; } ;;
          pgprox-cache) "$path" | grep -q '^cache_hit$' && { echo "$path"; return; } ;;
          pgprox-session) "$path" | grep -q '^held_read$' && { echo "$path"; return; } ;;
        esac
      fi
    done
}

echo "=== BENCH: instruction counts ==="

for crate in "${CRATES[@]}"; do
  binary="$(bench_binary "$crate")"
  if [[ -z "$binary" ]]; then
    fail "no bench binary for $crate"
    continue
  fi

  echo "  running $crate"
  while read -r name; do
    [[ -n "$name" ]] || continue
    one="$(instructions "$binary" "$name" "$ITERATIONS")"
    two="$(instructions "$binary" "$name" "$(( ITERATIONS * 2 ))")"
    if [[ -z "$one" || -z "$two" ]]; then
      fail "$crate::$name produced no count"
      continue
    fi
    printf '%s %s\n' "$crate::$name" "$(( (two - one) / ITERATIONS ))" >> "$MEASURED"
  done < <("$binary")
done

if [[ ! -s "$MEASURED" ]]; then
  fail "no benchmark produced a count"
  finish
fi

if [[ -n "$UPDATE" ]]; then
  {
    echo "{"
    echo '  "_comment": "Instructions per iteration for the declared hot paths, measured by scripts/bench.sh as the difference between 2N and N iterations under callgrind. Rewriting this is a deliberate act with a reason in the commit message.",'
    awk '{ printf "  \"%s\": %s,\n", $1, $2 }' "$MEASURED" | sed '$ s/,$//'
    echo "}"
  } > "$BASELINE"
  ok "baseline rewritten: $(wc -l < "$MEASURED") benchmark(s)"
  finish
fi

if [[ ! -f "$BASELINE" ]]; then
  fail "no baseline at $BASELINE (run scripts/bench.sh --update)"
  finish
fi

# A benchmark missing from the baseline is reported rather than skipped: a new
# hot path with no recorded count is a hot path nobody is watching.
while read -r name count; do
  # Matched on the whole quoted key, because a benchmark name contains colons
  # and splitting on them turns every lookup into a miss.
  expected="$(sed -n "s/^ *\"$name\": \([0-9]*\).*/\1/p" "$BASELINE")"
  if [[ -z "$expected" ]]; then
    warn "$name: $count instructions, not in the baseline (run --update to record it)"
    continue
  fi

  percent=$(( (count - expected) * 100 / expected ))
  if (( percent > TOLERANCE_PERCENT )); then
    fail "$name: $count instructions, was $expected (+${percent}%)"
  elif (( percent < -TOLERANCE_PERCENT )); then
    ok "$name: $count instructions, was $expected (${percent}%, an improvement worth recording)"
  else
    ok "$name: $count instructions (${percent}% against baseline)"
  fi
done < "$MEASURED"

finish
