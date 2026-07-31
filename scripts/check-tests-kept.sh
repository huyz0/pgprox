#!/usr/bin/env bash
# A test that disappears has to be declared. `M13.2`.
#
# Usage: check-tests-kept.sh <path-to-commit-msg-file>
#
# `AGENTS.md` non-negotiable 2 is "never lower a threshold or delete a test to
# make a check pass". `M13.1` fixed the threshold half. Nothing watched the
# other half at all, and the coverage gate does not: a crate can sit at 97% with
# a test quietly gone, because the tests that remain still cover the lines.
#
# ## Why this is not a count
#
# The obvious check is "the number of tests did not go down". A commit that
# deletes one test and adds another passes it while doing exactly the thing the
# rule forbids, and the number is back where it started. So this names what
# disappeared, by function name, and a rename reads as one removal and one
# addition because that is what it is.
#
# ## Why the escape hatch is a line in the commit message
#
# Tests are legitimately deleted. A test for a feature that was removed should
# go, and so should one that was a duplicate. What the rule is against is
# deleting a test *to make a check pass*, and no script can read intent.
#
# What a script can do is refuse to let it happen silently. Declaring the
# removal in the commit message puts it in front of whoever reads the diff, in
# the one place that travels with the change forever:
#
#     Removes-test: a_bind_with_no_parameters_is_still_a_bind
#
# That is deliberately not an environment variable and not a flag. `M13.1` is
# the reason: a switch is something a future run can set by accident, and a
# commit message is written once, by hand, and reviewed.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

MSG_FILE="${1:-}"
if [[ -z "$MSG_FILE" || ! -f "$MSG_FILE" ]]; then
  fail "check-tests-kept.sh needs a commit message file"
  finish
fi

# The mechanical subjects git writes have no author to declare anything, and a
# merge legitimately carries whatever both sides did.
subject="$(head -1 "$MSG_FILE")"
if [[ "$subject" =~ ^(Merge|Revert|fixup!|squash!) ]]; then
  ok "tests kept (not checked for $subject)"
  finish
fi

# The repo declares tests as `#[test]` or `#[tokio::test...]` on the line before
# the function, 1,607 of them in one style. Anything else is not a test here,
# and if that changes this is the place that has to know.
test_names() {
  awk '
    /^[[:space:]]*#\[(tokio::)?test([(].*[)])?\]/ { pending = 1; next }
    pending && match($0, /fn[[:space:]]+[A-Za-z_][A-Za-z0-9_]*/) {
      name = substr($0, RSTART, RLENGTH)
      sub(/^fn[[:space:]]+/, "", name)
      print name
      pending = 0
      next
    }
    # An attribute between the test marker and the function, like #[should_panic]
    # or a #[cfg], keeps the marker alive rather than cancelling it.
    pending && /^[[:space:]]*#\[/ { next }
    pending && /[^[:space:]]/ { pending = 0 }
  '
}

# Only staged Rust files can have lost a test, and only the ones that existed
# before. A new file cannot have removed anything.
staged_rs="$(git diff --cached --name-only --diff-filter=ACMRD -- '*.rs' || true)"
if [[ -z "$staged_rs" ]]; then
  ok "tests kept (no Rust file staged)"
  finish
fi

removed_total=0
missing_declaration=0

while IFS= read -r file; do
  [[ -n "$file" ]] || continue

  before="$(git show "HEAD:$file" 2>/dev/null | test_names | sort -u || true)"
  [[ -n "$before" ]] || continue

  # A deleted file shows nothing in the index, which is the correct reading:
  # every test it held is gone.
  after="$(git show ":$file" 2>/dev/null | test_names | sort -u || true)"

  while IFS= read -r gone; do
    [[ -n "$gone" ]] || continue
    removed_total=$(( removed_total + 1 ))
    if grep -qE "^[[:space:]]*Removes-test:[[:space:]]*${gone}[[:space:]]*$" "$MSG_FILE"; then
      warn "removed test declared: $gone ($file)"
    else
      fail "test removed without declaring it: $gone ($file)"
      missing_declaration=$(( missing_declaration + 1 ))
    fi
  done < <(comm -23 <(printf '%s\n' "$before") <(printf '%s\n' "$after"))
done <<< "$staged_rs"

if (( missing_declaration > 0 )); then
  printf '\n       Add one line per removed test to the commit message:\n'
  printf '           Removes-test: <the function name>\n'
  printf '       and say in the body why it went. A test deleted to make a\n'
  printf '       check pass is what AGENTS.md non-negotiable 2 forbids; a test\n'
  printf '       deleted because what it covered is gone is ordinary work.\n'
elif (( removed_total > 0 )); then
  ok "$removed_total removed test(s), each declared"
else
  ok "tests kept"
fi

finish
