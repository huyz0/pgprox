#!/usr/bin/env bash
# The crate dependency rule, enforced rather than trusted.
#
# standards/contracts.md calls this the thing that lets tracks run in parallel:
# every crate depends on pgprox-core and on nothing else in the workspace, with
# pgprox-session and bin/pgprox as the two stated exceptions.
#
# It went unchecked for everything except pgprox-core until the second M1F
# review noticed. A rule with no gate is a preference.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

if ! has_rust; then
  skip "layering (no crates yet)"
  finish
fi

# Crates permitted to compose others, from product/architecture.md.
COMPOSERS=("pgprox-session" "pgprox")

# Test-only scaffolding. Shipping this in a product binary would put test
# helpers in the deployed artifact.
DEV_ONLY=("pgprox-testkit")

is_composer() {
  local name="$1"
  for c in "${COMPOSERS[@]}"; do [[ "$name" == "$c" ]] && return 0; done
  return 1
}

is_dev_only() {
  local name="$1"
  for c in "${DEV_ONLY[@]}"; do [[ "$name" == "$c" ]] && return 0; done
  return 1
}

violations=0

for manifest in crates/*/Cargo.toml bin/*/Cargo.toml; do
  [[ -f "$manifest" ]] || continue
  crate="$(basename "$(dirname "$manifest")")"

  # Runtime dependencies only: everything between [dependencies] and the next
  # section header. dev-dependencies are exempt, since a test may compose.
  runtime="$(awk '
    /^\[dependencies\]/            { in_deps = 1; next }
    /^\[[a-z]/                     { in_deps = 0 }
    in_deps && /^pgprox-/          { sub(/ .*/, "", $0); print }
  ' "$manifest")"

  while read -r dep; do
    [[ -n "$dep" ]] || continue

    if is_dev_only "$dep"; then
      fail "$crate has a runtime dependency on $dep, which is test-only scaffolding"
      violations=$((violations + 1))
      continue
    fi

    if [[ "$dep" == "pgprox-core" ]]; then
      continue
    fi

    if is_composer "$crate"; then
      continue
    fi

    fail "$crate depends sideways on $dep (only pgprox-core is allowed)"
    violations=$((violations + 1))
  done <<< "$runtime"
done

# pgprox-core itself may depend on nothing in the workspace.
if [[ -f crates/pgprox-core/Cargo.toml ]]; then
  core_deps="$(awk '
    /^\[dependencies\]/   { in_deps = 1; next }
    /^\[[a-z]/            { in_deps = 0 }
    in_deps && /^pgprox-/ { print }
  ' crates/pgprox-core/Cargo.toml)"
  if [[ -n "$core_deps" ]]; then
    fail "pgprox-core depends on a workspace crate, which breaks parallel tracks"
    violations=$((violations + 1))
  fi
fi

(( violations == 0 )) && ok "every crate depends only on pgprox-core"

finish
