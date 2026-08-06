#!/usr/bin/env bash
# Portability audit: would a tool other than Claude Code get the full picture?
#
# check-drift.sh verifies derived files match canonical source. This goes
# further and asks whether the canonical source is actually readable by a tool
# that has never heard of Claude Code, which is the property the whole
# AGENTS.md/SKILL.md choice exists to buy.
#
# Mechanical only. It cannot tell you whether another agent *follows* the
# standards, which is why M-1.17 also requires a human running a real task on a
# second tool.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

# --- AGENTS.md is the canonical file, and is plain Markdown -------------------
if [[ -s AGENTS.md ]]; then
  ok "AGENTS.md exists (the open standard read by 30+ tools)"
else
  fail "AGENTS.md missing: tools other than Claude Code would get no context"
fi

# The @-import syntax is Claude Code specific. It belongs in CLAUDE.md, which is
# the adapter, and must never appear in AGENTS.md, which every tool reads.
if grep -qE '^@[A-Za-z]' AGENTS.md 2>/dev/null; then
  fail "AGENTS.md uses @-import syntax, which only Claude Code understands. Use a Markdown link."
else
  ok "AGENTS.md uses no vendor-specific syntax"
fi

for f in crates/*/AGENTS.md; do
  [[ -f "$f" ]] || continue
  grep -qE '^@[A-Za-z]' "$f" && fail "$f uses @-import syntax"
done

# --- skills parse as the Agent Skills format ---------------------------------
skill_count=0
for s in .agents/skills/*/SKILL.md; do
  [[ -f "$s" ]] || continue
  skill_count=$((skill_count + 1))
  name="$(basename "$(dirname "$s")")"

  # Frontmatter must be valid YAML with name and description. A tool that
  # cannot parse it silently ignores the skill.
  if ! python3 - "$s" <<'PY'
import sys, re
src = open(sys.argv[1], encoding="utf-8").read()
m = re.match(r"^---\n(.*?)\n---\n", src, re.S)
if not m:
    sys.exit(1)
try:
    import yaml
    fm = yaml.safe_load(m.group(1))
except ImportError:
    fm = {}
    for line in m.group(1).splitlines():
        if ":" in line and not line.startswith((" ", "\t", "#")):
            k, v = line.split(":", 1)
            fm[k.strip()] = v.strip()
if not isinstance(fm, dict):
    sys.exit(1)
if not fm.get("name") or not fm.get("description"):
    sys.exit(1)
# The description is the entire retrieval surface until the skill fires. One
# that says only what it does, with no trigger conditions, never fires.
if len(str(fm["description"])) < 40:
    sys.exit(2)
PY
  then
    case $? in
      2) fail "skill $name: description too short to be a retrieval surface" ;;
      *) fail "skill $name: frontmatter is not parseable YAML with name and description" ;;
    esac
  fi
done

if (( skill_count > 0 )); then
  ok "$skill_count skills parse as the Agent Skills format"
else
  fail "no skills found"
fi

# --- skills call scripts, not tool built-ins ---------------------------------
# Script invocation is the one capability every coding agent has. A skill that
# calls a tool-specific built-in works in exactly one tool.
no_script_ref=0
for s in .agents/skills/*/SKILL.md; do
  [[ -f "$s" ]] || continue
  name="$(basename "$(dirname "$s")")"
  # Skills that run nothing are fine; skills that run something must use scripts/
  if grep -qE '^\s*```(bash|sh|console)' "$s" && ! grep -q 'scripts/' "$s"; then
    warn "skill $name: has shell blocks but references no scripts/ entry point"
    no_script_ref=$((no_script_ref + 1))
  fi
done
(( no_script_ref == 0 )) && ok "skills with commands call scripts/"

# --- the scripts themselves depend on nothing exotic -------------------------
for f in scripts/*.sh scripts/gates/*.sh; do
  [[ -f "$f" ]] || continue
  head -1 "$f" | grep -q '^#!/usr/bin/env bash' \
    || fail "$(basename "$f"): needs '#!/usr/bin/env bash' to be portable across machines"
  [[ -x "$f" ]] || fail "$(basename "$f"): not executable"
done
ok "scripts are portable bash and executable"

# --- no script hard-depends on one vendor ------------------------------------
# Naming a vendor path in a comment or in prose is fine and often clearer. What
# breaks portability is executable logic that *requires* one tool's file, since
# that makes the check fail for a developer using a different tool.
#
# So: ignore comments, look only at live lines in scripts and hook configs.
# A line naming two or more vendors is an accept-any-adapter list, which is the
# correct pattern. A line naming exactly one is a hard dependency on that tool.
leaks=0
for f in scripts/*.sh scripts/gates/*.sh .pre-commit-config.yaml .github/workflows/*.yml; do
  [[ -f "$f" ]] || continue
  # These two legitimately inspect vendor paths; that is their job.
  case "$f" in *check-portability.sh|*check-drift.sh) continue;; esac

  while IFS= read -r line; do
    [[ -n "$line" ]] || continue
    body="${line#*:}"
    [[ "$body" =~ ^[[:space:]]*# ]] && continue
    vendors="$(grep -oE '\.(claude|cursor|windsurf)/' <<< "$body" | sort -u | wc -l)"
    if (( vendors == 1 )); then
      (( leaks == 0 )) && fail "executable logic depends on a single vendor's files:"
      leaks=$((leaks + 1))
      printf '        %s:%s\n' "$f" "$line"
    fi
  done < <(grep -nE '\.(claude|cursor|windsurf)/' "$f" 2>/dev/null || true)
done

if (( leaks > 0 )); then
  printf '        Enumerate every adapter you accept, or the check fails for\n'
  printf '        anyone working in a different tool.\n'
else
  ok "no script hard-depends on a single vendor"
fi

finish
