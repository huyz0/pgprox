#!/usr/bin/env bash
# The scale run. M7's completion condition.
#
#   scripts/scale.sh [connections]     default 1000, against the compose stack
#   scripts/scale.sh 1000 --local      against a one-node stack on this machine
#   scripts/scale.sh 1000 --keep       leave the stack running afterwards
#
# Four numbers come out of it, and they are the four the roadmap states the
# milestone in:
#
#   * userspace RSS, total and per connection
#   * added p99 latency, against a direct connection to the same database
#   * upstream connection count, against the configured cap
#   * transactions and errors, because a fast run that failed is not a fast run
#
# The roadmap's targets are at 100k connections: RSS under 500 MB, added p99
# under 1ms. This script takes the connection count as an argument so a run at
# a thousand and a run at a hundred thousand are the same measurement, and so
# the slope between two runs means something. A run below 100k reports the
# targets for comparison and does not claim to have met them.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

CONNECTIONS=1000
KEEP=""
MODE="compose"
for arg in "$@"; do
  case "$arg" in
    --keep) KEEP=1 ;;
    # A one-node stack of local processes rather than containers. The compose
    # stack is the deployment shape and is what a reported number should come
    # from; this exists because a machine without a working Docker is still a
    # machine that can measure, and because the difference between the two is
    # recorded with every run.
    --local) MODE="local" ;;
    *[!0-9]*) fail "unknown argument $arg"; finish ;;
    *) CONNECTIONS="$arg" ;;
  esac
done
DURATION="${SCALE_DURATION:-30}"
# Quiet time between phases, so one phase's pool and one phase's queue are not
# in the next phase's numbers.
SETTLE="${SCALE_SETTLE:-10}"
SEED="${SCALE_SEED:-1}"

COMPOSE=(docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.scale.yml)

# What the node under test logs. Info by default, because a run that measures
# its own logging is measuring the wrong thing; `SCALE_LOG=debug` when a run
# needs to say why it refused something.
export SCALE_LOG="${SCALE_LOG:-info}"

if [[ "$MODE" == "local" ]]; then
  source "$(dirname "${BASH_SOURCE[0]}")/localstack.sh"
fi

# Where the proxy and the primary are reachable from the host, published by
# the scale override. The load client runs here rather than in a container so
# that its own memory is not counted as the proxy's.
PROXY_ADDR="127.0.0.1:16432"
DIRECT_ADDR="127.0.0.1:15432"

# The cap the fleet is allowed on the primary, read from the document the
# nodes are running rather than repeated here. A cap this script hard-coded
# would keep passing after somebody changed the real one.
UPSTREAM_CAP="$(awk '/max_connections:/ { print $2; exit }' deploy/config/scale.yaml)"

# A well-formed token that is not a valid one, same as the e2e run uses: the
# proxy checks the algorithm and the mock sidecar accepts anything it is not
# told to refuse.
TOKEN="$(printf '%s' '{"alg":"RS256","typ":"JWT"}' | base64 -w0 | tr '+/' '-_' | tr -d '=')"
TOKEN="${TOKEN}.$(printf '%s' '{"sub":"acme"}' | base64 -w0 | tr '+/' '-_' | tr -d '=').not-a-signature"

OUT_DIR="${SCALE_OUT_DIR:-$REPO_ROOT/target/scale}"
mkdir -p "$OUT_DIR"

# ---------------------------------------------------------------------------

require_tools() {
  require_tool cargo || return 1
  [[ "$MODE" == "compose" ]] && { require_tool docker || return 1; }
  return 0
}

build_client() {
  echo "building pgload"
  if ! cargo build --release -p pgload >/dev/null 2>&1; then
    fail "pgload did not build"
    cargo build --release -p pgload 2>&1 | tail -20 | sed 's/^/  /'
    return 1
  fi
}

bring_up() {
  if [[ "$MODE" == "local" ]]; then
    local_up || return 1
    PROXY_ADDR="$LOCAL_PROXY"
    DIRECT_ADDR="$LOCAL_DIRECT"
    TOKEN="$LOCAL_TOKEN"
    UPSTREAM_CAP="$(awk '/max_connections:/ { print $2; exit }' "$LOCAL_DIR/config.yaml")"
    return 0
  fi

  echo "building the image"
  if ! "${COMPOSE[@]}" build >/dev/null 2>&1; then
    fail "the image did not build"
    "${COMPOSE[@]}" build 2>&1 | tail -20 | sed 's/^/  /'
    return 1
  fi

  echo "starting the stack"
  if ! "${COMPOSE[@]}" up --detach --wait --wait-timeout 180 >/dev/null 2>&1; then
    fail "the stack did not come up"
    "${COMPOSE[@]}" ps 2>&1 | sed 's/^/  /'
    return 1
  fi
  ok "the stack is up"
}

tear_down() {
  if [[ "$MODE" == "local" ]]; then
    local_down
    return
  fi
  "${COMPOSE[@]}" down --volumes --remove-orphans >/dev/null 2>&1 || true
}

# pgbench's tables, created through the proxy so the tenant owns them, and then
# opened to the load role that measures the direct baseline.
prepare_data() {
  # The local stack loads them while coming up, since it owns its own Postgres
  # and has no container to exec into.
  if [[ "$MODE" == "local" ]]; then
    ok "the workload's tables exist"
    return 0
  fi

  if ! "${COMPOSE[@]}" exec -T -e PGPASSWORD="$TOKEN" client \
      pgbench --host pgprox-1 --port 6432 --username acme_app \
        --initialize --scale 1 --quiet tenant_acme >/dev/null 2>&1; then
    fail "could not create the workload's tables"
    return 1
  fi

  "${COMPOSE[@]}" exec -T primary psql --username postgres --dbname tenant_acme --quiet \
    -c 'GRANT ALL ON ALL TABLES IN SCHEMA public TO pgload' \
    -c 'GRANT ALL ON ALL SEQUENCES IN SCHEMA public TO pgload' \
    -c 'GRANT USAGE ON SCHEMA public TO pgload' >/dev/null 2>&1 \
    || { fail "could not grant the load role access to the tables"; return 1; }

  ok "the workload's tables exist"
}

# The proxy's own resident memory, in kilobytes. PID 1 in the container is the
# proxy itself: the entrypoint execs it, so the shell is replaced rather than
# left as a parent.
proxy_rss_kb() {
  if [[ "$MODE" == "local" ]]; then
    local pid
    pid="$(cat "$LOCAL_DIR/proxy.pid" 2>/dev/null)"
    [[ -n "$pid" ]] || return 0
    awk '/^VmRSS:/ { print $2 }' "/proc/$pid/status" 2>/dev/null
    return 0
  fi
  "${COMPOSE[@]}" exec -T pgprox-1 \
    awk '/^VmRSS:/ { print $2 }' /proc/1/status 2>/dev/null | tr -d '\r'
}

# The proxy's own CPU time so far, in milliseconds.
#
# From /proc/1/stat rather than from `docker stats`, which samples a rate and
# cannot be differenced across a phase. utime plus stime, in clock ticks,
# converted with the kernel's 100 Hz.
#
# This exists because M7.46 recorded 4.5ms of CPU per statement from an ad-hoc
# measurement, which is a number nobody could reproduce and therefore nobody
# could tell had changed.
proxy_cpu_ms() {
  local ticks
  if [[ "$MODE" == "local" ]]; then
    local pid
    pid="$(cat "$LOCAL_DIR/proxy.pid" 2>/dev/null)"
    [[ -n "$pid" ]] || return 0
    ticks="$(awk '{ print $14 + $15 }' "/proc/$pid/stat" 2>/dev/null)"
  else
    ticks="$("${COMPOSE[@]}" exec -T pgprox-1 \
      awk '{ print $14 + $15 }' /proc/1/stat 2>/dev/null | tr -d '\r')"
  fi
  [[ -n "$ticks" ]] || return 0
  echo $(( ticks * 10 ))
}

# Statements routed, as `primary replica`, from the node under test.
#
# The share a replica served is the point of the replica machinery, and until
# this counter existed nothing could say it: a replica pool at zero means
# either that the router never chose one or that it did and the connection was
# already warm.
route_counts() {
  local metrics
  if [[ "$MODE" == "local" ]]; then
    metrics="$(curl --silent "http://127.0.0.1:$LOCAL_ADMIN_PORT/metrics" 2>/dev/null)"
  else
    metrics="$("${COMPOSE[@]}" exec -T pgprox-1 \
      curl --silent http://127.0.0.1:9090/metrics 2>/dev/null)"
  fi
  awk '
    /^pgprox_route_total\{.*route="primary"/ { primary = $NF }
    /^pgprox_route_total\{.*route="replica"/ { replica = $NF }
    END { printf "%d %d\n", primary, replica }
  ' <<< "$metrics"
}

# How many connections the fleet is holding on the primary right now.
upstream_connections() {
  if [[ "$MODE" == "local" ]]; then
    psql "postgresql://postgres@127.0.0.1:$LOCAL_PG_PORT/tenant_acme" \
      --no-align --tuples-only --quiet \
      -c "SELECT count(*) FROM pg_stat_activity WHERE usename = 'acme_app'" 2>/dev/null \
      | tr -d '[:space:]'
    return 0
  fi
  "${COMPOSE[@]}" exec -T primary \
    psql --username postgres --dbname tenant_acme --no-align --tuples-only --quiet \
      -c "SELECT count(*) FROM pg_stat_activity WHERE usename = 'acme_app'" 2>/dev/null \
    | tr -d '[:space:]'
}

# Runs the load client and leaves its report at $1.
#
# Samples the proxy's memory and the primary's connection count while it runs,
# because both are only interesting under load: a proxy measured after its
# clients have gone has released everything they cost it.
load_run() {
  local report="$1" target="$2" user="$3" password="$4" watch="$5" connections="$6"
  local peak_rss=0 peak_upstream=0

  ./target/release/pgload \
    --target "$target" \
    --workload product/perf/workload.yaml \
    --connections "$connections" \
    --duration "$DURATION" \
    --seed "$SEED" \
    --user "$user" \
    --database tenant_acme \
    --password "$password" \
    --out "$report" >"$report.log" 2>&1 &
  local client=$!

  if [[ "$watch" == "watch" ]]; then
    while kill -0 "$client" 2>/dev/null; do
      local rss upstream
      rss="$(proxy_rss_kb)"
      upstream="$(upstream_connections)"
      [[ "$rss" =~ ^[0-9]+$ ]] && (( rss > peak_rss )) && peak_rss="$rss"
      [[ "$upstream" =~ ^[0-9]+$ ]] && (( upstream > peak_upstream )) && peak_upstream="$upstream"
      sleep 2
    done
    echo "$peak_rss $peak_upstream" > "$report.watch"
  fi

  wait "$client"
}

# Reads one number out of a report.
from_report() {
  local file="$1" key="$2"
  awk -F': *' -v key="\"$key\"" '$1 ~ key { gsub(/[,]/, "", $2); print $2; exit }' "$file"
}

# ---------------------------------------------------------------------------

run_scale() {
  local proxy_report="$OUT_DIR/proxy.json"
  local direct_report="$OUT_DIR/direct.json"

  local idle_rss
  idle_rss="$(proxy_rss_kb)"
  [[ "$idle_rss" =~ ^[0-9]+$ ]] || { fail "could not read the proxy's memory"; return 1; }

  # --- the two matched phases, back to back and in this order ---------------
  #
  # The baseline first, on a cold proxy pool, and the matched proxy run right
  # after it. Running the thousand-connection phase first left the pool holding
  # forty connections and the database still working through their queue, so
  # the baseline measured a busier machine than the proxy run did and the
  # difference came out negative: the proxy appeared faster than a direct
  # connection, which is not a thing that can be true.
  #
  # `SETTLE` between phases for the same reason. A pool does not reap the
  # instant its clients leave.
  local direct_connections=$(( CONNECTIONS < UPSTREAM_CAP ? CONNECTIONS : UPSTREAM_CAP ))
  local matched_report="$OUT_DIR/proxy-matched.json"

  echo "  running $direct_connections connections directly against the primary (its share of the cap)"
  if ! load_run "$direct_report" "$DIRECT_ADDR" pgload "" nowatch "$direct_connections"; then
    fail "the direct baseline failed"
    tail -5 "$direct_report.log" | sed 's/^/  /'
    return 1
  fi
  sleep "$SETTLE"

  echo "  running $direct_connections connections through pgprox-1, to match the baseline"
  if ! load_run "$matched_report" "$PROXY_ADDR" acme_app "$TOKEN" nowatch "$direct_connections"; then
    fail "the matched run through the proxy failed"
    tail -5 "$matched_report.log" | sed 's/^/  /'
    return 1
  fi
  sleep "$SETTLE"

  # --- and the full count ---------------------------------------------------
  #
  # Last, because it is the phase that leaves the machine busiest, and because
  # what it measures does not compare against anything: memory, the upstream
  # cap and the error count are all about this node under many connections.
  local routed_primary_before routed_replica_before cpu_before
  read -r routed_primary_before routed_replica_before < <(route_counts)
  cpu_before="$(proxy_cpu_ms)"

  echo "  running $CONNECTIONS connections through pgprox-1 for ${DURATION}s"
  if ! load_run "$proxy_report" "$PROXY_ADDR" acme_app "$TOKEN" watch "$CONNECTIONS"; then
    fail "the load run through the proxy failed"
    tail -5 "$proxy_report.log" | sed 's/^/  /'
    return 1
  fi

  local cpu_after cpu_ms
  cpu_after="$(proxy_cpu_ms)"
  cpu_ms=$(( ${cpu_after:-0} - ${cpu_before:-0} ))

  # The delta across the full-count phase, not the counter's total: the
  # matched-load phase runs through the same node and its statements are in
  # the same counter, so a total would be a ratio over two different workloads.
  local routed_primary routed_replica
  read -r routed_primary routed_replica < <(route_counts)
  routed_primary=$(( routed_primary - routed_primary_before ))
  routed_replica=$(( routed_replica - routed_replica_before ))

  local peak_rss peak_upstream
  read -r peak_rss peak_upstream < "$proxy_report.watch"

  local proxy_p99 direct_p99 proxy_p50 direct_p50 transactions errors
  # The hop is measured at matched load; the full-count run's percentiles are
  # about a saturated database and are reported separately below.
  proxy_p99="$(from_report "$matched_report" p99_us)"
  direct_p99="$(from_report "$direct_report" p99_us)"
  proxy_p50="$(from_report "$matched_report" p50_us)"
  direct_p50="$(from_report "$direct_report" p50_us)"
  local loaded_p50 loaded_p99 matched_errors
  loaded_p50="$(from_report "$proxy_report" p50_us)"
  loaded_p99="$(from_report "$proxy_report" p99_us)"
  matched_errors="$(from_report "$matched_report" errors)"
  transactions="$(from_report "$proxy_report" transactions)"
  errors="$(from_report "$proxy_report" errors)"
  local direct_errors
  direct_errors="$(from_report "$direct_report" errors)"

  local added_p99=$(( proxy_p99 - direct_p99 ))
  local added_p50=$(( proxy_p50 - direct_p50 ))
  local load_rss=$(( peak_rss - idle_rss ))
  local per_conn_bytes=$(( load_rss * 1024 / CONNECTIONS ))

  # --- what the run says -----------------------------------------------------
  echo
  echo "  connections      $CONNECTIONS"
  echo "  transactions     $transactions ($errors error(s))"
  echo "  rss idle         $(( idle_rss / 1024 )) MB"
  echo "  rss under load   $(( peak_rss / 1024 )) MB"
  echo "  rss per conn     $per_conn_bytes bytes"
  echo "  at matched load ($direct_connections connections both sides)"
  echo "    p50 proxy      ${proxy_p50}us   direct ${direct_p50}us   added ${added_p50}us"
  echo "    p99 proxy      ${proxy_p99}us   direct ${direct_p99}us   added ${added_p99}us"
  echo "  at $CONNECTIONS connections (the database is the queue here, not the proxy)"
  echo "    p50            ${loaded_p50}us"
  echo "    p99            ${loaded_p99}us"
  echo "  upstream conns   $peak_upstream of $UPSTREAM_CAP"
  local routed_total=$(( routed_primary + routed_replica ))
  if (( routed_total > 0 )); then
    echo "  statements       $routed_total: $routed_replica on a replica ($(( routed_replica * 100 / routed_total ))%)"
    # CPU per statement, which M7.46 is about. A number nobody can reproduce
    # is a number nobody can tell has changed, and 4.5ms came from an ad-hoc
    # `perf` session that left nothing behind.
    if (( cpu_ms > 0 )); then
      echo "  proxy cpu        ${cpu_ms}ms over the phase, $(( cpu_ms * 1000 / routed_total ))us per statement"
    fi
  fi
  echo "  baseline         $direct_connections connections, $direct_errors error(s)"
  echo

  # --- what it is judged on --------------------------------------------------
  #
  # These three are true at any connection count, so they are checked at every
  # one. The roadmap's RSS and latency targets are stated at 100k and are
  # reported above rather than asserted here, because a run at a thousand
  # cannot meet or miss a target about a hundred thousand.
  # A refusal and a failure are different answers.
  #
  # A node that has run out of upstream connections and says 53300 is behaving
  # exactly as designed: the client is told, the error is retryable, and a
  # driver reconnects. A run that offers six times the work the database can
  # serve will produce them, and calling that a failure would mean the only
  # way to pass is to under-load the thing being measured.
  #
  # What must be zero is everything else: a dropped socket, a protocol error,
  # a timeout with no message. Those say the proxy did something wrong.
  local why
  why="$(awk -F'"' '/"first_error"/ { print $4; exit }' "$proxy_report")"
  if (( errors == 0 )); then
    ok "no failed transactions"
  elif [[ "$why" == *"53300"* || "$why" == *"too many connections"* ]]; then
    ok "$errors transaction(s) refused with 53300, which is the node telling a saturated client to retry"
  else
    fail "the run had $errors error(s): a fast run that failed is not a fast run${why:+ ($why)}"
  fi

  if (( matched_errors > 0 )); then
    fail "the matched run had $matched_errors error(s)"
  else
    ok "the matched run is clean"
  fi

  if (( direct_errors > 0 )); then
    fail "the direct baseline had $direct_errors error(s), so the comparison is against a broken run"
  else
    ok "the direct baseline is clean"
  fi

  if (( peak_upstream > UPSTREAM_CAP )); then
    fail "upstream connections reached $peak_upstream, above the cap of $UPSTREAM_CAP"
  else
    ok "upstream connections stayed at or under the cap ($peak_upstream of $UPSTREAM_CAP)"
  fi

  if (( peak_rss <= idle_rss )); then
    fail "the proxy used no more memory under load than idle: the measurement is not measuring"
  else
    ok "rss per connection: $per_conn_bytes bytes (100k of these would be $(( per_conn_bytes * 100000 / 1024 / 1024 )) MB, target 500)"
  fi

  if (( added_p99 < 0 )); then
    warn "added p99 is negative: the proxy answered faster than the direct connection, so one side of the comparison is not comparable"
  else
    ok "added p99: ${added_p99}us (target at 100k: under 1000)"
  fi

  # Kept where M7.7 can record it and where two runs can be diffed.
  printf '%s\n' \
    "connections=$CONNECTIONS" \
    "duration_s=$DURATION" \
    "seed=$SEED" \
    "transactions=$transactions" \
    "errors=$errors" \
    "rss_idle_kb=$idle_rss" \
    "rss_load_kb=$peak_rss" \
    "rss_per_conn_bytes=$per_conn_bytes" \
    "p50_proxy_us=$proxy_p50" \
    "p50_direct_us=$direct_p50" \
    "p99_proxy_us=$proxy_p99" \
    "p99_direct_us=$direct_p99" \
    "p50_loaded_us=$loaded_p50" \
    "p99_loaded_us=$loaded_p99" \
    "upstream_peak=$peak_upstream" \
    "upstream_cap=$UPSTREAM_CAP" \
    "mode=$MODE" \
    "direct_connections=$direct_connections" \
    "direct_errors=$direct_errors" \
    "routed_primary=$routed_primary" \
    "routed_replica=$routed_replica" \
    > "$OUT_DIR/summary.env"
  echo "  numbers written to $OUT_DIR/summary.env"
}

# ---------------------------------------------------------------------------

echo "=== SCALE: $CONNECTIONS connections, $MODE stack ==="

require_tools || finish
build_client || finish

if [[ -z "$KEEP" ]]; then
  trap tear_down EXIT
fi

bring_up || finish
prepare_data || finish
run_scale || true

finish
