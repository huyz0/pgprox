#!/usr/bin/env bash
# M39: documentation for people who are not this repo.
#
#   scripts/m39-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# # What a gate can check about prose
#
# Not whether it is any good. What it can check is the two ways a doc site rots
# without anybody noticing: a link that stops resolving, and a reference that
# stops naming what the code reads. Both are mechanical and both are the reason
# `M13` exists, applied to the outside of the repo rather than the inside.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M39: documentation for people who are not this repo"
echo

BACKLOG="${PGPROX_BACKLOG:-product/backlog.md}"
SELF="${BASH_SOURCE[0]}"
DOCS="${PGPROX_DOCS:-docs}"

finished="$(sed -n '/^## M39:/,/^## /p' "$BACKLOG" \
  | sed -n 's/^- \[x\] `\(M39\.[0-9]*\)`.*/\1/p' \
  | grep -vE '^M39\.(0|2)$' || true)"

if [[ -z "$finished" ]]; then
  ok "no finding has been ticked yet, so none can be unchecked"
else
  unchecked=""
  while read -r task; do
    [[ -z "$task" ]] && continue
    grep -q "^# --- $task:" "$SELF" || unchecked+=" $task"
  done <<<"$finished"

  if [[ -z "$unchecked" ]]; then
    ok "every finished M39 task has checks here ($(wc -w <<<"$finished"))"
  else
    fail "these M39 tasks are ticked and nothing here checks them:$unchecked"
  fi
fi

# --- the findings that have landed -------------------------------------------

# --- M39.1: a documentation site, and a README that routes to it --------------

for page in index getting-started configuration operations architecture performance; do
  if [[ -f "$DOCS/$page.md" ]]; then
    ok "$DOCS/$page.md is there"
  else
    fail "$DOCS/$page.md is missing, and something links to it"
  fi
done

[[ -f README.md ]] && ok "a README exists" || fail "there is no README"

# Every relative link resolves. A doc site's first failure mode is a link that
# stopped pointing at anything, and it is invisible to a reader who never
# clicks it.
broken=""
while read -r src target; do
  [[ -z "$target" ]] && continue
  case "$target" in
    http*|\#*) continue ;;
  esac
  resolved="$(dirname "$src")/${target%%#*}"
  [[ -e "$resolved" ]] || broken+=" $src -> $target"
done < <(grep -ohEn '\]\([^)]+\)' README.md "$DOCS"/*.md --with-filename \
         | sed -E 's/^([^:]+):[0-9]+:\]\((.*)\)$/\1 \2/')

if [[ -z "$broken" ]]; then
  ok "every relative link in the docs resolves"
else
  fail "these links point at nothing:$broken"
fi

# The configuration reference names what the code reads. A field added to the
# document's schema and not written down is a setting nobody outside this repo
# can discover, which is the whole gap the milestone exists to close.
missing=""
for field in $(sed -n '/^pub struct Config {/,/^}/p' crates/pgprox-core/src/config.rs \
               | sed -n 's/^    pub \([a-z_]*\):.*/\1/p'); do
  grep -q "\`$field\`" "$DOCS/configuration.md" || missing+=" $field"
done
if [[ -z "$missing" ]]; then
  ok "the configuration reference names every field the document has"
else
  fail "these configuration fields are undocumented:$missing"
fi

# And the same for the arguments a node takes, read from the parser's own match
# arms rather than derived from the struct's field names.
#
# Deriving them failed on its first run and the check was wrong rather than the
# document: `--peer` is repeatable, so its field is `peers` and the flag is
# singular. Reading the arms also keeps the two deliberate typos the parser's
# tests use, `--conifg` and `--nope`, out of the list, which scanning the whole
# file would not.
argsmissing=""
for flag in $(awk '/pub fn parse/,/^    }$/' bin/pgprox/src/entry.rs \
              | grep -oE '"--[a-z-]+"' | sort -u | tr -d '"'); do
  grep -q -- "$flag" "$DOCS/configuration.md" || argsmissing+=" $flag"
done
if [[ -z "$argsmissing" ]]; then
  ok "the configuration reference names every argument a node takes"
else
  fail "these arguments are undocumented:$argsmissing"
fi

finish
