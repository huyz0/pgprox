#!/usr/bin/env bash
# What a fleet with no upstream capacity left tells the clients a dead node
# displaces. `M11.6`.
#
#   scripts/admission.sh            the run, on the compose stack
#   scripts/admission.sh --no-kill  the control: the same load, nobody killed
#   scripts/admission.sh --keep     leave the stack up afterwards
#
# `M8`'s rolling-upgrade rehearsal killed a node and lost 22 of 21,088
# transactions, and said in its own write-up that it does not cover a fleet
# already at its cap. `M11.3` then found that the mechanism that sentence named,
# shedding, is refused at the cap by design. What is actually untested there is
# admission, and this is the run for it.
#
# The question it answers is not "are clients refused". It is **which refusal
# they get**, because the pool has two and they send an operator to different
# places:
#
#   * `53300 too_many_connections`, when the pool is at its limit. The server
#     is full. Add upstream capacity.
#   * `57014 query_canceled`, when the pool has headroom and the wait merely
#     expired. This node is full. Add nodes.
#
# `Waiters::give_up` chooses between them from the pool's state at the instant
# the caller gives up, so the interesting failure is `57014` arriving while the
# fleet is genuinely full: an operator sent to buy nodes when the database is
# the wall.
#
# What the client cannot tell you, and why the node is sampled too. Both
# refusals reach a client as the same sentence: `ClientError::client_message`
# is deliberately vague so an untrusted client learns no upstream hostname and
# no cap. So the load client's report gives the code distribution, and
# `/v1/servers` and `/v1/pools` sampled across the kill give the state that
# produced it. Neither is the answer on its own.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

COMPOSE=(docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.admission.yml)
NODES=(pgprox-1 pgprox-2 pgprox-3)
# Chosen at the moment of the kill rather than named here. See `fattest_node`.
VICTIM=""

KEEP=""
KILL=1
for arg in "$@"; do
  case "$arg" in
    --keep) KEEP=1 ;;
    # The control arm. A saturated fleet that loses nobody refuses nobody, and
    # a run that did not establish that could not attribute anything to the
    # kill. Same load, same seed, same duration.
    --no-kill) KILL=0 ;;
    *) fail "unknown argument $arg"; finish ;;
  esac
done

# Enough clients that sixty upstream connections are all checked out and the
# rest are queued. The reference workload thinks for 50-500ms between
# transactions, so a connection is busy a low single-digit percentage of the
# time and saturation takes hundreds rather than tens: forty clients left
# forty-eight of the sixty free, and a thousand leave every node's pool at its
# limit with nothing idle and roughly three hundred callers queued behind each.
CONNECTIONS="${ADMISSION_CONNECTIONS:-1000}"
DURATION="${ADMISSION_DURATION:-120}"
# When the node dies, as a fraction of the run. Late enough that the fleet is
# saturated and its leases are settled, early enough that the displaced clients
# have most of the run to be told something.
KILL_AT="${ADMISSION_KILL_AT:-60}"
WORKLOAD="${ADMISSION_WORKLOAD:-/workload/workload.yaml}"
SEED="${ADMISSION_SEED:-11}"
# Twice the proxy's own `ACQUIRE_TIMEOUT`, which is 30 seconds.
#
# This is not a tuning knob, it is what makes the run answer its question. The
# load client's default connect timeout is also 30 seconds, so the first
# version of this run had client and server giving up in the same instant: 112
# clients recorded "startup did not finish within 30s" and not one recorded a
# SQLSTATE. A client that gives up when the server does measures its own
# patience. It has to outlast the server's deadline for the server's answer to
# exist.
CONNECT_TIMEOUT="${ADMISSION_CONNECT_TIMEOUT:-60}"

OUT_DIR="${ADMISSION_OUT_DIR:-$REPO_ROOT/target/admission}"
mkdir -p "$OUT_DIR"
# Named per arm, so a control run does not overwrite the run it is the control
# for. Both are wanted at the same time when the record is written.
ARM="kill"
(( KILL )) || ARM="control"
SAMPLES="$OUT_DIR/$ARM-samples.jsonl"
REPORT="$OUT_DIR/$ARM.json"

# The same well-formed, unsigned token the e2e run uses: the proxy checks the
# algorithm and never the signature, and the mock sidecar accepts what it is
# not told to refuse.
TOKEN="$(printf '%s' '{"alg":"RS256","typ":"JWT"}' | base64 -w0 | tr '+/' '-_' | tr -d '=')"
TOKEN="${TOKEN}.$(printf '%s' '{"sub":"acme"}' | base64 -w0 | tr '+/' '-_' | tr -d '=').not-a-signature"

# ---------------------------------------------------------------------------

bring_up() {
  echo "building the image"
  if ! "${COMPOSE[@]}" build >/dev/null 2>&1; then
    fail "the image did not build"
    "${COMPOSE[@]}" build 2>&1 | tail -20 | sed 's/^/  /'
    return 1
  fi

  echo "starting the stack"
  if ! "${COMPOSE[@]}" up --detach --wait --wait-timeout 240 >/dev/null 2>&1; then
    fail "the stack did not come up"
    "${COMPOSE[@]}" ps 2>&1 | sed 's/^/  /'
    return 1
  fi
  ok "the stack is up: 3 nodes behind the alias pgprox, a primary, 2 replicas"
}

tear_down() {
  [[ -n "$KEEP" ]] && { warn "the stack is still up (--keep)"; return; }
  "${COMPOSE[@]}" down --volumes --remove-orphans >/dev/null 2>&1 || true
}

# The workload's tables, created through the proxy so the tenant owns them.
prepare_data() {
  if ! "${COMPOSE[@]}" exec -T -e PGPASSWORD="$TOKEN" -e PGSSLMODE=require client \
      pgbench --host pgprox-1 --port 6432 --username acme_app \
        --initialize --scale 1 --quiet tenant_acme >/dev/null 2>&1; then
    fail "could not create the workload's tables"
    return 1
  fi
  ok "the workload's tables exist"
}

# One node's answer to an admin path, or an empty string if it is not there.
#
# A dead node is expected here rather than exceptional: half the point of the
# sampling is what the survivors say while the third is gone.
admin() {
  local node="$1" path="$2"
  "${COMPOSE[@]}" exec -T "$node" \
    curl --silent --max-time 2 "http://127.0.0.1:9090$path" 2>/dev/null || true
}

# How many connections the primary itself says the fleet is holding.
#
# The ground truth, and it is not the same question as `/v1/servers`. That view
# is assembled from what every node last gossiped, so a node that has just died
# is still counted in it while its sockets are already gone. The first version
# of this run watched the fleet report 81 connections against a cap of 60 after
# a kill, which is either a breached cap or a stale view, and only the database
# can say which.
primary_conns() {
  "${COMPOSE[@]}" exec -T primary \
    psql --username postgres --dbname tenant_acme --no-align --tuples-only --quiet \
      -c "SELECT count(*) FROM pg_stat_activity WHERE usename = 'acme_app'" 2>/dev/null \
    | tr -d '[:space:]' | grep -E '^[0-9]+$' || echo 0
}

# One line of JSON per sample: when, what happened, the database's own count,
# and every node's view of the quota, of its own pool, and of its clients.
#
# JSONL rather than a table because the interesting moment is a few seconds
# wide and the reading afterwards is a window rather than a glance.
sample() {
  local phase="$1" node servers pools stats
  local line="{\"t\":$(date +%s.%N),\"phase\":\"$phase\",\"primary_conns\":$(primary_conns),\"nodes\":{"
  local first=1
  for node in "${NODES[@]}"; do
    servers="$(admin "$node" /v1/servers)"
    pools="$(admin "$node" /v1/pools?scope=local)"
    stats="$(admin "$node" /v1/stats)"
    [[ -z "$servers" ]] && servers=null
    [[ -z "$pools" ]] && pools=null
    [[ -z "$stats" ]] && stats=null
    (( first )) || line+=","
    first=0
    line+="\"$node\":{\"servers\":$servers,\"pools\":$pools,\"stats\":$stats}"
  done
  echo "$line}}" >> "$SAMPLES"
}

# The fleet's view of the primary's cap, from one node, as `in_use headroom`.
quota_of() {
  local node="$1" body
  body="$(admin "$node" /v1/servers)"
  python3 - "$body" <<'PY' 2>/dev/null || echo "0 999"
import json, sys
try:
    rows = json.loads(sys.argv[1])
except Exception:
    print("0 999"); raise SystemExit
for row in rows:
    if row.get("server", "").startswith("primary"):
        print(row["in_use"], row["headroom"]); raise SystemExit
print("0 999")
PY
}

# How many callers are queued for a connection to the primary, fleet-wide.
#
# The other half of "saturated", and the half that matters. A quota with no
# headroom but nothing waiting is a fleet holding sixty idle connections, and a
# client displaced onto it would be served from one of them. The refusals this
# run is about only exist when callers are queueing.
waiting_now() {
  local node total=0 body count
  for node in "${NODES[@]}"; do
    body="$(admin "$node" '/v1/pools?scope=local')"
    count="$(python3 - "$body" <<'PY' 2>/dev/null || echo 0
import json, sys
try:
    rows = json.loads(sys.argv[1])
except Exception:
    print(0); raise SystemExit
print(sum(r["waiting"] for r in rows if r.get("server", "").startswith("primary")))
PY
)"
    total=$(( total + count ))
  done
  echo "$total"
}

# One client, arriving through the alias, timed, with whatever it was told.
#
# The load client answers this in aggregate and cannot answer it cleanly: its
# thousand connections are all reconnecting at once, so what any one of them
# records is as much about the queue behind it as about the node in front. This
# is one well-behaved client doing exactly what a displaced client does, at a
# known offset from the kill, and it is the direct form of the question.
#
# `PGCONNECT_TIMEOUT` is generous on purpose. A probe that gave up before the
# node answered would record its own patience, which is the mistake the first
# version of this run made with the load client.
probe() {
  local at="$1" start end answer status
  start="$(date +%s.%N)"
  answer="$("${COMPOSE[@]}" exec -T \
    -e PGPASSWORD="$TOKEN" -e PGSSLMODE=require -e PGCONNECT_TIMEOUT=120 client \
    psql --host pgprox --port 6432 --username acme_app --dbname tenant_acme \
      --no-align --tuples-only --quiet -c 'SELECT 1' 2>&1 | tr '\n' ' ')" || true
  end="$(date +%s.%N)"
  status=served
  [[ "$(tr -d '[:space:]' <<<"$answer")" == "1" ]] || status=refused
  printf '%s\t%s\t%s\t%s\n' "$at" "$status" \
    "$(python3 -c "print(f'{$end-$start:.2f}')")" "$answer" >> "$OUT_DIR/$ARM-probes.tsv"
}

# Which node is holding the most of the fleet's upstream capacity right now.
#
# The victim is chosen rather than named, because which node holds the lease is
# not a property of the deployment. Every node is guaranteed a third of half
# the cap, ten connections here, and the remaining thirty are leased to
# whichever node asks for them first. Two runs of this script put that lease in
# different places: 11/14/35 in one, 40/10/10 in another.
#
# Killing a node holding its guaranteed ten would be killing a sixth of the
# fleet's capacity and calling it a third. Killing the node holding the lease
# is the case `M8` said it had not covered, so it is the one to take, and the
# choice is recorded with the result rather than hidden in a default.
fattest_node() {
  local node body limit best="" best_limit=-1
  for node in "${NODES[@]}"; do
    body="$(admin "$node" '/v1/pools?scope=local')"
    limit="$(python3 - "$body" <<'PY' 2>/dev/null || echo 0
import json, sys
try:
    rows = json.loads(sys.argv[1])
except Exception:
    print(0); raise SystemExit
print(sum(r["limit"] for r in rows if r.get("server", "").startswith("primary")))
PY
)"
    if (( limit > best_limit )); then
      best_limit="$limit"
      best="$node"
    fi
  done
  echo "$best $best_limit"
}

# ---------------------------------------------------------------------------

run() {
  : > "$SAMPLES"
  : > "$OUT_DIR/$ARM-probes.tsv"

  echo "  starting $CONNECTIONS clients against the alias, for ${DURATION}s"
  "${COMPOSE[@]}" exec -T loadgen \
    pgload --target pgprox:6432 \
      --workload "$WORKLOAD" \
      --connections "$CONNECTIONS" \
      --duration "$DURATION" \
      --seed "$SEED" \
      --user acme_app \
      --database tenant_acme \
      --password "$TOKEN" \
      --tls-insecure \
      --connect-timeout "$CONNECT_TIMEOUT" \
      --out "/out/$ARM.json" >"$OUT_DIR/$ARM-pgload.log" 2>&1 &
  local client=$!

  # --- wait for the fleet to be full ----------------------------------------
  #
  # Asserted rather than assumed. A run that killed a node while the fleet had
  # headroom would answer a different question and look identical.
  local waited=0 in_use=0 headroom=999 queued=0
  while (( waited < KILL_AT )); do
    sleep 2
    waited=$(( waited + 2 ))
    sample saturating
    read -r in_use headroom < <(quota_of pgprox-1)
    queued="$(waiting_now)"
    (( headroom == 0 && queued > 0 )) && break
  done

  if (( headroom != 0 || queued == 0 )); then
    fail "the fleet never saturated: $in_use in use, $headroom to spare, $queued queued"
    fail "nothing below is about a full fleet; raise ADMISSION_CONNECTIONS and re-run"
    kill "$client" 2>/dev/null || true
    wait "$client" 2>/dev/null || true
    return 1
  fi
  ok "the fleet is saturated: $in_use of 60 in use, 0 headroom, $queued callers queued"

  # Settled: a cap reached in the first seconds of a ramp is a stampede, and
  # the leases behind it are still moving.
  sleep 6
  sample before-kill
  # The control for the probes below: what one client is told by a saturated
  # fleet that has lost nobody.
  probe "-1s"

  # --- the kill --------------------------------------------------------------
  local killed_at victim_limit
  read -r VICTIM victim_limit < <(fattest_node)
  killed_at="$(date +%s.%N)"
  if (( KILL )); then
    "${COMPOSE[@]}" kill "$VICTIM" >/dev/null 2>&1
    ok "$VICTIM killed outright, holding $victim_limit of the fleet's 60 upstream connections"
  else
    ok "control arm: nobody killed ($VICTIM holds $victim_limit of 60 and keeps them)"
  fi
  echo "$VICTIM $victim_limit" > "$OUT_DIR/$ARM-victim"
  sample after-kill

  # The window the answer is in. Sampled tightly, because the leases the dead
  # node held are reclaimed on a timer and what the survivors tell clients
  # before that happens is the question.
  local ticks=0
  while (( ticks < 30 )); do
    sleep 1
    ticks=$(( ticks + 1 ))
    sample displaced
    # A handful of moments across the window rather than every second: each
    # probe opens a real connection, and a probe per second would be a second
    # load generator sampling its own effect.
    case "$ticks" in
      2 | 5 | 10 | 20 | 29) probe "+${ticks}s" ;;
    esac
  done

  # --- and the rest of the run ----------------------------------------------
  while kill -0 "$client" 2>/dev/null; do
    sleep 3
    sample recovering
  done
  wait "$client" || true

  # `/out` in the container is `target/admission` here, so the report is
  # already at `$REPORT` and there is nothing to copy back.
  [[ -s "$REPORT" ]] || {
    fail "the load client wrote no report"
    tail -20 "$OUT_DIR/$ARM-pgload.log" | sed 's/^/  /'
    return 1
  }

  # --- what the survivors logged --------------------------------------------
  #
  # The one thing that separates the two `53300`s, and it is not on the wire.
  # A node refusing at its own client ceiling logs it at warn; a pool refusing
  # at the upstream cap does not.
  local ceiling=0 node
  for node in "${NODES[@]}"; do
    [[ "$node" == "$VICTIM" ]] && continue
    local count
    count="$("${COMPOSE[@]}" logs "$node" 2>/dev/null | grep -c 'at the connection ceiling' || true)"
    ceiling=$(( ceiling + count ))
  done
  echo "$ceiling" > "$OUT_DIR/$ARM-ceiling-refusals"

  # Kept, because a stack is torn down at the end of the run and the reason a
  # client waited is in here rather than in any counter.
  for node in "${NODES[@]}"; do
    "${COMPOSE[@]}" logs "$node" > "$OUT_DIR/$ARM-$node.log" 2>&1 || true
  done

  report_run "$killed_at" "$ceiling"
}

report_run() {
  local killed_at="$1" ceiling="$2"

  python3 - "$REPORT" "$SAMPLES" "$killed_at" "$ceiling" "$OUT_DIR/$ARM-probes.tsv" <<'PY'
import json, sys

report = json.load(open(sys.argv[1]))
samples = [json.loads(line) for line in open(sys.argv[2]) if line.strip()]
killed_at = float(sys.argv[3])
ceiling = int(sys.argv[4])

told = report.get("outcomes", {})
total = sum(o["count"] for o in told.values())

print()
print(f"  connections      {report['connections']}")
print(f"  transactions     {report['transactions']}")
print(f"  errors           {report['errors']}")
print(f"  relocations      {report['relocations']}")
print(f"  p50 / p99        {report['latency']['p50_us']}us / {report['latency']['p99_us']}us")
print()
print("  what clients were told")
if not told:
    print("    nothing: no client saw a failure or a relocation")
for code, outcome in sorted(told.items(), key=lambda kv: -kv[1]["count"]):
    share = 100.0 * outcome["count"] / total if total else 0.0
    print(f"    {code:<6} {outcome['count']:>7}  {share:5.1f}%")
    for message, count in sorted(outcome["messages"].items(), key=lambda kv: -kv[1]):
        print(f"           {count:>7}  {message}")
try:
    probes = [l.rstrip("\n").split("\t") for l in open(sys.argv[5]) if l.strip()]
except OSError:
    probes = []
if probes:
    print()
    print("  and one client at a time, arriving through the alias")
    print("    when     outcome   seconds  what it got")
    for at, status, secs, answer in probes:
        print(f"    {at:<8} {status:<9} {secs:>7}  {answer.strip()[:60]}")

print()
print(f"  refused at a node's client ceiling  {ceiling}")
print("    from the survivors' logs, because the wire cannot say it. Zero means")
print("    every 53300 above came from the upstream pool.")

def quota(sample, node):
    view = sample["nodes"].get(node, {}).get("servers")
    if not view:
        return None
    for row in view:
        if row.get("server", "").startswith("primary"):
            return row
    return None

def waiting(sample, node):
    view = sample["nodes"].get(node, {}).get("pools")
    if not view:
        return None
    return sum(p["waiting"] for p in view), sum(p["active"] for p in view)

print()
print("  the cap, as the fleet reports it and as the database has it")
print("    the first is assembled from gossip and counts a node that has just")
print("    died; the second is pg_stat_activity. Only the second can say whether")
print("    a cap was breached.")
print("    t        reported  actual  cap")
for sample in samples:
    offset = sample["t"] - killed_at
    if not (-8 <= offset <= 30):
        continue
    reported = None
    for node in ("pgprox-1", "pgprox-2", "pgprox-3"):
        row = quota(sample, node)
        if row:
            reported = row["in_use"]
            break
    actual = sample.get("primary_conns")
    flag = "  <-- over the cap" if actual is not None and actual > 60 else ""
    print(f"    {offset:+7.1f}  {str(reported):>8}  {str(actual):>6}   60{flag}")

print()
print("  the fleet's own view, across the kill")
print("    t        phase        node       in_use  headroom  guaranteed  leased  active  waiting")
for sample in samples:
    if sample["phase"] not in ("before-kill", "after-kill", "displaced", "recovering"):
        continue
    offset = sample["t"] - killed_at
    if offset > 30:
        continue
    for node in ("pgprox-1", "pgprox-2", "pgprox-3"):
        row = quota(sample, node)
        pool = waiting(sample, node)
        if row is None:
            print(f"    {offset:+7.1f}  {sample['phase']:<12} {node:<10} gone")
            continue
        active, wait = (pool[1], pool[0]) if pool else (0, 0)
        print(
            f"    {offset:+7.1f}  {sample['phase']:<12} {node:<10} "
            f"{row['in_use']:>6}  {row['headroom']:>8}  {row['guaranteed']:>10}  "
            f"{row['leased']:>6}  {active:>6}  {wait:>7}"
        )
PY
}

# ---------------------------------------------------------------------------

require_tool docker || finish
require_tool python3 || finish

trap tear_down EXIT

if bring_up && prepare_data; then
  run || fail "the run did not complete"
fi

echo
echo "  artefacts in ${OUT_DIR#"$REPO_ROOT"/}: $ARM.json, $ARM-samples.jsonl, $ARM-pgload.log"
finish
