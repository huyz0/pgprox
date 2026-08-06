#!/usr/bin/env bash
# Business logic is sans-I/O, enforced rather than trusted. `M13.4`.
#
#   scripts/check-sans-io.sh                the workspace
#   PGPROX_SANS_IO_ROOTS='a/*.rs' ...       a set of files, for the negative suite
#
# `AGENTS.md` non-negotiable 5 says business logic is sans-I/O and points at
# `docs/internal/standards/async-concurrency.md`, which says it "does not touch a socket, a
# clock, or a syscall". The script `AGENTS.md` credits with enforcing it,
# `check-layering.sh`, enforces the crate dependency rule. That is a real rule
# and it is a different one: a crate can depend on nothing but `pgprox-core` and
# still open a socket in the middle of its state machine.
#
# ## What the rule turns into, mechanically
#
# `docs/internal/product/architecture.md` gives the shape: "The I/O shell that wraps it is
# generic over `AsyncRead + AsyncWrite + Unpin`." So a concrete socket type
# named inside a library crate is the violation, and the generic bound is not.
# That is checkable, and the tree already satisfies it: `pgprox-session` holds
# the whole I/O shell and names no concrete socket type anywhere.
#
# Clocks are the same rule and the same shape. `pgprox-core::clock` exists so
# that everything else takes the time as input, and the audit that produced this
# script found 109 `now()` calls across six business-logic crates with **every
# one in test code** except the four inside `clock.rs` itself. The rule is
# already followed; what was missing was anything to notice if it stopped being.
#
# ## The exceptions, each with a reason
#
# Kept short on purpose. An exception list is where a rule goes to die, so each
# entry names why it is not business logic.
#
#   crates/*/src/bin/*        a binary is a composition root, not a library
#   crates/pgprox-auth/src/client.rs
#                             the sidecar adapter. ADR 0003 chose gRPC over a
#                             unix socket, so this file *is* the I/O boundary
#   crates/pgprox-core/src/clock.rs
#                             the one place allowed to read the real clock,
#                             which is the point of it existing
#
# `bin/` is not scanned at all. Composition roots exist to hold the concrete
# types the libraries are generic over.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

ROOTS="${PGPROX_SANS_IO_ROOTS:-}"
if [[ -z "$ROOTS" ]]; then
  mapfile -t FILES < <(git ls-files 'crates/*/src/*.rs' 'crates/*/src/**/*.rs' 2>/dev/null | sort -u)
else
  # shellcheck disable=SC2206  # deliberate word splitting: the caller passes globs
  FILES=($ROOTS)
fi

if (( ${#FILES[@]} == 0 )); then
  warn "no library sources to check"
  finish
fi

is_exempt() {
  case "$1" in
    */src/bin/*)                            return 0 ;;
    crates/pgprox-auth/src/client.rs)       return 0 ;;
    crates/pgprox-core/src/clock.rs)        return 0 ;;
    *)                                      return 1 ;;
  esac
}

violations=0
scanned=0
while IFS= read -r hit; do
  [[ -n "$hit" ]] || continue
  fail "$hit"
  violations=$(( violations + 1 ))
done < <(
  for f in "${FILES[@]}"; do
    [[ -f "$f" ]] || continue
    is_exempt "$f" && continue
    # Production code only. Everything from the first `#[cfg(test)]` onward is a
    # test, and a test is allowed a clock: that is how a fake clock gets its
    # starting instant. This is the repo's layout, in-file `mod tests`, and the
    # rule would need revisiting if that changed.
    awk -v file="$f" '
      /#\[cfg\(test\)\]/ { exit }
      {
        line = $0
        sub(/\/\/.*/, "", line)

        # Concrete socket types. `std::net::IpAddr` and `SocketAddr` are value
        # types that perform no I/O and are deliberately not listed: pgprox-core
        # names `IpAddr` in an auth contract and is right to.
        if (line ~ /(^|[^A-Za-z0-9_])(TcpStream|TcpListener|UnixStream|UnixListener|tokio::net)/) {
          printf "%s:%d names a concrete socket type; the I/O shell is generic over AsyncRead + AsyncWrite\n", file, NR
        }
        # The *real* clock. tokio::time::Instant::now() is the runtime clock,
        # which #[tokio::test(start_paused = true)] makes virtual, so it costs
        # nothing in determinism and is not what this rule is about. That is a
        # distinction the tree earns rather than claims: its only two uses are
        # the buffer-wait deadline in pgprox-session/src/shell.rs, and the tests
        # driving that path do pause time.
        #
        # No apostrophes in here. This awk program is single-quoted, so one
        # closes it and bash then tries to parse awk.
        if (line ~ /(Instant|SystemTime)::now[[:space:]]*\(/ && line !~ /tokio::time/) {
          printf "%s:%d reads the real clock; take the time as input, or use pgprox_core::Clock\n", file, NR
        }
      }
    ' "$f"
  done
)

for f in "${FILES[@]}"; do
  [[ -f "$f" ]] || continue
  is_exempt "$f" && continue
  scanned=$(( scanned + 1 ))
done

if (( violations == 0 )); then
  ok "business logic is sans-I/O ($scanned file(s), no socket and no clock)"
else
  printf '\n       docs/internal/standards/async-concurrency.md: business logic is a pure function\n'
  printf '       of state and input events. Move the socket to the I/O shell,\n'
  printf '       which is generic over AsyncRead + AsyncWrite + Unpin, and take\n'
  printf '       the time as an input instead of reading it.\n'
fi

finish
