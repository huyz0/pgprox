#!/usr/bin/env bash
# M-1 completion condition. This is what /goal hands its checker.
#
# It exits zero only when the AI development system is actually complete and
# usable, and reports every individual failure rather than stopping at the
# first, so a failing run tells the loop exactly what is left.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M-1: AI development system"
echo

# --- context -----------------------------------------------------------------
[[ -s AGENTS.md ]] && ok "AGENTS.md" || fail "AGENTS.md missing or empty"

# --- standards ---------------------------------------------------------------
for f in rust-style error-handling async-concurrency testing observability \
         security contracts behavior; do
  [[ -s "docs/internal/standards/$f.md" ]] && ok "docs/internal/standards/$f.md" || fail "docs/internal/standards/$f.md missing or empty"
done

# --- product docs ------------------------------------------------------------
for f in mission architecture roadmap backlog plan; do
  [[ -s "docs/internal/product/$f.md" ]] && ok "docs/internal/product/$f.md" || fail "docs/internal/product/$f.md missing or empty"
done

# --- ADRs: one per row of the decisions table in the plan --------------------
adr_count="$(find docs/internal/product/decisions -name '[0-9]*.md' 2>/dev/null | wc -l)"
if (( adr_count >= 10 )); then
  ok "ADRs: $adr_count records"
else
  fail "ADRs: found $adr_count, expected at least 10 (one per decision-table row)"
fi
for adr in docs/internal/product/decisions/[0-9]*.md; do
  [[ -f "$adr" ]] || continue
  grep -qi '^## *consequences' "$adr" \
    || fail "$(basename "$adr"): no Consequences section"
done

# --- skills ------------------------------------------------------------------
for s in spec tdd next-task contract-change crate-review adr hot-path \
         wire-debug skill-forge; do
  [[ -s ".agents/skills/$s/SKILL.md" ]] && ok "skill: $s" || fail "skill missing: $s"
done

# --- enforcement -------------------------------------------------------------
for s in check-fmt check-crate check-coverage check-drift check-portability; do
  if [[ -x "scripts/$s.sh" ]]; then
    ok "scripts/$s.sh"
  else
    fail "scripts/$s.sh missing or not executable"
  fi
done

[[ -f .pre-commit-config.yaml ]] && ok ".pre-commit-config.yaml" \
  || fail ".pre-commit-config.yaml missing"

installed=0
for h in pre-commit commit-msg pre-push; do
  if [[ -f ".git/hooks/$h" ]] && grep -qs pre-commit ".git/hooks/$h"; then
    installed=$((installed + 1))
  else
    fail "git hook not installed: $h  (run: pre-commit install)"
  fi
done
(( installed == 3 )) && ok "pre-commit hooks installed (pre-commit, commit-msg, pre-push)"

if compgen -G ".github/workflows/*.yml" >/dev/null; then
  # CI must call the scripts, not reimplement the checks, or the two drift.
  if grep -rqs 'scripts/' .github/workflows/; then
    ok "CI workflow calls scripts/"
  else
    fail "CI workflow does not call scripts/ (checks must not be implemented twice)"
  fi
else
  fail "no CI workflow"
fi

# At least one agent-hook adapter, whichever tool the developer uses. Requiring
# a specific vendor's file here would mean M-1 can never pass for someone
# working only in Cursor or Codex, which is the exact failure the portability
# work exists to prevent.
adapters=(.claude/settings.json .cursor/hooks.json .windsurf/hooks.json)
found_adapter=""
for a in "${adapters[@]}"; do
  [[ -f "$a" ]] && found_adapter="$a" && break
done

if [[ -z "$found_adapter" ]]; then
  fail "no agent-hook adapter found (looked for: ${adapters[*]})"
elif grep -qs 'scripts/' "$found_adapter"; then
  ok "agent hooks call scripts/ ($found_adapter)"
else
  fail "$found_adapter does not call scripts/ (accelerator only, never a second implementation)"
fi

# --- portability -------------------------------------------------------------
if grep -rqsil 'portability' docs/internal/product/decisions/ 2>/dev/null; then
  ok "second-tool portability check recorded"
else
  fail "no ADR recording the second-tool portability check (M-1.17)"
fi

# --- drift -------------------------------------------------------------------
echo
for c in check-drift check-portability; do
  if "./scripts/$c.sh" >/dev/null 2>&1; then
    ok "$c.sh"
  else
    fail "$c.sh (run it directly for detail)"
  fi
done

finish
