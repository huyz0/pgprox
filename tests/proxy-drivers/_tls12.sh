#!/usr/bin/env bash
# A probe that pins the handshake to TLS 1.2 and to one cipher suite.
#
# `M11.2` exists because every driver on the machine that generated the cipher
# matrix negotiated TLS 1.3, whose three suites are all FIPS-approved, so the
# restriction FIPS mode actually imposes was never reached. The restriction is
# on TLS 1.2: FIPS drops ChaCha20-Poly1305 and keeps ECDHE suites with AES-GCM.
#
# Pinning happens on the client rather than on the proxy. Giving the proxy a
# maximum-version knob would be adding a production surface so that a test could
# reach a state, and the state is reachable from outside: libpq uses OpenSSL,
# and OpenSSL reads `OPENSSL_CONF`.
#
# Sourced by the two probes beside it, which differ only in the suite they ask
# for. One is approved in FIPS mode and one is not, so the pair is the whole
# experiment: if both behave the same on both builds, the claim in
# `scripts/cipher-matrix.sh` is wrong.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/_env.sh"

# The caller sets this. No default: a probe that silently tested some other
# suite would produce a row that looks like a result.
: "${TLS12_CIPHER:?the suite to pin, e.g. ECDHE-RSA-CHACHA20-POLY1305}"

conf="$(mktemp)"
trap 'rm -f "$conf"' EXIT

# `MaxProtocol` and `MinProtocol` together, so the handshake cannot fall back up
# to 1.3 and quietly test nothing. `CipherString` is TLS 1.2 and below;
# `Ciphersuites` would be the 1.3 list and is deliberately left alone.
#
# `SECLEVEL=0` because the stack's certificate is a test certificate and the
# defaults on this machine reject it at the security level OpenSSL 3 ships. It
# lowers what the client will accept, never what the server will offer, so it
# cannot manufacture the difference this probe is looking for.
cat >"$conf" <<EOF
openssl_conf = default_conf

[default_conf]
ssl_conf = ssl_sect

[ssl_sect]
system_default = system_default_sect

[system_default_sect]
MinProtocol = TLSv1.2
MaxProtocol = TLSv1.2
CipherString = ${TLS12_CIPHER}:@SECLEVEL=0
EOF

export OPENSSL_CONF="$conf"
export PGPASSWORD="$PGPROX_TOKEN" PGSSLMODE=require PGCONNECT_TIMEOUT=15
CONN="host=$PGPROX_HOST port=$PGPROX_PORT user=$PGPROX_USER dbname=$PGPROX_DB"

# One statement. This probe is about the handshake; what the proxy does after it
# is what every other probe in this directory already covers.
out="$(psql "$CONN" -tAc 'SELECT 1')"
[[ "$out" == "1" ]] || { echo "tls12($TLS12_CIPHER): query gave '$out'" >&2; exit 1; }

echo "tls12($TLS12_CIPHER): ok"
