#!/usr/bin/env bash
# M23: the streaming question M16 left open, at the scale one machine has.
#
#   scripts/m23-complete.sh
#
# Under `M12.8`'s constraint: no check here may match a filename or a word where
# it can run something and read an exit code.
#
# # What this gate does not do
#
# It does not run the scale runs. They need a Postgres, a sidecar and a proxy,
# and they take minutes each; `scripts/scale.sh` is the thing that runs them.
#
# What it can check without any of that is the property that makes the pair a
# comparison rather than two numbers: the two workloads differ in their
# statements and nowhere else. Every derived workload in this directory claims
# that in its header and nothing has ever checked one.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

cd "$REPO_ROOT"

echo "M23: streaming under concurrency, at the scale one machine has"
echo

REFERENCE="${PGPROX_WORKLOAD_REF:-product/perf/workload.yaml}"
LARGE="${PGPROX_WORKLOAD_LARGE:-product/perf/workload-large.yaml}"

for path in "$REFERENCE" "$LARGE" product/perf/run-2026-08-03-streaming-concurrent.md; do
  if [[ -f "$path" ]]; then
    ok "$path exists"
  else
    fail "$path is missing"
  fi
done

if [[ ! -f "$REFERENCE" || ! -f "$LARGE" ]]; then
  finish
fi

# Every field except `statements`, compared. A run against each is a comparison
# only if one thing differs, and the header of every derived workload here says
# it is derived rather than edited. `M7.55`'s pair rests on the same claim and
# nothing checks it either.
#
# Read with a parser rather than by diffing text, so a comment or a reordering
# is not a failure and a changed value is.
compare="$(python3 - "$REFERENCE" "$LARGE" <<'PY'
import sys, re

def load(path):
    """The top-level blocks of one workload, by key, as raw text."""
    blocks, key = {}, None
    for line in open(path):
        if re.match(r'^[a-z_]+:', line):
            key = line.split(':', 1)[0]
            blocks[key] = [line.rstrip()]
        elif key and line.strip() and not line.lstrip().startswith('#'):
            blocks[key].append(line.rstrip())
    return {k: '\n'.join(v) for k, v in blocks.items()}

a, b = load(sys.argv[1]), load(sys.argv[2])
if set(a) != set(b):
    print('the two workloads do not have the same fields:',
          sorted(set(a) ^ set(b)))
    raise SystemExit
differ = sorted(k for k in a if a[k] != b[k])
print(' '.join(differ) if differ else 'nothing')
PY
)"

case "$compare" in
  statements)
    ok "the two workloads differ in their statements and nowhere else"
    ;;
  nothing)
    fail "$LARGE is identical to $REFERENCE, so a run against each compares nothing"
    ;;
  *)
    fail "the two workloads differ in more than their statements: $compare"
    printf '       a run against each would have more than one cause for any difference\n'
    ;;
esac

finish
