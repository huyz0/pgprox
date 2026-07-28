#!/usr/bin/env bash
# psql over TLS against the proxy.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/_env.sh"

# sslmode=require rather than prefer: a client that quietly fell back to
# cleartext would report a successful connection and no cipher at all, which is
# the one answer this matrix must not produce by accident.
export PGPASSWORD="$PGPROX_TOKEN" PGSSLMODE=require PGCONNECT_TIMEOUT=15
out="$(psql "host=$PGPROX_HOST port=$PGPROX_PORT user=$PGPROX_USER dbname=$PGPROX_DB" -tAc 'SELECT 1')"
[[ "$out" == "1" ]] || { echo "psql: expected 1, got '$out'" >&2; exit 1; }
echo "psql: connected"
