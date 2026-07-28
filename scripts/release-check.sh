#!/usr/bin/env bash
# M8 completion condition: the FIPS variant is built rather than declared, the
# suites it leaves a driver are recorded, the deployment manifest wires the
# drain sequence to Kubernetes, and a rolling upgrade has been rehearsed.
#
# What this script does not do is run those things. Building AWS-LC from source
# takes minutes and the rehearsal wants a compose stack, so both live in their
# own scripts and this checks that they exist, that they assert what the
# roadmap asks for, and that a run has been recorded. The parts that are cheap
# to verify here, `helm lint` and the rendered probes, are verified here.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M8: FIPS and release"
echo

# --- the FIPS variant ---------------------------------------------------------
#
# The feature has to be forwarded from the binary to every crate that holds a
# crypto provider. Half a process on a validated module is the failure this
# checks for, and it is invisible at runtime.
for crate in bin/pgprox crates/pgprox-tls crates/pgprox-auth; do
  if grep -qs '^fips = ' "$crate/Cargo.toml"; then
    ok "fips feature declared: $crate"
  else
    fail "$crate has no fips feature: the flag stops at the crate above it"
  fi
done

# `assert_fips` is what makes the feature mean something. Without the call, a
# FIPS build that silently fell back to the default provider would start.
if grep -qs 'assert_fips' crates/pgprox-tls/src/lib.rs; then
  ok "the TLS configuration asserts FIPS mode before returning"
else
  fail "no assert_fips: a FIPS build could run non-validated crypto and start"
fi

# A test that only compiles under the feature. Everything else about FIPS can
# be tested with the flag passed in as a value; this is the one assertion that
# needs the module actually linked. The attribute form rather than `cfg!`,
# because `FIPS_BUILD` is defined with `cfg!` and would satisfy a looser grep
# without a single test having been gated on the feature.
if grep -rqs --include='*.rs' '#\[cfg(feature = "fips")\]' crates/pgprox-tls/src; then
  ok "a test asserts the real provider reports FIPS"
else
  fail "nothing exercises the linked FIPS module: the feature has never run"
fi

# And something that runs it. A gated test nothing invokes is a test that
# passes for years without compiling, which is the state this milestone found
# the FIPS feature in.
if [[ -x scripts/fips-check.sh ]]; then
  ok "scripts/fips-check.sh runs the FIPS suite"
  # And something runs *that*, on a schedule. A script only a person can
  # remember to run is a script that is remembered on release day.
  if grep -qs 'fips-check.sh' .github/workflows/ci.yml \
     && grep -qs 'schedule:' .github/workflows/ci.yml; then
    ok "a scheduled job runs it"
  else
    fail "nothing runs scripts/fips-check.sh on a schedule"
  fi
else
  fail "scripts/fips-check.sh missing: the FIPS-gated test has no caller"
fi

if grep -qsE '^FROM .* AS fips' deploy/Dockerfile; then
  ok "the Dockerfile has a FIPS stage"
  # And it is not the one a bare `docker build` produces. Docker builds the
  # last stage when no --target is given, so a FIPS stage moved to the end
  # would silently turn every compose build into a FIPS build, on a toolchain
  # the default stage does not have.
  if [[ "$(grep -E '^FROM ' deploy/Dockerfile | tail -1)" == *' AS fips' ]]; then
    fail "the FIPS stage is last: a bare 'docker build' would produce it"
  else
    ok "the default stage is still what a bare 'docker build' produces"
  fi
else
  fail "no FIPS stage in deploy/Dockerfile: there is no image to ship"
fi

# --- the cipher-suite matrix --------------------------------------------------
#
# Named drivers rather than a file that exists, because the point of the matrix
# is which client stops working under FIPS, and a matrix missing a driver
# answers that question wrongly by omission.
MATRIX=product/release/cipher-matrix.md
if [[ -f $MATRIX ]]; then
  ok "the cipher-suite matrix is committed"
  for driver in psql pgx asyncpg jdbc npgsql; do
    grep -qs "$driver" "$MATRIX" \
      && ok "matrix covers: $driver" \
      || fail "matrix does not mention $driver: FIPS compatibility is unknown for it"
  done
else
  fail "$MATRIX missing: nobody knows which drivers survive FIPS mode"
fi

# --- the chart ----------------------------------------------------------------
CHART=deploy/helm/pgprox
if [[ -f $CHART/Chart.yaml ]]; then
  ok "the Helm chart exists"
  if have helm; then
    helm lint "$CHART" >/dev/null 2>&1 \
      && ok "helm lint" || fail "helm lint failed for $CHART"

    rendered=$(helm template pgprox "$CHART" 2>/dev/null || true)
    if [[ -z $rendered ]]; then
      fail "helm template rendered nothing: the chart produces no manifests"
    else
      # Against a real API server when one is reachable, because that is the
      # only thing that checks a field name against the schema rather than
      # against a grep. A rendered manifest with `readinessProbe` misspelt
      # passes every offline check there is and is silently ignored by the
      # kubelet.
      if kubectl version -o json >/dev/null 2>&1; then
        if printf '%s' "$rendered" | kubectl apply --dry-run=server -f - >/dev/null 2>&1; then
          ok "the API server accepts the rendered manifests"
        else
          fail "the API server rejected the rendered manifests"
        fi
      else
        skip "kubectl apply --dry-run=server (no cluster reachable)"
      fi

      # The drain sequence needs all three or it does not work. A readiness
      # probe with no preStop hook means Kubernetes pulls the endpoint and then
      # SIGTERMs before the in-flight transactions finish, which is the case
      # this milestone exists to prevent.
      grep -qs '/readyz' <<<"$rendered" \
        && ok "readiness probe on /readyz" \
        || fail "no readiness probe on /readyz: a draining node keeps taking clients"
      grep -qs '/healthz' <<<"$rendered" \
        && ok "liveness probe on /healthz" || fail "no liveness probe on /healthz"
      grep -qs 'preStop' <<<"$rendered" \
        && ok "preStop hook" \
        || fail "no preStop hook: SIGTERM lands before the drain finishes"
    fi
  else
    skip "helm lint (helm not installed)"
  fi
else
  fail "$CHART/Chart.yaml missing: there is no way to deploy this"
fi

# --- the rolling upgrade rehearsal --------------------------------------------
if [[ -f scripts/rolling-upgrade.sh ]]; then
  ok "scripts/rolling-upgrade.sh exists"
  grep -qsi 'failed' scripts/rolling-upgrade.sh \
    && ok "the rehearsal reports failed transactions" \
    || fail "the rehearsal does not count failed transactions, so it cannot show zero"
else
  fail "scripts/rolling-upgrade.sh missing: the upgrade path is untested"
fi

if compgen -G 'product/release/rehearsal-*.md' >/dev/null; then
  ok "a rehearsal is recorded ($(compgen -G 'product/release/rehearsal-*.md' | wc -l) file(s))"
else
  fail "no rehearsal recorded in product/release/: the result exists only in a terminal"
fi

# --- MSRV ---------------------------------------------------------------------
#
# The FIPS toolchain constrains the build image, so the pinned version is a
# constraint rather than a note. A pin nothing builds against drifts silently.
if [[ -x scripts/msrv.sh ]] && msrv="$(./scripts/msrv.sh 2>/dev/null)"; then
  ok "MSRV pinned: $msrv"
  # That CI derives it, rather than that the number appears in ci.yml. A
  # literal there would satisfy a grep and then drift the moment Cargo.toml
  # moved, which is the failure this check would exist to prevent.
  if grep -qs 'scripts/msrv.sh' .github/workflows/ci.yml; then
    ok "CI builds on the pinned MSRV, read from Cargo.toml"
  else
    fail "no CI job derives the MSRV: the pin is a comment, not a constraint"
  fi
else
  fail "scripts/msrv.sh missing or no rust-version in Cargo.toml"
fi

# --- the usual gates ----------------------------------------------------------
./scripts/check-layering.sh >/dev/null 2>&1 \
  && ok "crate dependency rule" || fail "crate dependency rule"
./scripts/check-drift.sh >/dev/null 2>&1 \
  && ok "derived files match their source" || fail "derived files have drifted"

finish
