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
if compgen -G 'product/perf/run-*.md' >/dev/null; then
  ok "a scale run is recorded ($(compgen -G 'product/perf/run-*.md' | wc -l) file(s))"
else
  fail "no run recorded in product/perf/: the numbers exist only in a terminal"
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
  [pgprox-cluster]="gossip digest encode and decode"
)
for crate in "${!BUDGETS[@]}"; do
  if grep -rqs --include='*.rs' 'dhat' "crates/$crate/src" "crates/$crate/tests" 2>/dev/null; then
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
