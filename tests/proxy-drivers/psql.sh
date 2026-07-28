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
