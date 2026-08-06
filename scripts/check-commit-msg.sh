#!/usr/bin/env bash
# One task, one commit. The subject references the backlog task so history
# stays traceable to the plan. See docs/internal/standards/behavior.md.
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
if [[ ! "$subject" =~ ^M-?[0-9]+[A-Z]*\.[0-9]+: ]]; then
  fail "commit subject must start with a backlog task ID, e.g. 'M-1.7: add ADRs'"
  printf '       got: %s\n' "$subject"
  printf '       see: docs/internal/standards/behavior.md\n'
  finish
fi

task="${subject%%:*}"

# Well formed is not the same as real. This check used to stop at the pattern
# while its own comment promised the subject "references the backlog task so
# history stays traceable to the plan", and a subject can satisfy a regex while
# referring to nothing. `M11.11` was committed with no such task in the backlog
# and this hook passed it; the entry had to be filed afterwards.
#
# Read from the index rather than the working tree. A task's own filing commit
# is the commit that adds the entry it references, so the backlog that matters
# is the one being committed. `git show :path` is that. Outside a repository,
# or before the file is tracked, fall back to the working tree and say which
# was used, because a check that silently degrades to a weaker check is the
# thing this milestone is about.
backlog=""
source_desc="the index"
if ! backlog="$(git -C "$REPO_ROOT" show :docs/internal/product/backlog.md 2>/dev/null)" || [[ -z "$backlog" ]]; then
  source_desc="the working tree"
  backlog="$(cat "$REPO_ROOT/docs/internal/product/backlog.md" 2>/dev/null || true)"
fi

if [[ -z "$backlog" ]]; then
  fail "cannot read docs/internal/product/backlog.md, so '$task' cannot be resolved to a task"
elif grep -qF -- "$(printf '`%s`' "$task")" <<<"$backlog"; then
  ok "commit subject references $task"
else
  fail "commit subject references $task, which is not a task in docs/internal/product/backlog.md"
  printf '       file the task before the commit that does it, not after\n'
  printf '       read from: %s\n' "$source_desc"
  printf '       see: docs/internal/standards/behavior.md\n'
fi

finish
