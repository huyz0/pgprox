#!/usr/bin/env bash
# Shared by every driver script: builds and starts the conformance server,
# exports PGPROX_HARNESS_PORT, and stops it on exit.
#
# Sourced, not executed.
set -euo pipefail

CONFORMANCE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
HARNESS_PID=""

stop_harness() {
  [[ -n "$HARNESS_PID" ]] && kill "$HARNESS_PID" 2>/dev/null || true
  HARNESS_PID=""
}
trap stop_harness EXIT INT TERM

start_harness() {
  cargo build -q -p pgprox-proto --example conformance_server \
    --manifest-path "$CONFORMANCE_ROOT/Cargo.toml"

  local port_file
  port_file="$(mktemp)"
  "$CONFORMANCE_ROOT/target/debug/examples/conformance_server" >"$port_file" 2>/dev/null &
  HARNESS_PID=$!

  # The server prints its bound port as its first line.
  for _ in $(seq 1 50); do
    if [[ -s "$port_file" ]]; then
      PGPROX_HARNESS_PORT="$(head -1 "$port_file")"
      export PGPROX_HARNESS_PORT
      rm -f "$port_file"
      return 0
    fi
    sleep 0.1
  done

  rm -f "$port_file"
  echo "conformance server did not report a port" >&2
  return 1
}
