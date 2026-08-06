#!/usr/bin/env bash
# Unsafe is a governed exception, and these are the conditions. `M27.1`.
#
#   scripts/check-unsafe.sh
#
# The workspace lint is `deny` rather than `forbid`, so an `#[allow(unsafe_code)]`
# is possible. That is the point and it is also the risk: a lint anybody can
# switch off in one line is a lint that switches itself off. This script is what
# an exception has to get past.
#
# # The five conditions
#
#   1. The crates whose argument is about untrusted bytes keep
#      `#![forbid(unsafe_code)]` in their own `lib.rs`, where no `#[allow]`
#      can reach them.
#   2. Every `#[allow(unsafe_code)]` carries a `SAFETY-POLICY:` line naming the
#      benchmark that justifies it.
#   3. That benchmark exists in `docs/internal/product/perf/baseline.json`. Unsafe with no
#      number is a liability with no evidence of upside.
#   4. A crate holding `unsafe` is named in the Miri job, and the job exists.
#   5. Nothing outside a crate's own source takes the exception: not a test,
#      not a bench, not a build script.
#
# # Why a script rather than a paragraph
#
# `M13` audited seven non-negotiables and found four with no script or the
# wrong one credited. A standard that says "unsafe needs a measurement" and has
# nothing to read the measurement is the same shape, and this milestone would
# have been the fifth instance.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "=== UNSAFE: the conditions on the exception ==="
echo

# Overridable so the checks below can be run against a planted violation, which
# is how `tests/gates/negative.sh` proves they can fail. A check nobody has seen
# fail is a check nobody knows the failure mode of.
CRATES_DIR="${PGPROX_CRATES_DIR:-crates}"
BINS_DIR="${PGPROX_BINS_DIR:-bin}"
BASELINE="${PGPROX_BASELINE:-docs/internal/product/perf/baseline.json}"
WORKFLOW="${PGPROX_WORKFLOW:-.github/workflows/ci.yml}"

# --- 1. the crates that stay shut --------------------------------------------
#
# Named here rather than derived, because the list is a judgement about which
# code an unauthenticated peer's bytes reach and no rule can infer it. Each is
# a crate `docs/internal/standards/security.md` is about when it says the failure mode of a
# decoder bug must be a wrong answer and never memory corruption.
CLOSED=(
  pgprox-proto  # the wire codec: the primary attack surface in the process
  pgprox-core   # sql::Lexer decides which untrusted text is SQL; SecretString
  pgprox-route  # classifies untrusted SQL
  pgprox-auth   # a JWT header and a SCRAM exchange, both peer-chosen bytes
  pgprox-tls    # the path a client's first bytes take
)

shut=0
for crate in "${CLOSED[@]}"; do
  lib="$CRATES_DIR/$crate/src/lib.rs"
  if [[ ! -f "$lib" ]]; then
    fail "$crate has no lib.rs, so the closed list names a crate that is not there"
    continue
  fi
  if grep -q '^#!\[forbid(unsafe_code)\]' "$lib"; then
    shut=$((shut + 1))
  else
    fail "$crate is on the closed list and does not forbid unsafe in its own lib.rs"
    printf '       the workspace lint is `deny`, so without this an `#[allow]` reaches it\n'
  fi
done
(( shut == ${#CLOSED[@]} )) && ok "the $shut crates on the closed list forbid unsafe themselves"

# --- 2 and 3. every exception names a benchmark, and the benchmark exists -----
#
# `grep -n` over source, then the line above each hit. The comment goes above
# the attribute because that is where a reader looks, and requiring it there
# rather than anywhere in the file is what stops one justification covering a
# second exception nobody argued for.
allows="$(grep -rn --include='*.rs' '#\[allow(unsafe_code)\]' "$CRATES_DIR" "$BINS_DIR" 2>/dev/null || true)"

if [[ -z "$allows" ]]; then
  ok "no exception is taken anywhere, so none can be unjustified"
else
  while IFS= read -r hit; do
    [[ -n "$hit" ]] || continue
    file="${hit%%:*}"
    rest="${hit#*:}"
    line="${rest%%:*}"

    # An exception on the first line has no line above it, and asking `sed` for
    # line 0 is an error rather than an empty answer. Under `set -e` that killed
    # the run instead of failing the check, which is `M12`'s subject appearing
    # in a gate written to enforce a rule about care.
    if (( line <= 1 )); then
      above=""
    else
      above="$(sed -n "$((line - 1))p" "$file")"
    fi

    if [[ "$above" != *"SAFETY-POLICY:"* ]]; then
      fail "$file:$line takes the exception with no SAFETY-POLICY line above it"
      printf '       the line above an `#[allow(unsafe_code)]` names the benchmark it is for\n'
      continue
    fi

    # The benchmark it claims, which has to be one the baseline actually holds.
    bench="$(sed "s/.*SAFETY-POLICY:[[:space:]]*//" <<<"$above" | awk '{print $1}')"
    if [[ -z "$bench" ]]; then
      fail "$file:$line names no benchmark after SAFETY-POLICY:"
    elif grep -q "\"$bench\"" "$BASELINE"; then
      ok "$file:$line is justified by $bench"
    else
      fail "$file:$line names $bench, which is not in $BASELINE"
      printf '       unsafe with no number is a liability with no evidence of upside\n'
    fi
  done <<<"$allows"
fi

# --- 4. a crate holding unsafe is under Miri ---------------------------------
#
# The verification duty. Unsafe without Miri in CI is unsafe nobody can
# maintain, and a job that exists but does not name the crate is the same as no
# job for that crate.
#
# `unsafe` inside a string or a comment is not unsafe, so this looks for the
# block and function forms at the start of a token rather than the word
# anywhere. The cost of being slightly over-eager here is a crate named in the
# Miri job that did not need to be, which is not a cost.
holders="$(grep -rlE '(^|[^[:alnum:]_])unsafe[[:space:]]*(\{|fn |impl |trait )' \
  --include='*.rs' "$CRATES_DIR" "$BINS_DIR" 2>/dev/null \
  | sed -E "s#^($CRATES_DIR|$BINS_DIR)/([^/]+)/.*#\2#" | sort -u || true)"

if [[ -z "$holders" ]]; then
  ok "no crate holds unsafe, so none needs Miri yet"
elif ! grep -q 'name: tier 3 - miri' "$WORKFLOW"; then
  fail "these crates hold unsafe and $WORKFLOW has no Miri job:"
  printf '       %s\n' $holders
else
  missing=""
  for crate in $holders; do
    grep -q -- "-p $crate" "$WORKFLOW" || missing+=" $crate"
  done
  if [[ -z "$missing" ]]; then
    ok "every crate holding unsafe is named in the Miri job"
  else
    fail "these crates hold unsafe and the Miri job does not name them:$missing"
  fi
fi

# --- 5. the exception is not taken from outside the source --------------------
#
# A test or a bench that allows unsafe is unsafe nothing else in this file
# checks: it is not on the closed list's paths, it is not what the benchmark
# justifies, and Miri may never reach it. Refused outright rather than governed.
outside="$(grep -rln '#\[allow(unsafe_code)\]' \
  --include='*.rs' "$CRATES_DIR" "$BINS_DIR" 2>/dev/null \
  | grep -E '/(tests|benches|examples)/|build\.rs$' || true)"

if [[ -z "$outside" ]]; then
  ok "no test, bench or build script takes the exception"
else
  fail "the exception is taken outside crate source, where nothing here governs it:"
  printf '       %s\n' $outside
fi

finish
