#!/usr/bin/env bash
# M7 completion condition: the measurement apparatus exists, the budgets are
# assertions rather than claims, and a scale run has been recorded.
#
# The roadmap names scripts/scale.sh as the milestone's condition. That script
# needs Docker and a few minutes. This one is the part that runs without
# either, plus the check that scale.sh exists and reports what M7 is judged on.
# Run both.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M7: scale and performance"
echo

# --- the reference workload ---------------------------------------------------
#
# Everything else measures against this one file. Without it, two profiles a
# week apart are anecdotes rather than a comparison.
if [[ -f product/perf/workload.yaml ]]; then
  ok "the reference workload is committed"
else
  fail "product/perf/workload.yaml missing: nothing to measure against"
fi

[[ -f crates/pgprox-load/Cargo.toml ]] \
  && ok "pgprox-load exists" || fail "pgprox-load missing: no workload parser or sampler"
[[ -f bin/pgload/Cargo.toml ]] \
  && ok "bin/pgload exists" || fail "bin/pgload missing: nothing can generate the load"

# --- the scale run ------------------------------------------------------------
#
# Named properties rather than a word count, so an unrelated edit cannot
# satisfy this the way "grep scale" would.
if [[ -f scripts/scale.sh ]]; then
  ok "scripts/scale.sh exists"
  for property in rss p99 upstream; do
    grep -qs "$property" scripts/scale.sh \
      && ok "scale.sh reports: $property" || fail "scale.sh does not report: $property"
  done
  grep -qsE '\$\{?1' scripts/scale.sh \
    && ok "scale.sh takes the connection count" \
    || fail "scale.sh has the connection count baked in, so no two runs compare"
else
  fail "scripts/scale.sh missing"
fi

# A recorded run, and not an empty directory. The roadmap's 100k target is not
# this check: a recorded 1000-connection run is what M7 asks for, and the file
# is what makes the next count comparable to it.
#
# This used to glob `product/perf/run-*.md` and report the match count. On this
# tree it said "a scale run is recorded (16 file(s))", of which five were scale
# runs and eleven were cache, admission, throughput, saturation and pinning
# documents that the pattern cannot tell apart. It would have passed with none
# of the five present, which makes it a check that the directory is non-empty
# wearing the words of a check about scale. `M12.2`.
#
# So read the documents. A scale run says so in its title and records the
# connection count it ran at, which is also the number the comment above has
# always named as the requirement.
PERF_DIR="${PGPROX_PERF_DIR:-product/perf}"
# A constant, and it was not one when M12.2 wrote it. That task added a
# settable pass/fail threshold during a milestone about checks that do not
# check, which is the finding M13.0 kept rather than tidying away. `M13.1`.
SCALE_MINIMUM=1000
scale_runs=0
largest=0
for run in "$PERF_DIR"/run-*.md; do
  [[ -f "$run" ]] || continue
  head -1 "$run" | grep -qiE '^#[[:space:]]*scale run' || continue
  scale_runs=$(( scale_runs + 1 ))
  # The count lives in the run's own summary table, as `| Connections | 1000 |`
  # or, for the run that aimed at the headline number, `| Target | 100,000 ... |`.
  # Digits only, so "100,000 client connections" reads as 100000.
  count="$(awk -F'|' '
    /^[[:space:]]*\|[[:space:]]*(Connections|Target)[[:space:]]*\|/ {
      v = $3; gsub(/[^0-9]/, "", v)
      if (v + 0 > m) m = v + 0
    }
    END { print m + 0 }' "$run")"
  (( count > largest )) && largest="$count"
done

if (( scale_runs == 0 )); then
  fail "no scale run recorded in $PERF_DIR: the numbers exist only in a terminal"
elif (( largest < SCALE_MINIMUM )); then
  fail "$scale_runs scale run(s) recorded, the largest at $largest connections; M7 asks for $SCALE_MINIMUM"
else
  ok "a scale run is recorded: $scale_runs run(s), the largest at $largest connections"
fi

# --- allocation budgets -------------------------------------------------------
#
# One per declared hot path, in the crate that owns it. A budget in the wrong
# crate is a budget nobody runs when that crate changes.
declare -A BUDGETS=(
  [pgprox-proto]="frame scanning and the relay step"
  [pgprox-pool]="warm acquire and the release decision"
  [pgprox-route]="the route decision"
  [pgprox-auth]="the grant cache lookup"
)
# The gossip digest is encoded in the binary rather than in pgprox-cluster:
# the cluster layer owns the digest as a value, this owns how it travels.
BUDGETS[pgprox]="gossip digest encode and decode"
for crate in "${!BUDGETS[@]}"; do
  # `dhat::Profiler` rather than the word `dhat`: a comment mentioning the
  # crate would otherwise satisfy this, which is the shape of check that
  # passes for years while the thing it names does not exist.
  if grep -rqs --include='*.rs' 'dhat::Profiler' \
       "crates/$crate/src" "crates/$crate/tests" \
       "bin/$crate/src" "bin/$crate/tests" 2>/dev/null; then
    ok "allocation budget: $crate (${BUDGETS[$crate]})"
  else
    fail "no allocation budget in $crate: ${BUDGETS[$crate]} is still a claim"
  fi
done

# --- instruction counts -------------------------------------------------------
if [[ -f scripts/bench.sh ]]; then
  ok "scripts/bench.sh exists"
  compgen -G 'crates/*/benches/*.rs' >/dev/null \
    && ok "benchmarks exist" || fail "scripts/bench.sh has nothing to run"
  [[ -f product/perf/baseline.json ]] \
    && ok "an instruction-count baseline is committed" \
    || fail "no baseline: a regression has nothing to be a regression against"
else
  fail "scripts/bench.sh missing"
fi

# --- the semantic coverage report ---------------------------------------------
[[ -f scripts/profile.sh ]] \
  && ok "scripts/profile.sh exists" || fail "scripts/profile.sh missing"
if [[ -f product/perf/semantic-coverage.md ]]; then
  ok "the semantic coverage report is committed"
else
  fail "no semantic coverage report: hot and under-tested is still an opinion"
fi

# --- buffer reclaim -----------------------------------------------------------
#
# The slab has had tests, a bound and no caller since M0. This is the check
# that says whether M7 changed that.
if grep -rqs --include='*.rs' --exclude-dir=target 'BufferSlab' \
   crates/pgprox-session bin/pgprox 2>/dev/null; then
  ok "the buffer slab has a caller"
else
  fail "BufferSlab is still unused: every connection holds its buffers while idle"
fi

# --- the usual gates ----------------------------------------------------------
for c in pgprox-load pgload; do
  [[ -f "crates/$c/Cargo.toml" || -f "bin/$c/Cargo.toml" ]] || continue
  ./scripts/check-crate.sh "$c" >/dev/null 2>&1 \
    && ok "fmt, clippy, doctests ($c)" || fail "workspace checks ($c)"
  ./scripts/check-coverage.sh "$c" >/dev/null 2>&1 \
    && ok "coverage ($c)" || fail "coverage ($c)"
done

./scripts/check-layering.sh >/dev/null 2>&1 \
  && ok "crate dependency rule" || fail "crate dependency rule"

finish
