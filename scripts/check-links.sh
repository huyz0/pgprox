#!/usr/bin/env bash
# Every relative link in a Markdown file points at something that is there.
#
#   scripts/check-links.sh                the whole repository
#   PGPROX_MD_FILES='a.md b.md' ...       a set of files, for the negative suite
#
# # Why this exists
#
# `check-drift.sh` already checks the links out of `AGENTS.md`, because those
# are the ones that send a reader to a standard. Nothing checked the other
# hundred and forty. When this was written fifteen were broken, all in
# `docs/internal/product/roadmap.md`, and they had been broken across several milestones:
# every link it makes to a run document and every link it makes to a page uses
# one `../` too many, as if the file lived a directory deeper than it does.
#
# That is the failure mode worth having a check for. Not a typo, which somebody
# notices, but a consistent misreading of where a file sits, which produces
# dozens of wrong links that all look right and none of which anybody clicks
# until they need one.
#
# # What it does not check
#
# The path, not the fragment. `[x](features.md#no-such-heading)` passes here,
# because resolving an anchor means reproducing the site generator's heading
# slugs, and two implementations of that is two chances to disagree. The
# fragment half is worth doing and is not done.
#
# Absolute URLs are not fetched. A check that hits the network is a check that
# fails when GitHub is slow, and it would run in a pre-commit hook.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "Markdown links"
echo

FILES="${PGPROX_MD_FILES:-}"
if [[ -z "$FILES" ]]; then
  mapfile -t MD_FILES < <(git ls-files '*.md' 2>/dev/null || true)
else
  # shellcheck disable=SC2206  # deliberate word splitting: the caller passes globs
  MD_FILES=($FILES)
fi

if (( ${#MD_FILES[@]} == 0 )); then
  fail "no Markdown files to check, which is not a tree this can say anything about"
  finish
fi

checked=0
broken=0

for file in "${MD_FILES[@]}"; do
  [[ -f "$file" ]] || continue
  dir="$(dirname "$file")"

  while read -r href; do
    [[ -n "$href" ]] || continue

    # Absolute, protocol-relative, root-relative, and pure anchors are not this
    # check's business. Neither is `...`, which the ADR skill uses to show the
    # shape of a link rather than to make one.
    [[ "$href" =~ ^([a-z]+:|//|/|#) ]] && continue
    [[ "$href" == "..." ]] && continue

    path="${href%%#*}"
    [[ -n "$path" ]] || continue

    checked=$((checked + 1))
    # `-e` rather than `-f`: several links name a directory on purpose, and the
    # shell resolves the `..` in a path against the filesystem, which is what
    # makes this a real resolution rather than a string comparison.
    if [[ ! -e "$dir/$path" ]]; then
      fail "$file links to $href, which is not there"
      broken=$((broken + 1))
    fi
  done < <(grep -oE '\]\([^)[:space:]]+\)' "$file" | sed 's/^](//; s/)$//')
done

if (( broken == 0 )); then
  ok "every relative Markdown link resolves ($checked across ${#MD_FILES[@]} files)"
else
  printf '       %s of %s relative links do not resolve\n' "$broken" "$checked"
fi

finish
