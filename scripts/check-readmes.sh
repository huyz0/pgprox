#!/usr/bin/env bash
# Every crate has a README, and the relationships it claims are the real ones.
#
#   scripts/check-readmes.sh              the workspace
#   PGPROX_CRATE_DIRS='a/* b/*' ...       a set of directories, for the negative suite
#
# # What is checkable about a page of prose
#
# The part a reader relies on and cannot verify: which other crates this one is
# built on. That is a list in `Cargo.toml`, so a README naming a different set
# is checkable against it, and a dependency added later without a word in the
# README leaves the crate map wrong in the one file somebody reads first.
#
# Both directions, because they fail differently. A dependency the manifest has
# and the README does not is a reader with an incomplete picture. A crate the
# README names that does not exist is a reader sent to look for something that
# was renamed or deleted, which wastes more of their time.
#
# # What it does not check
#
# That the prose is true. Nothing can. It checks the one claim with a machine
# readable source of truth beside it, and `scripts/check-links.sh` checks that
# the links out of it resolve.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "Crate READMEs"
echo

CRATE_DIRS="${PGPROX_CRATE_DIRS:-crates/* bin/*}"
# shellcheck disable=SC2206  # deliberate word splitting: the caller passes globs
DIRS=($CRATE_DIRS)

# Every workspace crate that exists, so a README naming one that does not can be
# told apart from a README naming one this check simply did not look at.
known=""
for dir in "${DIRS[@]}"; do
  [[ -f "$dir/Cargo.toml" ]] || continue
  known+=" $(sed -n 's/^name = "\(.*\)"$/\1/p' "$dir/Cargo.toml" | head -1)"
done

if [[ -z "${known// /}" ]]; then
  fail "no crates found under $CRATE_DIRS, so this check has nothing to say"
  finish
fi

missing=""
undocumented=""
invented=""
checked=0

for dir in "${DIRS[@]}"; do
  [[ -f "$dir/Cargo.toml" ]] || continue
  name="$(sed -n 's/^name = "\(.*\)"$/\1/p' "$dir/Cargo.toml" | head -1)"

  if [[ ! -f "$dir/README.md" ]]; then
    missing+=" $name"
    continue
  fi
  checked=$((checked + 1))

  # Path dependencies only. A crates.io dependency is not part of the crate map
  # a reader is being shown, and listing every one of them in prose would be a
  # second `Cargo.toml` that nobody maintains.
  #
  # `[dev-dependencies]` are included. `pgprox-testkit` exists only as one, and
  # a README that could not mention it would be the wrong shape for the one
  # crate whose entire relationship to the workspace is that it is a test-only
  # dependency of two others.
  for dep in $(sed -n 's/^\(pgprox-[a-z]*\) = { path.*/\1/p' "$dir/Cargo.toml" | sort -u); do
    grep -q "\`$dep\`" "$dir/README.md" || undocumented+=" $name:$dep"
  done

  for named in $(grep -ohE '`pgprox-[a-z]+`' "$dir/README.md" | tr -d '`' | sort -u); do
    grep -qw -- "$named" <<<"$known" || invented+=" $name:$named"
  done
done

if [[ -z "$missing" ]]; then
  ok "every crate has a README ($checked)"
else
  fail "these crates have no README, and a reader landing in one has nothing:$missing"
fi

if [[ -z "$undocumented" ]]; then
  ok "every crate a README is built on is named in it"
else
  fail "these dependencies are in Cargo.toml and not in the README:$undocumented"
  printf '       the crate map is wrong in the file somebody reads first\n'
fi

if [[ -z "$invented" ]]; then
  ok "no README names a crate this workspace does not have"
else
  fail "these READMEs name crates that do not exist:$invented"
fi

finish
