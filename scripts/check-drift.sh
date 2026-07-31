#!/usr/bin/env bash
# Derived files still match canonical source.
#
# AGENTS.md and .agents/skills/ are canonical. Everything vendor-specific is
# derived from them. This catches the failure where someone edits .claude/ or a
# per-crate CLAUDE.md directly and the standards quietly fork per tool.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

# --- CLAUDE.md files are a one-line import, nothing else ---------------------
check_import() {
  local f="$1"
  if [[ ! -f "$f" ]]; then
    fail "missing $f (expected the one-line @AGENTS.md import)"
    return
  fi
  local content
  content="$(tr -d '[:space:]' < "$f")"
  if [[ "$content" == "@AGENTS.md" ]]; then
    ok "$f is the canonical import"
  else
    fail "$f must contain exactly '@AGENTS.md'. Put content in AGENTS.md instead, so every tool sees it."
  fi
}

check_import CLAUDE.md

if [[ -d crates ]]; then
  for d in crates/*/; do
    [[ -d "$d" ]] || continue
    if [[ -f "$d/AGENTS.md" ]]; then
      check_import "$d/CLAUDE.md"
    else
      fail "$d has no AGENTS.md (every crate carries its own context)"
    fi
  done
fi

# --- skill discovery symlink -------------------------------------------------
if [[ -L .claude/skills ]]; then
  target="$(readlink .claude/skills)"
  if [[ "$target" == "../.agents/skills" ]]; then
    ok ".claude/skills -> $target"
  else
    fail ".claude/skills points at '$target', expected '../.agents/skills'"
  fi
elif [[ -e .claude/skills ]]; then
  fail ".claude/skills is a real directory. It must be a symlink to ../.agents/skills so skills have one source."
else
  fail ".claude/skills symlink missing (ln -s ../.agents/skills .claude/skills)"
fi

# --- skills are portable -----------------------------------------------------
if [[ -d .agents/skills ]]; then
  found_skill=0
  for s in .agents/skills/*/SKILL.md; do
    [[ -f "$s" ]] || continue
    found_skill=1
    name="$(basename "$(dirname "$s")")"

    if ! head -1 "$s" | grep -q '^---$'; then
      fail "skill $name: SKILL.md must open with YAML frontmatter"
      continue
    fi
    fm="$(sed -n '2,/^---$/p' "$s")"
    grep -q '^name:' <<< "$fm"        || fail "skill $name: frontmatter has no 'name'"
    grep -q '^description:' <<< "$fm" || fail "skill $name: frontmatter has no 'description'"

    # Vendor-neutral bodies. A skill naming a tool-specific path only works in
    # that one tool, which defeats the point of the SKILL.md standard.
    if grep -nE '\.claude/|\.cursor/|\.github/copilot|\.windsurf/' "$s" >/dev/null; then
      fail "skill $name: body references a vendor-specific path. Reference scripts/ or AGENTS.md instead."
    fi
  done
  if (( found_skill )); then
    ok "skills are well-formed and vendor-neutral"
  else
    warn "no skills defined yet"
  fi
else
  fail ".agents/skills/ missing"
fi

# --- a gate that cannot fail --------------------------------------------------
#
# `fail` increments `_fail_count` in the shell that runs it. The right-hand side
# of a pipeline is a subshell, so a check written as
#
#     something | { read -r verdict; case ... fail "..." ... }
#
# prints FAIL in red, with the right message, and exits 0. `M11.7` shipped
# exactly that for one commit and it was caught by checking an exit code rather
# than reading output. A gate that cannot fail is worse than no gate, because
# the roadmap cites it as evidence. `M12.6`.
#
# The rule arms on a pipeline whose right-hand side opens a block and disarms on
# the line that closes it. `|| { fail ...; }` is a brace group in the current
# shell, not a subshell, and is the dominant idiom in scripts/, so the pattern
# deliberately does not match a `|` preceded by another `|`.
#
# The alternative fix, `shopt -s lastpipe`, is not used here: it needs job
# control off and applies to the last stage only, so it would trade a visible
# rule for an invisible one.
# The scan roots are a variable so `tests/gates/negative.sh` can point the rule
# at planted files instead of writing them into `scripts/`. A test that plants a
# deliberately broken script in the tree it is testing leaves it there when it
# is interrupted, and this runs in pre-commit.
SHELL_ROOTS="${PGPROX_SHELL_ROOTS:-scripts/*.sh tests/gates/*.sh}"

subshell_fail=0
while read -r hit; do
  [[ -n "$hit" ]] || continue
  fail "$hit"
  subshell_fail=1
done < <(
  for f in $SHELL_ROOTS; do
    [[ -f "$f" ]] || continue
    awk -v file="$f" '
      # A heredoc body is data, not code. Without this the rule flags the
      # deliberately broken fixtures inside tests/gates/negative.sh, which are
      # examples of the bug rather than the bug. `<<<` is a here-string and
      # opens nothing.
      hd != "" {
        if ($0 ~ ("^[[:space:]]*" hd "[[:space:]]*$")) hd = ""
        next
      }
      $0 !~ /<<</ && match($0, /<<-?[[:space:]]*[\047"]?[A-Za-z_][A-Za-z0-9_]*/) {
        w = substr($0, RSTART, RLENGTH)
        sub(/^<<-?[[:space:]]*[\047"]?/, "", w)
        hd = w
        next
      }
      # A pipe that is not "||" and not "|&", followed by a block opener.
      /(^|[^|&>])\|[[:space:]]*(while|\{|\()[[:space:]]*$/ ||
      /(^|[^|&>])\|[[:space:]]*(while|read)[[:space:]]/ {
        armed = 1; opened = NR; next
      }
      armed && /^[[:space:]]*(done|\}|\))/ { armed = 0; next }
      armed && /(^|[^[:alnum:]_])fail[[:space:]]+"/ {
        printf "%s:%d calls fail inside a pipeline subshell (opened at line %d); it would print FAIL and exit 0\n", file, NR, opened
      }
    ' "$f"
  done
)
(( subshell_fail == 0 )) && ok "no check calls fail from inside a pipeline subshell"

# --- the delegated-check skip never reaches CI --------------------------------
#
# `PGPROX_SKIP_DELEGATED_CHECKS` makes `check-crate.sh` and `check-coverage.sh`
# exit 0 without running, so that `tests/gates/negative.sh` can invoke a gate
# once per broken artefact without paying for cargo each time. In CI it would
# turn off clippy and the 95% coverage gate while every milestone still reported
# green, which is this repo's worst failure mode wearing a helpful name. `M12.11`.
skip_leaked=0
for f in .github/workflows/ci.yml .pre-commit-config.yaml; do
  [[ -f "$f" ]] || continue
  if grep -q 'PGPROX_SKIP_DELEGATED_CHECKS' "$f"; then
    fail "$f sets PGPROX_SKIP_DELEGATED_CHECKS: clippy and the coverage gate would report green without running"
    skip_leaked=1
  fi
done
(( skip_leaked == 0 )) && ok "the delegated-check skip is not set in CI or pre-commit"

# --- every milestone gate is wired into CI -----------------------------------
#
# A gate nobody runs is worse than no gate, because the roadmap cites it as
# evidence that a milestone still holds. Eight of these had fired exactly once,
# on the commit that closed their milestone, until `M10.1`. This is here rather
# than in a milestone script so that adding an `m11-complete.sh` and forgetting
# to wire it fails the pre-commit hook rather than waiting for somebody to
# notice.
CI_WORKFLOW=".github/workflows/ci.yml"
if [[ -f "$CI_WORKFLOW" ]]; then
  unwired=0
  # The fuzzer is in this list for the same reason the gates are: `pgprox-proto`
  # says the codec is fuzzed rather than assumed, and a script nobody runs makes
  # that a claim rather than a fact.
  # `tests/gates/negative.sh` is in this list for the strongest version of the
  # same reason: it is the only thing that checks the gates can fail at all, so
  # a tree where it is not run is a tree where every other name in this list is
  # a claim. `M12.1`.
  for gate in scripts/m*-complete.sh scripts/release-check.sh scripts/fuzz.sh \
              scripts/mutants.sh tests/gates/negative.sh; do
    [[ -f "$gate" ]] || continue
    if ! grep -qF "$gate" "$CI_WORKFLOW"; then
      fail "$gate is not run by $CI_WORKFLOW: a gate nobody runs is a record, not a gate"
      unwired=1
    fi
  done
  (( unwired == 0 )) && ok "every milestone gate is wired into CI"
else
  fail "missing $CI_WORKFLOW"
fi

# --- standards referenced by AGENTS.md actually exist ------------------------
missing=0
while read -r link; do
  [[ -f "$link" || -d "$link" ]] || { fail "AGENTS.md links to missing path: $link"; missing=1; }
done < <(grep -oE '\]\((standards|product|\.agents)/[^)]*\)' AGENTS.md | sed 's/^](//; s/)$//' | sort -u)
(( missing == 0 )) && ok "every path AGENTS.md links to exists"

finish
