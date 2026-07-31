#!/usr/bin/env bash
# An exposed credential never reaches a formatter. `M13.3`.
#
#   scripts/check-secrets.sh              the workspace
#   PGPROX_RUST_ROOTS='a/*.rs' ...        a set of files, for the negative suite
#
# `AGENTS.md` non-negotiable 7 is "credentials never reach a log", and until
# this it was held up by one unit test, `a_token_cannot_reach_a_log` in
# `crates/pgprox-session/src/auth.rs`, across fifteen crates.
#
# ## What the design already guarantees, and what it does not
#
# `SecretString` carries every credential and cannot be printed: `Debug` and
# `Display` both render `[redacted]`, and it deliberately has no `PartialEq`,
# `Deref`, `AsRef<str>` or `From` back to `String`. So the only route to the
# real value is `expose()`, which is why that name was chosen: it greps.
#
# That makes the leak path exactly one shape. `SecretString::expose`'s own
# documentation says it: "never pass it to a formatter that reaches a log, a
# span attribute, a metric label, or an error variant." Nothing enforced that
# sentence. This does.
#
# ## What it does not check, said plainly
#
# It does not prove a credential never reaches a log. It proves the one route
# the type system leaves open is not taken through a formatting macro. A value
# exposed into a local, then formatted three functions later, is not caught. The
# end-to-end version of this claim is a run of the stack grepping its own logs
# for the token it authenticated with, which needs Docker and is `M13.8`.
#
# Assertions are not formatters and are not flagged. `assert_eq!(s.expose(),
# "hunter2")` is how the secret module tests itself, and a rule that failed on
# it would be deleted rather than obeyed.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

RUST_ROOTS="${PGPROX_RUST_ROOTS:-}"
if [[ -z "$RUST_ROOTS" ]]; then
  mapfile -t RUST_FILES < <(git ls-files '*.rs' 2>/dev/null || true)
else
  # shellcheck disable=SC2206  # deliberate word splitting: the caller passes globs
  RUST_FILES=($RUST_ROOTS)
fi

if (( ${#RUST_FILES[@]} == 0 )); then
  warn "no Rust files to check"
  finish
fi

leaked=0
while IFS= read -r hit; do
  [[ -n "$hit" ]] || continue
  fail "$hit"
  leaked=1
done < <(
  for f in "${RUST_FILES[@]}"; do
    [[ -f "$f" ]] || continue
    awk -v file="$f" '
      # The formatting and logging macros. `assert` and `debug_assert` are
      # deliberately absent: comparing an exposed value is how secret.rs tests
      # that expose works, and it reaches no log.
      #
      # A macro invocation can span lines, so this arms on one that does not
      # close on its own line and disarms on the line that closes it. That is a
      # heuristic, the same shape as the subshell rule in check-drift.sh, and it
      # is written down as one rather than presented as a parser.
      {
        line = $0
        sub(/\/\/.*/, "", line)          # a comment is not code

        # No `\b`: it is a GNU extension and this ran under an awk without it,
        # so the pattern matched nothing and the check reported the whole
        # workspace clean. Found by testing a planted leak, not by reading.
        opens = line ~ /(^|[^A-Za-z0-9_])(trace|debug|info|warn|error|event|span|println|eprintln|print|eprint|format|write|writeln|panic|todo|unimplemented)!/

        if (opens && line ~ /\.expose\(\)/) {
          printf "%s:%d passes an exposed credential to a formatter\n", file, NR
          next
        }

        if (opens) {
          # Balanced on this line means the invocation ended here.
          n = gsub(/\(/, "(", line) - gsub(/\)/, ")", line)
          if (n > 0) { armed = 1; depth = n; next }
          next
        }

        if (armed) {
          if (line ~ /\.expose\(\)/) {
            printf "%s:%d passes an exposed credential to a formatter\n", file, NR
          }
          depth += gsub(/\(/, "(", line) - gsub(/\)/, ")", line)
          if (depth <= 0) armed = 0
        }
      }
    ' "$f"
  done
)

if (( leaked == 0 )); then
  ok "no exposed credential reaches a formatter (${#RUST_FILES[@]} file(s))"
else
  printf '\n       SecretString::expose says: never pass it to a formatter that\n'
  printf '       reaches a log, a span attribute, a metric label, or an error\n'
  printf '       variant. Log something that identifies the credential instead,\n'
  printf '       such as its length or the user it belongs to.\n'
fi

finish
