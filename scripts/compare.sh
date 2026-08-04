#!/usr/bin/env bash
# pgprox against pgbouncer and pgcat, on one machine and one workload. `M32.3`.
#
#   scripts/compare.sh [connections]     default 200
#   scripts/compare.sh 200 --keep        leave the stack running afterwards
#
# Every claim this project makes about pooling is against its own baseline.
# `product/perf` holds twenty run documents and not one of them has another
# pooler in it. This is the one that does.
#
# # What it answers
#
# Two questions, and it is worth being narrow about them because a comparison
# that tries to answer everything answers nothing:
#
#   * Does per-connection memory beat a C pooler tuned for it since 2007.
#   * What does an arm holding a fleet-wide cap cost next to two that do not
#     coordinate at all.
#
# # One arm at a time
#
# Three poolers under load on one machine measure the machine. Each arm gets the
# stack to itself, with `SETTLE` between them so one arm's pool and one arm's
# queue are not in the next arm's numbers.
#
# # What it refuses to do
#
# Report a number it did not get. An arm that fails is named and its log is
# shown, and the table says so rather than leaving a blank that reads like a
# zero.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

WORKLOAD="${WORKLOAD:-product/perf/workload.yaml}"
CONNECTIONS=200
KEEP=""
for arg in "$@"; do
  case "$arg" in
    --keep) KEEP=1 ;;
    *[!0-9]*) fail "unknown argument $arg"; finish ;;
    *) CONNECTIONS="$arg" ;;
  esac
done

DURATION="${COMPARE_DURATION:-30}"
# Quiet time between arms. A pool does not reap the instant its clients leave,
# and an arm measured while the previous one is still returning connections is
# measuring the previous one.
SETTLE="${COMPARE_SETTLE:-10}"
SEED="${COMPARE_SEED:-1}"

COMPOSE=(docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.compare.yml)
SERVICES=(primary pgprox-1 pgbouncer pgcat)

OUT_DIR="${COMPARE_OUT_DIR:-$REPO_ROOT/target/compare}"
mkdir -p "$OUT_DIR"

# A well-formed token that is not a valid one, same as the e2e and scale runs
# use. The proxy checks the algorithm and the mock sidecar accepts anything it
# is not told to refuse.
TOKEN="$(printf '%s' '{"alg":"RS256","typ":"JWT"}' | base64 -w0 | tr '+/' '-_' | tr -d '=')"
TOKEN="${TOKEN}.$(printf '%s' '{"sub":"acme"}' | base64 -w0 | tr '+/' '-_' | tr -d '=').not-a-signature"

# Each arm: name, host port, container, the role it connects as, and its
# password. `direct` is the floor rather than a pooler, and it connects as the
# trust-authenticated `pgload` role for the reason `deploy/primary/init.sh`
# gives: giving the tenant's own role a trust rule would change what the
# poolers do on their upstream connections, which is part of what is measured.
ARMS=(
  "direct    15432 primary   pgload   "
  "pgprox    16441 pgprox-1  acme_app $TOKEN"
  "pgbouncer 16442 pgbouncer acme_app acme-password"
  "pgcat     16443 pgcat     acme_app acme-password"
)

# ---------------------------------------------------------------------------

# The cap each arm is configured with, read from the file it runs rather than
# restated here. A comparison whose arms are capped differently is not one, and
# a script that hardcoded the number would keep passing after somebody changed
# one of the three.
check_caps_agree() {
  local pgprox pgbouncer_default pgbouncer_max pgcat

  pgprox="$(awk '/max_connections:/ { print $2; exit }' deploy/config/compare.yaml)"
  pgbouncer_default="$(awk -F'= *' '/^default_pool_size/ { print $2; exit }' deploy/compare/pgbouncer.ini)"
  pgbouncer_max="$(awk -F'= *' '/^max_db_connections/ { print $2; exit }' deploy/compare/pgbouncer.ini)"
  pgcat="$(awk -F'= *' '/^pool_size/ { print $2; exit }' deploy/compare/pgcat.toml)"

  if [[ "$pgprox" == "$pgbouncer_default" && "$pgprox" == "$pgbouncer_max" \
     && "$pgprox" == "$pgcat" ]]; then
    ok "all three arms are capped at $pgprox upstream connections"
    UPSTREAM_CAP="$pgprox"
    return 0
  fi

  fail "the arms are capped differently, so this would not be a comparison"
  printf '       pgprox %s, pgbouncer %s/%s, pgcat %s\n' \
    "$pgprox" "$pgbouncer_default" "$pgbouncer_max" "$pgcat"
  return 1
}

# Prepared statements mapped in all three, checked the same way and for a
# sharper reason: without them every named `Parse` fails, and the arm reports a
# catastrophe that is a missing line of configuration. `M32.6` nearly published
# exactly that about pgcat.
check_statements_are_mapped() {
  local missing=""
  grep -q '^max_prepared_statements *= *[1-9]' deploy/compare/pgbouncer.ini || missing+=" pgbouncer"
  grep -q '^prepared_statements_cache_size *= *[1-9]' deploy/compare/pgcat.toml || missing+=" pgcat"

  if [[ -z "$missing" ]]; then
    ok "prepared statements are mapped in every arm that needs telling"
  else
    fail "these arms would fail every named Parse, which is configuration and not a finding:$missing"
    return 1
  fi
}

bring_up() {
  echo "building the image"
  if ! "${COMPOSE[@]}" build pgprox-1 >/dev/null 2>&1; then
    fail "the image did not build"
    "${COMPOSE[@]}" build pgprox-1 2>&1 | tail -20 | sed 's/^/  /'
    return 1
  fi

  echo "starting the stack"
  if ! "${COMPOSE[@]}" up --detach --wait --wait-timeout 300 "${SERVICES[@]}" >/dev/null 2>&1; then
    fail "the stack did not come up"
    "${COMPOSE[@]}" ps 2>&1 | sed 's/^/  /'
    return 1
  fi
  ok "all four arms are up"
}

tear_down() {
  "${COMPOSE[@]}" down --volumes --remove-orphans >/dev/null 2>&1 || true
}

# pgbench's tables, and the grants the direct arm needs to read them.
prepare_data() {
  if ! "${COMPOSE[@]}" exec -T -e PGPASSWORD=acme-password primary \
      pgbench --host primary --port 5432 --username acme_app \
        --initialize --scale 1 --quiet tenant_acme >/dev/null 2>&1; then
    fail "could not create the workload's tables"
    return 1
  fi

  "${COMPOSE[@]}" exec -T primary psql --username postgres --dbname tenant_acme --quiet \
    -c 'GRANT ALL ON ALL TABLES IN SCHEMA public TO pgload' \
    -c 'GRANT ALL ON ALL SEQUENCES IN SCHEMA public TO pgload' \
    -c 'GRANT USAGE ON SCHEMA public TO pgload' >/dev/null 2>&1 \
    || { fail "could not grant the direct arm access to the tables"; return 1; }

  ok "the workload's tables exist"
}

# What each pooler reports at startup, so the run document names versions rather
# than a tag that moves.
arm_versions() {
  local pgb pgc
  pgb="$("${COMPOSE[@]}" logs pgbouncer 2>/dev/null | grep -o 'PgBouncer [0-9.]*' | head -1)"
  pgc="$("${COMPOSE[@]}" logs pgcat 2>/dev/null | grep -o 'Version [0-9.]*' | head -1)"
  [[ -n "$pgc" ]] && pgc="PgCat ${pgc#Version }"
  printf '%s\n' "${pgb:-pgbouncer (version not reported)}" "${pgc:-pgcat (version not reported)}"
}

# One container's resident memory, in kilobytes. PID 1 is the process itself in
# all three images: each execs its binary rather than leaving a shell as parent.
arm_rss_kb() {
  "${COMPOSE[@]}" exec -T "$1" awk '/^VmRSS:/ { print $2 }' /proc/1/status 2>/dev/null | tr -d '\r'
}

# The address a container reaches the primary from, so its connections can be
# told apart from another arm's.
arm_address() {
  local id
  id="$("${COMPOSE[@]}" ps -q "$1" 2>/dev/null)"
  [[ -n "$id" ]] || return 1
  docker inspect "$id" --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' 2>/dev/null
}

# How many connections the primary is holding for one arm right now.
#
# By `client_addr`, which is the only thing that attributes a connection to the
# arm that opened it. Counting by role instead reported pgbouncer at 111 against
# a configured cap of 60, and it was not a breach: the arms run in sequence, the
# previous arm's pool had not been reaped, and 111 was pgbouncer's 60 plus
# pgprox's 51 still sitting there. That number would have been published as a
# finding about pgbouncer.
#
# Waiting out every reaper between arms would take longer than the run and would
# still be a guess. This asks the question the table means to ask.
#
# The direct arm has no container, because `bin/pgload` runs on the host and its
# address is whatever the bridge gives it. It connects as `pgload` and nothing
# else does, so that arm is counted by role.
upstream_connections() {
  local where="usename = 'pgload'"
  if [[ -n "${ARM_ADDRESS:-}" ]]; then
    where="client_addr = '$ARM_ADDRESS'"
  fi

  "${COMPOSE[@]}" exec -T primary \
    psql --username postgres --dbname tenant_acme --no-align --tuples-only --quiet \
      -c "SELECT count(*) FROM pg_stat_activity WHERE $where" 2>/dev/null \
    | tr -d '[:space:]'
}

# Reads one number out of a report.
from_report() {
  awk -F': *' -v key="\"$2\"" '$1 ~ key { gsub(/[,]/, "", $2); print $2; exit }' "$1"
}

# Runs one arm and leaves its report and its samples in $OUT_DIR.
run_arm() {
  local name="$1" port="$2" container="$3" user="$4" password="$5"
  local report="$OUT_DIR/$name.json"
  local peak_rss=0 peak_upstream=0 idle_rss=0

  # The floor arm is Postgres itself, which has no pooler to measure the memory
  # of, and it runs at the cap rather than at the full connection count because
  # that is all a database without a pooler in front of it would accept.
  local connections="$CONNECTIONS"
  ARM_ADDRESS=""
  if [[ "$name" == "direct" ]]; then
    connections=$(( CONNECTIONS < UPSTREAM_CAP ? CONNECTIONS : UPSTREAM_CAP ))
  else
    idle_rss="$(arm_rss_kb "$container")"
    ARM_ADDRESS="$(arm_address "$container")"
    if [[ -z "$ARM_ADDRESS" ]]; then
      fail "could not find the address $name connects from, so its upstream count would be another arm's"
      return 1
    fi
  fi

  echo "  $name: $connections connections for ${DURATION}s"
  ./target/release/pgload \
    --target "127.0.0.1:$port" \
    --workload "$WORKLOAD" \
    --connections "$connections" \
    --duration "$DURATION" \
    --seed "$SEED" \
    --user "$user" \
    --database tenant_acme \
    --password "$password" \
    --out "$report" >"$report.log" 2>&1 &
  local client=$!

  while kill -0 "$client" 2>/dev/null; do
    local rss upstream
    upstream="$(upstream_connections)"
    [[ "$upstream" =~ ^[0-9]+$ ]] && (( upstream > peak_upstream )) && peak_upstream="$upstream"
    if [[ "$name" != "direct" ]]; then
      rss="$(arm_rss_kb "$container")"
      [[ "$rss" =~ ^[0-9]+$ ]] && (( rss > peak_rss )) && peak_rss="$rss"
    fi
    sleep 2
  done

  if ! wait "$client"; then
    fail "the $name arm did not finish"
    tail -5 "$report.log" | sed 's/^/       /'
    return 1
  fi

  printf '%s %s %s %s\n' "$connections" "$idle_rss" "$peak_rss" "$peak_upstream" \
    > "$OUT_DIR/$name.samples"
  ok "$name finished"
}

report() {
  echo
  echo "=== COMPARE: $CONNECTIONS connections, ${DURATION}s, seed $SEED ==="
  echo
  arm_versions | sed 's/^/  /'
  echo
  printf '  %-10s %7s %7s %9s %9s %9s %8s %9s\n' \
    arm conns tx errors p50_us p99_us upstream 'RSS/conn'
  printf '  %-10s %7s %7s %9s %9s %9s %8s %9s\n' \
    ---------- ------- ------- --------- --------- --------- -------- ---------

  local name port container
  for arm in "${ARMS[@]}"; do
    set -- $arm
    name="$1"
    local report_file="$OUT_DIR/$name.json"
    local samples="$OUT_DIR/$name.samples"

    if [[ ! -s "$report_file" || ! -s "$samples" ]]; then
      printf '  %-10s %s\n' "$name" "did not run"
      continue
    fi

    local conns idle_rss peak_rss peak_upstream
    read -r conns idle_rss peak_rss peak_upstream < "$samples"

    local per_conn="-"
    if (( peak_rss > 0 && conns > 0 )); then
      per_conn="$(( (peak_rss - idle_rss) * 1024 / conns ))"
    fi

    printf '  %-10s %7s %7s %9s %9s %9s %8s %9s\n' \
      "$name" \
      "$conns" \
      "$(from_report "$report_file" transactions)" \
      "$(from_report "$report_file" errors)" \
      "$(from_report "$report_file" p50_us)" \
      "$(from_report "$report_file" p99_us)" \
      "$peak_upstream" \
      "$per_conn"
  done

  echo
  echo "  RSS/conn is bytes above the arm's own idle, divided by its connections."
  echo "  direct has no pooler to measure, so it reports none."
  echo "  The ramp is not in these numbers: pgprox resolves a grant per connect"
  echo "  and the other two read a password file, which is not the same work."
}

# ---------------------------------------------------------------------------

echo "=== COMPARE: pgprox against pgbouncer and pgcat ==="
echo

require_tool docker || finish
require_tool cargo || finish

check_caps_agree || finish
check_statements_are_mapped || finish

echo "building pgload"
if ! cargo build --release -p pgload >/dev/null 2>&1; then
  fail "pgload did not build"
  cargo build --release -p pgload 2>&1 | tail -20 | sed 's/^/  /'
  finish
fi

[[ -n "$KEEP" ]] || trap tear_down EXIT

bring_up || finish
prepare_data || finish

echo
for arm in "${ARMS[@]}"; do
  set -- $arm
  run_arm "$1" "$2" "$3" "$4" "${5:-}" || true
  sleep "$SETTLE"
done

report
finish
