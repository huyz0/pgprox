#!/usr/bin/env bash
# asyncpg over TLS against the proxy.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/_env.sh"

WORK="$CIPHER_WORK/asyncpg"
mkdir -p "$WORK"
cat > "$WORK/probe.py" <<'PY'
import asyncio
import os
import ssl
import sys

import asyncpg


async def main() -> None:
    # The stack's certificate is self-signed, so there is nothing to verify.
    # asyncpg defaults to verifying once ssl= is set, which would make this a
    # certificate test rather than a cipher one.
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
        value = await conn.fetchval("SELECT 1")
        if value != 1:
            print(f"asyncpg: expected 1, got {value}", file=sys.stderr)
            raise SystemExit(1)
    finally:
        await conn.close()
    print("asyncpg: connected")


asyncio.run(main())
PY

cd "$WORK"
uv run --quiet --with asyncpg python probe.py
