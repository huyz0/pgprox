#!/usr/bin/env bash
# M44: the pages a review asks for.
#
#   scripts/m44-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# # What is checkable about five prose pages
#
# The same thing `M43` found: a page that claims a list is a page that can go
# quietly short. Five lists here have a source of truth in the code, and each
# one is read from there rather than restated.
#
# The list this milestone found already wrong is `SHOW`. `M39` documented
# `SHOW MEM`, which the parser has a test rejecting by name. Nothing compared
# the page against the enum, so it sat there through four milestones of docs
# work. That is the shape every check below is for.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M44: the pages a review asks for"
echo

DOCS="${PGPROX_DOCS:-docs}"
MULTI="$DOCS/multitenancy.md"
ADMIN="$DOCS/admin.md"
CLUSTER="$DOCS/clustering.md"
SECURITY="$DOCS/security.md"
FIPS="$DOCS/fips.md"
OPT="$DOCS/optimizations.md"

for page in "$MULTI" "$ADMIN" "$CLUSTER" "$SECURITY" "$FIPS" "$OPT"; do
  [[ -f "$page" ]] && ok "$(basename "$page") is there" \
    || fail "$page is missing, and the navigation links to it"
done

# --- the SHOW surface ---------------------------------------------------------
#
# Both directions, because they fail differently. A command the code has and the
# page does not is an operator who never learns it exists. A command the page has
# and the code does not is an operator typing it during an incident and getting
# an error, which is the one that wastes time at the worst moment, and it is the
# one that was actually here.
targets="$(sed -n 's/^            Self::[A-Za-z]* => "\([a-z]*\)",$/\1/p' \
           crates/pgprox-admin/src/show.rs | sort -u)"

if [[ -z "$targets" ]]; then
  fail "no SHOW targets could be read from the parser, so nothing below means anything"
else
  undocumented=""
  for target in $targets; do
    grep -qi "SHOW ${target}\`" "$ADMIN" || undocumented+=" $target"
  done
  if [[ -z "$undocumented" ]]; then
    ok "every SHOW command the parser accepts is on the admin page ($(wc -w <<<"$targets"))"
  else
    fail "these SHOW commands exist and the admin page does not list them:$undocumented"
  fi

  # `SHOW LOCAL ...` in prose matches the pattern with LOCAL as its target, so
  # it is excluded here rather than being reported as a command that does not
  # exist. The scope prefix is checked by nothing, which is the honest state.
  invented=""
  for page in "$ADMIN" "$DOCS/operations.md"; do
    for word in $(grep -oE 'SHOW (LOCAL )?[A-Z]{3,}' "$page" \
                  | sed 's/^SHOW //; s/^LOCAL //' | sort -u); do
      [[ "$word" == "LOCAL" ]] && continue
      grep -qx "$(tr '[:upper:]' '[:lower:]' <<<"$word")" <<<"$targets" \
        || invented+=" $word"
    done
  done
  if [[ -z "$invented" ]]; then
    ok "no page documents a SHOW command the parser would refuse"
  else
    fail "these SHOW commands are documented and the parser rejects them:$invented"
    printf '       an operator types one of these during an incident and gets an error\n'
  fi
fi

# --- the HTTP admin surface ---------------------------------------------------
#
# The route list in that crate is the router rather than a copy of it, so this
# reads the same declaration axum does. A path served with no line on the page
# is a capability nobody outside the source can find.
paths="$(sed -n 's/^    \(get\|post\) "\([^"]*\)".*/\2/p' crates/pgprox-admin/src/api.rs)"

if [[ -z "$paths" ]]; then
  fail "no admin API paths could be read from the router declaration"
else
  missing=""
  for path in $paths; do
    grep -qF "$path" "$ADMIN" || missing+=" $path"
  done
  if [[ -z "$missing" ]]; then
    ok "every admin API path is on the admin page ($(wc -w <<<"$paths"))"
  else
    fail "these endpoints are served and appear on no page:$missing"
  fi
fi

# --- the algorithm allowlist --------------------------------------------------
#
# The security page states which JWT algorithms are accepted. That list is a
# constant, and a reader who trusts a stale copy of it configures their token
# service to sign with something this proxy rejects at connect time.
algorithms="$(sed -n 's/^pub const ALLOWED_ALGORITHMS.*= &\[\(.*\)\];$/\1/p' \
              crates/pgprox-auth/src/jwt.rs | tr -d '"' | tr ',' ' ')"

if [[ -z "$algorithms" ]]; then
  fail "the JWT algorithm allowlist could not be read, so the page is unchecked"
else
  absent=""
  for alg in $algorithms; do
    grep -qF "$alg" "$SECURITY" || absent+=" $alg"
  done
  if [[ -z "$absent" ]]; then
    ok "every accepted JWT algorithm is named on the security page ($(wc -w <<<"$algorithms"))"
  else
    fail "these algorithms are accepted and the security page does not name them:$absent"
  fi
fi

# --- the crates that forbid unsafe --------------------------------------------
#
# The claim is "no unsafe in the crates that read what a peer chose", and it is
# worth exactly the list behind it. That list is a judgement about which code an
# unauthenticated peer's bytes reach, so it is read from where the judgement
# lives rather than from every crate that happens to carry the attribute:
# `pgprox-cache` forbids unsafe too and is deliberately not on it, having been
# the one candidate the policy was asked about and refused.
forbidding="$(sed -n '/^CLOSED=(/,/^)$/p' scripts/check-unsafe.sh \
              | sed -n 's/^  \(pgprox-[a-z]*\).*/\1/p')"

if [[ -z "$forbidding" ]]; then
  fail "the closed crate list could not be read, so the security page is unchecked"
else
  unnamed=""
  for crate in $forbidding; do
    grep -qF "\`$crate\`" "$SECURITY" || unnamed+=" $crate"
  done
  if [[ -z "$unnamed" ]]; then
    ok "every crate that forbids unsafe is named on the security page ($(wc -w <<<"$forbidding"))"
  else
    fail "these crates forbid unsafe and the security page does not name them:$unnamed"
  fi
fi

# --- the cache key ------------------------------------------------------------
#
# The isolation claim on two pages is that the query cache key carries six
# things. A seventh added without a word on either page leaves a reader with a
# wrong picture of what separates one tenant's answers from another's, which is
# the one mistake this whole page set exists to prevent.
fields="$(sed -n '/^pub struct CacheKey {$/,/^}$/p' crates/pgprox-core/src/cache.rs \
          | grep -c '^    pub ')"
words=(zero one two three four five six seven eight nine ten)

if (( fields == 0 || fields >= ${#words[@]} )); then
  fail "the cache key has $fields fields, which this check cannot put into words"
elif grep -qi "all ${words[$fields]}" "$MULTI" && grep -qi "all ${words[$fields]}" "$DOCS/features.md"; then
  ok "both pages describing the cache key agree it has ${words[$fields]} components"
else
  fail "the cache key has ${words[$fields]} components and the pages claim otherwise"
  printf '       a reader is told less separates two tenants answers than actually does\n'
fi

# --- the optimization figures -------------------------------------------------
#
# The "after" column of that table is the committed baseline. Rebaselining a hot
# path is a deliberate act with a reason in the commit message, and this makes
# updating the page part of it rather than something to notice later.
table="$(sed -n '/^## The four that mattered$/,/^## [^T]/p' "$OPT" | grep '^| `')"

if [[ -z "$table" ]]; then
  fail "$OPT has no table of measured paths, so its figures are unchecked"
else
  stale=""
  unknown=""
  while IFS= read -r row; do
    bench="$(awk -F'|' '{print $2}' <<<"$row" | tr -d ' `')"
    claimed="$(awk -F'|' '{print $4}' <<<"$row" | tr -d ' ,')"
    actual="$(sed -n "s/.*::${bench}\": \([0-9]*\).*/\1/p" product/perf/baseline.json)"
    if [[ -z "$actual" ]]; then
      unknown+=" $bench"
    elif [[ "$claimed" != "$actual" ]]; then
      stale+=" $bench(page $claimed, baseline $actual)"
    fi
  done <<<"$table"

  [[ -n "$unknown" ]] && fail "the page names benchmarks the baseline does not have:$unknown"
  if [[ -n "$stale" ]]; then
    fail "these figures no longer match the committed baseline:$stale"
  elif [[ -z "$unknown" ]]; then
    ok "every figure the page quotes is the number CI enforces ($(wc -l <<<"$table"))"
  fi
fi

# --- M44.1: where an edit link actually lands ---------------------------------
#
# Starlight builds a page's edit URL with `new URL(path, baseUrl)`, against the
# path the content collection stored. Two settings in two files, and they are
# only wrong together: while the collection read `../docs`, every path began
# `../`, resolution spent it on the base rather than the path, and `edit/main/`
# became `edit/`. Fourteen pages linking to a branch called `docs`, invisible
# outside the built output.
#
# So this resolves one the way a browser would rather than pattern-matching
# either setting, which is what makes it survive the collection moving again.
# Textual, because it has to run on a machine with no Node.
CONFIG="${PGPROX_ASTRO_CONFIG:-docs/astro.config.mjs}"
COLLECTION="${PGPROX_ASTRO_COLLECTION:-docs/src/content.config.ts}"

# `new URL`, for the part of it these two settings can reach: an absolute base
# ending in a directory, and a relative reference with `.` and `..` in it.
resolve_url() {
  local base="$1" rel="$2" scheme rest host part
  local -a segs=() parts=()
  scheme="${base%%://*}"
  rest="${base#*://}"
  host="${rest%%/*}"

  local IFS=/
  read -r -a parts <<<"${rest#*/}"
  for part in "${parts[@]}"; do
    case "$part" in ''|.) ;; *) segs+=("$part") ;; esac
  done
  read -r -a parts <<<"$rel"
  for part in "${parts[@]}"; do
    case "$part" in
      ''|.) ;;
      ..) (( ${#segs[@]} )) && unset 'segs[-1]' && segs=("${segs[@]}") ;;
      *) segs+=("$part") ;;
    esac
  done
  printf '%s://%s/%s\n' "$scheme" "$host" "${segs[*]}"
}

edit_base="$(sed -n "s/^        baseUrl: '\([^']*\)',$/\1/p" "$CONFIG")"
collection_base="$(sed -n "s/.*base: '\([^']*\)'.*/\1/p" "$COLLECTION")"
sample="$(basename "$(find "$DOCS" -maxdepth 1 -name '*.md' | sort | head -1)")"

if [[ -z "$edit_base" ]]; then
  fail "$CONFIG configures no edit link, so every page's edit button is absent"
elif [[ -z "$collection_base" || -z "$sample" ]]; then
  fail "the collection's base or a page to test it with could not be read"
else
  landed="$(resolve_url "$edit_base" "$collection_base/$sample")"
  # The branch has to survive, and the path after it has to be where the page
  # really is. Both halves matter: a base that eats the branch fails the first,
  # and a collection moved without the base following fails the second.
  if [[ "$landed" =~ /edit/[^/]+/$DOCS/$sample$ ]]; then
    ok "an edit link resolves to $landed"
  else
    fail "an edit link resolves to $landed, which is not $DOCS/$sample on a branch"
    printf '       the base and the collection are each fine alone and wrong together\n'
  fi
fi

# --- the cluster timings ------------------------------------------------------
#
# The clustering page publishes the numbers an operator sizes a deployment
# around, and every one of them is a constant somewhere. A default that moves
# without the page moving is worse than an undocumented one: somebody plans a
# rolling upgrade around a recovery window that is no longer true.
#
# The takeover wait is derived rather than read, because that is how the code
# gets it: `effective_lease` takes the maximum of what is configured and
# `ttl + suspect_after`, so a page quoting a configured value would be quoting
# something the coordinator may overrule.
suspect="$(sed -n 's/^            suspect_after: Duration::from_secs(\([0-9]*\)),$/\1/p' \
           crates/pgprox-cluster/src/membership.rs | head -1)"
dead="$(sed -n 's/^            dead_after: Duration::from_secs(\([0-9]*\)),$/\1/p' \
        crates/pgprox-cluster/src/membership.rs | head -1)"
ttl="$(sed -n '/^impl Default for LeaseConfig {$/,/^}$/p' crates/pgprox-cluster/src/lease.rs \
       | sed -n 's/^            ttl: Duration::from_secs(\([0-9]*\)),$/\1/p')"
peer="$(sed -n 's/^pub const PEER_TIMEOUT: Duration = Duration::from_secs(\([0-9]*\));$/\1/p' \
        bin/pgprox/src/gossip.rs)"
home="$(sed -n 's/^            home_share: \([0-9.]*\),$/\1/p' crates/pgprox-cluster/src/reservation.rs)"
decay="$(sed -n 's/^            decay_rounds: \([0-9]*\),$/\1/p' crates/pgprox-cluster/src/reservation.rs)"

if [[ -z "$suspect" || -z "$dead" || -z "$ttl" || -z "$peer" || -z "$home" || -z "$decay" ]]; then
  fail "a cluster default could not be read, so the clustering page is unchecked"
  printf '       suspect=%s dead=%s ttl=%s peer=%s home=%s decay=%s\n' \
    "${suspect:-?}" "${dead:-?}" "${ttl:-?}" "${peer:-?}" "${home:-?}" "${decay:-?}"
else
  wrong=""
  for pair in "peer doubted:${suspect}s" "peer dropped:${dead}s" "lease TTL:${ttl}s" \
              "peer timeout:${peer}s" "takeover wait:$((ttl + suspect))s" \
              "home share:$home" "reservation decay:$decay"; do
    grep -qF "${pair#*:}" "$CLUSTER" || wrong+=" ${pair%%:*}"
  done
  if [[ -z "$wrong" ]]; then
    ok "every cluster default the page publishes is the one the code uses (7)"
  else
    fail "these defaults have moved and the clustering page still has the old ones:$wrong"
    printf '       somebody sizes a rolling upgrade around a window that is not real\n'
  fi
fi

# --- the FIPS instructions ----------------------------------------------------
#
# Two things a reader will actually type or look for. A build target that is not
# a stage produces an error naming the target, and a provider string that has
# drifted means an operator cannot tell a validated pod from a default one,
# which is the whole reason that field is logged.
if grep -qE '^FROM .* AS fips$' deploy/Dockerfile; then
  ok "the image target the FIPS page tells a reader to build is a real stage"
else
  fail "deploy/Dockerfile has no stage named fips, and the page says to build one"
fi

provider="$(sed -n 's/^        "\(aws-lc-rs-fips\)"$/\1/p' crates/pgprox-tls/src/lib.rs)"
if [[ -z "$provider" ]]; then
  fail "the FIPS provider name could not be read, so the page's log line is unchecked"
elif grep -qF "$provider" "$FIPS"; then
  ok "the startup line the page tells an operator to look for is the one emitted"
else
  fail "a FIPS build logs $provider and the page tells a reader to expect something else"
fi

finish
