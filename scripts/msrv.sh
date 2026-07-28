#!/usr/bin/env bash
# Prints the workspace's minimum supported Rust version.
#
# One source for it: `rust-version` in the workspace Cargo.toml, which is also
# what cargo itself enforces on dependencies. CI installs whatever this prints
# rather than carrying its own copy of the number, because a pin written down
# twice is a pin that drifts, and the half that drifts is always the half
# nobody runs.
#
# The MSRV is a constraint rather than a note here: the FIPS build image is
# built from a distribution's toolchain, and that is what limits how new the
# version can be.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

version="$(grep -m1 '^rust-version' "$root/Cargo.toml" | grep -oE '[0-9]+\.[0-9]+(\.[0-9]+)?' || true)"

if [[ -z $version ]]; then
  echo "no rust-version in Cargo.toml" >&2
  exit 1
fi

printf '%s\n' "$version"
