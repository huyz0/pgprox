#!/usr/bin/env bash
# A `pgprox-core` trait change arrives whole. `M13.5`.
#
#   scripts/check-core-contract.sh          the staged change
#
# `AGENTS.md` non-negotiable 6 and `docs/internal/standards/contracts.md` say a contract
# change is "one atomic commit containing" six things: the trait change, every
# fake, every implementation, every call site, the ADR recording why, and any
# dependent track's spec.
#
# `m0-complete.sh` was credited with this and checks something adjacent and
# static: that every public trait has a fake. That is worth checking and it says
# nothing about a commit, which is what the rule is about. A trait can gain a
# method, leave four implementors broken and no ADR written, and pass it.
#
# ## What is checkable and what is not
#
# Two of the six are mechanical, and they are the two that hurt when missed:
#
#   3. every implementation   the implementors are greppable, and a trait method
#                             added without them is a build break for someone else
#   5. the ADR                a file under docs/internal/product/decisions/ in the same commit
#
# The fakes live in the same file as their trait here, so "the fake was updated"
# is already implied by the trait file being staged and is not worth a check of
# its own. Call sites and dependent specs are not mechanically distinguishable
# from ordinary edits, and pretending otherwise would make this a rule people
# route around. They stay in the skill and in review.
#
# ## Why the trait's method set and not the file
#
# Editing a doc comment on a trait is not a contract change and must not demand
# an ADR, or the rule becomes noise and gets disabled. So this compares the
# `fn` signatures inside each `pub trait` block between HEAD and the index, and
# fires only when that set actually differs.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

CORE_DIR="${PGPROX_CORE_DIR:-crates/pgprox-core/src}"

if ! git rev-parse --verify HEAD >/dev/null 2>&1; then
  skip "core contract (no HEAD to compare against)"
  finish
fi

staged="$(git diff --cached --name-only || true)"
if [[ -z "$staged" ]]; then
  ok "core contract (nothing staged)"
  finish
fi

# The `fn` signatures inside each `pub trait NAME` block, one line per method,
# prefixed by the trait they belong to.
trait_methods() {
  awk '
    /^pub trait [A-Za-z_][A-Za-z0-9_]*/ {
      t = $3
      sub(/[^A-Za-z0-9_].*$/, "", t)
      in_trait = 1
      depth = 0
    }
    in_trait {
      depth += gsub(/\{/, "{") - gsub(/\}/, "}")
      if ($0 ~ /(^|[^A-Za-z0-9_])fn[[:space:]]+[A-Za-z_]/) {
        sig = $0
        sub(/^[[:space:]]+/, "", sig)
        sub(/[[:space:]]+$/, "", sig)
        gsub(/[[:space:]]+/, " ", sig)
        print t "|" sig
      }
      if (depth <= 0 && /\}/) in_trait = 0
    }
  '
}

changed_traits=""
while IFS= read -r file; do
  [[ "$file" == "$CORE_DIR"/*.rs ]] || continue

  before="$(git show "HEAD:$file" 2>/dev/null | trait_methods | sort || true)"
  after="$(git show ":$file" 2>/dev/null | trait_methods | sort || true)"
  [[ "$before" == "$after" ]] && continue

  while IFS= read -r t; do
    [[ -n "$t" ]] || continue
    case " $changed_traits " in
      *" $t "*) ;;
      *) changed_traits="$changed_traits $t" ;;
    esac
  done < <(diff <(printf '%s\n' "$before") <(printf '%s\n' "$after") \
             | grep -E '^[<>]' | sed 's/^[<>] //' | cut -d'|' -f1 | sort -u)
done <<< "$staged"

changed_traits="${changed_traits# }"
if [[ -z "$changed_traits" ]]; then
  ok "core contract (no trait method changed)"
  finish
fi

problems=0
for t in $changed_traits; do
  # Implementors, from the index so the check sees the commit being made.
  #
  # `impl Trait for` or `impl pgprox_core::module::Trait for`, and deliberately
  # not any other path: `impl pb::credential_resolver_server::CredentialResolver`
  # is the generated gRPC service in the mock sidecar, a different trait that
  # happens to share a name.
  while IFS= read -r impl_file; do
    [[ -n "$impl_file" ]] || continue
    if grep -qxF "$impl_file" <<< "$staged"; then
      ok "$t: implementor staged ($impl_file)"
    else
      fail "$t changed and $impl_file implements it, but is not in this commit"
      problems=$(( problems + 1 ))
    fi
  done < <(git grep --cached -lE "impl([[:space:]]*<[^>]*>)?[[:space:]]+(pgprox_core::[a-z_]+::)?${t}([[:space:]]*<[^>]*>)?[[:space:]]+for" -- '*.rs' 2>/dev/null \
             | grep -v "^${CORE_DIR}/" || true)
done

if grep -qE '^docs/internal/product/decisions/.*\.md$' <<< "$staged"; then
  ok "an ADR is in this commit"
else
  fail "a core trait changed (${changed_traits// /, }) and no ADR is in this commit"
  problems=$(( problems + 1 ))
fi

if (( problems > 0 )); then
  printf '\n       docs/internal/standards/contracts.md: a contract change is one atomic commit\n'
  printf '       with the trait, every fake, every implementation, every call\n'
  printf '       site, the ADR recording why including what was rejected, and\n'
  printf '       any dependent spec. The contract-change skill walks this.\n'
fi

finish
