#!/usr/bin/env bash
# A one-node stack on this machine: a Postgres, a mock sidecar, and a proxy.
#
# Sourced by scripts/profile.sh, and usable on its own:
#
#   source scripts/localstack.sh
#   local_up          # brings it up, exports LOCAL_PROXY and LOCAL_DIRECT
#   local_down        # stops everything and removes the data directory
#
# # Why this exists next to deploy/
#
# The compose stack is the deployment shape and is what `scripts/e2e.sh` and
# `scripts/scale.sh` measure. It needs Docker. This needs a `postgres` on the
# machine, which is a different dependency, and it exists so that a profile or
# a measurement is possible when one of the two is unavailable. It is
# deliberately one node: gossip, quota and cancellation across nodes are the
# compose stack's business, and a profile of the relay loop does not need them.
#
# Nothing here is a substitute for the e2e run. It has no replica, so it cannot
# say anything about watermarks or replica routing.

: "${REPO_ROOT:?localstack.sh expects lib.sh to have been sourced}"

LOCAL_DIR="${LOCAL_DIR:-$REPO_ROOT/target/localstack}"

# Low ports on purpose. Every port in the fifty-five-thousands on this machine
# refuses a bind while looking free to anything that only tries to connect,
# which is what WSL's reserved ranges do; the failure reads as "another
# postmaster is already running" and there is none. The defaults are
# overridable, which is how a second stack runs beside a first.
# Deliberately not 15432 or 16432: `deploy/docker-compose.scale.yml` publishes
# those, so a local stack on them cannot run while the compose stack is up, and
# the failure reads as "another postmaster is already running" with no hint of
# which one.
LOCAL_PG_PORT="${LOCAL_PG_PORT:-15532}"
LOCAL_PROXY_PORT="${LOCAL_PROXY_PORT:-16532}"
LOCAL_ADMIN_PORT="${LOCAL_ADMIN_PORT:-19190}"
LOCAL_GOSSIP_PORT="${LOCAL_GOSSIP_PORT:-16533}"

LOCAL_PROXY="127.0.0.1:$LOCAL_PROXY_PORT"
LOCAL_DIRECT="127.0.0.1:$LOCAL_PG_PORT"
export LOCAL_PROXY LOCAL_DIRECT

# The same token shape the e2e run uses: an approved algorithm in the header,
# no valid signature, which the mock sidecar accepts and the proxy's algorithm
# check passes.
LOCAL_TOKEN="$(printf '%s' '{"alg":"RS256","typ":"JWT"}' | base64 -w0 | tr '+/' '-_' | tr -d '=')"
LOCAL_TOKEN="${LOCAL_TOKEN}.$(printf '%s' '{"sub":"acme"}' | base64 -w0 | tr '+/' '-_' | tr -d '=').not-a-signature"
export LOCAL_TOKEN

_pg_bin() {
  local dir
  for dir in /usr/lib/postgresql/*/bin; do
    [[ -x "$dir/initdb" ]] && { echo "$dir"; return 0; }
  done
  command -v initdb >/dev/null 2>&1 && { dirname "$(command -v initdb)"; return 0; }
  return 1
}

local_down() {
  local bin
  bin="$(_pg_bin || true)"
  [[ -f "$LOCAL_DIR/proxy.pid" ]] && kill "$(cat "$LOCAL_DIR/proxy.pid")" 2>/dev/null
  [[ -f "$LOCAL_DIR/sidecar.pid" ]] && kill "$(cat "$LOCAL_DIR/sidecar.pid")" 2>/dev/null
  if [[ -n "$bin" && -d "$LOCAL_DIR/pgdata" ]]; then
    "$bin/pg_ctl" --pgdata "$LOCAL_DIR/pgdata" --silent stop >/dev/null 2>&1
  fi
  rm -rf "$LOCAL_DIR"
}

# Brings up Postgres, the mock sidecar and the proxy, and waits for each.
#
# Set `LOCAL_PROXY_BIN` to run a binary this function did not build, which is
# how `scripts/profile.sh` runs an instrumented one. The proxy is exec'd
# directly rather than through `cargo run` on purpose: `cargo run` is the
# parent, so the recorded pid was cargo's, and a `SIGTERM` to it left the proxy
# running and its coverage counters unwritten.
local_up() {
  local bin
  bin="$(_pg_bin)" || { fail "no postgres on this machine"; return 1; }

  rm -rf "$LOCAL_DIR"
  mkdir -p "$LOCAL_DIR"

  echo "  starting postgres on $LOCAL_PG_PORT"
  # Trust, because this is a throwaway cluster on the loopback that exists for
  # the length of one measurement, and because the load client speaks no
  # password method: what is being measured is the proxy hop, not SCRAM.
  "$bin/initdb" --pgdata "$LOCAL_DIR/pgdata" --auth=trust --username postgres \
    >"$LOCAL_DIR/initdb.log" 2>&1 || { fail "initdb failed"; return 1; }

  "$bin/pg_ctl" --pgdata "$LOCAL_DIR/pgdata" --silent \
    --options "-p $LOCAL_PG_PORT -c listen_addresses=127.0.0.1 -c max_connections=200 -c unix_socket_directories=$LOCAL_DIR" \
    --log "$LOCAL_DIR/postgres.log" start \
    >/dev/null 2>&1 || { fail "postgres did not start"; return 1; }

  "$bin/psql" --host 127.0.0.1 --port "$LOCAL_PG_PORT" --username postgres --quiet \
    -c "CREATE ROLE acme_app WITH LOGIN PASSWORD 'acme-password'" \
    -c "CREATE ROLE pgload WITH LOGIN SUPERUSER" \
    -c "CREATE DATABASE tenant_acme OWNER acme_app" \
    >/dev/null 2>&1 || { fail "could not create the tenant"; return 1; }

  echo "  loading the workload's tables"
  PGPASSWORD=acme-password "$bin/pgbench" --host 127.0.0.1 --port "$LOCAL_PG_PORT" \
    --username acme_app --initialize --scale 1 --quiet tenant_acme \
    >"$LOCAL_DIR/pgbench-init.log" 2>&1 || { fail "pgbench could not initialise"; return 1; }

  echo "  starting the sidecar"
  cargo build --release -p pgprox-auth --features integration --bin mock_sidecar \
    >/dev/null 2>&1 \
    || { fail "mock_sidecar did not build"; return 1; }
  PGPROX_MOCK_PRIMARY="127.0.0.1:$LOCAL_PG_PORT" \
  PGPROX_MOCK_REPLICAS="" \
  PGPROX_MOCK_DATABASE=tenant_acme \
  PGPROX_MOCK_USER=acme_app \
  PGPROX_MOCK_PASSWORD=acme-password \
  PGPROX_MOCK_TLS=disabled \
    ./target/release/mock_sidecar "$LOCAL_DIR/sidecar.sock" >"$LOCAL_DIR/sidecar.log" 2>&1 &
  echo $! > "$LOCAL_DIR/sidecar.pid"

  local waited=0
  while [[ ! -S "$LOCAL_DIR/sidecar.sock" ]]; do
    sleep 0.2
    waited=$((waited + 1))
    (( waited > 100 )) && { fail "the sidecar never listened"; return 1; }
  done

  cat > "$LOCAL_DIR/config.yaml" <<CONFIG
# One node, and the same upstream cap the compose stack uses, so a number from
# here and a number from there are about the same multiplexing.
max_client_conns: 20000
drain_grace: 10s
grant_ttl_cap: 300s

servers:
  - server: 127.0.0.1:$LOCAL_PG_PORT
    max_connections: 60
    guaranteed_fraction: 1.0

nodes:
  pgprox-1: {}
CONFIG

  local proxy="${LOCAL_PROXY_BIN:-}"
  if [[ -z "$proxy" ]]; then
    cargo build --release --bin pgprox >/dev/null 2>&1 \
      || { fail "pgprox did not build"; return 1; }
    proxy="$REPO_ROOT/target/release/pgprox"
  fi

  echo "  starting the proxy on $LOCAL_PROXY_PORT"
  "$proxy" \
    --config "$LOCAL_DIR/config.yaml" \
    --sidecar "$LOCAL_DIR/sidecar.sock" \
    --listen "0.0.0.0:$LOCAL_PROXY_PORT" \
    --admin "127.0.0.1:$LOCAL_ADMIN_PORT" \
    --gossip "127.0.0.1:$LOCAL_GOSSIP_PORT" \
    --node 1 --node-name pgprox-1 \
    >"$LOCAL_DIR/proxy.log" 2>&1 &
  echo $! > "$LOCAL_DIR/proxy.pid"

  waited=0
  until curl --fail --silent "http://127.0.0.1:$LOCAL_ADMIN_PORT/readyz" >/dev/null 2>&1; do
    sleep 0.5
    waited=$((waited + 1))
    if (( waited > 240 )); then
      fail "the proxy never became ready"
      tail -20 "$LOCAL_DIR/proxy.log" | sed 's/^/  /'
      return 1
    fi
  done
  ok "the local stack is up (postgres $LOCAL_PG_PORT, proxy $LOCAL_PROXY_PORT)"
}
