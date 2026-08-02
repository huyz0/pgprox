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

# The scan roots are a variable so `tests/gates/negative.sh` can point the rule
# at planted files instead of writing them into `scripts/`. A test that plants a
# deliberately broken script in the tree it is testing leaves it there when it
# is interrupted, and this runs in pre-commit.
SHELL_ROOTS="${PGPROX_SHELL_ROOTS:-scripts/*.sh tests/gates/*.sh}"

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

# --- a pass/fail threshold is not a setting -----------------------------------
#
# AGENTS.md's non-negotiable 2 is that a threshold is never lowered to make a
# check pass. A `${NAME:-95}` default is a threshold anyone can lower, including
# by accident from an exported variable, and the gate then reports its own
# weakened bar as a pass: `ok coverage (pgprox-route): 99.65% >= 10%`. `M13.1`.
#
# This is about pass/fail thresholds only. Most `${X:-n}` defaults in scripts/
# are run parameters, and overriding a connection count, duration, seed or port
# is what they exist for. The list below is the values that decide whether a
# check passes.
threshold_settable=0
for t in COVERAGE_MIN BENCH_TOLERANCE PGPROX_SCALE_MINIMUM TOLERANCE_PERCENT SCALE_MINIMUM; do
  while read -r hit; do
    [[ -n "$hit" ]] || continue
    fail "$hit sets a pass/fail threshold from the environment; make it a constant (M13.1)"
    threshold_settable=1
    # Comments stripped first. The first version of this flagged lib.sh on the
    # comment explaining why COVERAGE_MIN is a constant, which is M12.8's
    # mistake repeated: matching text that looks like the thing, not the thing.
  done < <(for f in $SHELL_ROOTS; do
             [[ -f "$f" ]] || continue
             sed 's/#.*//' "$f" | grep -qE "\\$\\{$t:-" && echo "$f"
           done)
done
(( threshold_settable == 0 )) && ok "no pass/fail threshold can be moved from the environment"

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

# --- a milestone in the status table can be checked ---------------------------
#
# `M18.3`. The rule below this one walks `scripts/m*-complete.sh` and requires
# each to be named in CI. That is the wrong direction and it is why `M16` and
# `M17` both closed with nothing to run: it checks that the gates that exist are
# wired, never that a milestone has one. `M10.17` established that a milestone
# whose completion condition does not exist cannot be closed, and `M12` spent a
# milestone on gates that cannot fail. This was both at once, and it passed.
#
# What is required is a fenced `bash` block in the milestone's own roadmap
# section, and that every `scripts/...` path inside it exists. Not an
# `mNN-complete.sh`: three milestones legitimately point elsewhere. `M1`'s gate
# is `scripts/conformance.sh 17 18`, `M2`'s is a `cargo nextest` invocation and
# `M8`'s is four scripts led by `scripts/release-check.sh`. A rule demanding the
# naming convention would have failed all three and been turned off, which is
# the failure mode `M12.8` names: a check people route around is worse than no
# check.
ROADMAP="${PGPROX_ROADMAP:-product/roadmap.md}"

if [[ -f "$ROADMAP" ]]; then
  ungated=0
  # The status table's first column, skipping the header and separator rows.
  while read -r milestone; do
    [[ -n "$milestone" ]] || continue
    # The section, from its heading to the next one. `awk` rather than `sed`
    # because a milestone id is a regex metacharacter waiting to happen.
    block="$(awk -v want="## $milestone: " '
      index($0, want) == 1 { inside = 1; next }
      inside && /^## / { exit }
      inside { print }
    ' "$ROADMAP")"

    if [[ -z "$block" ]]; then
      fail "$milestone is in the roadmap's status table with no section: nothing says how it would be checked"
      ungated=1
      continue
    fi

    fenced="$(printf '%s\n' "$block" | awk '/^```bash/ { inside = 1; next } inside && /^```/ { inside = 0 } inside { print }')"
    if [[ -z "$fenced" ]]; then
      fail "$milestone names no command: a milestone with no completion condition cannot be closed"
      ungated=1
      continue
    fi

    # Any script it points at has to be there. This is the half that catches a
    # gate renamed out from under the roadmap.
    while read -r named; do
      [[ -n "$named" ]] || continue
      if [[ ! -f "$named" ]]; then
        fail "$milestone names $named, which does not exist"
        ungated=1
      fi
    done < <(printf '%s\n' "$fenced" | grep -oE '(scripts|tests)/[A-Za-z0-9_./-]+\.sh' | sort -u)
  done < <(awk -F'|' '/^\| *M[-0-9A-Z.]+ *\|/ { gsub(/ /, "", $2); print $2 }' "$ROADMAP")
  (( ungated == 0 )) && ok "every milestone in the roadmap names a way to check it"
fi

# --- a library an ADR says it uses is a library that is depended on ----------
#
# `M18.1` found ADR 0004 describing "SWIM gossip over UDP using `foca`". There
# is no `foca` in any `Cargo.toml` and no `UdpSocket` in the workspace: the
# transport is TCP carrying JSON over a peer list from `--peer`. The ADR had
# said so since M0 and nothing objected, because an ADR is prose and prose is
# not checked.
#
# The rule is narrow on purpose. It matches the one construction an ADR uses to
# name a dependency, "using `x`", and asks whether `x` appears as a dependency
# somewhere. Two ADRs use it: 0003 names `tonic`, which is real, and 0004 named
# `foca`, which was not. A broader rule over every backticked word would match
# field names, SQL functions and other crates' internals, and a check that
# cries wolf is a check people learn to skip.
#
# Blockquoted lines are skipped. An ADR that corrects itself quotes what it used
# to claim, and `0004` quotes the `foca` sentence it was wrong about: matching
# there would make the check fire on the record of its own finding, and the fix
# would be to delete the record. A `>` prefix is the Markdown for "this is what
# was said", which is exactly the thing this rule must not read as a decision.
# The scan root is a variable for the reason `SHELL_ROOTS` above is one: so a
# planted case in `tests/gates/negative.sh` never has to write a broken ADR into
# `product/decisions/` and hope it is cleaned up.
ADR_ROOTS="${PGPROX_ADR_ROOTS:-product/decisions/*.md}"

adr_named=0
# shellcheck disable=SC2086
for adr in $ADR_ROOTS; do
  [[ -f "$adr" ]] || continue
  while read -r crate; do
    [[ -n "$crate" ]] || continue
    if ! grep -rqE "^(${crate}|${crate} )[[:space:]]*=" Cargo.toml crates/*/Cargo.toml bin/*/Cargo.toml 2>/dev/null; then
      fail "$adr says it uses \`$crate\`, which is not a dependency of anything: an ADR nobody can check is prose"
      adr_named=1
    fi
  done < <(grep -vE '^[[:space:]]*>' "$adr" | grep -oE 'using `[a-z0-9_-]+`' | sed 's/using `//; s/`//')
done
(( adr_named == 0 )) && ok "every library an ADR says it uses is depended on"

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

# --- AGENTS.md names scripts that exist ---------------------------------------
#
# The non-negotiables each credit a script now, which is only worth anything if
# the script is there. `M13` found rule 5 crediting `check-layering.sh`, which
# enforces a different rule, and rules 2, 3 and 7 crediting nothing at all while
# the sentence above them said all seven were enforced. A named script that does
# not exist is the same failure with less ambiguity. `M13.6`.
missing_script=0
while read -r script; do
  [[ -n "$script" ]] || continue
  if [[ ! -f "$script" ]]; then
    fail "AGENTS.md names $script, which does not exist"
    missing_script=1
  elif [[ ! -x "$script" ]]; then
    fail "AGENTS.md names $script, which is not executable"
    missing_script=1
  fi
done < <(grep -oE 'scripts/[a-z0-9-]+\.sh' AGENTS.md | sort -u)
(( missing_script == 0 )) && ok "every script AGENTS.md names exists and runs"

# --- standards referenced by AGENTS.md actually exist ------------------------
missing=0
while read -r link; do
  [[ -f "$link" || -d "$link" ]] || { fail "AGENTS.md links to missing path: $link"; missing=1; }
done < <(grep -oE '\]\((standards|product|\.agents)/[^)]*\)' AGENTS.md | sed 's/^](//; s/)$//' | sort -u)
(( missing == 0 )) && ok "every path AGENTS.md links to exists"

finish
