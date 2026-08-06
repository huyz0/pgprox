#!/usr/bin/env bash
# M0 completion condition. This is what /goal hands its checker.
#
# M0 is done when pgprox-core holds every contract with a tested fake, and the
# quality apparatus is enforcing on real code rather than on an empty tree.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M0: contracts and quality gates"
echo

# --- workspace ---------------------------------------------------------------
if ! has_rust; then
  fail "no workspace Cargo.toml"
  finish
fi

for f in rustfmt.toml deny.toml .cargo/config.toml; do
  [[ -f "$f" ]] && ok "$f" || fail "$f missing"
done

if cargo metadata --no-deps --format-version 1 >/dev/null 2>&1; then
  ok "workspace metadata resolves"
else
  fail "cargo metadata failed"
  finish
fi

if cargo build --workspace --all-targets >/dev/null 2>&1; then
  ok "workspace builds"
else
  fail "workspace does not build (run: cargo build --workspace --all-targets)"
fi

# --- the gates on real code --------------------------------------------------
./scripts/check-fmt.sh >/dev/null 2>&1 && ok "fmt" || fail "fmt (run: scripts/check-fmt.sh)"
./scripts/check-crate.sh >/dev/null 2>&1 && ok "clippy" || fail "clippy (run: scripts/check-crate.sh)"
./scripts/check-coverage.sh >/dev/null 2>&1 && ok "coverage >= ${COVERAGE_MIN}% per crate" \
  || fail "coverage (run: scripts/check-coverage.sh)"

if cargo deny check >/dev/null 2>&1; then
  ok "cargo deny"
else
  fail "cargo deny (run: cargo deny check)"
fi

# --- contracts ---------------------------------------------------------------
CORE=crates/pgprox-core/src

for m in ids secret error clock buf auth pool cluster config route cache; do
  [[ -f "$CORE/$m.rs" ]] && ok "pgprox-core::$m" || fail "pgprox-core::$m missing"
done

# Every public trait needs a fake. A trait without one is not done, because the
# tracks that depend on it cannot test against it. See docs/internal/standards/contracts.md.
if [[ -d "$CORE" ]]; then
  missing_fakes=0
  while read -r trait_name; do
    [[ -n "$trait_name" ]] || continue
    # A fake is any type implementing the trait whose name starts with Fake.
    if grep -rqs "impl .*$trait_name for Fake" "$CORE" \
       || grep -rqs "impl $trait_name for Fake" "$CORE"; then
      ok "fake for $trait_name"
    else
      fail "no fake implementing $trait_name"
      missing_fakes=$((missing_fakes + 1))
    fi
  done < <(grep -rhoE '^pub (async )?trait [A-Za-z0-9_]+' "$CORE" 2>/dev/null \
           | awk '{print $NF}' | sort -u)
  (( missing_fakes == 0 )) || printf '        Every trait ships a working fake, not a mock.\n'
fi

# --- no sideways dependencies ------------------------------------------------
# Delegated, so the rule has one implementation. This used to check only
# pgprox-core, which left every other crate free to depend sideways.
./scripts/check-layering.sh >/dev/null 2>&1 \
  && ok "crate dependency rule" \
  || fail "crate dependency rule (run: scripts/check-layering.sh)"

finish
