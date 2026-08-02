#!/usr/bin/env bash
# Every supported driver, against the proxy, with a real Postgres behind it.
#
#   scripts/driver-matrix.sh
#
# `scripts/conformance.sh` has run these five drivers since M1, against
# `pgprox-proto`'s conformance server. That answers a different question:
# whether our codec and our harness agree with each other. They are the same
# code, so a misunderstanding shared between them is invisible by construction.
#
# The one time a driver was pointed at the real proxy, in M8.4, asyncpg
# deadlocked on its first parameterised query and had done since M6. `Flush`
# has no terminator, the relay read until `ReadyForQuery`, and both ends
# waited. Nothing in the suite could have found it, because the harness answers
# a `Flush` the same wrong way the proxy did.
#
# So this is the same drivers against `bin/pgprox`, through TLS, onto Postgres.
# What it finds is its own backlog: this script reports, it does not fix.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

COMPOSE=(docker compose -f deploy/docker-compose.yml)
OUT="${OUT:-product/conformance/driver-matrix.md}"
DRIVERS=(psql pgx asyncpg jdbc npgsql)

# pgprox-2 rather than pgprox-1: it requires TLS, which is the posture a real
# deployment runs in, and the token travels in the password field.
NODE=pgprox-2
PORT=16433

TOKEN="$(printf '%s' '{"alg":"RS256","typ":"JWT"}' | base64 -w0 | tr '+/' '-_' | tr -d '=')"
TOKEN="${TOKEN}.$(printf '%s' '{"sub":"acme"}' | base64 -w0 | tr '+/' '-_' | tr -d '=').not-a-signature"

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

echo "driver matrix, against the proxy"
echo

if ! docker info >/dev/null 2>&1; then
  fail "docker daemon unreachable"
  finish
fi

echo "bringing up the stack"
"${COMPOSE[@]}" up -d --build "$NODE" primary replica-1 replica-2 client >/dev/null 2>&1 || {
  fail "the stack did not come up"
  finish
}

for _ in $(seq 1 60); do
  state="$("${COMPOSE[@]}" ps --format '{{.Health}}' "$NODE" 2>/dev/null | head -1)"
  [[ "$state" == "healthy" ]] && break
  sleep 2
done
if [[ "$state" == "healthy" ]]; then
  ok "$NODE is up"
else
  fail "$NODE never became healthy (last state: ${state:-unknown})"
  finish
fi

# The tables the probes read. Created through the proxy, which is also the
# first thing that would notice if it could not pass a COPY through.
if "${COMPOSE[@]}" exec -T -e PGPASSWORD="$TOKEN" -e PGSSLMODE=require client \
    pgbench --host "$NODE" --port 6432 --username acme_app \
      --initialize --scale 1 --quiet tenant_acme >/dev/null 2>&1; then
  ok "the tables exist"
else
  # Not fatal: the probes below use generate_series rather than pgbench's
  # tables, so this only costs the COPY path that created them.
  warn "could not initialise pgbench's tables through the proxy"
fi

# --- the probes ---------------------------------------------------------------
declare -A RESULT DETAIL
declare -a SKIPPED=()

for driver in "${DRIVERS[@]}"; do
  if ! driver_available "$driver"; then
    SKIPPED+=("$driver")
    skip "driver $driver (toolchain not installed)"
    continue
  fi

  log="$(mktemp -t pgprox-driver-XXXXXX.log)"
  # Bounded, because the failure this script exists to find is a hang. Ten
  # minutes is generous on purpose: a driver's first run fetches its
  # toolchain's dependencies.
  if timeout "${PROBE_TIMEOUT:-600}" env \
     PGPROX_HOST=127.0.0.1 PGPROX_PORT="$PORT" \
     PGPROX_USER=acme_app PGPROX_DB=tenant_acme PGPROX_TOKEN="$TOKEN" \
     "tests/proxy-drivers/$driver.sh" >"$log" 2>&1; then
    RESULT["$driver"]=pass
    ok "driver $driver"
  else
    status=$?
    RESULT["$driver"]=fail
    if (( status == 124 )); then
      DETAIL["$driver"]="timed out"
    else
      DETAIL["$driver"]="$(grep -m1 . "$log" | head -c 160)"
    fi
    fail "driver $driver: ${DETAIL[$driver]:-no output}"
  fi
  rm -f "$log"
done

# --- the record ---------------------------------------------------------------
#
# The report carries which tree it describes, not only when it was written.
# `M21.1`: a date is not checkable, because nothing knows what the code looked
# like on it. The newest commit touching the proxy is, and it is what makes
# "this report predates the thing it is evidence about" a question with an
# answer. Without it the report read "Generated on 2026-07-28" through thirteen
# milestones and every gate stayed green.
#
# Empty outside a git checkout, which is a tarball rather than a repository and
# is not something to fail over here; `m21-complete.sh` is where an absent line
# becomes a failure, because that is the place with a repository to ask.
PROXY_PATHS=(bin/pgprox crates/pgprox-session crates/pgprox-proto)
DESCRIBES="$(git log -1 --format=%H -- "${PROXY_PATHS[@]}" 2>/dev/null || true)"

mkdir -p "$(dirname "$OUT")"
{
  echo "# Driver matrix, against the proxy"
  echo
  echo "Generated by \`scripts/driver-matrix.sh\` on $(date -u +%Y-%m-%d)."
  echo
  echo "Describes: ${DESCRIBES:-unknown}"
  echo
  echo "That is the newest commit touching \`bin/pgprox\`,"
  echo "\`crates/pgprox-session\` or \`crates/pgprox-proto\` when this ran."
  echo "\`scripts/m21-complete.sh\` compares it against the tree to say how far"
  echo "behind these results are, because a date cannot be compared to code."
  echo
  echo "Five drivers against \`bin/pgprox\` over TLS, with a real Postgres"
  echo "behind it. Each one runs both wire protocols, a prepared statement"
  echo "reused on one session, a result larger than one segment, a"
  echo "transaction, and an error with a statement after it."
  echo
  echo "\`M21\` added what \`M20\` changed: a statement given back with a"
  echo "protocol \`Close\` and prepared again, in the three drivers that keep"
  echo "a cache; the unnamed statement, counted on the server rather than"
  echo "merely run; and the startup packet, meaning a \`search_path\` from"
  echo "\`options\`, an \`application_name\`, and a replication connection"
  echo "refused by name."
  echo
  echo "| Driver | Result |"
  echo "| --- | --- |"
  for driver in "${DRIVERS[@]}"; do
    case "${RESULT[$driver]:-}" in
      pass) echo "| $driver | pass |" ;;
      fail) echo "| $driver | **fail**: ${DETAIL[$driver]:-no output} |" ;;
      *)    echo "| $driver | not run |" ;;
    esac
  done
  echo
  if (( ${#SKIPPED[@]} > 0 )); then
    echo "Not run, toolchain missing on the machine that generated this:"
    echo "${SKIPPED[*]}. A skipped driver is a gap, not a pass."
    echo
  fi
  echo "## Why this exists beside scripts/conformance.sh"
  echo
  echo "That suite runs the same five drivers against \`pgprox-proto\`'s"
  echo "conformance server, which answers whether our codec and our harness"
  echo "agree with each other. They are the same code, so a misunderstanding"
  echo "shared between them cannot show up there."
  echo
  echo "It did not show up there. asyncpg could not run a single parameterised"
  echo "query through the proxy from M6 until M8, because \`Flush\` has no"
  echo "terminator and the relay read until \`ReadyForQuery\`. The harness"
  echo "answered a \`Flush\` the same wrong way, so the suite was green"
  echo "throughout. This matrix is the check that would have caught it."
} > "$OUT"

ok "written: $OUT"

echo
echo "the stack is still up; stop it with:"
echo "  ${COMPOSE[*]} down -v"

finish
