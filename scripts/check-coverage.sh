#!/usr/bin/env bash
# The coverage gate. Per crate, never workspace-wide: a single global number
# lets a 99% crate mask a 70% one.
#
# Usage: check-coverage.sh [crate-name]
#   With a crate name, gates that crate only (what agent hooks call).
#   With no argument, gates every workspace member independently.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

# Skipped only by `tests/gates/negative.sh`, which invokes a gate once per
# broken artefact to prove the gate fails. Those cases exercise a gate's own
# logic; re-running cargo for each of them adds nothing and costs 84 seconds an
# invocation, which put the suite past any budget CI would accept. `M12.11`.
#
# CI runs this script in tier 1 regardless, so nothing goes unchecked, and
# `check-drift.sh` fails if `ci.yml` or the pre-commit config ever sets the
# variable. It announces itself on stderr because a knob that turns off the
# coverage gate should never fire quietly.
if [[ -n "${PGPROX_SKIP_DELEGATED_CHECKS:-}" ]]; then
  printf 'PGPROX_SKIP_DELEGATED_CHECKS is set: %s did not run\n' "$(basename "${BASH_SOURCE[0]}")" >&2
  exit 0
fi

CRATE="${1:-}"

if ! has_rust; then
  skip "coverage (no Cargo.toml yet)"
  finish
fi

require_tool cargo || finish
# protoc, because `pgprox-auth`'s build script compiles the sidecar `.proto`
# and prost-build shells out to it. Without this the failure is a build-script
# error four lines into cargo's output, under a heading that says clippy failed.
# `M55.0` is that failure, on the first push to a fresh runner.
require_tool protoc "apt-get install protobuf-compiler" || finish
if ! cargo llvm-cov --version >/dev/null 2>&1; then
  fail "missing cargo-llvm-cov  (install: cargo install cargo-llvm-cov)"
  finish
fi
if ! cargo nextest --version >/dev/null 2>&1; then
  fail "missing cargo-nextest  (install: cargo install cargo-nextest)"
  finish
fi

# Generated code and the composition root's main.rs are the only exclusions.
# Adding another needs a reason in the commit message. See docs/internal/standards/testing.md.
IGNORE_RE='(target-coverage|/OUT_DIR/|/out/|\.pb\.rs|bin/pgprox/src/main\.rs)'

run_gate() {
  local crate="$1"
  local out

  # One target directory per crate.
  #
  # Two problems, one fix. Coverage attributes a generic function to the file
  # it is written in, so a crate that instantiates another crate's generics
  # without exercising them lands in that crate's report: bin/pgprox
  # instantiating LivePool did exactly that, and pgprox-pool read 94% while its
  # own tests covered 99%. And a shared directory keeps object files from the
  # previous crate's run, so llvm-cov reads a stale binary and reports zero for
  # functions the run did execute: pgprox-config read 85% and 98% for the same
  # tree minutes apart, in the direction that fails a green crate. Clearing
  # only the profraw files fixed the first and not the second.
  #
  # Separate directories fix both by construction, and each crate keeps its own
  # warm build, so a repeated run of one crate is as fast as it was. The cost
  # is disk: one instrumented target tree per crate.
  local target="$COVERAGE_TARGET_DIR/$crate"
  CARGO_TARGET_DIR="$target" cargo llvm-cov clean \
    --workspace --profraw-only >/dev/null 2>&1 || true

  # stderr to a file rather than to /dev/null, and the file survives a failure.
  #
  # This used to be `2>/dev/null` with the message below and nothing else. A
  # full CI replay had `pgprox-session` and `pgprox` both report "test run
  # failed" here, and the run was not reproducible afterwards: the same command
  # passed clean, the same gate passed clean, and the exact CI sequence passed
  # clean. There was nothing left to look at, because the only copy of which
  # test failed and why had gone to /dev/null.
  #
  # An intermittent failure is the one kind that most needs its evidence kept,
  # and this gate was throwing it away for the two crates whose tests are
  # slowest and are the only ones that bind real sockets.
  local errs
  errs="$(mktemp -t pgprox-coverage-"$crate"-XXXXXX.log)"

  if ! out="$(CARGO_TARGET_DIR="$target" cargo llvm-cov nextest \
        -p "$crate" --lib --bins \
        --ignore-filename-regex "$IGNORE_RE" \
        --summary-only --json 2>"$errs")"; then
    fail "coverage ($crate): test run failed"

    # The failing tests, named, so a rerun is not the only way to learn
    # anything. nextest prints them to stderr as it goes.
    # `sort -u` because nextest names a failure twice, once as it happens and
    # once in the summary, and a gate that prints the same line twice reads
    # like two failures.
    # `|| true` because `set -e` is on and grep returns 1 when it matches
    # nothing, which is exactly the case this branch exists to handle. Without
    # it the script exited here, having printed the FAIL line and none of the
    # evidence below it. That is the second time this trap has been hit in this
    # repository; the first was an `&&` list in `scripts/mutants.sh`.
    local named
    named="$(grep -E '^\s+(FAIL|SIGABRT|SIGSEGV|TRY [0-9])' "$errs" | sort -u | head -5 || true)"

    if [[ -n "$named" ]]; then
      sed 's/^/       /' <<<"$named"
      printf '       full output: %s\n' "$errs"
      return
    fi

    # Nothing that looks like a failing test, so the run died some other way: a
    # build error, a signal, an out-of-memory. Print the tail inline.
    #
    # Inline rather than a path, and this is the correction to `M52.0`. That
    # change kept the evidence in a file and printed where it was, which works
    # on a developer machine and is worthless on CI, where the runner and every
    # file on it are destroyed when the job ends. The first CI failure after it
    # landed printed a path to a file nobody could ever open. `M55.2`.
    printf '       no failing test named, so the run died another way:\n'
    tail -20 "$errs" | sed 's/^/       | /'
    return
  fi
  rm -f "$errs"

  local pct
  pct="$(printf '%s' "$out" | python3 -c '
import json,sys
d=json.load(sys.stdin)
print(round(d["data"][0]["totals"]["lines"]["percent"], 2))
' 2>/dev/null)" || { fail "coverage ($crate): could not parse llvm-cov output"; return; }

  if python3 -c "import sys; sys.exit(0 if float('$pct') >= float('$COVERAGE_MIN') else 1)"; then
    ok "coverage ($crate): ${pct}% >= ${COVERAGE_MIN}%"
  else
    fail "coverage ($crate): ${pct}% < ${COVERAGE_MIN}%  (write tests, do not lower the gate)"
  fi
}

if [[ -n "$CRATE" ]]; then
  run_gate "$CRATE"
else
  members="$(cargo metadata --no-deps --format-version 1 2>/dev/null \
    | python3 -c 'import json,sys; print("\n".join(p["name"] for p in json.load(sys.stdin)["packages"]))')"
  if [[ -z "$members" ]]; then
    skip "coverage (workspace has no members yet)"
    finish
  fi
  while read -r m; do
    [[ -n "$m" ]] && run_gate "$m"
  done <<< "$members"
fi

finish
