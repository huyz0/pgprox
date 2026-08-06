#!/usr/bin/env bash
# Is the per-connection memory the allocator's or the connection's? `M34.1`.
#
#   scripts/arena.sh [connections]     default 200
#
# `M33` measured 22,835 bytes per connection and accounted for 5,048 of them.
# The read and write buffers are ruled out by experiment: quartering them moved
# the figure by 205 bytes. This asks whether most of the rest is a per-thread
# cost being divided by a connection count it has nothing to do with.
#
# # Why three arms and not two
#
# A single-threaded runtime changes the thread count and the arena count at
# once, so it cannot say which of them mattered. glibc gives each thread its own
# arena up to a cap, and `MALLOC_ARENA_MAX` moves the cap without moving the
# threads. So:
#
#   baseline   default threads, default arenas
#   one-arena  default threads, MALLOC_ARENA_MAX=1
#   one-thread TOKIO_WORKER_THREADS=1, default arenas
#
# If `one-arena` collapses the per-connection figure, it was the allocator. If
# only `one-thread` does, it is something per-worker that is not the arena. If
# neither does, it is a real per-connection cost and this run says so.
#
# # Only the pgprox arm
#
# The other two poolers are not being compared here. `M32` did that; this is
# about where one number in it came from.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

WORKLOAD="${WORKLOAD:-docs/internal/product/perf/workload.yaml}"
CONNECTIONS="${1:-200}"
DURATION="${ARENA_DURATION:-30}"
SETTLE="${ARENA_SETTLE:-8}"
SEED="${ARENA_SEED:-1}"

COMPOSE=(docker compose -f deploy/docker-compose.yml \
                        -f deploy/docker-compose.compare.yml \
                        -f deploy/docker-compose.arena.yml)

# What this machine would have chosen on its own, stated rather than left
# implicit. tokio defaults to one worker per core and glibc caps arenas at eight
# per core on 64-bit, so the baseline arm passes exactly those and the other two
# change one of them.
#
# Explicit in every arm because the empty string is not "unset": tokio parses
# `TOKIO_WORKER_THREADS=""` and panics on it, which killed the first run of this
# in all three arms at once.
CORES="$(nproc)"
DEFAULT_ARENAS=$(( CORES * 8 ))
OUT_DIR="${ARENA_OUT_DIR:-$REPO_ROOT/target/arena}"
mkdir -p "$OUT_DIR"

TOKEN="$(printf '%s' '{"alg":"RS256","typ":"JWT"}' | base64 -w0 | tr '+/' '-_' | tr -d '=')"
TOKEN="${TOKEN}.$(printf '%s' '{"sub":"acme"}' | base64 -w0 | tr '+/' '-_' | tr -d '=').not-a-signature"

# name, TOKIO_WORKER_THREADS, MALLOC_ARENA_MAX. Every arm states both.
ARMS=(
  "baseline    $CORES $DEFAULT_ARENAS"
  "one-arena   $CORES 1"
  "one-thread  1      $DEFAULT_ARENAS"
)

# ---------------------------------------------------------------------------

# Every compose call interpolates the overlay, so every one of them needs the
# variables present. Exported once here rather than threaded through each call:
# `start_proxy` overrides them per arm, which is the only place their value
# means anything.
export PGPROX_WORKER_THREADS="$CORES"
export PGPROX_ARENA_MAX="$DEFAULT_ARENAS"

proxy_rss_kb() {
  "${COMPOSE[@]}" exec -T pgprox-1 awk '/^VmRSS:/ { print $2 }' /proc/1/status 2>/dev/null | tr -d '\r'
}

# How many threads the process actually has, so an arm that meant to change it
# is checked rather than assumed. A run where `TOKIO_WORKER_THREADS` was ignored
# would otherwise report the baseline twice and look like a null result.
proxy_threads() {
  "${COMPOSE[@]}" exec -T pgprox-1 awk '/^Threads:/ { print $2 }' /proc/1/status 2>/dev/null | tr -d '\r'
}

start_proxy() {
  local threads="$1" arenas="$2"
  # Recreated rather than restarted, because the environment is baked at create
  # time and a restart would silently run the previous arm again.
  PGPROX_WORKER_THREADS="$threads" PGPROX_ARENA_MAX="$arenas" \
    "${COMPOSE[@]}" up --detach --force-recreate --wait --wait-timeout 180 pgprox-1 >/dev/null 2>&1
}

run_arm() {
  local name="$1" threads="$2" arenas="$3"
  local report="$OUT_DIR/$name.json"

  start_proxy "$threads" "$arenas" || { fail "$name: the proxy did not come up"; return 1; }
  sleep "$SETTLE"

  local threads_seen idle_rss peak_rss=0
  threads_seen="$(proxy_threads)"
  idle_rss="$(proxy_rss_kb)"
  [[ "$idle_rss" =~ ^[0-9]+$ ]] || { fail "$name: could not read the proxy's memory"; return 1; }

  echo "  $name: $CONNECTIONS connections, ${threads_seen} threads in the process"
  ./target/release/pgload \
    --target 127.0.0.1:16441 \
    --workload "$WORKLOAD" \
    --connections "$CONNECTIONS" \
    --duration "$DURATION" \
    --seed "$SEED" \
    --user acme_app \
    --database tenant_acme \
    --password "$TOKEN" \
    --out "$report" >"$report.log" 2>&1 &
  local client=$!

  while kill -0 "$client" 2>/dev/null; do
    local rss
    rss="$(proxy_rss_kb)"
    [[ "$rss" =~ ^[0-9]+$ ]] && (( rss > peak_rss )) && peak_rss="$rss"
    sleep 2
  done

  if ! wait "$client"; then
    fail "$name: the load run did not finish"
    tail -5 "$report.log" | sed 's/^/       /'
    return 1
  fi

  printf '%s %s %s %s %s\n' "$threads_seen" "$arenas" "$idle_rss" "$peak_rss" "$CONNECTIONS" \
    > "$OUT_DIR/$name.samples"
  ok "$name finished"
}

report() {
  echo
  echo "=== ARENA: $CONNECTIONS connections, ${DURATION}s ==="
  echo
  printf '  %-11s %7s %7s %9s %9s %10s %9s\n' \
    arm workers arenas 'idle kB' 'peak kB' 'delta kB' 'B/conn'
  printf '  %-11s %7s %7s %9s %9s %10s %9s\n' \
    ----------- ------- ------- --------- --------- ---------- ---------

  for arm in "${ARMS[@]}"; do
    set -- $arm
    local name="$1"
    local file="$OUT_DIR/$name.samples"
    if [[ ! -s "$file" ]]; then
      printf '  %-11s %s\n' "$name" "did not run"
      continue
    fi
    local threads arenas idle peak conns
    read -r threads arenas idle peak conns < "$file"
    printf '  %-11s %7s %7s %9s %9s %10s %9s\n' \
      "$name" "$threads" "$arenas" "$idle" "$peak" "$(( peak - idle ))" \
      "$(( (peak - idle) * 1024 / conns ))"
  done

  echo
  echo "  delta is peak under load minus idle before it, which is the figure M33"
  echo "  divided by connections and could not account for."
  echo "  If one-arena collapses B/conn, it was glibc. If only one-thread does,"
  echo "  it is per-worker and not the arena. If neither does, it is the"
  echo "  connection's own memory and M33's question has a different answer."
}

# ---------------------------------------------------------------------------

echo "=== ARENA: where the per-connection memory goes ==="
echo

require_tool docker || finish
require_tool cargo || finish

echo "building pgload"
cargo build --release -p pgload >/dev/null 2>&1 || { fail "pgload did not build"; finish; }

echo "starting the stack"
# The build needs the variables set only because the overlay refuses to
# interpolate without them, and it does not care what they are. The baseline
# arm's values, so nothing here invents a fourth configuration.
PGPROX_WORKER_THREADS="$CORES" PGPROX_ARENA_MAX="$DEFAULT_ARENAS" \
  "${COMPOSE[@]}" build pgprox-1 >/dev/null 2>&1 \
  || { fail "the image did not build"; finish; }
# The primary only. `pgprox-1` is created by each arm with that arm's
# environment, and the overlay refuses to interpolate without one, which is what
# stops an arm from silently running the previous arm's settings.
PGPROX_WORKER_THREADS="$CORES" PGPROX_ARENA_MAX="$DEFAULT_ARENAS" \
  "${COMPOSE[@]}" up --detach --wait --wait-timeout 300 primary >/dev/null 2>&1 \
  || { fail "the stack did not come up"; finish; }

"${COMPOSE[@]}" exec -T -e PGPASSWORD=acme-password primary \
  pgbench --host primary --port 5432 --username acme_app \
    --initialize --scale 1 --quiet tenant_acme >/dev/null 2>&1 \
  || { fail "could not create the workload's tables"; finish; }
ok "the workload's tables exist"
echo

for arm in "${ARMS[@]}"; do
  set -- $arm
  run_arm "$1" "$2" "$3" || true
  sleep "$SETTLE"
done

report
"${COMPOSE[@]}" down --volumes --remove-orphans >/dev/null 2>&1 || true
finish
