#!/usr/bin/env bash
# Every symbol on the wiring list is still reached.
#
# The list, not the workspace. This is a watchlist of things that were once
# written and not called, and it checks those. It cannot find the next one on
# its own, and the rationale below says why a script that tried would be worse:
# nearly every `pub` item in a library legitimately has no in-tree caller.
#
#   scripts/check-wired.sh
#
# # Why this exists
#
# Four findings across `M15` and `M16` were not wrong code. They were correct,
# tested code that nothing reached:
#
#   - `FrameRelay`, a streaming relay, while the proxy buffered whole messages
#   - `DEFAULT_MAX_INSPECT`, a documented memory bound with no reader
#   - `ClientStatements::close_all` and `ConnectionStatements::forget_all`
#   - `resume::observe_statement`, added by the fix for the third, and left
#     uncalled one commit after its own author wrote that a primitive with no
#     caller is the defect that milestone existed to fix
#
# The last one is the argument for this script. Knowing the rule, having just
# written it down, and being the person who found the other three did not stop
# it happening again. `dead_code` cannot help: every one of these is `pub`, so
# the compiler sees a library exporting an API and says nothing.
#
# # What it checks, and what it cannot
#
# For each pattern below: does it appear outside the file that defines it, in
# production code, on a line that is not an import.
#
# The import rule is the whole difference between this working and not. The
# first version of this script counted any mention, and reported `FrameRelay`
# reached because `lib.rs` re-exports it. A `pub use` is not a caller, and a
# `use` at the top of a file is not one either: both were true throughout the
# milestone in which nothing called it. A check that would have passed while
# the defect was live is not a check.
#
# Comments are excluded for the same reason and it took a second pass to see.
# With imports filtered, `FrameRelay` still reported two callers, and both were
# doc comments mentioning it by name. Prose about a thing is the least reliable
# evidence that anything calls it, since prose is exactly what says it should.
#
# This is still a weaker claim than "reached at runtime", and it is the one a
# script can make. It would not catch a caller that is itself unreachable,
# which is why the list is short and every entry is there because something
# went wrong rather than because it looked plausible.
#
# Entries are patterns, not bare names, so a common name can be qualified:
# three different types have an `observe_statement`, and the one this cares
# about is reached as `resume::observe_statement`.
#
# Adding a symbol here is a statement that it exists to be called from
# somewhere else. Most `pub` items are not that and do not belong here.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "wiring: every symbol on the list is still reached"
echo

# symbol <TAB> the file that defines it <TAB> what it is for
#
# Tab-separated so a description can contain spaces, which is the same shape
# `docs/internal/product/mutants-baseline.txt` uses and for the same reason.
WIRED="${PGPROX_WIRED_LIST:-docs/internal/product/wired.txt}"

# A pattern may be prefixed with `?` to mean "known to be reached from nowhere,
# and tracked". That is the shape `docs/internal/product/mutants-baseline.txt` uses: an entry
# is an argument, never an assertion, and the argument has to name the task that
# will settle it. Without the marker this check would have to be either red in
# CI or quietly missing an entry, and both of those are worse than saying so.
#
# The marker is not a way to pass. An entry carrying it must name a task, and an
# entry that carries it while the symbol *is* reached fails, so the debt cannot
# outlive its cause.

if [[ ! -f "$WIRED" ]]; then
  fail "$WIRED is missing, so nothing is being checked"
  finish
fi

# Where production code lives. Tests are excluded by path, and `#[cfg(test)]`
# modules inside a source file are excluded by dropping the defining file
# itself: every one of the four findings was defined and tested in one file and
# called from nowhere, which is exactly the shape this has to catch.
roots=(crates bin)

checked=0
while IFS=$'\t' read -r symbol defined_in purpose; do
  [[ -z "$symbol" || "$symbol" == \#* ]] && continue
  checked=$(( checked + 1 ))

  known_unwired=""
  if [[ "$symbol" == \?* ]]; then
    known_unwired=1
    symbol="${symbol#?}"
  fi

  if [[ ! -f "$defined_in" ]]; then
    fail "$symbol: $defined_in does not exist, so this entry describes nothing"
    continue
  fi

  # `-w` for a whole word, so `close_all` does not match `close_all_the_things`.
  # `--include` rather than a find, so the list of extensions is in one place.
  hits="$(grep -rnw --include='*.rs' -- "$symbol" "${roots[@]}" 2>/dev/null \
    | grep -v "^${defined_in}:" \
    | grep -v '/tests/' \
    | grep -v '/benches/' \
    | grep -v '/examples/' \
    | grep -vE '^[^:]+:[0-9]+: *(pub )?use ' \
    | grep -vE '^[^:]+:[0-9]+: *(//|\*|#)' \
    || true)"

  if [[ -z "$hits" ]]; then
    if [[ -n "$known_unwired" ]]; then
      if [[ "$purpose" =~ M-?[0-9]+[A-Z]*\.[0-9]+ ]]; then
        printf '  !!  %s is reached from nowhere, and known: %s\n' "$symbol" "$purpose"
      else
        fail "$symbol is marked as known-unwired and names no task to settle it"
      fi
      continue
    fi
    fail "$symbol is defined in $defined_in and reached from nowhere"
    printf '       it exists to %s\n' "$purpose"
    printf '       a thing written to be called and never called is this\n'
    printf '       project'"'"'s most repeated defect; see the header here\n'
  else
    if [[ -n "$known_unwired" ]]; then
      fail "$symbol is marked as known-unwired and is reached; drop the marker"
      continue
    fi
    reached="$(printf '%s\n' "$hits" | cut -d: -f1 | sort -u | wc -l | tr -d ' ')"
    ok "$symbol is reached, from $reached file(s) that are not imports"
  fi
done < "$WIRED"

if (( checked == 0 )); then
  fail "$WIRED lists nothing, so this check passes by describing an empty set"
fi

finish
