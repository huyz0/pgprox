#!/usr/bin/env bash
# Supply chain: advisories, licences, bans, sources.
#
# Runs from the pre-commit hook whenever a manifest or the lockfile moves, not
# only from CI. It was CI-only until the second M1F review, and had been failing
# since M1.10 on an unmaintained dependency that nobody saw.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

if ! has_rust || [[ ! -f deny.toml ]]; then
  skip "cargo deny (no workspace or policy yet)"
  finish
fi

if ! cargo deny --version >/dev/null 2>&1; then
  # Not installed is reported, not silently passed: a check that quietly
  # no-ops when its tool is missing is worse than no check.
  fail "missing cargo-deny  (install: cargo install cargo-deny)"
  finish
fi

# The exit code, not the output. The first version grepped the last line for
# FAILED and the last line is blank, so it reported ok on a real failure. A
# negative test caught it; without one it would have shipped as a check that
# always passes.
if cargo deny check >/dev/null 2>&1; then
  ok "cargo deny"
else
  fail "cargo deny (run: cargo deny check)"
fi

finish
