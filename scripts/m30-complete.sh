#!/usr/bin/env bash
# M30: the same procedure, applied to every crate.
#
#   scripts/m30-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# # How it passes while the milestone is open
#
# The same way M19 through M29 did: it checks what has landed rather than what
# is planned, and a finding gets its check in the commit that fixes it.
#
# That would be a gate anyone could pass by ticking a task and adding nothing,
# so the first check is the one that closes it: every M30 task the backlog marks
# done must be named here.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M30: the same procedure, applied to every crate"
echo

BACKLOG="${PGPROX_BACKLOG:-product/backlog.md}"
SELF="${BASH_SOURCE[0]}"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Each finding, named by the test that would fail if it came back. `--exact`
# with a name nothing matches exits non-zero, so this cannot pass by describing
# a test that is no longer there.
#
# No pipeline into `grep -q`: `set -o pipefail` is on and grep exits at its
# first match, which closes the pipe and can kill `cargo test` with SIGPIPE.
run_finding() {
  local crate="$1" name="$2" finding="$3"
  local out="$WORK/$crate-$RANDOM.out"

  cargo test -p "$crate" --all-targets -- --exact "$name" >"$out" 2>>"$WORK/log"
  if grep -q "^test $name \.\.\. ok$" "$out"; then
    ok "$finding"
  else
    fail "$finding: $crate $name did not run and pass"
    printf '       a finding this milestone fixed has no test standing behind it\n'
  fi
}

# A figure from the gated baseline, held under a ceiling. Used where what landed
# is a measurement rather than a behaviour: the test says the code is still
# right, and this says it is still fast, which are different claims.
under() {
  local key="$1" ceiling="$2"
  local measured
  measured="$(python3 -c "import json; print(json.load(open('product/perf/baseline.json')).get('$key', 999999))")"
  if (( measured < ceiling )); then
    ok "$key is $measured, under the $ceiling it was before"
  else
    fail "$key is $measured, back at or above the $ceiling this milestone moved it from"
  fi
}

# --- every finished task is checked here -------------------------------------
#
# Without this, a task ticked in the backlog and absent from this script is a
# finding nothing stands behind, and the gate would go on reporting green for
# the ones that did land.
#
# Two tasks are about the milestone rather than about a finding: `M30.0`
# planned it and `M30.7` closes it. Excluded by name rather than by a rule, so
# a third exclusion has to be written down here to exist.
finished="$(sed -n '/^## M30:/,/^## /p' "$BACKLOG" \
  | sed -n 's/^- \[x\] `\(M30\.[0-9]*\)`.*/\1/p' \
  | grep -vE '^M30\.(0|7)$' || true)"

if [[ -z "$finished" ]]; then
  ok "no finding has been ticked yet, so none can be unchecked"
else
  unchecked=""
  while read -r task; do
    [[ -z "$task" ]] && continue
    grep -q "^# --- $task:" "$SELF" || unchecked+=" $task"
  done <<<"$finished"

  if [[ -z "$unchecked" ]]; then
    ok "every finished M30 task has checks here ($(wc -w <<<"$finished"))"
  else
    fail "these M30 tasks are ticked and nothing here checks them:$unchecked"
    printf '       a finding reported fixed with no test standing behind it\n'
  fi
fi

# --- the findings that have landed -------------------------------------------

# --- M30.1: a statement lexed twice to read one word --------------------------
#
# The behaviour, by the test that fails if the early exit stops being sound.
# Two mutations were run against it before it was trusted: dropping the `SET`
# second-word guard, and dropping the opener check. Both fail it.
run_finding pgprox-route \
  classify::tests::a_statement_that_cannot_open_a_transaction_is_answered_by_its_first_words \
  "a statement that cannot open a transaction is answered by its first words"

# And the number, so a later change that quietly puts the second pass back is
# caught here rather than in a profile nobody runs. The figures before were
# 6,444 and 6,717.
under pgprox-route::route_point_select 5200
under pgprox-route::route_update 4800

# --- M30.2: every word compared against every keyword -------------------------
#
# Three checks rather than one, because the filter can be wrong in two
# directions and only one of them is visible in a benchmark. Letting a keyword
# through is a write classified as a read; rejecting nothing is a filter that
# costs and buys nothing, and every other test in the crate passes either way
# because the scan behind it still runs.
run_finding pgprox-route \
  classify::properties::the_filter_lets_every_word_on_every_list_through \
  "the filter lets every word on every list through"
run_finding pgprox-route \
  classify::properties::the_filter_is_a_filter_and_not_an_answer \
  "the filter rejects something, and does not answer for the scan"
run_finding pgprox-route \
  classify::properties::the_filter_and_the_scan_agree_on_everything \
  "the filter and the scan agree on words generated next to the lists"

# The route decision after both findings. It was 6,444 and 6,717.
under pgprox-route::route_point_select 4000
under pgprox-route::route_update 4200

# --- M30.3: a cryptographic hash over an integer this process issued ----------
#
# The hasher itself, by the test that says it is a hash rather than an identity
# function. A `HashMap` takes its bucket from the low bits and its control byte
# from the top seven, so a mixer that scrambles one end and not the other keeps
# working and quietly compares more keys on every lookup.
run_finding pgprox-core hash::tests::every_output_bit_moves \
  "every output bit of the issued-id mixer moves"
run_finding pgprox-core hash::tests::consecutive_ids_do_not_collide \
  "ids off one counter do not collide"
run_finding pgprox-core hash::tests::the_whole_hasher_avalanches_and_not_just_its_finalizer \
  "the whole hasher avalanches, and not just its finalizer"
run_finding pgprox-core hash::tests::every_field_reaches_the_hash \
  "every field of a key reaches the hash"

# And the number.
under pgprox-pool::acquire_and_release 350

# The rule is that peer-chosen keys keep RandomState, so the check is on where
# the fast hasher is allowed to appear at all. A file, not a line: the cache
# holds both halves of the rule in one declaration, `HashMap<TenantId,
# HashSet<Slot, IssuedIds>>`, where the outer key is a tenant from a client's
# token and the inner one is an index this file issues. A line-level check reads
# that as a violation and a file-level one asks the right question, which is
# whether somebody decided who chooses the key.
#
# Adding a file here is the friction, and it is the point: putting this hasher
# on a map is a decision about the key, and it should cost a line in a gate.
ALLOWED_TO_HASH_FAST=(
  crates/pgprox-core/src/hash.rs      # where it is defined
  crates/pgprox-pool/src/pool.rs      # checked_out, keyed on UpstreamId
  crates/pgprox-pool/src/live.rs      # connections, keyed on UpstreamId
  crates/pgprox-cache/src/store.rs    # the slot set inside by_tenant
)

stray=""
while read -r file; do
  [[ -n "$file" ]] || continue
  found=""
  for allowed in "${ALLOWED_TO_HASH_FAST[@]}"; do
    [[ "$file" == "$allowed" ]] && found=1
  done
  [[ -n "$found" ]] || stray+=" $file"
done <<<"$(grep -rl --include='*.rs' 'IssuedIds' crates bin 2>/dev/null || true)"

if [[ -z "$stray" ]]; then
  ok "the unseeded hasher appears only where a key this process issues is"
else
  fail "these files reach for the unseeded hasher and nothing decided about their keys:$stray"
  printf '       who chooses the key decides the hasher, see standards/security.md\n'
fi

# --- M30.4: a 16 KiB memset before every read ---------------------------------
#
# The behaviour first. `read_buf` fills the spare capacity that is there and
# asks for no more, so the reserve is what keeps a held read the size it was.
# Nothing else notices its absence: the frame still assembles and the buffer
# still stays small, and the only symptom is the syscall count.
run_finding pgprox-session \
  shell::tests::a_held_read_makes_room_for_a_whole_read_before_it_reads \
  "a held read makes room for a whole read before it reads"
run_finding pgprox-session \
  shell::tests::a_mid_frame_read_grows_the_buffer_by_one_read_and_no_more \
  "and grows the buffer by one read and no more"

# And the number. It was 18,669, of which 16,406 was the memset.
under pgprox-session::held_read 4000

# The whole finding is that no unsafe was needed for any of it, so the policy
# that would have governed it still reports nothing to govern.
if scripts/check-unsafe.sh >/dev/null 2>&1; then
  ok "the memset went without an exception to the unsafe policy"
else
  fail "scripts/check-unsafe.sh does not pass"
fi

# --- M30.5: the validation the policy will not let anyone skip ----------------
#
# The milestone's one negative result, so the check is the negative: the
# document exists, the crate it is about is still shut, and the two figures it
# compares against have not moved underneath it.
RUN="${PGPROX_RUN_DOC:-product/perf/run-2026-08-05-utf8-validation.md}"
if [[ -f "$RUN" ]]; then
  ok "$RUN records what the closed list costs"
else
  fail "$RUN is missing, so the price of the closed list is a claim in a commit message"
fi

# `#![forbid(unsafe_code)]` in the crate's own lib.rs is what refused the
# exception. check-unsafe.sh holds the whole list; this names the one crate this
# document is about, so a change that took pgprox-proto off the list fails here
# with the reason rather than only there.
if grep -q '^#!\[forbid(unsafe_code)\]' crates/pgprox-proto/src/lib.rs; then
  ok "pgprox-proto still forbids unsafe in its own lib.rs"
else
  fail "pgprox-proto no longer forbids unsafe, so the document describes a refusal"
  printf '       that did not happen\n'
fi

under pgprox-proto::decode_query 420
under pgprox-proto::decode_error_response 2000

# --- M30.6: a second benchmark that moved with a random seed ------------------
#
# Not `run_finding`: what landed is the shape of a measurement, and the place
# that is visible is the baseline. The check is `M28.2`'s, against the rule
# `standards/testing.md` now states: a gated benchmark measures at least a
# thousand instructions, because below that a `HashMap` probe count decides
# whether it passes.
served="$(python3 -c "import json; print(json.load(open('product/perf/baseline.json')).get('pgprox-cache::serves_a_mix_of_tenants', 0))")"
if (( served > 10000 )); then
  ok "the serves benchmark measures $served instructions, well past seed noise"
else
  fail "the serves benchmark is $served instructions, back in the range where"
  printf '       a HashMap probe count decides whether it passes\n'
fi

# And the unstable one is gone rather than carried beside its replacement, which
# would leave it still gating CI.
if grep -q '"pgprox-cache::serves"' product/perf/baseline.json; then
  fail "the baseline still carries pgprox-cache::serves, which is the unstable one"
else
  ok "the unstable benchmark is gone rather than kept beside its replacement"
fi

finish
