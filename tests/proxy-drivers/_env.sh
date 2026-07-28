#!/usr/bin/env bash
# Shared by every driver probe that runs against a live proxy.
#
# Each probe connects to a running proxy over TLS and drives it through the
# cases that separate a driver from a demo: both wire protocols, a prepared
# statement reused on one session, a result larger than one segment, a
# transaction, and an error with a statement after it.
#
# Two scripts use them for different questions. `cipher-matrix.sh` cares only
# that the handshake completed, and reads the suite from the proxy's log
# because only two of the five drivers will say what they negotiated.
# `driver-matrix.sh` cares about everything after the handshake, which is the
# part that had never been run against the proxy at all: five drivers have met
# `conformance_server` since M1 and the real thing never, and the one time one
# of them was pointed at the proxy it deadlocked on its first parameterised
# query.
#
# Sourced, not executed.
set -euo pipefail

: "${PGPROX_HOST:=127.0.0.1}"
: "${PGPROX_PORT:?PGPROX_PORT is required}"
: "${PGPROX_USER:=acme_app}"
: "${PGPROX_DB:=tenant_acme}"
: "${PGPROX_TOKEN:?PGPROX_TOKEN is required}"

# Every probe skips certificate verification. The stack generates a self-signed
# certificate per node at start, so there is nothing to verify against, and
# what is under test is the protocol behind the handshake rather than whether a
# name matches. Same reason bin/pgload has an insecure verifier and the same
# warning applies: it belongs in a test client and nowhere else.
PROBE_WORK="${PROBE_WORK:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/target/proxy-drivers}"
mkdir -p "$PROBE_WORK"
