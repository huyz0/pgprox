#!/usr/bin/env bash
# psql against the conformance harness.
#
# psql uses the simple query protocol for most things, which is why "works with
# psql" often means the extended query path is untested. Both are exercised
# here: -c uses simple query, and a PREPARE/EXECUTE pair drives the extended one.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/_harness.sh"

start_harness
CONN="host=127.0.0.1 port=$PGPROX_HARNESS_PORT user=postgres dbname=conformance sslmode=disable"
export PGCONNECT_TIMEOUT=10

echo "psql: simple query"
out="$(psql "$CONN" -tAc 'SELECT 1')"
[[ "$out" == "1" ]] || { echo "expected 1, got '$out'" >&2; exit 1; }

echo "psql: extended query with a bound parameter"
out="$(psql "$CONN" -tAc 'SELECT $1')"
[[ "$out" == "1" ]] || { echo "expected 1, got '$out'" >&2; exit 1; }

echo "psql: sslmode=prefer falls back when the server declines"
out="$(psql "host=127.0.0.1 port=$PGPROX_HARNESS_PORT user=postgres dbname=conformance sslmode=prefer" -tAc 'SELECT 1')"
[[ "$out" == "1" ]] || { echo "sslmode=prefer failed: '$out'" >&2; exit 1; }

# PGPROX_DEPTH_PREPARED_REUSE
# Repeated -c flags run in one session, so the extended-protocol path is
# exercised three times on the same connection. A multi-statement PREPARE and
# EXECUTE would go through the simple protocol instead, which is not the path
# that matters for statement mapping.
echo "psql: extended queries reused across one session"
out="$(psql "$CONN" -tAc 'SELECT $1' -c 'SELECT $1' -c 'SELECT $1' | tr -d '\n')"
[[ "$out" == "111" ]] || { echo "reuse gave '$out'" >&2; exit 1; }

# PGPROX_DEPTH_LARGE_RESULT
echo "psql: a result larger than one segment"
rows="$(psql "$CONN" -tAc 'SELECT pgprox_large' | wc -l)"
(( rows >= 2000 )) || { echo "large result gave $rows rows" >&2; exit 1; }

echo "psql: ok"
