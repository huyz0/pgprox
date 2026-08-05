#!/usr/bin/env bash
# M41: the docs become a site.
#
#   scripts/m41-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# # What this checks and what CI checks
#
# The site's toolchain is in `docsite/` and the pages it builds are in `docs/`,
# so the repository root stays a Rust project. The build itself runs in
# `.github/workflows/docs.yml`, on a runner with Node.
# This gate runs beside the Rust ones on a machine that may have neither Node
# nor the dependency tree, so it checks the things that decide whether that
# build produces a working site: that the pages carry what the generator needs,
# and that the two settings whose absence produces a site where every internal
# link 404s are still there.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M41: the docs become a site"
echo

BACKLOG="${PGPROX_BACKLOG:-product/backlog.md}"
SELF="${BASH_SOURCE[0]}"
DOCS="${PGPROX_DOCS:-docs}"
CONFIG="${PGPROX_ASTRO_CONFIG:-docsite/astro.config.mjs}"

finished="$(sed -n '/^## M41:/,/^## /p' "$BACKLOG" \
  | sed -n 's/^- \[x\] `\(M41\.[0-9]*\)`.*/\1/p' \
  | grep -vE '^M41\.(0|2)$' || true)"

if [[ -z "$finished" ]]; then
  ok "no finding has been ticked yet, so none can be unchecked"
else
  unchecked=""
  while read -r task; do
    [[ -z "$task" ]] && continue
    grep -q "^# --- $task:" "$SELF" || unchecked+=" $task"
  done <<<"$finished"

  if [[ -z "$unchecked" ]]; then
    ok "every finished M41 task has checks here ($(wc -w <<<"$finished"))"
  else
    fail "these M41 tasks are ticked and nothing here checks them:$unchecked"
  fi
fi

# --- the findings that have landed -------------------------------------------

# --- M41.1: a site built from docs/, deployed to Pages ------------------------

# Every page carries a title. Starlight refuses to build without one, so a page
# added with none breaks the site rather than appearing untitled, and it breaks
# it on the runner rather than here unless this says so first.
untitled=""
for page in "$DOCS"/*.md; do
  head -5 "$page" | grep -q '^title:' || untitled+=" $(basename "$page")"
done
if [[ -z "$untitled" ]]; then
  ok "every page carries the title the generator requires"
else
  fail "these pages have no title and the site will not build:$untitled"
fi

# Every page is in the navigation. A page that builds and appears in no sidebar
# is reachable only by search or by guessing its URL, which is the quiet way a
# doc site loses a page.
unlisted=""
for page in "$DOCS"/*.md; do
  name="$(basename "$page" .md)"
  [[ "$name" == "index" ]] && continue
  grep -q "/$name/" "$CONFIG" || unlisted+=" $name"
done
if [[ -z "$unlisted" ]]; then
  ok "every page appears in the site navigation"
else
  fail "these pages build and are in no sidebar, so nothing links to them:$unlisted"
fi

# The two settings that decide whether internal links work at all. A project
# page is served under /<repo>/, and `astro dev` serves from the root, so
# getting `base` wrong produces a site that is correct locally and 404s on every
# internal link once published. That is not visible without deploying.
for setting in "site:" "base:"; do
  grep -q "$setting" "$CONFIG" || fail "$CONFIG sets no $setting, so published links will not resolve"
done
grep -q 'site:' "$CONFIG" && grep -q 'base:' "$CONFIG" \
  && ok "the site knows where it will be served from"

# The rewriter, which is what lets one set of Markdown serve this site and
# GitHub at once. Without it the pages' own links stay literal `.md` and every
# one of them 404s on the site.
if grep -q 'rehypePlugins' "$CONFIG" && [[ -f docsite/src/rewrite-links.mjs ]]; then
  ok "the link rewriter is wired in, so one source serves both readers"
else
  fail "the link rewriter is missing, so the pages' relative .md links will 404"
fi

# And the lockfile, because the workflow installs from it rather than resolving
# afresh. Without one committed, `npm ci` fails and the site never publishes.
if [[ -f docsite/package-lock.json ]]; then
  ok "the dependency tree is locked, which is what the workflow installs from"
else
  fail "no docsite/package-lock.json, so the publish workflow cannot install"
fi

finish
