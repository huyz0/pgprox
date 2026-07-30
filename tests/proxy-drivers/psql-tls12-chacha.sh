#!/usr/bin/env bash
# TLS 1.2 with ChaCha20-Poly1305, which FIPS mode drops. The test: the default
# build should take this and the FIPS build should refuse it. If both take it,
# the claim `scripts/cipher-matrix.sh` is written around is wrong.
set -euo pipefail
TLS12_CIPHER=ECDHE-RSA-CHACHA20-POLY1305 \
  exec "$(dirname "${BASH_SOURCE[0]}")/_tls12.sh"
