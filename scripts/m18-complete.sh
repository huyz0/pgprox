#!/usr/bin/env bash
# M18: what the deployment story assumes.
#
#   scripts/m18-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code. This milestone is about documents
# rather than code, which makes that constraint harder rather than optional: the
# defect it fixed was prose nobody could check, so a gate that checked prose by
# grepping for a word would be the same defect in a new place.
#
# Every check below either runs another check and reads its exit code, or asks
# the filesystem a question with one answer.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M18: what the deployment story assumes"
echo

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# --- M18.1: an ADR may not name a library nobody depends on ------------------
#
# Run rather than described. `check-drift.sh` holds the rule, and it takes a
# scan root so this can point it at a planted ADR instead of asserting against
# the real ones, which would pass for as long as they happen to be right.
mkdir -p "$WORK/adr"
cat > "$WORK/adr/0001-planted.md" <<'PLANT'
# 0001. A decision about a library that is not here

## Decision

Gossip over UDP using `notacrate`, seeded from DNS.
PLANT
if PGPROX_ADR_ROOTS="$WORK/adr/*.md" scripts/check-drift.sh >"$WORK/drift.out" 2>&1; then
  fail "check-drift.sh accepted an ADR naming a library nothing depends on"
else
  ok "an ADR naming a library nobody depends on is refused"
fi

# And the real ones pass, which is the other half: a rule that only ever fires
# is as useless as one that never does.
if scripts/check-drift.sh >"$WORK/drift-real.out" 2>&1; then
  ok "every library this repo's ADRs say they use is depended on"
else
  fail "check-drift.sh fails on the tree as it stands"
  sed 's/^/       /' "$WORK/drift-real.out"
fi

# --- M18.1: the transport ADR 0004 describes is the one that exists ----------
#
# The two facts that were wrong for eight milestones, asked of the code rather
# than of the ADR. If either comes back, the ADR is fiction again.
if grep -rqE '^ *foca( |=)' Cargo.toml crates/*/Cargo.toml bin/*/Cargo.toml 2>/dev/null; then
  ok "there is a foca dependency, so ADR 0004 may describe SWIM again"
else
  if grep -rqn "UdpSocket" --include="*.rs" crates bin 2>/dev/null; then
    fail "something speaks UDP but no foca dependency exists: re-read ADR 0004"
  else
    ok "the gossip transport is not UDP, which is what ADR 0004 now says"
  fi
fi

# --- M18.2: the spec exists and says the thing it exists to say --------------
#
# One assertion about content, and it is the safety rule rather than a keyword.
# A spec that lost this sentence would be a spec that permits the change it was
# written to forbid.
#
# Matched against the file flattened to one line, with blockquote markers and
# runs of whitespace squeezed. The rule is a sentence and sentences get
# rewrapped; a check that broke every time somebody reflowed a paragraph would
# be edited out within a week.
SPEC="docs/internal/specs/2026-08-02-peer-discovery-seam/spec.md"
flat() { sed 's/^[[:space:]]*>[[:space:]]*/ /' "$1" | tr '\n' ' ' | tr -s ' '; }
if [[ -f "$SPEC" ]] && flat "$SPEC" | grep -q "never cause a node to be counted alive that gossip has not heard from"; then
  ok "the peer discovery spec states the rule that keeps the seam safe"
else
  fail "$SPEC is missing or no longer states what an external source may not do"
fi

for part in contracts.md tests.md tasks.md; do
  if [[ -s "docs/internal/specs/2026-08-02-peer-discovery-seam/$part" ]]; then
    ok "the spec has its $part"
  else
    fail "docs/internal/specs/2026-08-02-peer-discovery-seam/$part is missing or empty"
  fi
done

# --- M18.3: a milestone may not close without a way to check it --------------
#
# Run, with a planted roadmap, for the same reason as the ADR case above.
mkdir -p "$WORK/roadmap"
cat > "$WORK/roadmap/roadmap.md" <<'PLANT'
| Milestone | Name | State |
| --- | --- | --- |
| M99 | A milestone nobody can check | complete |

## M99: a milestone nobody can check (complete)

It claims to be done and says nothing about how anyone would know.
PLANT
if PGPROX_ROADMAP="$WORK/roadmap/roadmap.md" scripts/check-drift.sh >"$WORK/rm.out" 2>&1; then
  fail "check-drift.sh accepted a milestone with no way to check it"
else
  ok "a milestone with no way to check it is refused"
fi

finish
