#!/usr/bin/env bash
# M12: the gates that count files.
#
#   scripts/m12-complete.sh
#
# Written under a constraint the other gates did not have: no check in here may
# glob for a filename and report a conclusion, because that is the defect this
# milestone exists to remove. Every check below either runs something and reads
# its exit code, or asserts that a specific removed pattern has not come back.
#
# The milestone's real deliverable is `tests/gates/negative.sh`, so the first
# check runs it. A gate that only described the suite would be the thing it is
# meant to prevent.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M12: the gates that count files"
echo

# --- M12.1 and M12.7: the gates can fail --------------------------------------
#
# Run it, do not look for it. The suite breaks an artefact per case and asserts
# a non-zero exit, including the floor case that every gate objects to a tree
# holding none of its artefacts.
if tests/gates/negative.sh >/dev/null 2>&1; then
  ok "the gates can fail: tests/gates/negative.sh passes"
else
  fail "tests/gates/negative.sh does not pass, so the gates are not known to fail on anything"
  tests/gates/negative.sh 2>&1 | grep -E '^(FAIL|  --)' | sed 's/^/       /' || true
fi

# --- M12.1: the commit hook resolves the ID -----------------------------------
#
# The behaviour, not the source. A well-formed ID that answers to no task must
# be refused, and a real one must be accepted, or history stops being traceable
# to the plan while every commit still looks compliant.
probe="$(mktemp)"
trap 'rm -f "$probe"' EXIT

printf 'M99.99: a task that does not exist\n' > "$probe"
if scripts/check-commit-msg.sh "$probe" >/dev/null 2>&1; then
  fail "the commit hook accepts M99.99, an ID with no task behind it (M12.1)"
else
  ok "the commit hook refuses an ID with no task behind it"
fi

printf 'M12.8: write the gate\n' > "$probe"
if scripts/check-commit-msg.sh "$probe" >/dev/null 2>&1; then
  ok "the commit hook accepts a task that exists"
else
  fail "the commit hook refuses M12.8, which is a real task: it is too strict to be used"
fi

# --- M12.1 and M12.10: every ID in history resolves ---------------------------
#
# One pass rather than one hook invocation per commit. Every subject that looks
# like a task ID has to name a task the backlog actually lists, which is what
# `M1F.0` and `M-1.18` did not until `M12.10` filed them.
unresolved=0
while read -r id; do
  [[ -n "$id" ]] || continue
  grep -qF -- "$(printf '`%s`' "$id")" docs/internal/product/backlog.md || {
    fail "history references $id, which is not a task in docs/internal/product/backlog.md"
    unresolved=$(( unresolved + 1 ))
  }
done < <(git log --format='%s' | grep -oE '^M-?[0-9]+[A-Z]*\.[0-9]+' | sort -u)
(( unresolved == 0 )) && ok "every task ID in history resolves to a backlog entry"

# --- M12.6: the lint that catches a gate which cannot fail --------------------
#
# Plant the shape and require the lint to object. Checking that the rule is
# present in `check-drift.sh` would be a check on a filename by another name.
planted="$(mktemp -d)"
trap 'rm -f "$probe"; rm -rf "$planted"' EXIT
cat > "$planted/planted.sh" <<'PLANT'
#!/usr/bin/env bash
printf 'a\n' | {
  read -r verdict
  case "$verdict" in
    a) fail "this prints FAIL and exits 0" ;;
  esac
}
PLANT
if PGPROX_SHELL_ROOTS="$planted/*.sh" scripts/check-drift.sh >/dev/null 2>&1; then
  fail "the subshell lint does not catch a check that prints FAIL and exits 0 (M12.6)"
else
  ok "a check that would print FAIL and exit 0 is caught"
fi

# --- M12.9: no gate in CI is allowed to fail without failing the build --------
#
# Comments stripped before looking, because the first version of this check
# matched the word rather than the construct and failed on the comment left by
# `M12.9` explaining why the flag had been removed. Matching text that looks
# like the thing instead of the thing is the same defect one layer up, and it
# is worth the two lines to not ship a check with the shape it is testing for.
ci_setting="$(sed 's/#.*//' .github/workflows/ci.yml | grep -c 'continue-on-error' || true)"
if (( ci_setting > 0 )); then
  fail "a step in ci.yml carries continue-on-error: a gate whose failure is discarded is a gate that cannot fail"
else
  ok "no CI step discards its own failure"
fi

# --- M12.2 to M12.5: the rewritten checks have not regrown --------------------
#
# A gate cannot check prose, and it cannot easily check that a check is a good
# one. It can check that the four specific patterns this milestone removed are
# gone, which is the same device `M11.3` used for the roadmap sentence it
# corrected. Each entry is the glob that used to report a conclusion.
regrown=0
check_removed() {
  local gate="$1" pattern="$2" what="$3"
  [[ -f "$gate" ]] || { fail "$gate is missing"; regrown=1; return; }
  if grep -qF -- "$pattern" "$gate"; then
    fail "$(basename "$gate") reports $what from the glob $pattern again ($4)"
    regrown=1
  fi
}
check_removed scripts/m7-complete.sh  "compgen -G 'docs/internal/product/perf/run-*.md'" \
  "a scale run" "M12.2"
check_removed scripts/m9-complete.sh  "compgen -G 'docs/internal/product/perf/run-*cache*.md'" \
  "a cache run" "M12.3"
check_removed scripts/m11-complete.sh "compgen -G 'docs/internal/product/perf/*admission*.md'" \
  "an admission run" "M12.4"
check_removed scripts/m1f-complete.sh "compgen -G 'docs/internal/product/decisions/*protocol-3-2*'" \
  "a recorded decision" "M12.5"
(( regrown == 0 )) && ok "no rewritten check has gone back to globbing for a filename"

finish
