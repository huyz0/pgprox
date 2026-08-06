#!/usr/bin/env bash
# The rolling upgrade rehearsal: restart every node in the fleet, under load,
# and lose nothing.
#
#   scripts/rolling-upgrade.sh
#
# In Kubernetes rather than in compose, because what is being rehearsed is the
# chart. The drain sequence is four things acting together, and three of them
# only exist in a pod spec: the readiness probe that pulls the node out of the
# Service, the preStop hook that starts the drain and waits for it, and the
# termination grace period that gives the hook time to finish. A compose
# restart exercises the fourth on its own and would report a green run for a
# chart that wires none of them.
#
# The run has two halves and both matter. A rolling restart must lose no
# transaction; a node killed outright must lose some. Without the second, a
# zero in the first says nothing more than that the load generator was idle.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

CLUSTER="${CLUSTER:-pgprox-rehearsal}"
RELEASE=pgprox
IMAGE="${IMAGE:-pgprox-e2e-pgprox-1:latest}"
CHART=deploy/helm/pgprox
VALUES=deploy/kind/values.yaml
OUT="${OUT:-docs/internal/product/release/rehearsal-$(date -u +%Y-%m-%d).md}"
LOAD_SECS="${LOAD_SECS:-150}"
KEEP="${KEEP:-}"

# The same well-formed, unsigned token the e2e run uses. The proxy checks the
# algorithm named in the header before it calls the sidecar and never verifies
# the signature; the mock sidecar accepts any token it is not told to refuse.
# A password that is not a JWT at all is refused before the sidecar sees it,
# which would make every transaction in this rehearsal fail for the wrong
# reason.
TOKEN="$(printf '%s' '{"alg":"RS256","typ":"JWT"}' | base64 -w0 | tr '+/' '-_' | tr -d '=')"
TOKEN="${TOKEN}.$(printf '%s' '{"sub":"acme"}' | base64 -w0 | tr '+/' '-_' | tr -d '=').not-a-signature"

echo "rolling upgrade rehearsal"
echo

for tool in kind kubectl helm docker; do
  require_tool "$tool" || true
done
(( _fail_count > 0 )) && finish

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  fail "$IMAGE not built. Run: docker compose -f deploy/docker-compose.yml build pgprox-1"
  finish
fi

# --- the cluster --------------------------------------------------------------
if kind get clusters 2>/dev/null | grep -qx "$CLUSTER"; then
  ok "cluster $CLUSTER already exists"
else
  echo "creating cluster $CLUSTER"
  kind create cluster --name "$CLUSTER" --wait 120s >/dev/null 2>&1 \
    && ok "cluster $CLUSTER created" || { fail "kind create cluster failed"; finish; }
fi
KUBE=(kubectl --context "kind-$CLUSTER")

# The proxy image is loaded rather than pulled: it exists only on this machine,
# and a kind node is a separate container with its own image store.
#
# Postgres is not, and is left to the node to pull. Loading it needs the local
# store to hold every layer of the manifest it was pulled under, and a store
# that has served a different platform at some point does not: `kind load` then
# fails with `content digest ... not found`, which reads as a kind problem and
# is a docker one. There is nothing local about that image worth preserving.
kind load docker-image --name "$CLUSTER" "$IMAGE" >/dev/null 2>&1 \
  && ok "loaded $IMAGE" || { fail "could not load $IMAGE into the cluster"; finish; }

# --- the database -------------------------------------------------------------
#
# Recreated every run. There is no volume, so the init hook runs again and the
# workload's tables come back with it; a database left over from a previous
# rehearsal would carry that run's rows into this one's numbers.
"${KUBE[@]}" delete -f deploy/kind/postgres.yaml --ignore-not-found --wait >/dev/null 2>&1
"${KUBE[@]}" apply -f deploy/kind/postgres.yaml >/dev/null
if "${KUBE[@]}" rollout status deployment/postgres --timeout=180s >/dev/null 2>&1; then
  ok "postgres is up"
else
  fail "postgres never became ready"
  finish
fi

# --- the fleet ----------------------------------------------------------------
helm upgrade --install "$RELEASE" "$CHART" -f "$VALUES" \
  --kube-context "kind-$CLUSTER" --wait --timeout 5m >/dev/null 2>&1 \
  && ok "chart installed" || { fail "helm install failed"; finish; }

if "${KUBE[@]}" rollout status "statefulset/$RELEASE" --timeout=300s >/dev/null 2>&1; then
  ok "all $("${KUBE[@]}" get statefulset "$RELEASE" -o jsonpath='{.status.readyReplicas}') nodes ready"
else
  fail "the fleet never became ready"
  "${KUBE[@]}" get pods
  finish
fi

# --- one load run, and what happens to the fleet during it --------------------
#
# `disrupt` is the difference between the two halves. Everything else about the
# two runs is identical, so a difference in the error count has one cause.
run_load() {
  local name="$1" disrupt="$2"

  # pgload logs to stderr and writes its report to a file, so the pod's stdout
  # is the report and nothing else. That is what makes `kubectl logs` a usable
  # way to get it back without a volume or a copy.
  #
  # An override rather than `--command --`, because the pod also needs the
  # workload mounted, and `kubectl run` has no flag for that.
  local script="/usr/local/bin/pgload \
    --target $RELEASE:6432 --workload /workload/workload.yaml \
    --connections 40 --duration $LOAD_SECS --ramp 5 \
    --user acme_app --database tenant_acme --password $TOKEN \
    --out /tmp/report.json >/dev/null 2>/tmp/pgload.err; \
    cat /tmp/report.json 2>/dev/null || cat /tmp/pgload.err >&2"

  "${KUBE[@]}" delete pod "$name" --ignore-not-found >/dev/null 2>&1
  "${KUBE[@]}" run "$name" --image="$IMAGE" --restart=Never \
    --overrides="$(python3 - "$name" "$IMAGE" "$script" <<'PY'
import json, sys
name, image, script = sys.argv[1], sys.argv[2], sys.argv[3]
print(json.dumps({
    "spec": {
        "containers": [{
            "name": name,
            "image": image,
            "imagePullPolicy": "Never",
            "command": ["/bin/sh", "-c", script],
            "volumeMounts": [{"name": "workload", "mountPath": "/workload"}],
        }],
        "volumes": [{"name": "workload", "configMap": {"name": "pgload-workload"}}],
        "restartPolicy": "Never",
    }
}))
PY
)" >/dev/null 2>&1

  # Let the connections arrive and settle before touching the fleet, so what
  # the run measures is a restart rather than a startup.
  sleep 25
  "$disrupt"

  # The pod exits when pgload does. Polled rather than `kubectl wait` on
  # Succeeded, because a run that fails never reaches that phase and the wait
  # would sit out its whole timeout before anyone found out why.
  local phase=""
  for _ in $(seq 1 $((LOAD_SECS + 120))); do
    phase="$("${KUBE[@]}" get pod "$name" -o jsonpath='{.status.phase}' 2>/dev/null)"
    [[ "$phase" == Succeeded || "$phase" == Failed ]] && break
    sleep 1
  done
  # On a failure this is pgload's own error rather than a report, which is what
  # the caller needs to see: "measured nothing" is a symptom, not a cause.
  "${KUBE[@]}" logs "$name" 2>/dev/null
}

rolling_restart() {
  "${KUBE[@]}" rollout restart "statefulset/$RELEASE" >/dev/null 2>&1
  "${KUBE[@]}" rollout status "statefulset/$RELEASE" --timeout=300s >/dev/null 2>&1
}

hard_kill() {
  # SIGKILL, from outside the pod's PID namespace. This is the control: what a
  # node going away costs when nothing gets to say anything to anyone.
  #
  # Three ways that do not work, all worth writing down because each one
  # produced a control that looked like it had run and had not:
  #
  #   `kubectl exec -- kill -9 1` does nothing at all. The kernel discards a
  #   signal sent to PID 1 from inside its own PID namespace unless that
  #   process has a handler, and SIGKILL cannot have one. Restart count stayed
  #   at zero.
  #
  #   `kubectl delete --grace-period=0 --force` removes the API object at once
  #   and leaves the kubelet to terminate the container its own way, so some
  #   clients still left with a polite 57P01.
  #
  #   `kubectl delete --grace-period=1` cuts the preStop hook short but still
  #   sends SIGTERM, and the proxy's own shutdown path closes its clients with
  #   57P01 before exiting. It disturbed fourteen clients and lost nothing,
  #   which says something good about the proxy and nothing about the drain.
  #
  # So the kill has to come from the node, through the container runtime. This
  # couples the control to kind, which the script already requires.
  local node cid
  node="$CLUSTER-control-plane"
  cid="$(docker exec "$node" \
    crictl ps --name pgprox -q --label "io.kubernetes.pod.name=$RELEASE-0" 2>/dev/null | head -1)"
  if [[ -z $cid ]]; then
    warn "could not find $RELEASE-0's container on $node: the control will not disrupt anything"
    return
  fi
  docker exec "$node" crictl stop --timeout 0 "$cid" >/dev/null 2>&1
  "${KUBE[@]}" rollout status "statefulset/$RELEASE" --timeout=300s >/dev/null 2>&1
}

field_of() { python3 -c "import json,sys; print(json.load(sys.stdin).get('$1','?'))" 2>/dev/null; }

"${KUBE[@]}" create configmap pgload-workload \
  --from-file=workload.yaml=docs/internal/product/perf/workload.yaml \
  --dry-run=client -o yaml | "${KUBE[@]}" apply -f - >/dev/null

echo
echo "run 1 of 2: rolling restart under load (${LOAD_SECS}s)"
ROLLING="$(run_load load-rolling rolling_restart)"
ROLLING_ERRORS="$(printf '%s' "$ROLLING" | field_of errors)"
ROLLING_MOVED="$(printf '%s' "$ROLLING" | field_of relocations)"
ROLLING_TX="$(printf '%s' "$ROLLING" | field_of transactions)"

echo "run 2 of 2: a node killed without a drain, under the same load"
CONTROL="$(run_load load-control hard_kill)"
CONTROL_ERRORS="$(printf '%s' "$CONTROL" | field_of errors)"
CONTROL_MOVED="$(printf '%s' "$CONTROL" | field_of relocations)"
CONTROL_TX="$(printf '%s' "$CONTROL" | field_of transactions)"

echo
if [[ "$ROLLING_TX" == "?" || "${ROLLING_TX:-0}" -lt 100 ]]; then
  fail "the rolling run did only ${ROLLING_TX:-0} transactions: it measured nothing"
elif [[ "$ROLLING_ERRORS" == "0" ]]; then
  ok "rolling restart: $ROLLING_TX transactions, 0 lost, $ROLLING_MOVED relocated"
else
  fail "rolling restart: $ROLLING_TX transactions, $ROLLING_ERRORS lost"
fi

# A drain that moved nobody is a drain that had nothing to move, which makes
# the zero above a fact about the load rather than about the drain.
if [[ "${ROLLING_MOVED:-0}" -lt 1 ]]; then
  fail "the rolling run relocated nobody: it did not exercise the drain"
else
  ok "the drain relocated $ROLLING_MOVED clients, so it ran"
fi

if [[ "$CONTROL_ERRORS" == "0" && "${CONTROL_MOVED:-0}" == "0" ]]; then
  fail "the control run saw nothing at all: it did not disrupt the fleet"
elif [[ "$CONTROL_ERRORS" == "0" ]]; then
  fail "the control run lost nothing either: a zero above proves nothing"
else
  ok "control, no drain: $CONTROL_TX transactions, $CONTROL_ERRORS lost, as it should"
fi

# --- the record ---------------------------------------------------------------
mkdir -p "$(dirname "$OUT")"
{
  echo "# Rolling upgrade rehearsal"
  echo
  echo "Generated by \`scripts/rolling-upgrade.sh\` on $(date -u +%Y-%m-%d),"
  echo "in a kind cluster, against the chart in \`deploy/helm/pgprox\`."
  echo
  echo "| Run | What happened to the fleet | Transactions | Lost | Relocated |"
  echo "| --- | --- | --- | --- | --- |"
  echo "| 1 | \`kubectl rollout restart\`, all three nodes in turn | $ROLLING_TX | $ROLLING_ERRORS | $ROLLING_MOVED |"
  echo "| 2 | one node's container SIGKILLed from the node | $CONTROL_TX | $CONTROL_ERRORS | $CONTROL_MOVED |"
  echo
  echo "Lost and relocated are different things and the distinction is the"
  echo "whole result. A drain closes a connection that is between transactions"
  echo "with \`57P01\`, which every mainstream driver answers by reconnecting;"
  echo "that costs a reconnect and no work, and it is the relocated column."
  echo "Lost is a transaction that had already started. Counting the two"
  echo "together, which the load client used to do, makes \"zero failed"
  echo "transactions\" a target a working drain can never hit."
  echo
  echo "Run 2 is the control. It is the same load against the same fleet with"
  echo "the drain skipped, so its losses are what run 1's zero is a claim"
  echo "against. A rehearsal with only the first row cannot tell a working"
  echo "drain from an idle load generator."
  echo
  echo "## What ran"
  echo
  echo "- ${LOAD_SECS}s of \`bin/pgload\` at 40 connections against the Service,"
  echo "  replaying \`docs/internal/product/perf/workload.yaml\`"
  echo "- three proxy nodes, \`drain.graceSeconds\` of 10, one Postgres"
  echo "- the restart begins 25s in, after the connections have settled, so"
  echo "  what is measured is a restart rather than a startup"
  echo
  echo "## What this does not cover"
  echo
  echo "One node's clients move to two others on a single machine. It does not"
  echo "say what happens when the fleet is at its connection cap and a third of"
  echo "it goes away at once, which is the case where shedding has to work and"
  echo "where a rehearsal on real hardware would be worth running."
} > "$OUT"

ok "written: $OUT"

if [[ -n "$KEEP" ]]; then
  echo
  echo "cluster kept. delete it with:"
  echo "  kind delete cluster --name $CLUSTER"
else
  "${KUBE[@]}" delete pod load-rolling load-control --ignore-not-found >/dev/null 2>&1
fi

finish
