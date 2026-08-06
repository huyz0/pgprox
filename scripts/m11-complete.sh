#!/usr/bin/env bash
# M11 completion condition: the gaps the finished milestones name are either
# closed or correctly restated.
#
# Written before the milestone needed closing, which is `M10.17`'s lesson: M10's
# gate was named in the roadmap from the day the milestone was filed, did not
# exist, and nothing noticed until every task was done and the milestone could
# not be closed. This one is expected to fail while the milestone is open. That
# is what it is for.
#
# Each check is a recorded artefact rather than a rerun. The runs behind them
# take an hour of wall clock and need Docker; asserting that the record exists
# and says something is what a seconds-long gate can honestly do. Where a claim
# can be checked rather than merely found, it is.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M11: the gaps the completed milestones name"
echo

# --- M11.1: the throughput question -------------------------------------------
#
# `M9.24` said throughput is pinned by the database, so the cache can only
# change who waits. Eight matched pairs say otherwise. The check is that the
# verdict is recorded and that it is a verdict rather than a shrug.
doc="docs/internal/product/perf/run-2026-07-31-throughput.md"
if [[ -f $doc ]]; then
  ok "the throughput question has a recorded run"
else
  fail "$doc missing: M10.9 left the question open and nothing has answered it"
fi

# The count had to be argued before the runs, not after. A power calculation in
# the document is the evidence that it was.
if [[ -f $doc ]] && grep -qs 'power' "$doc"; then
  ok "the number of pairs was argued rather than picked"
else
  fail "$doc does not say how many pairs it took or why"
fi

# --- M11.2: the TLS 1.2 restriction -------------------------------------------
#
# `M8`'s matrix was all TLS 1.3, whose suites are all FIPS-approved, so the
# restriction FIPS actually imposes was never reached.
matrix="docs/internal/product/release/cipher-matrix.md"
if [[ -f $matrix ]] && grep -qs 'TLSv1_2' "$matrix"; then
  ok "the cipher matrix has TLS 1.2 rows"
else
  fail "$matrix has no TLS 1.2 row: it has not tested the restriction it is written for"
fi

# And the rows have to show the difference, not merely exist. A matrix where
# both builds took every suite would mean the restriction does not hold.
if [[ -f $matrix ]] && grep -qs 'refused' "$matrix"; then
  ok "a suite the default build takes is refused by the FIPS build"
else
  fail "$matrix shows no refusal: either FIPS restricts nothing or the probe never reached its policy"
fi

# The two probes are the experiment. The AES one is the control and losing it
# would leave the ChaCha row meaning nothing.
if [[ -x tests/proxy-drivers/psql-tls12-chacha.sh \
   && -x tests/proxy-drivers/psql-tls12-aes.sh ]]; then
  ok "both TLS 1.2 probes are present, including the control"
else
  fail "a TLS 1.2 probe is missing; the ChaCha row means nothing without the AES control beside it"
fi

# --- M11.3: what the cap does, stated correctly -------------------------------
#
# The task asked for a run showing the shed path fire at the connection cap. It
# cannot fire there: `shed::decide` refuses with `NoHeadroomAtHome`. The check
# is on the code rather than on a document, because this is a claim about
# behaviour and the behaviour is what should hold it up.
if grep -qs 'NoHeadroomAtHome' crates/pgprox-cluster/src/shed.rs; then
  ok "shedding still refuses when the home node is full"
else
  fail "shed.rs no longer refuses on headroom: M11.3's finding, and M11.6's premise, are stale"
fi

# The roadmap sentence that was wrong. A gate cannot check prose, but it can
# check that the specific wrong clause has not come back.
if grep -qs 'which is where shedding has to work' docs/internal/product/roadmap.md; then
  fail "the roadmap says the cap is where shedding has to work; M11.3 showed it is where shedding is designed not to"
else
  ok "the roadmap no longer claims the cap is where shedding works"
fi

# --- M11.4 and M11.7: what pinning costs multiplexing -------------------------
#
# ADR 0001 calls this an open question and hands it to the plan. The half that
# can be answered here is the curve: how the upstream connection count moves as
# the share of pinned sessions rises.
#
# This used to glob for `docs/internal/product/perf/*pinning*.md` and pass. It passed on
# `run-2026-07-31-pinning.md`, a document whose own title says it is not the
# curve, which is the exact failure this repo keeps rediscovering: a check that
# tests whether somebody wrote a file, not whether the file answers anything.
#
# So it checks the recorded counts instead. A curve needs a y-axis that moved,
# and it needs to have moved for the right reason, which means the control arm
# has to have been below the cap where it had somewhere to move from.
CURVE=docs/internal/product/perf/curve-*-pinning.tsv
if ! compgen -G "$CURVE" >/dev/null; then
  fail "no pinning curve: ADR 0001's open question has no measured half yet (M11.4 then M11.7)"
else
  verdict_line="$(awk -F'\t' '
    /^#/ || NF < 4 { next }
    { peak[$1] = $2 + 0; pins[$1] = $4 + 0; n++ }
    END {
      if (n < 3) { print "few\t" n; exit }
      if (peak["none"] >= 60) { print "capped\t" peak["none"]; exit }
      if (pins["none"] != 0) { print "pinned\t" pins["none"]; exit }
      span = peak["high"] - peak["none"]
      if (span <= 0) { print "flat\t" span; exit }
      print "ok\t" peak["none"] "\t" peak["high"] "\t" span
    }' $CURVE)"
  # Read from a here-string rather than a pipe. `fail` counts into `_fail_count`,
  # and the right-hand side of a pipeline is a subshell, so a piped version of
  # this printed FAIL and then exited 0 with "all checks passed". Found by
  # checking the exit code of a deliberately bad curve rather than its output,
  # which is the only way this kind of bug is visible.
  IFS=$'\t' read -r verdict a b span <<<"$verdict_line"
  case "$verdict" in
    ok)   ok "the pinning curve moved: upstream $a to $b as the pinned share rose" ;;
    flat) fail "the pinning curve's upstream axis moved by $a connections; this is not a curve" ;;
    capped) fail "the pinning control arm peaked at $a, at the cap, so no arm could rise above it" ;;
    pinned) fail "the pinning control arm pinned $a sessions; its workload has no LISTEN, so the x-axis is not what it claims" ;;
    few)  fail "the pinning curve has $a arms; the task asks for three or more" ;;
    *)    fail "the pinning curve could not be read" ;;
  esac
fi

# --- M11.6: admission when every survivor is full -----------------------------
#
# This globbed `docs/internal/product/perf/*admission*.md` and reported what a full fleet
# tells displaced clients, which a filename cannot say. `M12.4`.
#
# The claim is about two specific SQLSTATEs. The pool distinguishes `53300`,
# no connection available, from `57014`, the wait cancelled, and M11.6's whole
# result is which of them a displaced client sees: neither. A run that does not
# name both has not addressed the question, whatever its filename says.
ADMISSION_DIR="${PGPROX_PERF_DIR:-docs/internal/product/perf}"
admission=""
for run in "$ADMISSION_DIR"/*admission*.md; do
  [[ -f "$run" ]] || continue
  grep -qF '53300' "$run" || continue
  grep -qF '57014' "$run" || continue
  admission="$(basename "$run")"
  break
done

if [[ -n "$admission" ]]; then
  ok "what a full fleet tells displaced clients is recorded, by SQLSTATE ($admission)"
elif compgen -G "$ADMISSION_DIR/*admission*.md" >/dev/null; then
  fail "the admission run does not name both 53300 and 57014, so it does not say what a displaced client is told (M11.6)"
else
  fail "no admission run: what the survivors tell displaced clients is unmeasured (M11.6)"
fi

finish
