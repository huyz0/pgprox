#!/usr/bin/env bash
# Which protocol messages the conformance suite actually exercised.
#
# "We decode it" and "we tested it" are different claims, and only the second is
# worth anything. This turns the difference into a pass or a fail instead of
# something a human has to notice during review.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

MAJOR="${1:-18}"
LOG="$(mktemp)"
CONTAINER="pgprox-msgcov-$$"

cleanup() {
  docker rm -f "$CONTAINER" >/dev/null 2>&1
  rm -f "$LOG"
}
trap cleanup EXIT INT TERM

if ! docker info >/dev/null 2>&1; then
  fail "docker daemon unreachable"
  finish
fi

docker run -d --rm --name "$CONTAINER" \
  -e POSTGRES_HOST_AUTH_METHOD=trust \
  -e POSTGRES_DB=conformance \
  -P "postgres:$MAJOR-alpine" >/dev/null || { fail "could not start Postgres"; finish; }

PG_PORT="$(docker port "$CONTAINER" 5432/tcp | head -1 | sed 's/.*://')"

PGPROX_TAG_LOG="$LOG" PGPROX_PG_MAJOR="$MAJOR" PGPROX_PG_PORT="$PG_PORT" \
  cargo nextest run -p pgprox-proto --features integration \
    --run-ignored all -E 'test(conformance_client)' >/dev/null 2>&1 \
  || { fail "the conformance suite did not pass, so its coverage means nothing"; finish; }

if [[ ! -s "$LOG" ]]; then
  fail "no tags recorded; the instrumentation is not wired up"
  finish
fi

seen_backend="$(awk '$1=="backend"{print $2}' "$LOG" | sort -un | tr '\n' ' ')"
seen_frontend="$(awk '$1=="frontend"{print $2}' "$LOG" | sort -un | tr '\n' ' ')"

# Messages with a decoder that the suite must actually have driven. Listed by
# byte so this file does not have to parse Rust.
#
# Deliberately absent: Authentication and BackendKeyData appear only in the
# startup path the helper does not route through record_tag, and COPY_BOTH is
# replication, which ADR 0015 passes through rather than exercises.
declare -A BACKEND_REQUIRED=(
  [90]="ReadyForQuery"      [67]="CommandComplete"  [73]="EmptyQueryResponse"
  [69]="ErrorResponse"      [83]="ParameterStatus"  [65]="NotificationResponse"
  [68]="DataRow"            [84]="RowDescription"   [71]="CopyInResponse"
  [72]="CopyOutResponse"    [99]="CopyDone"         [100]="CopyData"
  [49]="ParseComplete"      [50]="BindComplete"     [116]="ParameterDescription"
)
declare -A FRONTEND_REQUIRED=(
  [81]="Query"   [80]="Parse"    [66]="Bind"     [69]="Execute"
  [83]="Sync"    [100]="CopyData" [99]="CopyDone"
)

check() {
  local -n required=$1
  local seen="$2"
  local side="$3"
  local missing=0
  for code in "${!required[@]}"; do
    if ! grep -qw "$code" <<< "$seen"; then
      fail "$side ${required[$code]} has a decoder but the suite never saw one"
      missing=1
    fi
  done
  (( missing == 0 )) && ok "$side: every required message was exercised"
}

check BACKEND_REQUIRED "$seen_backend" "backend"
check FRONTEND_REQUIRED "$seen_frontend" "frontend"

echo
echo "backend tags seen:  $(awk '$1=="backend"{print $2}' "$LOG" | sort -un | wc -l)"
echo "frontend tags seen: $(awk '$1=="frontend"{print $2}' "$LOG" | sort -un | wc -l)"

finish
