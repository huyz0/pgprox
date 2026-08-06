#!/usr/bin/env bash
# The crate dependency rule, enforced rather than trusted.
#
# docs/internal/standards/contracts.md calls this the thing that lets tracks run in parallel:
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

# Crates permitted to compose others, from docs/internal/product/architecture.md.
COMPOSERS=("pgprox-session" "pgprox" "pgload")

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

# --- #[non_exhaustive] belongs on enums, not on constructible DTOs ------------
#
# The attribute makes a struct unconstructable outside its own crate. On an enum
# describing external state that is the point; on a DTO a downstream crate
# assembles it is a compile error waiting for whoever writes that crate.
#
# It happened three times in pgprox-core (Grant, Member, ClusterDigest) before
# anyone automated it, each discovered only when a second crate tried to build
# the type.
if [[ -d crates/pgprox-core/src ]]; then
  bad_structs=0
  while read -r location; do
    [[ -n "$location" ]] || continue
    file="${location%%:*}"
    line="${location##*:}"
    # The declaration on the line after the attribute.
    decl="$(sed -n "$((line + 1))p" "$file")"
    if [[ "$decl" =~ ^pub\ struct ]]; then
      name="$(awk '{print $3}' <<< "$decl" | tr -d '{')"
      fail "pgprox-core::$name is a #[non_exhaustive] struct; downstream crates cannot build it"
      bad_structs=$((bad_structs + 1))
    fi
  done < <(grep -rn '^#\[non_exhaustive\]' crates/pgprox-core/src | cut -d: -f1,2)

  (( bad_structs == 0 )) && ok "no #[non_exhaustive] structs in pgprox-core"
fi

finish
