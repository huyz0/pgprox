#!/usr/bin/env sh
# One certificate authority and one server certificate, made at stack start and
# left in a volume the databases and one proxy node share.
#
# # Why a CA at all, when every other certificate here is self-signed
#
# The proxy nodes make their own self-signed certificates because the property
# their clients test is that TLS is terminated, and psql is told
# `PGSSLMODE=require`, which encrypts without verifying. The proxy's *upstream*
# side has no such mode. `TlsMode::Verified` is the only mode it has and there
# is deliberately no "verify nothing" variant, so proving that path works needs
# a chain that actually verifies: a self-signed server certificate would fail
# with `CaUsedAsEndEntity` and prove only that the failure path works.
#
# # One certificate for three hosts
#
# The replicas are `pg_basebackup` clones of the primary and inherit whatever it
# has, so the leaf names all three. The alternative is three certificates, three
# copies of this and a replica whose certificate says `primary`.
#
# # This is a test CA and it is not a secret
#
# The key is written world-readable on purpose. It lives for the length of a
# compose stack, it is generated fresh every time, and nothing outside this
# network trusts it. `deploy/primary/tls.sh` says why the file the server opens
# is a copy of this one rather than this one.
set -eu

OUT=/upstream-tls
DAYS=1

# Idempotent, so `docker compose up` on a stack that is already part-way up does
# not hand the databases a certificate their running peers do not trust.
if [ -s "$OUT/ca.crt" ] && [ -s "$OUT/server.crt" ] && [ -s "$OUT/server.key" ]; then
  echo "upstream-tls: certificates already present, leaving them alone"
  exit 0
fi

openssl req -x509 -newkey rsa:2048 -nodes -days "$DAYS" \
  -subj "/CN=pgprox-e2e-upstream-ca" \
  -keyout "$OUT/ca.key" -out "$OUT/ca.crt" >/dev/null 2>&1

openssl req -newkey rsa:2048 -nodes \
  -subj "/CN=primary" \
  -keyout "$OUT/server.key" -out "$OUT/server.csr" >/dev/null 2>&1

# `basicConstraints=CA:FALSE` explicitly: rustls refuses a certificate that is
# both the authority and the leaf, which is exactly what a self-signed one is
# and exactly the failure this service exists to avoid.
cat > "$OUT/leaf.ext" <<EXT
subjectAltName = DNS:primary, DNS:replica-1, DNS:replica-2
basicConstraints = CA:FALSE
extendedKeyUsage = serverAuth
EXT

openssl x509 -req -in "$OUT/server.csr" \
  -CA "$OUT/ca.crt" -CAkey "$OUT/ca.key" -CAcreateserial \
  -out "$OUT/server.crt" -days "$DAYS" -extfile "$OUT/leaf.ext" >/dev/null 2>&1

chmod 644 "$OUT/ca.crt" "$OUT/server.crt" "$OUT/server.key"

echo "upstream-tls: a CA and a leaf for primary, replica-1 and replica-2"
