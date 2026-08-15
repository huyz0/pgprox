# Seven correctness fixes, thirty milestones, and a baseline nobody rewrote

Date: 2026-08-15. `M91.0`. No code changed.

`M90.25`'s own push turned `instruction counts` red for the first time this
session noticed. Bisection found it had actually been red since `M78.0`,
eleven milestones after `M59.1` last wrote the baseline — thirty milestones of
silent drift, all of it legitimate correctness work that nobody re-baselined
after landing. This records what moved, which commit moved it, and why each
one is a fix rather than a regression to revert.

## How this was found

`scripts/bench.sh` needs `valgrind`, which this sandbox has no root to
install via `apt`. `apt-get download valgrind` still works without root — it
only writes to the working directory, not the system — and `dpkg-deb -x`
unpacks the `.deb` without installing it. Pointing `PATH` at the unpacked
`usr/bin` and `VALGRIND_LIB` at its `usr/libexec/valgrind` runs the same
`callgrind` measurement `scripts/bench.sh` already uses, confirmed against six
recent CI runs' own `instruction counts` logs: the ten non-cache figures below
matched CI to the instruction in every sample, the same "bit-identical"
property `run-2026-08-06-ci-baseline.md` already established.

Bisection then walked every commit between `M59.1` (`9c460c0`) and `HEAD` that
touched `pgprox-route/src`, `pgprox-pool/src` or `pgprox-cache/src`, measuring
each one in a separate worktree to isolate exactly which commit moved which
number.

## The ten that hold at their old number

`scan_frame`, `decode_backend_message`, `relay_frame`, `decode_query`,
`decode_error_response`, `serves_a_mix_of_tenants` and `held_read` are
untouched. Their entries in `baseline.json` do not change.

## `pgprox-route`: one commit, most of the cost

| benchmark | was | now | after `M88.3` |
| --- | --- | --- | --- |
| `route_point_select` | 3,716 | 4,093 | 4,076 (97% of the total move) |
| `route_update` | 3,969 | 4,418 | 4,409 (98%) |
| `route_begin` | 1,165 | 1,411 | 1,391 (92%) |

`M88.3` (`2157752`) replaced `split_whitespace` with the shared,
comment-aware `Lexer` in `parse_route_assignment` and `begins_transaction` —
fixing a real bug where a leading comment before `BEGIN` or a route hint hid
the real token behind it, the same defect class `M24` found and this crate
carries a written rule against repeating. Reading through a lexer instead of
splitting on whitespace costs more per call; correctness on a case this crate
explicitly promises to handle is worth it.

The remaining move is `M90.1` (`9c858aa`, `SessionRouter` keeps classifying
for `wrote` once its target is fixed) and `M90.14` (`721ef60`, `RouteCtx`
gains a `wrote` field so a failed post-commit LSN probe still forces the
primary) — both landed this milestone, both already reviewed and tested
findings, not new information.

## `pgprox-pool`: two commits

| benchmark | was | now | after `M76.1` | after `M90.12` |
| --- | --- | --- | --- | --- |
| `acquire_and_release` | 278 | 325 | 299 | 325 |

`M76.1` (`aa4b832`, "a lowered limit reaches the connections that already
exist") added the check that makes a pool ceiling lowered mid-run apply to
connections already checked out, not only to new ones. `M90.12` (`afdc1dc`,
this milestone's own finding) wired `ReapConfig::max_lifetime` into
`Pool::release`, since the documented connection lifetime bound had no caller
anywhere outside its own unit tests before this. Both are enforcement that
was missing, now present, each paid for on every acquire and release.

## `pgprox-cache`: one commit, and six benchmarks that were never bit-identical

`cache_hit`, `cache_hit_rotating`, `cache_miss`, `cache_put` and
`invalidate_a_tenants_entries` are the same five (plus `serves_a_mix_of_
tenants`, which did not move outside noise) `run-2026-08-06-ci-baseline.md`
already found are not bit-identical between a developer machine and a GitHub
runner — HashMap iteration order depends on a per-process random seed, and
these benchmarks' instruction count depends on it. `M59.1` set them from the
median of six CI runs; the same method is used here, six consecutive CI runs
from this session's own recent pushes (`M90.20` through `M90.25`), read from
the archived `instruction counts` logs:

```bash
gh run view <id> --job <instruction counts's job id> --log | grep instructions
```

| benchmark | was | six CI samples | median (new baseline) |
| --- | --- | --- | --- |
| `cache_hit` | 1,461 | 1612, 1614, 1615, 1619, 1620, 1620 | 1,617 |
| `cache_hit_rotating` | 1,810 | 1926, 1927, 1931, 1932, 1932, 1933 | 1,932 |
| `cache_miss` | 1,239 | 1291, 1358, 1362, 1363, 1366, 1507 | 1,363 |
| `cache_put` | 3,695 | 3943, 3943, 3946, 3950, 3953, 3979 | 3,948 |
| `invalidate_a_tenants_entries` | 85,633 | 94229, 94443, 94515, 94637, 94844, 95174 | 94,576 |
| `serves_a_mix_of_tenants` | 38,319 | 38254, 38258, 38259, 38259, 38267, 38267 | 38,259 |

All six moved together at one commit: `M78.0` (`bd068a6`, "the cache key
names how the answer was rendered"), which added result-format tracking to
`CacheKey` so a session that changes how it wants results rendered cannot be
served a cached answer rendered the other way — a correctness fix for a
cache-key collision, not a slowdown. `M88.12`, `M88.18` and `M90.11` (the
ASCII-only fold, this milestone's own finding) move these numbers by single
digits, inside the spread already visible across the six samples above.

## Why now

Nothing between `M78.0` and `M90.25` ran `scripts/bench.sh` and looked at the
result before committing: it is deliberately not part of the pre-commit gate
(`scripts/bench.sh`'s own header: "Measurement is slower and runs when asked
rather than per commit"), and CI's `instruction counts` job has been failing
silently on every push since, without blocking anything, because nothing in
this repository's branch protection or process stops a merge on it. `M90.25`
happened to be the push where a session read the CI run through to its
individual job results rather than trusting the pre-commit-equivalent jobs
alone, which is what surfaced this.
