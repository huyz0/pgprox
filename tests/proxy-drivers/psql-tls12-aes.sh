#!/usr/bin/env bash
# TLS 1.2 with an AES-GCM suite, which FIPS mode approves. The control: if this
# is refused by the FIPS build then the build is broken rather than restrictive.
set -euo pipefail
TLS12_CIPHER=ECDHE-RSA-AES256-GCM-SHA384 \
  exec "$(dirname "${BASH_SOURCE[0]}")/_tls12.sh"
