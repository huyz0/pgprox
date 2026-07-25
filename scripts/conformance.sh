#!/usr/bin/env bash
# M1 completion condition: the wire codec works against real Postgres, and real
# drivers work against it.
#
# The proxy binary does not exist until M6, so this cannot test "the proxy". It
# tests the codec from both sides:
#
#   client side  our codec drives real Postgres in a container
#   server side  real drivers connect to a harness built on our codec
#
# Usage: conformance.sh [major-version ...]     (default: 17 18)
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

VERSIONS=("$@")
if (( ${#VERSIONS[@]} == 0 )); then
  VERSIONS=(17 18)
fi

echo "M1: protocol conformance against Postgres ${VERSIONS[*]}"
echo

# --- prerequisites -----------------------------------------------------------
if ! has_rust; then
  fail "no workspace Cargo.toml"
  finish
fi

if ! docker info >/dev/null 2>&1; then
  fail "docker daemon unreachable; testcontainers cannot start Postgres"
  finish
fi
ok "docker daemon reachable"

for crate in pgprox-proto pgprox-tls; do
  if [[ -f "crates/$crate/Cargo.toml" ]]; then
    ok "$crate exists"
  else
    fail "$crate missing"
  fi
done

if (( _fail_count > 0 )); then
  finish
fi

# --- unit and property tests -------------------------------------------------
for crate in pgprox-proto pgprox-tls; do
  if cargo nextest run -p "$crate" >/dev/null 2>&1; then
    ok "$crate unit tests"
  else
    fail "$crate unit tests (run: cargo nextest run -p $crate)"
  fi
done

# --- Postgres lifecycle ------------------------------------------------------
# The harness owns the container, not the tests. nextest runs each test in its
# own process, so tests starting their own would mean one container per test and
# a name collision between them.
PG_CONTAINER=""

start_postgres() {
  local major="$1"
  PG_CONTAINER="pgprox-conformance-$major-$$"
  docker rm -f "$PG_CONTAINER" >/dev/null 2>&1 || true
  docker run -d --rm --name "$PG_CONTAINER" \
    -e POSTGRES_HOST_AUTH_METHOD=trust \
    -e POSTGRES_DB=conformance \
    -P "postgres:$major-alpine" >/dev/null || return 1
  docker port "$PG_CONTAINER" 5432/tcp | head -1 | sed 's/.*://'
}

stop_postgres() {
  [[ -n "$PG_CONTAINER" ]] && docker rm -f "$PG_CONTAINER" >/dev/null 2>&1
  PG_CONTAINER=""
}

# Containers must not outlive a failed or interrupted run.
trap stop_postgres EXIT INT TERM

# --- client side: our codec against real Postgres ----------------------------
for version in "${VERSIONS[@]}"; do
  if ! PG_PORT="$(start_postgres "$version")" || [[ -z "$PG_PORT" ]]; then
    fail "could not start Postgres $version"
    continue
  fi

  if PGPROX_PG_MAJOR="$version" PGPROX_PG_PORT="$PG_PORT" \
     cargo nextest run -p pgprox-proto --features integration \
       --run-ignored all -E 'test(conformance_client)' >/dev/null 2>&1; then
    ok "client side vs Postgres $version"
  else
    fail "client side vs Postgres $version (re-run with PGPROX_PG_PORT=$PG_PORT for output)"
  fi
  stop_postgres
done

# --- server side: real drivers against our harness ---------------------------
# Every driver is named explicitly and reported as ran or skipped. A suite that
# quietly runs three of five reads as full coverage when it is not.
DRIVERS=(psql pgx asyncpg jdbc npgsql)
declare -a SKIPPED=()

driver_available() {
  case "$1" in
    psql)    have psql ;;
    pgx)     have go ;;
    asyncpg) have python3 && have uv ;;
    jdbc)    have java && have mvn ;;
    npgsql)  have dotnet ;;
    *)       return 1 ;;
  esac
}

for driver in "${DRIVERS[@]}"; do
  if ! driver_available "$driver"; then
    SKIPPED+=("$driver")
    skip "driver $driver (toolchain not installed)"
    continue
  fi
  if [[ -x "tests/conformance/drivers/$driver.sh" ]]; then
    if "tests/conformance/drivers/$driver.sh" >/dev/null 2>&1; then
      ok "driver $driver"
    else
      fail "driver $driver (run: tests/conformance/drivers/$driver.sh)"
    fi
  else
    fail "driver $driver: tests/conformance/drivers/$driver.sh missing"
  fi
done

# A skipped driver is a gap in coverage, not a pass. Say so loudly enough that
# nobody reads a green run as "all five drivers work".
if (( ${#SKIPPED[@]} > 0 )); then
  echo
  warn "${#SKIPPED[@]} of ${#DRIVERS[@]} drivers were skipped: ${SKIPPED[*]}"
  warn "coverage is partial; install the toolchains or run this in CI"
fi

finish
