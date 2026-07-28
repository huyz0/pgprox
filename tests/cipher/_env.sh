#!/usr/bin/env bash
# Shared by every cipher-matrix driver probe.
#
# Each probe does one thing: complete a TLS handshake with the proxy and run
# one statement. What suite it negotiated is read from the proxy's log rather
# than from the driver, because only two of the five drivers will tell you, and
# a matrix with three blanks in it answers the FIPS compatibility question
# badly. The server knows for all five.
#
# Sourced, not executed.
set -euo pipefail

: "${PGPROX_HOST:=127.0.0.1}"
: "${PGPROX_PORT:?PGPROX_PORT is required}"
: "${PGPROX_USER:=acme_app}"
: "${PGPROX_DB:=tenant_acme}"
: "${PGPROX_TOKEN:?PGPROX_TOKEN is required}"

# Every probe skips certificate verification. The stack generates a self-signed
# certificate per node at start, so there is nothing to verify against, and the
# property under test is which cipher suite the two sides agree on rather than
# whether a name matches. This is the same reason bin/pgload has an insecure
# verifier and the same warning applies: it belongs in a test client and
# nowhere else.
CIPHER_WORK="${CIPHER_WORK:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/target/cipher}"
mkdir -p "$CIPHER_WORK"
