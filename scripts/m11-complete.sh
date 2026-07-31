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
doc="product/perf/run-2026-07-31-throughput.md"
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
matrix="product/release/cipher-matrix.md"
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
if grep -qs 'which is where shedding has to work' product/roadmap.md; then
  fail "the roadmap says the cap is where shedding has to work; M11.3 showed it is where shedding is designed not to"
else
  ok "the roadmap no longer claims the cap is where shedding works"
fi

# --- M11.4: what pinning costs multiplexing -----------------------------------
#
# ADR 0001 calls this an open question and hands it to the plan. The half that
# can be answered here is the curve: how the upstream connection count and the
# median move as the share of pinned sessions rises.
if compgen -G 'product/perf/*pinning*.md' >/dev/null; then
  ok "the pinning curve is recorded"
else
  fail "no pinning curve: ADR 0001's open question has no measured half yet (M11.4)"
fi

# --- M11.6: admission when every survivor is full -----------------------------
if compgen -G 'product/perf/*admission*.md' >/dev/null; then
  ok "what a full fleet tells displaced clients is recorded"
else
  fail "no admission run: what the survivors tell displaced clients is unmeasured (M11.6)"
fi

finish
