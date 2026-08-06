#!/usr/bin/env bash
# M13: the non-negotiables that nothing enforces.
#
#   scripts/m13-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code. Almost all of these plant a
# violation and require the rule to object, which is the only way to know a rule
# is awake. `M13.3`'s first lint reported the whole workspace clean while
# matching nothing at all.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M13: the non-negotiables that nothing enforces"
echo

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# --- M13.1: a threshold is not a setting --------------------------------------
#
# The property, not the source text: an exported value must not reach the gate.
if COVERAGE_MIN=10 bash -c 'source scripts/lib.sh >/dev/null 2>&1; [[ "$COVERAGE_MIN" == 95 ]]'; then
  ok "the 95% gate cannot be lowered from the environment"
else
  fail "COVERAGE_MIN=10 reaches the coverage gate (M13.1)"
fi

# --- M13.2: a removed test is declared ----------------------------------------
#
# A throwaway repository, because the check reads the index and the real one
# must not be staged against.
repo="$WORK/kept"
mkdir -p "$repo/scripts" "$repo/s"
cp scripts/*.sh "$repo/scripts/"
git -C "$repo" init -q .
git -C "$repo" config user.email gate@example.com
git -C "$repo" config user.name gate
printf '#[test]\nfn one() {}\n#[test]\nfn two() {}\n' > "$repo/s/a.rs"
git -C "$repo" add -A
git -C "$repo" commit -qm "M0.1: seed"
printf '#[test]\nfn one() {}\n' > "$repo/s/a.rs"
git -C "$repo" add -A
printf 'M0.2: drop one\n' > "$repo/msg"
if bash "$repo/scripts/check-tests-kept.sh" "$repo/msg" >/dev/null 2>&1; then
  fail "a test can be deleted without declaring it (M13.2)"
else
  ok "a deleted test has to be declared"
fi

# --- M13.3: an exposed credential never reaches a formatter -------------------
mkdir -p "$WORK/sec"
printf 'fn f() {\n    warn!("token {}", s.expose());\n}\n' > "$WORK/sec/leak.rs"
if PGPROX_RUST_ROOTS="$WORK/sec/leak.rs" scripts/check-secrets.sh >/dev/null 2>&1; then
  fail "an exposed credential can reach a log macro (M13.3)"
else
  ok "an exposed credential cannot reach a formatter"
fi

# And the workspace itself is clean, which is the claim rather than the rule.
if scripts/check-secrets.sh >/dev/null 2>&1; then
  ok "no credential reaches a formatter in this tree"
else
  fail "check-secrets.sh fails on this tree"
fi

# --- M13.4: business logic is sans-I/O ----------------------------------------
mkdir -p "$WORK/sio"
printf 'use tokio::net::TcpStream;\nfn f() { TcpStream::connect("x"); }\n' > "$WORK/sio/sock.rs"
if PGPROX_SANS_IO_ROOTS="$WORK/sio/sock.rs" scripts/check-sans-io.sh >/dev/null 2>&1; then
  fail "a library can name a concrete socket type (M13.4)"
else
  ok "business logic cannot name a concrete socket type"
fi

if scripts/check-sans-io.sh >/dev/null 2>&1; then
  ok "business logic is sans-I/O in this tree"
else
  fail "check-sans-io.sh fails on this tree"
fi

# --- M13.5: a core trait change arrives whole ---------------------------------
repo="$WORK/contract"
mkdir -p "$repo/scripts" "$repo/crates/pgprox-core/src" "$repo/crates/other/src" \
         "$repo/docs/internal/product/decisions"
cp scripts/*.sh "$repo/scripts/"
git -C "$repo" init -q .
git -C "$repo" config user.email gate@example.com
git -C "$repo" config user.name gate
printf 'pub trait T: Send {\n    fn a(&self);\n}\n' > "$repo/crates/pgprox-core/src/t.rs"
printf 'impl T for X {\n    fn a(&self) {}\n}\n' > "$repo/crates/other/src/x.rs"
git -C "$repo" add -A
git -C "$repo" commit -qm "M0.1: seed"
printf 'pub trait T: Send {\n    fn a(&self);\n    fn b(&self);\n}\n' > "$repo/crates/pgprox-core/src/t.rs"
git -C "$repo" add crates/pgprox-core/src/t.rs
if bash "$repo/scripts/check-core-contract.sh" >/dev/null 2>&1; then
  fail "a core trait can change without its implementor or an ADR (M13.5)"
else
  ok "a core trait change has to bring its implementors and an ADR"
fi

# --- M13.6: the list says what it enforces ------------------------------------
#
# A gate cannot check prose. It can check that the specific wrong sentence has
# not come back, which is the device `M11.3` used, and that the rule with no
# script is still marked as having none.
if grep -q 'Each is enforced by a script' AGENTS.md; then
  fail "AGENTS.md claims all seven non-negotiables are enforced; M13 found four that were not"
else
  ok "AGENTS.md no longer claims an enforcement it does not have"
fi

if grep -q '\*\*No script enforces this\.\*\*' AGENTS.md; then
  ok "the rule that cannot be enforced is marked as such"
else
  fail "rule 3 is no longer marked as unenforced, and no script has appeared that could enforce it"
fi

# --- the suite that proves all of the above can fail --------------------------
#
# Last because it is the slowest, and included because every check above is
# one case of it: this is what says the rules are awake rather than absent.
if tests/gates/negative.sh >/dev/null 2>&1; then
  ok "the checks can fail: tests/gates/negative.sh passes"
else
  fail "tests/gates/negative.sh does not pass"
fi

finish
