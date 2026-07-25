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
  [[ -s "standards/$f.md" ]] && ok "standards/$f.md" || fail "standards/$f.md missing or empty"
done

# --- product docs ------------------------------------------------------------
for f in mission architecture roadmap backlog plan; do
  [[ -s "product/$f.md" ]] && ok "product/$f.md" || fail "product/$f.md missing or empty"
done

# --- ADRs: one per row of the decisions table in the plan --------------------
adr_count="$(find product/decisions -name '[0-9]*.md' 2>/dev/null | wc -l)"
if (( adr_count >= 10 )); then
  ok "ADRs: $adr_count records"
else
  fail "ADRs: found $adr_count, expected at least 10 (one per decision-table row)"
fi
for adr in product/decisions/[0-9]*.md; do
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
for s in check-fmt check-crate check-coverage check-drift; do
  if [[ -x "scripts/$s.sh" ]]; then
    ok "scripts/$s.sh"
  else
    fail "scripts/$s.sh missing or not executable"
  fi
done

[[ -f lefthook.yml ]] && ok "lefthook.yml" || fail "lefthook.yml missing"

if [[ -d .git/hooks ]] && grep -rqs lefthook .git/hooks 2>/dev/null; then
  ok "lefthook hooks installed"
else
  fail "lefthook hooks not installed (run: lefthook install)"
fi

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

if [[ -f .claude/settings.json ]]; then
  if grep -qs 'scripts/' .claude/settings.json; then
    ok "agent hooks call scripts/"
  else
    fail "agent hooks do not call scripts/ (accelerator only, never a second implementation)"
  fi
else
  fail ".claude/settings.json missing (agent hook accelerator)"
fi

# --- portability -------------------------------------------------------------
if grep -rqsil 'portability' product/decisions/ 2>/dev/null; then
  ok "second-tool portability check recorded"
else
  fail "no ADR recording the second-tool portability check (M-1.17)"
fi

# --- drift -------------------------------------------------------------------
echo
if ./scripts/check-drift.sh >/dev/null 2>&1; then
  ok "check-drift.sh"
else
  fail "check-drift.sh (run it directly for detail)"
fi

finish
