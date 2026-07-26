#!/usr/bin/env bash
# The semantic coverage report: which code the reference workload actually runs.
#
#   scripts/profile.sh [seconds]      default 30
#
# Line coverage says a line was executed. It says nothing about whether that
# line runs a billion times a day or twice a year, and a proxy lives on the
# difference. This replays the reference workload against an instrumented
# binary, keeps LLVM's execution *counts* rather than its hit/miss booleans,
# and sorts the result into the three lists `standards/testing.md` describes:
#
#   * hot and under-tested   high count, low coverage. Write tests here first.
#   * hot and expensive      the optimization queue, ordered by contribution.
#   * cold and complex       big and never run. Candidates for deletion.
#
# It runs against the local stack, so it needs a Postgres on the machine rather
# than Docker. It is a nightly-and-before-a-milestone job, not a pre-commit
# one: it takes minutes and it rebuilds the world instrumented.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
source "$(dirname "${BASH_SOURCE[0]}")/localstack.sh"

cd "$REPO_ROOT"

SECONDS_TO_RUN="${1:-30}"
CONNECTIONS="${PROFILE_CONNECTIONS:-200}"
REPORT="product/perf/semantic-coverage.md"
WORK="$REPO_ROOT/target/profile"

# Its own directory, because an instrumented build invalidates the ordinary
# one and a profile run should not cost the next `cargo test` a rebuild.
export CARGO_TARGET_DIR="$WORK/target"
export CARGO_LLVM_COV_TARGET_DIR="$CARGO_TARGET_DIR"

require_tool cargo || finish
require_tool python3 || finish
have cargo-llvm-cov || { fail "cargo-llvm-cov missing (cargo install cargo-llvm-cov)"; finish; }

mkdir -p "$WORK"
trap 'local_down' EXIT

echo "=== PROFILE: the reference workload against an instrumented proxy ==="

cargo llvm-cov clean --workspace >/dev/null 2>&1

# The load client is not the thing being profiled, so it is built normally and
# in the ordinary target directory.
CARGO_TARGET_DIR="$REPO_ROOT/target" cargo build --release -p pgload >/dev/null 2>&1 \
  || { fail "pgload did not build"; finish; }

# The documented way to instrument something cargo does not run for you: take
# the environment llvm-cov would have set, build with it, and run the binary
# directly. Running it through `cargo run` would make cargo the process this
# script signals, and the proxy would keep going with its counters unwritten.
eval "$(cargo llvm-cov show-env --export-prefix)"
echo "  building an instrumented proxy"
cargo build --release --bin pgprox >/dev/null 2>&1 \
  || { fail "the instrumented proxy did not build"; finish; }
export LOCAL_PROXY_BIN="$CARGO_TARGET_DIR/release/pgprox"

if ! local_up; then
  fail "the local stack did not come up"
  finish
fi

echo "  replaying the workload: $CONNECTIONS connections for ${SECONDS_TO_RUN}s"
"$REPO_ROOT/target/release/pgload" \
  --target "$LOCAL_PROXY" \
  --workload product/perf/workload.yaml \
  --connections "$CONNECTIONS" \
  --duration "$SECONDS_TO_RUN" \
  --user acme_app --database tenant_acme \
  --password "$LOCAL_TOKEN" \
  --out "$WORK/load.json" >"$WORK/load.log" 2>&1 \
  || { fail "the replay failed"; tail -5 "$WORK/load.log" | sed 's/^/  /'; finish; }

# A graceful stop, so the instrumented process writes its counters out. Killing
# it would leave a profile of nothing.
if [[ -f "$LOCAL_DIR/proxy.pid" ]]; then
  kill -TERM "$(cat "$LOCAL_DIR/proxy.pid")" 2>/dev/null
  for _ in $(seq 1 60); do
    kill -0 "$(cat "$LOCAL_DIR/proxy.pid")" 2>/dev/null || break
    sleep 0.5
  done
fi

echo "  collecting counts"
cargo llvm-cov report --release --json --output-path "$WORK/coverage.json" >/dev/null 2>&1 \
  || { fail "llvm-cov produced no report"; finish; }

python3 scripts/semantic_coverage.py "$WORK/coverage.json" "$WORK/load.json" \
  "$CONNECTIONS" "$SECONDS_TO_RUN" > "$REPORT" \
  || { fail "the report could not be built"; finish; }

ok "$REPORT written"
finish
