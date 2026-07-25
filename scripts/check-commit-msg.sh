#!/usr/bin/env bash
# One task, one commit. The subject references the backlog task so history
# stays traceable to the plan. See standards/behavior.md.
#
# Usage: check-commit-msg.sh <path-to-commit-msg-file>
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

MSG_FILE="${1:-}"
if [[ -z "$MSG_FILE" || ! -f "$MSG_FILE" ]]; then
  fail "check-commit-msg.sh needs a commit message file"
  finish
fi

subject="$(head -1 "$MSG_FILE")"

# Allow the mechanical commits git itself writes, plus fixup/squash which carry
# the original subject underneath.
if [[ "$subject" =~ ^(Merge|Revert|fixup!|squash!) ]]; then
  ok "commit subject: $subject"
  finish
fi

# M-1.7, M0.3, M1.12, M1R.2: an optional leading dash for the pre-milestone and
# an optional letter suffix for a revision milestone.
if [[ "$subject" =~ ^M-?[0-9]+[A-Z]*\.[0-9]+: ]]; then
  ok "commit subject references ${subject%%:*}"
else
  fail "commit subject must start with a backlog task ID, e.g. 'M-1.7: add ADRs'"
  printf '       got: %s\n' "$subject"
  printf '       see: standards/behavior.md\n'
fi

finish
