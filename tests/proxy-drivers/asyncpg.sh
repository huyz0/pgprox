#!/usr/bin/env bash
# asyncpg against the proxy.
#
# The driver that found M8.11: it prepares with Parse, Describe, Flush rather
# than with a Sync, and nothing else in the supported set does.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/_env.sh"

WORK="$PROBE_WORK/asyncpg"
mkdir -p "$WORK"
cat > "$WORK/probe.py" <<'PY'
import asyncio
import os
import ssl
import sys

import asyncpg

STEP = 20


def die(what: str) -> None:
    print(f"asyncpg: {what}", file=sys.stderr)
    raise SystemExit(1)


async def main() -> None:
    # The stack's certificate is self-signed, so there is nothing to verify.
    # asyncpg verifies once ssl= is set, which would make this a certificate
    # test rather than a protocol one.
    context = ssl.create_default_context()
    context.check_hostname = False
    context.verify_mode = ssl.CERT_NONE

    conn = await asyncpg.connect(
        host=os.environ["PGPROX_HOST"],
        port=int(os.environ["PGPROX_PORT"]),
        user=os.environ["PGPROX_USER"],
        password=os.environ["PGPROX_TOKEN"],
        database=os.environ["PGPROX_DB"],
        ssl=context,
        timeout=15,
    )
    try:
        # execute() is the simple protocol; everything below is the extended
        # one, and the difference is what M8.11 hung on.
        await asyncio.wait_for(conn.execute("SELECT 1"), STEP)

        if await asyncio.wait_for(conn.fetchval("SELECT $1::int + 1", 41), STEP) != 42:
            die("a bound parameter came back wrong")

        # PGPROX_DEPTH_PREPARED_REUSE.
        prepared = await asyncio.wait_for(conn.prepare("SELECT $1::int"), STEP)
        for _ in range(5):
            if await asyncio.wait_for(prepared.fetchval(7), STEP) != 7:
                die("a reused prepared statement came back wrong")

        # PGPROX_DEPTH_LARGE_RESULT.
        rows = await asyncio.wait_for(
            conn.fetch("SELECT generate_series(1, $1)", 5000), STEP
        )
        if len(rows) != 5000:
            die(f"large result gave {len(rows)} rows")

        async with conn.transaction():
            if await asyncio.wait_for(conn.fetchval("SELECT 2"), STEP) != 2:
                die("a statement inside a transaction came back wrong")

        # A statement returning nothing, which is answered by NoData rather
        # than RowDescription and is the other half of M8.11's counting.
        if await asyncio.wait_for(conn.fetch("SELECT 1 WHERE false"), STEP) != []:
            die("a no-rows result came back with rows")

        try:
            await asyncio.wait_for(conn.fetchval("SELECT no_such_column_xyz"), STEP)
        except asyncpg.PostgresError:
            pass
        else:
            die("a bad column succeeded")

        if await asyncio.wait_for(conn.fetchval("SELECT 3"), STEP) != 3:
            die("a statement after an error came back wrong")
    finally:
        await conn.close()

    print("asyncpg: ok")


asyncio.run(main())
PY

cd "$WORK"
uv run --quiet --with asyncpg python probe.py
