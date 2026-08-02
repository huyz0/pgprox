#!/usr/bin/env bash
# psql against the proxy.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/_env.sh"

# sslmode=require rather than prefer: the node under test requires TLS, and a
# client that quietly fell back to cleartext would be a client whose token
# crossed the network in the clear.
export PGPASSWORD="$PGPROX_TOKEN" PGSSLMODE=require PGCONNECT_TIMEOUT=15
CONN="host=$PGPROX_HOST port=$PGPROX_PORT user=$PGPROX_USER dbname=$PGPROX_DB"

out="$(psql "$CONN" -tAc 'SELECT 1')"
[[ "$out" == "1" ]] || { echo "psql: simple query gave '$out'" >&2; exit 1; }

# The extended protocol, which is where a proxy that maps statement names
# wrongly stops working. Through `\bind` and stdin rather than `-c`: psql
# reads a `-c` string as SQL unless it begins with a backslash, so the
# meta-command has to arrive on the input stream.
#
# The conformance suite gets away with `psql -c 'SELECT $1'` because the
# harness answers it with a canned row. Real Postgres says "there is no
# parameter $1", correctly, and that difference is the reason this matrix
# exists.
out="$(printf 'SELECT $1::int + 1 \\bind 41 \\g\n' | psql "$CONN" -tA)"
[[ "$out" == "42" ]] || { echo "psql: extended query gave '$out'" >&2; exit 1; }

# PGPROX_DEPTH_PREPARED_REUSE. One session, one statement name, five uses.
out="$(printf 'PREPARE p(int) AS SELECT $1;\nEXECUTE p(7);\nEXECUTE p(7);\nEXECUTE p(7);\nEXECUTE p(7);\nEXECUTE p(7);\n' \
  | psql "$CONN" -tAq | tr -d '\n')"
[[ "$out" == "77777" ]] || { echo "psql: prepared reuse gave '$out'" >&2; exit 1; }

# PGPROX_DEPTH_STARTUP_SETTINGS. `M20.2` and `M20.7`: what a client asks for in
# its connection string. libpq packs `-c name=value` into the startup packet's
# `options` and sends `application_name` as a parameter of its own, and this
# proxy parsed both, stored them, and read them nowhere.
#
# psql is the right driver for these because what is under test is the packet
# libpq builds rather than anything a driver does with it, and every other
# driver here either uses libpq or rebuilds the same fields.
#
# `pg_catalog` rather than a schema this stack creates: it exists on any
# Postgres and is not what the default resolves to, so this answer cannot be
# the server agreeing by accident.
out="$(PGOPTIONS='-c search_path=pg_catalog' psql "$CONN" -tAc 'SHOW search_path')"
[[ "$out" == "pg_catalog" ]] || {
  echo "psql: search_path from options gave '$out'" >&2; exit 1; }

# The client's own name wins. `pgprox` here is `M20.7` having been reverted:
# the proxy sets that on upstream connections for a DBA's benefit, and a
# connection actively serving a tenant is supposed to show the tenant's.
out="$(PGAPPNAME='pgprox_driver_probe' psql "$CONN" -tAc 'SHOW application_name')"
[[ "$out" == "pgprox_driver_probe" ]] || {
  echo "psql: application_name from the startup packet gave '$out'" >&2; exit 1; }

# PGPROX_DEPTH_REFUSED_AT_CONNECT. `M20.8`. A replication connection is a
# session by definition and this proxy cannot serve one, so it says so at
# connect rather than letting the client find out when IDENTIFY_SYSTEM fails.
#
# Matched on the message rather than only on the failure, because a refusal for
# the wrong reason looks identical from out here: a stack that is down, a token
# that expired and a feature that is not offered all end as "connection
# failed".
refusal="$(psql "$CONN sslmode=require replication=database" -tAc 'IDENTIFY_SYSTEM' 2>&1 || true)"
grep -q "replication connections are not proxied" <<<"$refusal" || {
  echo "psql: a replication connection was not refused by name: $refusal" >&2; exit 1; }

# And an ordinary connection still works after it, which is the half a refusal
# case can quietly break: a proxy refusing everything would pass the assertion
# above.
out="$(psql "$CONN" -tAc 'SELECT 1')"
[[ "$out" == "1" ]] || { echo "psql: refused an ordinary connection too" >&2; exit 1; }

# PGPROX_DEPTH_LARGE_RESULT: more rows than fit in one segment.
rows="$(psql "$CONN" -tAc 'SELECT generate_series(1, 5000)' | wc -l)"
(( rows == 5000 )) || { echo "psql: large result gave $rows rows" >&2; exit 1; }

# A transaction, which is what the pool releases on.
out="$(psql "$CONN" -tAqc 'BEGIN; SELECT 2; COMMIT;' | tr -d '\n')"
[[ "$out" == "2" ]] || { echo "psql: transaction gave '$out'" >&2; exit 1; }

# An error, and a statement after it. A session the proxy left mid-transaction
# would fail the second one.
psql "$CONN" -tAc 'SELECT no_such_column_xyz' >/dev/null 2>&1 \
  && { echo "psql: a bad column succeeded" >&2; exit 1; }
out="$(psql "$CONN" -tAc 'SELECT 3')"
[[ "$out" == "3" ]] || { echo "psql: after an error gave '$out'" >&2; exit 1; }

echo "psql: ok"
