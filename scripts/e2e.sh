#!/usr/bin/env bash
# The end-to-end stack, and the three properties M6 is judged on.
#
#   scripts/e2e.sh          bring the stack up, run every assertion, tear down
#   scripts/e2e.sh up       bring it up and leave it running
#   scripts/e2e.sh down     tear it down
#   scripts/e2e.sh prove    break each property on purpose and check it is caught
#
# The point of this script over `docker compose up` is that a failure says
# which component failed and what it last said, rather than an exit code. A
# stack of six services that dies with `exit 1` is a stack nobody can debug.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

COMPOSE=(docker compose -f deploy/docker-compose.yml)
PROXIES=(pgprox-1 pgprox-2 pgprox-3)
DATABASES=(primary replica-1 replica-2)

# A token whose header names an approved algorithm. The proxy checks the
# algorithm before it calls the sidecar and never verifies the signature; the
# mock sidecar accepts any token it is not told to refuse. So this is a
# well-formed token that is not a valid one, which is exactly what a test wants.
TOKEN="$(printf '%s' '{"alg":"RS256","typ":"JWT"}' | base64 -w0 | tr '+/' '-_' | tr -d '=')"
TOKEN="${TOKEN}.$(printf '%s' '{"sub":"acme"}' | base64 -w0 | tr '+/' '-_' | tr -d '=').not-a-signature"

# Where the client container reaches the proxies. Inside the compose network,
# so the assertions do not depend on published ports.
PROXY_PORT=6432
ADMIN_PORT=9090

# ---------------------------------------------------------------------------
# Running things inside the stack

# Runs psql in the client container against a proxy node.
in_psql() {
  local node="$1" sql="$2"
  # PGSSLMODE=require rather than the default `prefer`: the nodes are started
  # with --require-tls, and a client that quietly fell back to cleartext would
  # be a client whose token crossed the network in the clear.
  "${COMPOSE[@]}" exec -T \
    -e PGPASSWORD="$TOKEN" -e PGSSLMODE=require client \
    psql --host "$node" --port "$PROXY_PORT" --username acme_app --dbname tenant_acme \
      --no-align --tuples-only --quiet -c "$sql" 2>&1
}

# One HTTP request against a node's admin port, from inside the network.
#
# Answers as `<status> <body>`, because every caller wants the code and most
# want the reason with it.
in_admin() {
  local node="$1" method="$2" path="$3"
  # Run inside the node itself rather than from the client container: the proxy
  # image carries curl for its own health check, and the client image's busybox
  # wget cannot send a POST.
  "${COMPOSE[@]}" exec -T "$node" \
    curl --silent --show-error --request "$method" \
      --write-out ' %{http_code}' \
      "http://127.0.0.1:$ADMIN_PORT$path" 2>&1
}

# What a component last said, for a failure message that is worth reading.
last_words() {
  local service="$1"
  echo "  --- last 20 lines from $service ---"
  "${COMPOSE[@]}" logs --tail 20 "$service" 2>&1 | sed 's/^/  /'
}

# ---------------------------------------------------------------------------
# Bringing it up

bring_up() {
  # Built first and separately, because the build is minutes of Rust and the
  # start is seconds of Postgres: one timeout covering both would either be
  # generous enough to hide a node that never becomes ready, or tight enough
  # to fail a cold build.
  echo "building the image"
  if ! "${COMPOSE[@]}" build >/dev/null 2>&1; then
    fail "the image did not build"
    "${COMPOSE[@]}" build 2>&1 | tail -20 | sed 's/^/  /'
    return 1
  fi

  echo "starting the stack"
  if ! "${COMPOSE[@]}" up --detach --wait --wait-timeout 180 >/dev/null 2>&1; then
    fail "the stack did not come up"
    # Which one, rather than which exit code. This is the whole reason this
    # script exists rather than a bare compose invocation.
    for service in "${DATABASES[@]}" "${PROXIES[@]}"; do
      local state
      state="$("${COMPOSE[@]}" ps --format '{{.State}} {{.Health}}' "$service" 2>/dev/null)"
      if [[ "$state" != *running* || "$state" == *unhealthy* ]]; then
        echo "  $service: ${state:-not started}"
        last_words "$service"
      fi
    done
    return 1
  fi
  ok "the stack is up: ${#DATABASES[@]} databases, ${#PROXIES[@]} proxy nodes"
}

tear_down() {
  "${COMPOSE[@]}" down --volumes --remove-orphans >/dev/null 2>&1 || true
}

# ---------------------------------------------------------------------------
# The assertions M6 is judged on

# Every node serves a query, which is the stack working at all.
assert_every_node_serves() {
  local node answer
  for node in "${PROXIES[@]}"; do
    answer="$(in_psql "$node" 'SELECT 1' | tr -d '[:space:]')"
    if [[ "$answer" != "1" ]]; then
      fail "$node did not serve a query: $answer"
      last_words "$node"
      return 1
    fi
  done
  ok "every node serves a query"
}

# pgbench: the workload, clean, through the proxy.
assert_pgbench_is_clean() {
  local out
  echo "  running pgbench through pgprox-1"
  out="$("${COMPOSE[@]}" exec -T -e PGPASSWORD="$TOKEN" -e PGSSLMODE=require client \
    pgbench --host pgprox-1 --port "$PROXY_PORT" --username acme_app \
      --initialize --scale 1 --quiet tenant_acme 2>&1)" || {
    fail "pgbench could not initialise: $out"
    last_words pgprox-1
    return 1
  }

  out="$("${COMPOSE[@]}" exec -T -e PGPASSWORD="$TOKEN" -e PGSSLMODE=require client \
    pgbench --host pgprox-1 --port "$PROXY_PORT" --username acme_app \
      --client 8 --jobs 2 --time 15 --no-vacuum tenant_acme 2>&1)" || {
    fail "pgbench failed: $out"
    last_words pgprox-1
    return 1
  }

  # And again with named prepared statements, which is the protocol every
  # mainstream driver actually uses and the one that breaks the moment a
  # session's statement is bound on a connection that never parsed it.
  local prepared
  prepared="$("${COMPOSE[@]}" exec -T -e PGPASSWORD="$TOKEN" -e PGSSLMODE=require client \
    pgbench --host pgprox-1 --port "$PROXY_PORT" --username acme_app \
      --protocol prepared --client 4 --jobs 2 --time 10 --no-vacuum tenant_acme 2>&1)" || {
    fail "pgbench with prepared statements failed: $prepared"
    last_words pgprox-1
    return 1
  }
  if grep -qE 'number of failed transactions: 0 ' <<<"$prepared"; then
    ok "pgbench: clean with prepared statements"
  else
    fail "pgbench reported failed transactions with prepared statements"
    sed 's/^/  /' <<<"$prepared"
    return 1
  fi

  # "number of failed transactions: 0 (0.000%)" on a clean run. Anything else,
  # including the line being absent, is a failure: a run that cannot be checked
  # has not been checked.
  if grep -qE 'number of failed transactions: 0 ' <<<"$out"; then
    ok "pgbench: clean ($(grep -oE 'tps = [0-9.]+' <<<"$out" | head -1))"
  else
    fail "pgbench reported failed transactions"
    sed 's/^/  /' <<<"$out"
    return 1
  fi
}

# A drain, with traffic on the node, losing nothing.
assert_drain_loses_no_transactions() {
  local out drain_status ready

  echo "  running pgbench against pgprox-2 while draining it"
  "${COMPOSE[@]}" exec -T -e PGPASSWORD="$TOKEN" -e PGSSLMODE=require client \
    pgbench --host pgprox-2 --port "$PROXY_PORT" --username acme_app \
      --client 4 --jobs 2 --time 20 --no-vacuum tenant_acme >/tmp/pgprox-drain.out 2>&1 &
  local bench=$!

  # Long enough that the run is well under way, short enough that it is still
  # running when the drain lands.
  sleep 5
  drain_status="$(in_admin pgprox-2 POST /v1/drain)"
  if ! grep -qE ' 200$' <<<"$drain_status"; then
    fail "the drain was refused: $drain_status"
    kill "$bench" 2>/dev/null || true
    return 1
  fi

  # The probe must fail before the run ends, or the drain did nothing.
  sleep 3
  ready="$(in_admin pgprox-2 GET /readyz)"
  if ! grep -qE ' 503$' <<<"$ready"; then
    fail "a draining node still reports itself ready: $ready"
    kill "$bench" 2>/dev/null || true
    return 1
  fi
  ok "drain: /readyz fails first"

  wait "$bench" || true
  out="$(cat /tmp/pgprox-drain.out)"

  # A drained client reconnects, which pgbench does not do: what it must not
  # see is a transaction that failed. Connections it could not open after the
  # drain are the drain working.
  if grep -qE 'number of failed transactions: 0 ' <<<"$out"; then
    ok "drain: zero failed transactions"
  else
    fail "a drain lost transactions"
    sed 's/^/  /' <<<"$out"
    last_words pgprox-2
    return 1
  fi

  in_admin pgprox-2 POST /v1/undrain >/dev/null
}

# No read is served by a replica that has not replayed the session's own write.
#
# The check is a session that writes and then reads its own write back through
# the same connection. A replica behind the write returning the old value is
# exactly the stale read the watermark exists to prevent, and it is visible as
# a wrong answer rather than as a routing decision nobody can see.
assert_no_read_behind_the_watermark() {
  local answer
  in_psql pgprox-3 'CREATE TABLE IF NOT EXISTS watermark_probe (id int primary key, n int)' >/dev/null
  in_psql pgprox-3 'TRUNCATE watermark_probe' >/dev/null

  local round
  for round in $(seq 1 25); do
    answer="$(in_psql pgprox-3 "
      INSERT INTO watermark_probe VALUES ($round, $round)
        ON CONFLICT (id) DO UPDATE SET n = EXCLUDED.n;
      SELECT n FROM watermark_probe WHERE id = $round;
    " | tr -d '[:space:]')"

    if [[ "$answer" != "$round" ]]; then
      fail "round $round read behind its own write: expected $round, got '${answer:-nothing}'"
      last_words pgprox-3
      return 1
    fi
  done
  ok "watermark: 25 write-then-read rounds, none served stale"
}

# The node configured for upstream TLS actually reaches its database over TLS.
#
# Asked of the server rather than of the proxy. `pg_stat_ssl` is Postgres's own
# record of how each backend is connected, and joining it to `pg_backend_pid()`
# narrows it to the connection this very statement arrived on: the answer is
# the database saying whether the socket the proxy opened for this query is
# encrypted. Nothing the proxy reports about itself could say that.
#
# `M79.0`. The whole stack ran with `PGPROX_MOCK_TLS: disabled`, so the mode a
# real sidecar returns by default had never been pointed at a Postgres, and
# `M75.0` found it could not have reached one: it sent a TLS ClientHello where
# the protocol wants an `SSLRequest`. A unit test proved the negotiation and
# only a real server proves the negotiation is the one Postgres expects.
assert_upstream_tls_reaches_the_database() {
  local answer

  # That it connects at all is half the assertion and was the whole bug: a node
  # in this mode could not open an upstream connection, so every statement
  # failed with no answer rather than a wrong one.
  answer="$(in_psql pgprox-3 'SELECT 1' | tr -d '[:space:]')"
  if [[ "$answer" != "1" ]]; then
    fail "the node dialling upstream over TLS served nothing: ${answer:-nothing}"
    last_words pgprox-3
    return 1
  fi

  answer="$(in_psql pgprox-3 \
    'SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()' | tr -d '[:space:]')"
  if [[ "$answer" != "t" ]]; then
    fail "the upstream connection is not encrypted: pg_stat_ssl said '${answer:-nothing}'"
    last_words pgprox-3
    return 1
  fi
  ok "upstream TLS: the database says the proxy's connection to it is encrypted"
}

# ---------------------------------------------------------------------------
# Proving the assertions are not vacuous
#
# An assertion that cannot fail is worse than no assertion, because it is
# believed. Each of these breaks the property the assertion above it checks and
# requires the same predicate to notice. They run against the same stack, so
# what is being proven is the check rather than a copy of it.

# pgbench's summary is what says a run was clean, and this is a run that was
# not: a drained node refuses new connections, so the client cannot open the
# eight it wants.
prove_pgbench_check_catches_failures() {
  in_admin pgprox-3 POST /v1/drain >/dev/null
  sleep 3

  local out
  out="$("${COMPOSE[@]}" exec -T -e PGPASSWORD="$TOKEN" -e PGSSLMODE=require client \
    pgbench --host pgprox-3 --port "$PROXY_PORT" --username acme_app \
      --client 4 --jobs 2 --time 5 --no-vacuum tenant_acme 2>&1)" || true

  if grep -qE 'number of failed transactions: 0 ' <<<"$out"; then
    fail "the pgbench check reported a clean run against a node refusing connections"
  else
    ok "proven: the pgbench check catches a run that was not clean"
  fi

  in_admin pgprox-3 POST /v1/undrain >/dev/null
  sleep 2
}

# The drain assertion says a drain loses no transactions. This is what losing
# them looks like: the node is killed rather than drained, so whatever it was
# holding dies with it.
prove_drain_check_catches_losses() {
  local out
  "${COMPOSE[@]}" exec -T -e PGPASSWORD="$TOKEN" -e PGSSLMODE=require client \
    pgbench --host pgprox-3 --port "$PROXY_PORT" --username acme_app \
      --client 4 --jobs 2 --time 15 --no-vacuum tenant_acme >/tmp/pgprox-killed.out 2>&1 &
  local bench=$!

  sleep 5
  "${COMPOSE[@]}" kill pgprox-3 >/dev/null 2>&1
  wait "$bench" || true
  out="$(cat /tmp/pgprox-killed.out)"

  if grep -qE 'number of failed transactions: 0 ' <<<"$out"; then
    fail "the drain check reported no losses after the node was killed mid-transaction"
  else
    ok "proven: the drain check catches transactions lost to a node going away"
  fi

  "${COMPOSE[@]}" start pgprox-3 >/dev/null 2>&1
}

# The watermark assertion is a write followed by a read of the same row. This
# is what a stale read looks like: the same pair, against a replica whose
# replay is paused, with the proxy out of the way. If the predicate cannot see
# it here, it could not have seen it above.
prove_watermark_check_catches_a_stale_read() {
  local answer
  direct primary 'CREATE TABLE IF NOT EXISTS stale_probe (id int primary key, n int)' >/dev/null
  direct primary 'INSERT INTO stale_probe VALUES (1, 1) ON CONFLICT (id) DO UPDATE SET n = 1' >/dev/null
  # Let the replica catch up to the row existing, then stop it there.
  sleep 2
  # As the superuser: pausing replay is not a privilege a tenant's role has,
  # and the first version of this hid the refusal in /dev/null and concluded
  # the replica was up to date.
  local paused
  paused="$(as_superuser replica-1 'SELECT pg_wal_replay_pause()' 2>&1)"
  if grep -qi 'error' <<<"$paused"; then
    fail "could not pause replay, so nothing was proven: $paused"
    return 1
  fi

  direct primary 'UPDATE stale_probe SET n = 2 WHERE id = 1' >/dev/null
  answer="$(direct replica-1 'SELECT n FROM stale_probe WHERE id = 1' | tr -d '[:space:]')"

  if [[ "$answer" == "2" ]]; then
    fail "a replica with replay paused answered with the new value: the setup proves nothing"
  else
    ok "proven: the watermark check catches a read behind a write (replica said '${answer:-nothing}')"
  fi

  as_superuser replica-1 'SELECT pg_wal_replay_resume()' >/dev/null
}

# The upstream TLS assertion reads `pg_stat_ssl` and expects `t`. This is the
# same query through a node whose upstream mode is `disabled`, which must answer
# `f`: same statement, same database, same view, and the only difference is the
# thing being asserted. If the predicate cannot tell these two apart it was
# reading something other than what it claims to read.
#
# The two nodes are why this costs nothing to prove. pgprox-1 and pgprox-2 are
# left on plaintext deliberately, so the stack carries a control rather than
# needing one built.
prove_upstream_tls_check_catches_plaintext() {
  local answer
  answer="$(in_psql pgprox-1 \
    'SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()' | tr -d '[:space:]')"

  if [[ "$answer" == "t" ]]; then
    fail "a node with upstream TLS disabled reported an encrypted connection, so the check proves nothing"
  elif [[ "$answer" != "f" ]]; then
    fail "the control node answered neither t nor f: '${answer:-nothing}'"
  else
    ok "proven: the upstream TLS check tells an encrypted connection from a plaintext one"
  fi
}

# psql at a database as the superuser, for the things a tenant's role may not
# do. Only the negative tests need this.
as_superuser() {
  local host="$1" sql="$2"
  "${COMPOSE[@]}" exec -T -e PGPASSWORD=postgres client \
    psql --host "$host" --port 5432 --username postgres --dbname tenant_acme \
      --no-align --tuples-only --quiet -c "$sql" 2>&1
}

# psql straight at a database, with no proxy in the way.
direct() {
  local host="$1" sql="$2"
  "${COMPOSE[@]}" exec -T -e PGPASSWORD=acme-password client \
    psql --host "$host" --port 5432 --username acme_app --dbname tenant_acme \
      --no-align --tuples-only --quiet -c "$sql" 2>&1
}

# ---------------------------------------------------------------------------

# --- non-negotiable 7, end to end --------------------------------------------
#
# `M13.3` proved the one route `SecretString` leaves open is not taken through a
# formatting macro. That is static, and it cannot see a value exposed into a
# local and formatted three functions later, or anything a dependency prints.
#
# This is the claim itself: the fleet has just authenticated a real client with
# a real token and opened backend connections with a real password. Neither may
# appear in any node log. `M13.8`.
assert_no_credential_in_any_log() {
  local leaked=0 service line

  # The backend password the mock sidecar hands back, from the compose file, so
  # this does not drift if that value changes.
  local backend_password
  backend_password="$(grep -m1 'PGPROX_MOCK_PASSWORD' deploy/docker-compose.yml |
                        sed 's/.*PGPROX_MOCK_PASSWORD:[[:space:]]*//' | tr -d '"\r')"
  if [[ -z "$backend_password" ]]; then
    fail "could not read PGPROX_MOCK_PASSWORD from the compose file, so there is nothing to search for"
    return 1
  fi

  # A positive control, first. A search that finds nothing is worth nothing
  # until the search is known to find something, and this repo produced exactly
  # that failure one task earlier: M13.3's first lint reported the whole
  # workspace clean while its pattern matched no line at all.
  #
  # So: the same three greps, against a line that does contain each secret. If
  # any of them comes back clean here, the run below cannot be believed.
  local control="prefix $TOKEN and $backend_password suffix"
  if ! grep -qF -- "$TOKEN" <<< "$control" ||
     ! grep -qF -- 'not-a-signature' <<< "$control" ||
     ! grep -qF -- "$backend_password" <<< "$control"; then
    fail "the credential search does not find a credential it is given; a clean result would mean nothing"
    return 1
  fi

  # Every service the compose file defines, asked of compose rather than
  # listed here, so a node added later is searched without anyone remembering.
  local services
  services="$("${COMPOSE[@]}" config --services 2>/dev/null || true)"
  [[ -n "$services" ]] || { fail "could not list the stack services"; return 1; }

  for service in $services; do
    local logs
    logs="$("${COMPOSE[@]}" logs --no-color "$service" 2>&1 || true)"
    [[ -n "$logs" ]] || continue

    # The whole token, and its signature segment on its own. A log that prints a
    # truncated JWT still leaks the part that identifies the session.
    if grep -qF -- "$TOKEN" <<< "$logs"; then
      fail "$service logged the client token"
      leaked=1
    fi
    if grep -qF -- 'not-a-signature' <<< "$logs"; then
      fail "$service logged part of the client token"
      leaked=1
    fi
    if grep -qF -- "$backend_password" <<< "$logs"; then
      fail "$service logged the backend password"
      leaked=1
    fi
  done

  if (( leaked )); then
    printf '       AGENTS.md non-negotiable 7. The static half is\n'
    printf '       scripts/check-secrets.sh; this is the claim itself.\n'
    return 1
  fi

  ok "no node logged the client token or the backend password"
}

case "${1:-all}" in
  down)
    tear_down
    ok "the stack is down"
    finish
    ;;
  up)
    bring_up || { fail "see above"; finish; }
    finish
    ;;
  all | prove) ;;
  *)
    fail "usage: e2e.sh [up|down]"
    finish
    ;;
esac

trap tear_down EXIT

if bring_up; then
  if [[ "${1:-all}" == "prove" ]]; then
    prove_pgbench_check_catches_failures || true
    prove_drain_check_catches_losses || true
    prove_watermark_check_catches_a_stale_read || true
    prove_upstream_tls_check_catches_plaintext || true
  else
    assert_every_node_serves || true
    assert_upstream_tls_reaches_the_database || true
    assert_pgbench_is_clean || true
    assert_drain_loses_no_transactions || true
    assert_no_read_behind_the_watermark || true
    # Last, so it searches the logs of a fleet that has done all the work above
    # rather than of one that only just started.
    assert_no_credential_in_any_log || true
  fi
fi

finish
