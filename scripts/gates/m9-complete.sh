#!/usr/bin/env bash
# M9 completion condition: the query cache exists, says what it promises, and
# is measured rather than assumed to help.
#
# The roadmap named `cargo nextest run -p pgprox-cache`, which says the crate's
# own tests pass and nothing about whether the cache is correct to use. The
# three things that decide that are elsewhere: the ADR that states the
# staleness contract, the rule that decides what may be cached at all, and a
# recorded run showing whether it helped.
source "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

cd "$REPO_ROOT"

echo "M9: query cache"
echo

# --- the decision -------------------------------------------------------------
#
# First, because everything else assumes an answer to it. A cache whose
# guarantees were never written down is a cache somebody relies on for
# read-your-writes.
if compgen -G 'docs/internal/product/decisions/*query-cache*.md' >/dev/null \
   || grep -rlqs 'query cache' docs/internal/product/decisions/ 2>/dev/null; then
  ok "an ADR states what the cache promises"
else
  fail "no ADR for the cache: its staleness contract is unwritten"
fi

# --- the crate ----------------------------------------------------------------
if [[ -f crates/pgprox-cache/Cargo.toml ]]; then
  ok "pgprox-cache exists"
else
  # No early exit. The checks below all fail without the crate, and a gate
  # that stopped here would report one missing thing per run rather than the
  # shape of what is left.
  fail "crates/pgprox-cache/Cargo.toml missing: the trait still has no implementation"
fi

grep -qs 'pgprox-cache' Cargo.toml \
  && ok "it is a workspace member" \
  || fail "pgprox-cache is not in the workspace, so nothing builds or gates it"

# The trait `pgprox-core` has carried since M0. An implementation that does not
# implement it is a second contract nobody agreed to.
if grep -rqs --include='*.rs' 'impl .*QueryCache for' crates/pgprox-cache/src; then
  ok "it implements QueryCache"
else
  fail "nothing in pgprox-cache implements QueryCache"
fi

# --- what may be cached -------------------------------------------------------
#
# The half that is a correctness bug rather than a miss. A cache that serves a
# statement it should not have cached is wrong in the way a replica behind the
# watermark is wrong.
#
# A definition, not the word. The word appears in this crate's own prose
# explaining that the rule does not exist yet, which passed this check on the
# run that wrote it. That is three times in three milestones, so it is worth
# stating as a rule of its own: a gate greps for something only a definition
# can produce, never for a noun a comment can contain.
if grep -rqsE --include='*.rs' '(fn|enum|struct|trait) +[Cc]acheable' \
   crates/pgprox-cache/src; then
  ok "a rule decides what may be cached"
else
  fail "no cacheability rule: the cache would store whatever it was handed"
fi

# --- bounded by bytes ---------------------------------------------------------
#
# A cache bounded by entry count holds an unbounded amount of memory, which on
# a node aiming at 100k connections is the wrong thing to be unbounded.
if grep -rqs --include='*.rs' 'max_bytes\|bytes_used\|byte_budget' crates/pgprox-cache/src; then
  ok "the bound is on bytes"
else
  fail "the cache is not bounded by bytes: entry count is not a memory bound"
fi

# --- off by default -----------------------------------------------------------
#
# A field, not the word. `grep cache` matches the comment on `grant_ttl_cap`,
# which is about a different cache and passed this check on the run that wrote
# it: the shape of check that is green for years while the thing it names does
# not exist.
if grep -rqs --include='*.rs' 'query_cache' crates/pgprox-config/src; then
  ok "the config document has a query_cache section"
else
  fail "no query_cache setting in pgprox-config: it cannot be turned on or off"
fi

# --- measured -----------------------------------------------------------------
#
# This used to glob `docs/internal/product/perf/run-*cache*.md` and report the match count,
# which is a check that a filename exists reporting a conclusion about whether
# the cache helps. `M12.3`.
#
# What M9 actually claims is a number with a sign, and the roadmap states it in
# the milestone's own row. So take the figure from the roadmap and require a
# recorded run to contain it. That ties the claim to its evidence in the
# direction that matters: re-measure the cache and this fails until the roadmap
# is updated, and edit the roadmap's number and this fails until a run says so.
PERF_DIR="${PGPROX_PERF_DIR:-docs/internal/product/perf}"
ROADMAP="${PGPROX_ROADMAP:-docs/internal/product/roadmap.md}"

claim="$(grep -m1 '^| M9 ' "$ROADMAP" 2>/dev/null | grep -oE '[0-9]+(\.[0-9]+)?%' | head -1 || true)"

if [[ -z "$claim" ]]; then
  fail "the roadmap's M9 row states no figure for the cache, so there is nothing to hold it to"
else
  backing=""
  for run in "$PERF_DIR"/run-*cache*.md; do
    [[ -f "$run" ]] || continue
    if grep -qF -- "$claim" "$run"; then backing="$(basename "$run")"; break; fi
  done
  if [[ -n "$backing" ]]; then
    ok "the roadmap's $claim is backed by a recorded run ($backing)"
  else
    fail "the roadmap says the cache costs $claim and no run in $PERF_DIR records it: the number is an opinion"
  fi
fi

# --- the usual gates ----------------------------------------------------------
./scripts/check-crate.sh pgprox-cache >/dev/null 2>&1 \
  && ok "fmt, clippy, doctests (pgprox-cache)" || fail "workspace checks (pgprox-cache)"
./scripts/check-coverage.sh pgprox-cache >/dev/null 2>&1 \
  && ok "coverage (pgprox-cache)" || fail "coverage (pgprox-cache)"
./scripts/check-layering.sh >/dev/null 2>&1 \
  && ok "crate dependency rule" || fail "crate dependency rule"

finish
