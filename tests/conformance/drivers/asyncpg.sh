#!/usr/bin/env bash
# asyncpg against the conformance harness.
#
# asyncpg always uses the extended query protocol and named prepared statements,
# so it exercises the path psql mostly skips.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/_harness.sh"

start_harness

cat > "$CONFORMANCE_ROOT/target/asyncpg_check.py" <<'PY'
import asyncio, os, sys
import asyncpg

async def main() -> int:
    conn = await asyncpg.connect(
        host="127.0.0.1",
        port=int(os.environ["PGPROX_HARNESS_PORT"]),
        user="postgres",
        database="conformance",
        ssl=False,
        statement_cache_size=0,
    )
    try:
        # asyncpg prepares even a literal query, so this drives
        # Parse/Bind/Describe/Execute/Sync.
        value = await conn.fetchval("SELECT 1")
        if value != 1:
            print(f"expected 1, got {value!r}", file=sys.stderr)
            return 1

        # A second query on the same connection: the sequence must have closed
        # cleanly, or this hangs.
        again = await conn.fetchval("SELECT 1")
        if again != 1:
            print(f"second query returned {again!r}", file=sys.stderr)
            return 1
    finally:
        await conn.close()

    print("asyncpg: ok")
    return 0

sys.exit(asyncio.run(main()))
PY

uv run --quiet --with asyncpg python "$CONFORMANCE_ROOT/target/asyncpg_check.py"
