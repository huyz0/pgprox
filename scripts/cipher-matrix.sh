#!/usr/bin/env bash
# Which cipher suite each supported driver negotiates, in each build.
#
# FIPS mode drops ChaCha20-Poly1305 and restricts TLS 1.2 to ECDHE suites with
# extended master secret. The question that matters before committing to FIPS
# in production is not what the provider offers, it is which client stops
# working, and the only way to learn that is to make each client connect.
#
#   scripts/cipher-matrix.sh
#
# Both nodes live in one stack against one Postgres, so a difference between
# them has one cause. The suite is read from the proxy's log rather than from
# the driver, because only some drivers will tell you and the server knows for
# all of them.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

COMPOSE=(docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.fips.yml)
OUT="${OUT:-product/release/cipher-matrix.md}"
DRIVERS=(psql pgx asyncpg jdbc npgsql psql-tls12-aes psql-tls12-chacha)

# Named targets: a published port on the host, and the compose service whose
# log carries the handshake line.
declare -A PORT=([default]=16433 [fips]=16435)
declare -A SERVICE=([default]=pgprox-2 [fips]=pgprox-fips)
BUILDS=(default fips)

# The same well-formed, unsigned token the e2e run uses. The proxy checks the
# algorithm and never verifies the signature; the mock sidecar accepts any
# token it is not told to refuse.
TOKEN="$(printf '%s' '{"alg":"RS256","typ":"JWT"}' | base64 -w0 | tr '+/' '-_' | tr -d '=')"
TOKEN="${TOKEN}.$(printf '%s' '{"sub":"acme"}' | base64 -w0 | tr '+/' '-_' | tr -d '=').not-a-signature"

driver_available() {
  case "$1" in
    # The two TLS 1.2 probes are psql with `OPENSSL_CONF` pinning the version
    # and the suite. `M11.2`: every driver here negotiates TLS 1.3, whose suites
    # are all FIPS-approved, so without these two the matrix never reaches the
    # restriction FIPS mode actually imposes.
    psql|psql-tls12-*) have psql ;;
    pgx)     have go ;;
    asyncpg) have python3 && have uv ;;
    jdbc)    have java && have mvn ;;
    npgsql)  have dotnet ;;
    *)       return 1 ;;
  esac
}

echo "cipher-suite matrix"
echo

if ! docker info >/dev/null 2>&1; then
  fail "docker daemon unreachable"
  finish
fi

echo "bringing up the stack (this builds the FIPS image on a cold cache)"
"${COMPOSE[@]}" up -d --build >/dev/null 2>&1 || {
  fail "the stack did not come up: ${COMPOSE[*]} up -d --build"
  finish
}

for build in "${BUILDS[@]}"; do
  service="${SERVICE[$build]}"
  for _ in $(seq 1 60); do
    state="$("${COMPOSE[@]}" ps --format '{{.Health}}' "$service" 2>/dev/null | head -1)"
    [[ "$state" == "healthy" ]] && break
    sleep 2
  done
  if [[ "$state" == "healthy" ]]; then
    ok "$build node is up ($service)"
  else
    fail "$build node never became healthy ($service, last state: ${state:-unknown})"
  fi
done

if (( _fail_count > 0 )); then
  finish
fi

# Confirm the two nodes really are the two builds. Comparing them is the whole
# point, and a run where both were the default build would produce a matrix
# that looked fine and said nothing.
for build in "${BUILDS[@]}"; do
  want="aws-lc-rs"; [[ "$build" == fips ]] && want="aws-lc-rs-fips"
  got="$("${COMPOSE[@]}" logs "${SERVICE[$build]}" 2>/dev/null \
    | grep -o '"crypto":"[^"]*"' | tail -1 | cut -d'"' -f4)"
  if [[ "$got" == "$want" ]]; then
    ok "$build node reports crypto=$got"
  else
    fail "$build node reports crypto=${got:-nothing}, expected $want"
  fi
done

if (( _fail_count > 0 )); then
  finish
fi

# --- the probes ---------------------------------------------------------------
declare -A RESULT SUITE PROTOCOL
declare -a SKIPPED=()

for driver in "${DRIVERS[@]}"; do
  if ! driver_available "$driver"; then
    SKIPPED+=("$driver")
    skip "driver $driver (toolchain not installed)"
    continue
  fi
  for build in "${BUILDS[@]}"; do
    service="${SERVICE[$build]}"
    # A marker in the log, so the handshake line this probe produced can be
    # told from the ones the health check and the previous driver produced.
    since="$(date -u +%Y-%m-%dT%H:%M:%S)"
    sleep 1

    # Bounded, because a probe that hangs is a run that never finishes. The
    # budget is generous: the first invocation of a driver fetches its
    # toolchain's dependencies, which for asyncpg means building a C extension
    # and for npgsql means restoring a package graph.
    if timeout "${PROBE_TIMEOUT:-600}" env \
       PGPROX_HOST=127.0.0.1 PGPROX_PORT="${PORT[$build]}" \
       PGPROX_USER=acme_app PGPROX_DB=tenant_acme PGPROX_TOKEN="$TOKEN" \
       "tests/proxy-drivers/$driver.sh" >/dev/null 2>&1; then
      RESULT["$driver,$build"]=connected
    else
      RESULT["$driver,$build"]=refused
    fi

    line="$("${COMPOSE[@]}" logs --since "$since" "$service" 2>/dev/null \
      | grep '"message":"tls handshake"' | tail -1)"
    SUITE["$driver,$build"]="$(grep -o '"cipher":"[^"]*"' <<<"$line" | cut -d'"' -f4)"
    PROTOCOL["$driver,$build"]="$(grep -o '"protocol":"[^"]*"' <<<"$line" | cut -d'"' -f4)"

    if [[ "${RESULT["$driver,$build"]}" == connected ]]; then
      ok "$driver on $build: ${SUITE["$driver,$build"]:-suite not logged}"
    else
      # Not a failure of this script. A driver that cannot speak to a FIPS
      # node is the answer the matrix exists to record.
      warn "$driver on $build: refused"
    fi
  done
done

# --- what the TLS 1.2 pair has to show ----------------------------------------
#
# Every other row here is a record: whatever the driver did is the answer. These
# two are an experiment with a stated expectation, and without asserting it a
# probe broken by its own OpenSSL config would produce "refused on both", which
# reads as a difference and is not one.
#
# The AES probe is the control. FIPS mode approves ECDHE with AES-GCM, so a
# refusal there means the FIPS build is broken rather than restrictive, and the
# ChaCha row below it would mean nothing.
if driver_available psql-tls12-aes; then
  if [[ "${RESULT[psql-tls12-aes,default]}" == connected \
     && "${RESULT[psql-tls12-aes,fips]}" == connected ]]; then
    ok "TLS 1.2 with AES-GCM is taken by both builds"
  else
    fail "TLS 1.2 with AES-GCM was refused (default: ${RESULT[psql-tls12-aes,default]}, fips: ${RESULT[psql-tls12-aes,fips]}); the probe or the FIPS build is wrong, not the policy"
  fi

  # The test itself. Refused on both would mean the probe never reached the
  # server's policy; taken by both would mean the restriction this matrix is
  # written around does not exist.
  d="${RESULT[psql-tls12-chacha,default]}"
  f="${RESULT[psql-tls12-chacha,fips]}"
  if [[ "$d" == connected && "$f" == refused ]]; then
    ok "TLS 1.2 with ChaCha20-Poly1305 is taken by the default build and refused by FIPS"
  elif [[ "$d" == refused ]]; then
    fail "TLS 1.2 with ChaCha20-Poly1305 was refused by the default build too: the probe did not reach the server's policy"
  else
    fail "TLS 1.2 with ChaCha20-Poly1305 was taken by the FIPS build: the restriction this matrix is written around does not hold"
  fi
fi

# --- the record ---------------------------------------------------------------
mkdir -p "$(dirname "$OUT")"
{
  echo "# Cipher-suite matrix"
  echo
  echo "Generated by \`scripts/cipher-matrix.sh\` on $(date -u +%Y-%m-%d)."
  echo
  echo "Each driver connects to two nodes in one stack, against one Postgres:"
  echo "a default build and a FIPS build. The suite is what the proxy logged"
  echo "for that handshake, not what the driver reported, because only some"
  echo "drivers will say and the server knows for all of them."
  echo
  echo "| Driver | Default build | FIPS build |"
  echo "| --- | --- | --- |"
  for driver in "${DRIVERS[@]}"; do
    cells=()
    for build in "${BUILDS[@]}"; do
      case "${RESULT["$driver,$build"]:-}" in
        connected)
          cells+=("${SUITE["$driver,$build"]:-connected, suite not logged}")
          ;;
        refused) cells+=("**refused**") ;;
        *)       cells+=("not run") ;;
      esac
    done
    echo "| $driver | ${cells[0]} | ${cells[1]} |"
  done
  echo
  echo "Protocol versions negotiated:"
  echo
  echo "| Driver | Default build | FIPS build |"
  echo "| --- | --- | --- |"
  # Refusals are marked here as well as in the table above. Version negotiation
  # succeeds before suite negotiation fails, so a refused handshake still logs a
  # protocol, and a cell reading `TLSv1_2` beside a `**refused**` suite would say
  # the connection worked.
  for driver in "${DRIVERS[@]}"; do
    cells=()
    for build in "${BUILDS[@]}"; do
      if [[ "${RESULT["$driver,$build"]:-}" == refused ]]; then
        cells+=("**refused**")
      else
        cells+=("${PROTOCOL["$driver,$build"]:-not run}")
      fi
    done
    echo "| $driver | ${cells[0]} | ${cells[1]} |"
  done
  echo
  if (( ${#SKIPPED[@]} > 0 )); then
    echo "Not run, toolchain missing on the machine that generated this:"
    echo "${SKIPPED[*]}. A skipped driver is a gap, not a pass."
    echo
  fi
  echo "## What this does and does not say"
  echo
  echo "It says which drivers complete a handshake with each build and on what"
  echo "terms. It does not say anything about a driver pinned to an older TLS"
  echo "library than the one on this machine, which is the case a FIPS"
  echo "migration actually breaks. Treat a row here as a floor."
  echo
  # The interesting negative. plan.md expects FIPS to drop
  # ChaCha20-Poly1305 and restrict TLS 1.2 to ECDHE with extended master
  # secret. None of that is reachable if every client picks TLS 1.3, where
  # the three mandatory suites are FIPS-approved anyway, so a matrix with
  # no TLS 1.2 row in it has not tested the restriction it was written for.
  # Saying so is the difference between a clean result and a vacuous one.
  if printf '%s\n' "${PROTOCOL[@]}" | grep -qs 'TLSv1_2'; then
    echo "At least one driver negotiated TLS 1.2, which is where FIPS mode"
    echo "actually restricts the suite list. Those rows are the ones to read."
  else
    echo "Every driver on this machine negotiated TLS 1.3, on both builds. That"
    echo "is a clean result and a narrow one: TLS 1.3's suites are all"
    echo "FIPS-approved, so the restrictions FIPS mode imposes were never"
    echo "reached. \`plan.md\` expects FIPS to drop ChaCha20-Poly1305 and to"
    echo "restrict TLS 1.2 to ECDHE with extended master secret, and none of"
    echo "that was exercised here. The drivers that would meet it are older"
    echo "builds than the ones this machine has, which is exactly the"
    echo "population a FIPS migration breaks."
  fi
} > "$OUT"

ok "written: $OUT"

echo
echo "the stack is still up; stop it with:"
echo "  ${COMPOSE[*]} down -v"

finish
