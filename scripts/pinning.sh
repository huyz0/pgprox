#!/usr/bin/env bash
# What pinning costs multiplexing. `M11.7`.
#
#   scripts/pinning.sh              the curve, four points, on the compose stack
#   scripts/pinning.sh --keep       leave the stack up afterwards
#
# ADR 0001 calls this an open question and hands it to the plan. The question
# the plan asks needs a tenant population nobody here has. The one underneath
# it can be answered here: as the share of sessions holding a pin rises, what
# happens to the upstream connection count and to the median.
#
# ## Why the run is deliberately below saturation
#
# A pinned session holds an upstream connection for the rest of its life, so
# the cost of pinning is connections that stop being shared. That is only
# visible where the pool is demand-driven. At saturation every arm would report
# sixty connections and the curve would be flat for a reason that has nothing to
# do with pinning.
#
# So this runs at a connection count where the fleet uses well under its cap,
# and the number to watch is clients per upstream connection. Multiplexing stops
# paying for itself when that ratio approaches one, or sooner, when the pool
# reaches the cap and clients begin to queue for a connection that a pinned
# session is never going to give back.
#
# That paragraph used to be a claim the run did not check, and the run that did
# not check it violated it. The control arm's peak is now a guard: if it reaches
# the cap the run fails and says to lower the connection count, because from
# there every arm reports the cap and the curve is flat by construction.
#
# ## The four points
#
# `workload.yaml` is the zero, then the three `workload-pin-*` documents, which
# differ from it and from each other in exactly one weight. Same connections,
# same duration, same seed, same stack, one arm after another on a quiet
# machine.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

COMPOSE=(docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.fleet.yml)
NODES=(pgprox-1 pgprox-2 pgprox-3)

KEEP=""
for arg in "$@"; do
  case "$arg" in
    --keep) KEEP=1 ;;
    *) fail "unknown argument $arg"; finish ;;
  esac
done

# Below the point where the fleet reaches its sixty-connection cap on the
# unpinned workload, so the pool size is set by demand and the curve has room
# to move.
#
# A hundred and fifty was tried and does not meet that, which is the whole
# finding of the first run: the control arm peaked at the cap on a workload
# with no `LISTEN` in it. What made it look acceptable was reading the mean, 37
# of 60, and not the peak. The mean being under the cap is not the condition
# this curve needs. A pinned session takes a connection out of circulation for
# good, so what the pinned arms have to be able to do is *rise*, and they can
# only rise from a peak that is already below sixty.
#
# Forty, from the same calibration the old note quoted and then argued past:
# forty clients hold twelve of the sixty, four hundred hold all sixty. Twelve
# leaves the whole cap above it. It also keeps the x-axis honest, because at
# most forty sessions can pin and forty is under the cap too, where at a
# hundred and fifty the pin count saturated at the pool size and three
# different documents produced 60, 60 and 71.
#
# The corollary is that a clean run here should show no `53300` at all. The
# first run's errors were the pool refusing a queue it could not serve; with
# the cap above every arm's demand there is nothing to refuse, and any error
# that does appear is a finding rather than the arm's cost of pinning.
CONNECTIONS="${PINNING_CONNECTIONS:-40}"
# The fleet's own upstream cap, which is what "below saturation" is below. It
# lives in the compose file; it is repeated here because the guards and the
# table both have to compare against it, and a number two checks disagree about
# is worse than a number that is stated once.
CAP="${PINNING_CAP:-60}"
DURATION="${PINNING_DURATION:-120}"
SEED="${PINNING_SEED:-7}"
SETTLE="${PINNING_SETTLE:-20}"

# The arms, in order, as `label:document`.
ARMS=(
  "none:/workload/workload.yaml"
  "low:/workload/workload-pin-low.yaml"
  "mid:/workload/workload-pin-mid.yaml"
  "high:/workload/workload-pin-high.yaml"
)

# The loadgen container mounts `target/$PGPROX_RUN_DIR` as `/out`, so this and
# the directory the reports land in are one setting rather than two that have
# to agree. `M11.6`'s first pinning run wrote every report into the admission
# run's directory and reported four missing files.
export PGPROX_RUN_DIR=pinning
OUT_DIR="$REPO_ROOT/target/$PGPROX_RUN_DIR"
mkdir -p "$OUT_DIR"

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
  ok "the stack is up"
}

tear_down() {
  [[ -n "$KEEP" ]] && { warn "the stack is still up (--keep)"; return; }
  "${COMPOSE[@]}" down --volumes --remove-orphans >/dev/null 2>&1 || true
}

prepare_data() {
  if ! "${COMPOSE[@]}" exec -T -e PGPASSWORD="$TOKEN" -e PGSSLMODE=require client \
      pgbench --host pgprox-1 --port 6432 --username acme_app \
        --initialize --scale 1 --quiet tenant_acme >/dev/null 2>&1; then
    fail "could not create the workload's tables"
    return 1
  fi
  ok "the workload's tables exist"
}

# The database's own count of what the fleet is holding.
#
# Rather than `/v1/servers`, for the reason `M11.6` recorded: that view sums
# what every node last gossiped and can be stale. Nothing dies in this run, so
# the two should agree, and taking the database's answer means the curve does
# not depend on that being true.
primary_conns() {
  "${COMPOSE[@]}" exec -T primary \
    psql --username postgres --dbname tenant_acme --no-align --tuples-only --quiet \
      -c "SELECT count(*) FROM pg_stat_activity WHERE usename = 'acme_app'" 2>/dev/null \
    | tr -d '[:space:]' | grep -E '^[0-9]+$' || echo 0
}

# Sessions pinned so far, fleet-wide.
#
# A counter rather than a gauge: `pgprox_pin_total` counts sessions that pinned
# since the node started, which is why every arm reads it before and after and
# reports the difference.
#
# **Not by reason, and the task asked for by reason.** The label exists and is
# always `unknown`: `bin/pgprox/src/metrics.rs` says so in its own comment,
# because `Sessions` records the reason on the client and counts pins only in
# total. So this sums every series regardless of label, which is exact here for
# a reason worth stating rather than glossing: these four documents contain no
# advisory lock, no temp table, no `WITH HOLD` cursor, no SQL-level `PREPARE`
# and no unlisted `SET`, so `LISTEN` is the only thing in them that can pin.
# Every pin counted below is one of these documents' own.
pins_now() {
  local node total=0 count
  for node in "${NODES[@]}"; do
    count="$("${COMPOSE[@]}" exec -T "$node" curl --silent --max-time 3 \
      http://127.0.0.1:9090/metrics 2>/dev/null |
      awk '/^pgprox_pin_total\{/ { sum += $NF } END { printf "%d", sum }')"
    total=$(( total + ${count:-0} ))
  done
  echo "$total"
}

# One arm: the load, with the upstream connection count sampled while it runs.
arm() {
  local label="$1" document="$2"
  local report="$OUT_DIR/$label.json"
  local peak=0 sum=0 samples=0

  local pins_before pins_after
  pins_before="$(pins_now)"

  echo "  $label: $CONNECTIONS clients on $(basename "$document") for ${DURATION}s"
  "${COMPOSE[@]}" exec -T loadgen \
    pgload --target pgprox:6432 \
      --workload "$document" \
      --connections "$CONNECTIONS" \
      --duration "$DURATION" \
      --seed "$SEED" \
      --user acme_app \
      --database tenant_acme \
      --password "$TOKEN" \
      --tls-insecure \
      --connect-timeout 60 \
      --out "/out/$label.json" >"$OUT_DIR/$label.log" 2>&1 &
  local client=$!

  # Skip the ramp: connections arrive over the first seconds and a pool
  # measured through that is measuring the ramp.
  sleep 20
  while kill -0 "$client" 2>/dev/null; do
    local held
    held="$(primary_conns)"
    if [[ "$held" =~ ^[0-9]+$ ]]; then
      (( held > peak )) && peak="$held"
      sum=$(( sum + held ))
      samples=$(( samples + 1 ))
    fi
    sleep 3
  done
  wait "$client" || true

  [[ -s "$report" ]] || { fail "$label: no report"; tail -10 "$OUT_DIR/$label.log" | sed 's/^/  /'; return 1; }
  pins_after="$(pins_now)"

  local mean=0
  (( samples > 0 )) && mean=$(( sum / samples ))
  local pinned=$(( pins_after - pins_before ))

  # An arm that never saw an upstream connection measured nothing, and a curve
  # built from points like that is a curve of zeros with a shape. The first run
  # of this script reported `ok` for three arms at peak 0 and produced a report
  # anyway, which is the failure mode this repo keeps finding: a category the
  # harness prints is not a result until somebody has read what it is a category
  # of. So the arm fails here rather than contributing a point.
  if (( samples == 0 || peak == 0 )); then
    fail "$label: $samples samples, peak $peak upstream; this arm measured nothing"
    tail -10 "$OUT_DIR/$label.log" | sed 's/^/  /'
    return 1
  fi

  # And an arm whose workload declares `LISTEN` has to pin something, or the
  # document is not doing what its name says and the curve's x-axis is fiction.
  # `none` is the control and must pin nothing, which is the same check the
  # other way round.
  if [[ "$label" == none ]]; then
    (( pinned == 0 )) || { fail "none: $pinned sessions pinned by a workload with no LISTEN"; return 1; }
    # And the control has to leave the pinned arms somewhere to rise to. A
    # control already at the cap makes every other arm's upstream count a
    # reading of the cap, and the curve is flat for a reason that has nothing
    # to do with pinning. The first run of this script passed every other check
    # with a control at 60 of 60 and produced a table whose y-axis was constant.
    if (( peak >= CAP )); then
      fail "none: peak $peak of a $CAP cap, so the pinned arms cannot rise and this is not a curve"
      echo "    lower PINNING_CONNECTIONS (now $CONNECTIONS) until the control sits well under $CAP"
      return 1
    fi
  elif (( pinned == 0 )); then
    fail "$label: no session pinned, so this document's LISTEN never ran"
    tail -10 "$OUT_DIR/$label.log" | sed 's/^/  /'
    return 1
  fi

  printf '%s\t%s\t%s\t%s\t%s\n' "$label" "$peak" "$mean" \
    "$pinned" "$samples" >> "$OUT_DIR/held.tsv"

  ok "$label: peak $peak upstream, mean $mean, $pinned sessions pinned"
}

# ---------------------------------------------------------------------------

report_curve() {
  python3 - "$OUT_DIR" "$CONNECTIONS" "$CAP" <<'PY'
import json, os, sys

out_dir, connections, cap = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
held = {}
for line in open(os.path.join(out_dir, "held.tsv")):
    label, peak, mean, pins, samples = line.rstrip("\n").split("\t")
    held[label] = (int(peak), int(mean), int(pins), int(samples))

rows = []
for label in ("none", "low", "mid", "high"):
    path = os.path.join(out_dir, f"{label}.json")
    if not os.path.exists(path) or label not in held:
        continue
    report = json.load(open(path))
    peak, mean, pins, _ = held[label]
    rows.append((label, report, peak, mean, pins))

if not rows:
    print("  no arm produced a report")
    raise SystemExit

print()
print(f"  the curve, {connections} clients, one arm at a time")
print("    arm    pinned  upstream  clients per  p50 us    p99 us   transactions  errors")
print("           sessions  peak     connection")
base = None
for label, report, peak, mean, pins in rows:
    ratio = connections / peak if peak else 0.0
    if base is None:
        base = report["latency"]["p50_us"]
    print(
        f"    {label:<6} {pins:>7}  {peak:>7}  {ratio:>11.1f}  "
        f"{report['latency']['p50_us']:>7}  {report['latency']['p99_us']:>8}  "
        f"{report['transactions']:>12}  {report['errors']:>6}"
    )

print()
print("  against the unpinned arm")
first = rows[0]
for label, report, peak, mean, pins in rows[1:]:
    d_p50 = 100.0 * (report["latency"]["p50_us"] - first[1]["latency"]["p50_us"]) / first[1]["latency"]["p50_us"]
    d_conn = peak - first[2]
    d_tps = 100.0 * (report["transactions"] - first[1]["transactions"]) / first[1]["transactions"]
    print(f"    {label:<6} p50 {d_p50:+6.1f}%   upstream {d_conn:+4}   transactions {d_tps:+6.1f}%")

# The median is only comparable while the arms are comparable. An arm that
# refused a thousand clients has a median over the ones it did not refuse,
# which is a faster set, and it will read as an improvement. Said here rather
# than left for a reader to notice, because the first run of this curve
# reported the median 98% better on the arm that lost the most work.
losses = [report["errors"] for _, report, _, _, _ in rows]
if max(losses) > 0 and min(losses) != max(losses):
    print()
    print("  the medians above are not comparable across these arms.")
    print(f"    errors per arm: {dict((l, r['errors']) for l, r, _, _, _ in rows)}")
    print("    an arm that refused work has a median over the work it kept, which")
    print("    is the faster half. Read transactions and upstream, not p50.")

# The cap is the fleet's, and the database is the only thing that can say
# whether it held.
over = [(label, peak) for label, _, peak, _, _ in rows if peak > cap]
if over:
    print()
    print("  ** the database held more connections than the cap allows **")
    for label, peak in over:
        print(f"    {label}: {peak} against a cap of {cap}")

errors = {label: r["errors"] for label, r, _, _, _ in rows}
if any(errors.values()):
    print()
    print("  errors, which a clean curve should not have")
    for label, report, _, _, _ in rows:
        for code, outcome in report.get("outcomes", {}).items():
            print(f"    {label}: {code} x{outcome['count']}  {list(outcome['messages'])[:1]}")
PY
}

# ---------------------------------------------------------------------------

require_tool docker || finish
require_tool python3 || finish

# One at a time, and the reason is a run that was thrown away. Two of these
# overlapped: they share a compose project name, so each tore down the other's
# stack, and they share an output directory, so each appended to the other's
# results. What came out was a table with two arms missing, one arm reporting
# zero upstream connections, and two rows from a run at a different connection
# count. None of it looked like an error.
# Outside `$OUT_DIR`, because a caller that clears the output directory before
# a fresh run would otherwise delete the lock out from under a run in progress
# and both would proceed. That happened.
LOCK="${TMPDIR:-/tmp}/pgprox-pinning.lock"
exec 9>"$LOCK"
if ! flock --nonblock 9; then
  fail "another run is using $OUT_DIR; they would tear down each other's stack"
  finish
fi

trap tear_down EXIT

: > "$OUT_DIR/held.tsv"

if bring_up && prepare_data; then
  for entry in "${ARMS[@]}"; do
    arm "${entry%%:*}" "${entry#*:}" || fail "${entry%%:*} did not complete"
    # A pool does not reap the instant its clients leave, and a pinned arm
    # leaves connections held until their sessions end. Without this the next
    # arm starts on the previous one's pool.
    sleep "$SETTLE"
  done
  report_curve
fi

echo
echo "  artefacts in ${OUT_DIR#"$REPO_ROOT"/}"
finish
