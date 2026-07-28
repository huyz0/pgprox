#!/usr/bin/env bash
# The FIPS variant, compiled and run.
#
# Separate from check-crate.sh because this is minutes rather than seconds:
# aws-lc-fips-sys builds AWS-LC from source, runs the FIPS module through
# `delocate`, and links a static archive. Tier 1 must not carry that.
#
# What tier 1 does carry is `cargo clippy --all-features`, which does compile
# the feature. So the feature is not unbuilt; what has never happened before
# this script is a *test run* with the validated module linked, and that is the
# only way to learn what `ServerConfig::fips()` actually returns.
#
# The compiler is not the system default on purpose. See below.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "FIPS variant"
echo

# AWS-LC's FIPS module is delocated: its assembly is rewritten so the whole
# module lands in one contiguous text section whose hash can be checked at
# startup. The rewriter refuses any `.data` section in that module, and gcc 15
# emits `.data.rel.ro.local` for the module's relocatable read-only tables as
# soon as optimisation is on:
#
#   error while processing "\t.section\t.data.rel.ro.local,\"aw\"\n"
#   on line 406498: ".data section found in module"
#
# That is why `cargo build --features fips` passed here and `cargo test
# --features fips` did not: `[profile.test]` sets opt-level 1, cmake-rs turns
# that into RelWithDebInfo, and the same source stops delocating. A release
# build hits it for the same reason.
#
# clang does not emit those sections, which is why AWS-LC documents clang for
# the FIPS build. Pinning it here rather than leaving it to whatever `cc` is
# means this script gives the same answer on a machine with a different gcc.
FIPS_CC="${FIPS_CC:-clang}"
FIPS_CXX="${FIPS_CXX:-clang++}"

require_tool "$FIPS_CC" "apt-get install clang" || finish
require_tool cmake "apt-get install cmake" || finish
require_tool go "apt-get install golang" || finish

export CC="$FIPS_CC" CXX="$FIPS_CXX"
ok "compiler: $($FIPS_CC --version | head -1)"

# The test that needs the module linked. Everything else about FIPS is tested
# with the build flag passed in as a value, which keeps both branches reachable
# in a default build; this is the one assertion that cannot be.
#
# Both halves are checked: that the suite passed, and that the gated test was
# among what ran. A `#[cfg]` that stopped matching would leave a green suite
# with nothing in it having asked the provider anything.
log="$(mktemp -t pgprox-fips-XXXXXX.log)"
trap 'rm -f "$log"' EXIT

if cargo test -p pgprox-tls --features fips >"$log" 2>&1; then
  if grep -qs 'a_fips_build_produces_fips_configurations ... ok' "$log"; then
    ok "a real ServerConfig and ClientConfig both report FIPS mode"
  else
    fail "the FIPS-gated test did not run: the feature is on but nothing asserted"
  fi
else
  fail "the FIPS test suite failed (log: $log)"
  trap - EXIT
fi

# The binary the image ships. A test passing under `[profile.test]` says
# nothing about a release build, and the release profile is where the delocate
# failure showed up in the first place.
if cargo build --release -p pgprox --features fips >/dev/null 2>&1; then
  ok "the release binary builds with --features fips"
else
  fail "the FIPS release binary does not build"
fi

finish
