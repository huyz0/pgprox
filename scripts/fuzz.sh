#!/usr/bin/env bash
# The fuzz targets, seeded and run.
#
#   scripts/fuzz.sh              # 60 seconds per target
#   scripts/fuzz.sh 600          # ten minutes per target, for a nightly
#
# libFuzzer needs a nightly toolchain and cargo-fuzz. Both are installed by
# hand rather than by this script, because installing a toolchain is not
# something a check should do behind an operator's back.
#
# The corpus is seeded before every run and is not committed. It is derived
# from `crates/pgprox-proto/examples/seed_corpus.rs`, which is committed, and
# libFuzzer grows it from there into whatever a run discovers. Committing the
# grown version would be committing several thousand small files that no human
# reads and that the next run replaces.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

SECONDS_PER_TARGET="${1:-60}"
TARGETS=(frame_decode message_decode classify frame_relay)

echo "fuzzing, ${SECONDS_PER_TARGET}s per target"
echo

if ! rustup toolchain list 2>/dev/null | grep -q '^nightly'; then
  fail "no nightly toolchain  (install: rustup toolchain install nightly)"
  finish
fi
require_tool cargo-fuzz "cargo install cargo-fuzz" || finish

# Deterministic starting points: one file per message shape the proxy can
# encode, chosen from what the reference proxies test for. A run that begins
# from random bytes spends its first minutes rediscovering that a frame has a
# length prefix.
if cargo run -q -p pgprox-proto --example seed_corpus -- fuzz/corpus; then
  ok "corpus seeded"
else
  fail "could not seed the corpus"
  finish
fi

for target in "${TARGETS[@]}"; do
  if cargo +nightly fuzz run "$target" -- \
       -max_total_time="$SECONDS_PER_TARGET" >/dev/null 2>&1; then
    ok "$target"
  else
    # A crash leaves its input in fuzz/artifacts, and that file is the bug
    # report: `cargo +nightly fuzz run <target> <path>` replays it.
    fail "$target found something; see fuzz/artifacts/$target"
  fi
done

finish
