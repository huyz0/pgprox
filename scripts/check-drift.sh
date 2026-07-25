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

# --- standards referenced by AGENTS.md actually exist ------------------------
missing=0
while read -r link; do
  [[ -f "$link" || -d "$link" ]] || { fail "AGENTS.md links to missing path: $link"; missing=1; }
done < <(grep -oE '\]\((standards|product|\.agents)/[^)]*\)' AGENTS.md | sed 's/^](//; s/)$//' | sort -u)
(( missing == 0 )) && ok "every path AGENTS.md links to exists"

finish
