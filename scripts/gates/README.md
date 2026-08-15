# Milestone gates

One file per milestone, holding that milestone's completion condition: the
thing that has to keep being true for it to still count as done.

**You are not expected to read these.** They are satisfied rather than
maintained. `m7-complete.sh` is not something anybody edits; it is something
that fails on the day a measurement it named stops being there.

Every one runs in CI on every commit, and `check-drift.sh` fails if a gate
exists and CI does not run it. If you want to know what a gate is checking and
why, the milestone's section in
[roadmap.md](../../docs/internal/product/roadmap.md) says so in prose, names the
script, and is the thing worth reading.

## Not every milestone has one

Fifty-seven milestones, forty-five gates. The gap is deliberate and is the most
confusing thing about this directory, so it is written down rather than left to
be inferred from a missing filename:

| Milestone | What holds it instead |
| --- | --- |
| M1 | [`../conformance.sh`](../conformance.sh), the codec against real Postgres |
| M2 | `cargo nextest run -p pgprox-auth --features integration` |
| M8 | `release-check.sh`, in here, which predates the naming convention |
| M42, M45 | `m41-complete.sh`, which reads the paths those two moved |
| M46, M51 | `../check-drift.sh` |
| M47, M48, M49 | `../check-links.sh` |
| M50 | `../check-readmes.sh` |
| M52 | `../check-coverage.sh` |

A milestone whose condition is an ordinary check does not get a gate of its
own. Writing one that re-ran the same check would be a second place for the
same rule to live.

## The gates

Listed in milestone order, which is not the order `ls` gives: `m1f` sorts
between `m19` and `m20`, and `m3` after `m29`.

| Gate | Milestone | Subject |
| --- | --- | --- |
| `m-1-complete.sh` | M-1 | AI development system |
| `m0-complete.sh` | M0 | contracts and quality gates |
| `m1r-complete.sh` | M1R | protocol revision |
| `m1f-complete.sh` | M1F | full protocol coverage |
| `m3-complete.sh` | M3 | cluster |
| `m4-complete.sh` | M4 | operations |
| `m5-complete.sh` | M5 | pooling and routing |
| `m6-complete.sh` | M6 | integration |
| `m7-complete.sh` | M7 | scale and performance |
| `m9-complete.sh` | M9 | query cache (post-MVP, complete) |
| `m10-complete.sh` | M10 | the claims nothing enforces |
| `m11-complete.sh` | M11 | the gaps the completed milestones name |
| `m12-complete.sh` | M12 | the gates that count files |
| `m13-complete.sh` | M13 | the non-negotiables that nothing enforces |
| `m14-complete.sh` | M14 | the crates mutation testing never reached |
| `m15-complete.sh` | M15 | the protocol crate under a second reading |
| `m16-complete.sh` | M16 | the streaming relay nothing streams through |
| `m17-complete.sh` | M17 | the binaries mutation testing never reached |
| `m18-complete.sh` | M18 | what the deployment story assumes |
| `m19-complete.sh` | M19 | a seam for peer discovery |
| `m20-complete.sh` | M20 | the protocol layer against pgbouncer, pgcat and odyssey |
| `m21-complete.sh` | M21 | the driver matrix does not cover what M20 changed |
| `m22-complete.sh` | M22 | the mutants nobody has swept since M17 |
| `m23-complete.sh` | M23 | the streaming question M16 left open, at the scale one machine has |
| `m24-complete.sh` | M24 | a reading of every crate, and the nine things it found |
| `m25-complete.sh` | M25 | the query cache against pgpool-II |
| `m26-complete.sh` | M26 | what the query cache costs, measured for the first time |
| `m27-complete.sh` | M27 | unsafe becomes a governed exception rather than a closed door |
| `m28-complete.sh` | M28 | the build configuration nobody had measured |
| `m29-complete.sh` | M29 | the first exception the unsafe policy was asked for |
| `m30-complete.sh` | M30 | the same procedure, applied to every crate |
| `m31-complete.sh` | M31 | the comments at M30's optimisation sites |
| `m32-complete.sh` | M32 | the comparison against pgbouncer and pgcat |
| `m33-complete.sh` | M33 | what pgbouncer and pgcat do differently |
| `m34-complete.sh` | M34 | the seventeen kilobytes that are not the buffers |
| `m35-complete.sh` | M35 | per-connection memory is a curve, not a number |
| `m36-complete.sh` | M36 | what an open, quiet connection costs |
| `m37-complete.sh` | M37 | what a spawned task costs beyond its future |
| `m38-complete.sh` | M38 | the extrapolation M36 did not need to make |
| `m39-complete.sh` | M39 | documentation for people who are not this repo |
| `m40-complete.sh` | M40 | a control that only worked where nothing else was broken |
| `m41-complete.sh` | M41 | the docs become a site |
| `m43-complete.sh` | M43 | what it does, and what one request touches |
| `m44-complete.sh` | M44 | the pages a review asks for |
| `m88-complete.sh` | M88 | a second reading of every crate, and the eighteen things it found |
| `m90-complete.sh` | M90 | a third reading, from several angles at once, and what each one found |

## release-check.sh

M8's condition, and the one gate not named for its milestone. It checks that
the FIPS variant is built rather than declared, that the cipher-suite matrix
was recorded, and that the drain sequence is wired into the deployment
manifest.

It is here rather than in `../` because it is a milestone gate and belongs with
them, and it keeps its name because the name is referenced from the roadmap and
from CI. Renaming it would be tidier and would cost more than it returns.
