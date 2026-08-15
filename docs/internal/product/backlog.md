# Backlog

One task equals one commit equals one change that leaves the tree green. If a
task cannot be finished in one green commit, split it before writing code.

Task IDs are stable. Completed tasks stay here with their commit reference so
the history of why something was done survives.

Decomposition rule: only the current milestone is decomposed in detail. Future
milestones stay as roadmap entries until their turn, because decomposing them
early produces tasks that are wrong by the time they are reached.

## Contents

`M85.0` added this list so a reader can jump to a milestone instead of
scrolling or grepping for its number. `scripts/check-drift.sh` keeps it honest:
a heading added here without a line below fails the same way an unindexed
script or an unwired gate already does.

<!-- toc:start -->
- [M-1: AI development system](#m-1-ai-development-system)
- [M0: contracts and quality gates](#m0-contracts-and-quality-gates)
- [M1: protocol and TLS (track A)](#m1-protocol-and-tls-track-a)
- [M1R: protocol revision (streaming and test breadth)](#m1r-protocol-revision-streaming-and-test-breadth)
- [M1F: full protocol coverage](#m1f-full-protocol-coverage)
- [M3: cluster (track C)](#m3-cluster-track-c)
- [M2: auth and sidecar (track B)](#m2-auth-and-sidecar-track-b)
- [M5: pooling and routing (track E)](#m5-pooling-and-routing-track-e)
- [M4: operations (track D)](#m4-operations-track-d)
- [M6: integration](#m6-integration)
- [M7: scale and performance](#m7-scale-and-performance)
- [M8: FIPS and release](#m8-fips-and-release)
- [M9: query cache (post-MVP)](#m9-query-cache-post-mvp)
- [M10: the claims nothing enforces](#m10-the-claims-nothing-enforces)
- [M11: the gaps the completed milestones name](#m11-the-gaps-the-completed-milestones-name)
- [M12: the gates that count files](#m12-the-gates-that-count-files)
- [M13: the non-negotiables that nothing enforces](#m13-the-non-negotiables-that-nothing-enforces)
- [M14: the crates mutation testing never reached](#m14-the-crates-mutation-testing-never-reached)
- [M15: the protocol crate under a second reading](#m15-the-protocol-crate-under-a-second-reading)
- [M16: the streaming relay nothing streams through](#m16-the-streaming-relay-nothing-streams-through)
- [M17: the assumptions the last two milestones wrote down](#m17-the-assumptions-the-last-two-milestones-wrote-down)
- [M18: what the deployment story assumes](#m18-what-the-deployment-story-assumes)
- [M19: a seam for peer discovery](#m19-a-seam-for-peer-discovery)
- [M20: the protocol layer against pgbouncer, pgcat and odyssey](#m20-the-protocol-layer-against-pgbouncer-pgcat-and-odyssey)
- [M21: the driver matrix does not cover what M20 changed](#m21-the-driver-matrix-does-not-cover-what-m20-changed)
- [M22: the mutants nobody has swept since M17](#m22-the-mutants-nobody-has-swept-since-m17)
- [M23: the streaming question M16 left open, at the scale one machine has](#m23-the-streaming-question-m16-left-open-at-the-scale-one-machine-has)
- [M24: a reading of every crate, and the nine things it found](#m24-a-reading-of-every-crate-and-the-nine-things-it-found)
- [M25: the query cache against pgpool-II, and the three things it has that we do not](#m25-the-query-cache-against-pgpool-ii-and-the-three-things-it-has-that-we-do-not)
- [M26: what the query cache costs, measured for the first time](#m26-what-the-query-cache-costs-measured-for-the-first-time)
- [M27: unsafe becomes a governed exception rather than a closed door](#m27-unsafe-becomes-a-governed-exception-rather-than-a-closed-door)
- [M28: the build configuration nobody had measured](#m28-the-build-configuration-nobody-had-measured)
- [M29: the first exception the unsafe policy was asked for](#m29-the-first-exception-the-unsafe-policy-was-asked-for)
- [M30: the same procedure, applied to every crate](#m30-the-same-procedure-applied-to-every-crate)
- [M31: the comments at M30's optimisation sites](#m31-the-comments-at-m30s-optimisation-sites)
- [M32: the comparison against pgbouncer and pgcat](#m32-the-comparison-against-pgbouncer-and-pgcat)
- [M33: what pgbouncer and pgcat do differently](#m33-what-pgbouncer-and-pgcat-do-differently)
- [M34: the seventeen kilobytes that are not the buffers](#m34-the-seventeen-kilobytes-that-are-not-the-buffers)
- [M35: every per-connection memory figure so far was two numbers added together](#m35-every-per-connection-memory-figure-so-far-was-two-numbers-added-together)
- [M36: what an open, quiet connection costs](#m36-what-an-open-quiet-connection-costs)
- [M37: what a spawned task costs beyond the future it holds](#m37-what-a-spawned-task-costs-beyond-the-future-it-holds)
- [M38: the extrapolation M36 did not need to make](#m38-the-extrapolation-m36-did-not-need-to-make)
- [M39: documentation for people who are not this repo](#m39-documentation-for-people-who-are-not-this-repo)
- [M40: a control that only worked where nothing else was broken](#m40-a-control-that-only-worked-where-nothing-else-was-broken)
- [M41: the docs become a site](#m41-the-docs-become-a-site)
- [M42: the site's toolchain leaves the repository root](#m42-the-sites-toolchain-leaves-the-repository-root)
- [M43: what it does, and what one request touches](#m43-what-it-does-and-what-one-request-touches)
- [M44: the pages a review asks for](#m44-the-pages-a-review-asks-for)
- [M45: one directory for the pages and the thing that builds them](#m45-one-directory-for-the-pages-and-the-thing-that-builds-them)
- [M46: the licence three files have claimed and none granted](#m46-the-licence-three-files-have-claimed-and-none-granted)
- [M47: the links nothing was checking](#m47-the-links-nothing-was-checking)
- [M48: the design record moves under docs/](#m48-the-design-record-moves-under-docs)
- [M49: one place for what a run leaves behind](#m49-one-place-for-what-a-run-leaves-behind)
- [M50: a README in every crate](#m50-a-readme-in-every-crate)
- [M51: eighty scripts and no index](#m51-eighty-scripts-and-no-index)
- [M52: two failures from the CI replay, and what each turned out to be](#m52-two-failures-from-the-ci-replay-and-what-each-turned-out-to-be)
- [M53: the scripts read as stale, and two of them were](#m53-the-scripts-read-as-stale-and-two-of-them-were)
- [M54: the repository URL was aspirational](#m54-the-repository-url-was-aspirational)
- [M55: the first push found a dependency CI never installed](#m55-the-first-push-found-a-dependency-ci-never-installed)
- [M56: what the instrumentation finally showed](#m56-what-the-instrumentation-finally-showed)
- [M57: the cancel test discarded the line it was waiting for](#m57-the-cancel-test-discarded-the-line-it-was-waiting-for)
- [M58: the milestone job kept finding tools it did not have](#m58-the-milestone-job-kept-finding-tools-it-did-not-have)
- [M59: a benchmark that broke CI on a commit that did not touch it](#m59-a-benchmark-that-broke-ci-on-a-commit-that-did-not-touch-it)
- [M60: three gates read history and the runner had one commit](#m60-three-gates-read-history-and-the-runner-had-one-commit)
- [M61: five gates that ran suites and threw away the result](#m61-five-gates-that-ran-suites-and-threw-away-the-result)
- [M62: the evidence helper could not read coloured output](#m62-the-evidence-helper-could-not-read-coloured-output)
- [M63: a warning that killed the gate printing it](#m63-a-warning-that-killed-the-gate-printing-it)
- [M64: the allocation budgets counted the whole process](#m64-the-allocation-budgets-counted-the-whole-process)
- [M65: the index page did not say what fleet this is for](#m65-the-index-page-did-not-say-what-fleet-this-is-for)
- [M66: the site stopped being published and every check was green](#m66-the-site-stopped-being-published-and-every-check-was-green)
- [M67: every action was on a runtime with a removal date](#m67-every-action-was-on-a-runtime-with-a-removal-date)
- [M68: the docs said what read routing decides, never how](#m68-the-docs-said-what-read-routing-decides-never-how)
- [M69: a replica set that never changed after the first grant](#m69-a-replica-set-that-never-changed-after-the-first-grant)
- [M70: the document's server entries did not reach the cluster layer](#m70-the-documents-server-entries-did-not-reach-the-cluster-layer)
- [M71: a demoted primary could be handed to a new client for five minutes](#m71-a-demoted-primary-could-be-handed-to-a-new-client-for-five-minutes)
- [M72: an established session had no way to learn a corrected primary](#m72-an-established-session-had-no-way-to-learn-a-corrected-primary)
- [M73: a failed dial had exactly one chance, unconditionally](#m73-a-failed-dial-had-exactly-one-chance-unconditionally)
- [M74: an authenticated client that went quiet had no way to be closed](#m74-an-authenticated-client-that-went-quiet-had-no-way-to-be-closed)
- [M75: upstream TLS sent a ClientHello where Postgres expects a request](#m75-upstream-tls-sent-a-clienthello-where-postgres-expects-a-request)
- [M76: the allowance was divided into the pools, with a floor under the division](#m76-the-allowance-was-divided-into-the-pools-with-a-floor-under-the-division)
- [M77: the chart put an unauthenticated write API on the tenants' address](#m77-the-chart-put-an-unauthenticated-write-api-on-the-tenants-address)
- [M78: the cache key named which rows, not what the bytes said](#m78-the-cache-key-named-which-rows-not-what-the-bytes-said)
- [M79: the fixture gap M75.0 named](#m79-the-fixture-gap-m750-named)
- [M80: a dial that failed said the server was full](#m80-a-dial-that-failed-said-the-server-was-full)
- [M81: a test that counted everything the fake heard](#m81-a-test-that-counted-everything-the-fake-heard)
- [M82: the flake that will explain itself next time](#m82-the-flake-that-will-explain-itself-next-time)
- [M83: the pages still described a six-part cache key](#m83-the-pages-still-described-a-six-part-cache-key)
- [M84: the suites that need Docker, run against this session's changes](#m84-the-suites-that-need-docker-run-against-this-sessions-changes)
- [M85: eighty-seven milestones and no way to jump to one](#m85-eighty-seven-milestones-and-no-way-to-jump-to-one)
- [M86: the status table nobody kept adding rows to](#m86-the-status-table-nobody-kept-adding-rows-to)
- [M87: the mutants nobody has swept since M22](#m87-the-mutants-nobody-has-swept-since-m22)
- [M88: a second reading of every crate, and the eighteen things it found](#m88-a-second-reading-of-every-crate-and-the-eighteen-things-it-found)
- [M89: the review from outside this repo, and the four gaps it found](#m89-the-review-from-outside-this-repo-and-the-four-gaps-it-found)
- [M90: a third reading, from several angles at once, and what each one found](#m90-a-third-reading-from-several-angles-at-once-and-what-each-one-found)
<!-- toc:end -->

## M-1: AI development system

- [x] `M-1.1` Repository bootstrap. `git init`, `.gitignore`, plan copied to
  `product/plan.md`, roadmap with executable completion conditions, this file.
- [x] `M-1.2` Root context. `AGENTS.md` as the canonical instruction file,
  `CLAUDE.md` importing it. Root file stays an index and links out.
  Acceptance: both files exist, `CLAUDE.md` is the one-line import, `AGENTS.md`
  links every standards file.
- [x] `M-1.3` Standards, part one: `rust-style.md`, `error-handling.md`,
  `async-concurrency.md`. Acceptance: each states rules that are checkable, and
  names the lint or script that enforces it where one exists.
- [x] `M-1.4` Standards, part two: `testing.md`, `observability.md`,
  `security.md`. Acceptance: `testing.md` matches the three tiers and the hot
  path discipline in the plan without restating the plan.
- [x] `M-1.5` Standards, part three: `contracts.md` and `behavior.md`.
  Acceptance: `behavior.md` carries the commit-granularity rule and the
  escalation conditions verbatim enough that an autonomous turn can follow it.
- [x] `M-1.6` Product docs: `mission.md` and `architecture.md`. Acceptance:
  `architecture.md` states the crate dependency rule (everything depends on
  `pgprox-core` and nothing else in the workspace, with `pgprox-session` and
  `bin/pgprox` as the two stated exceptions).
- [x] `M-1.7` ADRs, one per row of the decisions table in the plan. Ten records,
  each naming the alternatives rejected and why. Acceptance: `decisions/0001`
  through `decisions/0010` exist and each has a Consequences section.
- [x] `M-1.8` Enforcement scripts: `check-fmt.sh`, `check-crate.sh`,
  `check-coverage.sh`, `check-drift.sh`. Acceptance: each runs and exits
  correctly on the current (Rust-free) tree, meaning they no-op cleanly rather
  than failing when there are no crates yet.
- [x] `M-1.9` `.pre-commit-config.yaml` calling the scripts, plus install instructions.
  Acceptance: hooks fire on a test commit and block on a seeded violation.
- [x] `M-1.10` CI workflow running the same scripts. Acceptance: workflow file
  validates and calls `scripts/` rather than reimplementing the checks.
- [x] `M-1.11` Claude Code hooks as accelerator, calling identical scripts.
  Acceptance: an agent-hook adapter references `scripts/`, no check is
  implemented twice.
- [x] `M-1.12` Skills, part one: `spec`, `tdd`, `next-task`. Acceptance: Agent
  Skills format, vendor-neutral bodies, no vendor-specific paths.
- [x] `M-1.13` Skills, part two: `contract-change`, `crate-review`, `adr`.
- [x] `M-1.14` Skills, part three: `hot-path`, `wire-debug`, `skill-forge`.
- [x] `M-1.15` Skill discovery symlink and per-crate `AGENTS.md` stubs for the
  twelve planned crates.
- [x] `M-1.16` `scripts/gates/m-1-complete.sh`, the milestone completion condition.
  Acceptance: exits zero on a complete M-1 and non-zero with a useful message on
  each individual failure.
- [~] `M-1.17` Portability check on a second tool. Run a small throwaway task
  under Codex CLI or Cursor and record the result as an ADR. Acceptance: the ADR
  states what worked, what did not, and what was changed as a result.

- [x] `M-1.18` Close M-1 and unblock M0.
  Filed by `M12.10`, long after the commit that did it, for the same reason as
  `M1F.0`: the hook checked that the ID was well formed and not that it referred
  to anything. `scripts/gates/m-1-complete.sh` exits zero and M0 was cleared to start.
  The caveat it carried forward is still the right one and is still open:
  `M-1.17` is structurally complete and interactively outstanding, which ADR
  0012 records. A milestone closed with a known outstanding item is closed
  honestly only if the item stays visible, and it is, in `M-1.17`'s `[~]`.

## M0: contracts and quality gates

`pgprox-core` holds the traits and types every other crate depends on, plus a
working fake for each. It is what lets five tracks run in parallel from M1.

Sizing note: the coverage gate is 95% per crate, so a task that adds a trait
without its fake and tests leaves the tree red and is half a task. Every entry
below is types plus tests plus fake where one applies.

- [x] `M0.1` Define M0: this decomposition, and `scripts/gates/m0-complete.sh`.
  Acceptance: the script exits non-zero now, naming each thing that is missing.
- [x] `M0.2` Workspace skeleton. Root `Cargo.toml` with `[workspace.lints]`,
  `rustfmt.toml`, `deny.toml`, `.cargo/config.toml`.
  Acceptance: `cargo metadata` succeeds, `scripts/check-fmt.sh` passes,
  `cargo deny check` passes.
- [x] `M0.3` `pgprox-core` crate and the ID newtypes: `TenantId`, `NodeId`,
  `ServerId`, `ConnId`, `Lsn`, `PoolKey`.
  Acceptance: a swapped pair of IDs fails to compile; `Lsn` orders correctly
  across the 32-bit boundary; `ConnId` round-trips its node ID.
- [x] `M0.4` `SecretString`.
  Acceptance: `Debug` and `Display` print no part of the secret; the value is
  reachable only through `expose()`; memory is zeroed on drop.
- [x] `M0.5` Error taxonomy and the SQLSTATE mapping.
  Acceptance: every client-visible error maps to the code in the table in
  `standards/error-handling.md`; no error variant can carry a credential.
- [x] `M0.6` `Clock` trait, `SystemClock`, and `FakeClock`.
  Acceptance: `FakeClock` advances only when told, and a test using it completes
  without sleeping.
- [x] `M0.7` Buffer slab.
  Acceptance: a borrowed buffer returns to the slab on drop; the slab bounds
  total outstanding buffers rather than allocating without limit; borrowing from
  a warm slab does not allocate.
- [x] `M0.8` Auth DTOs: `Backend`, `Grant`, `AuthRequest`, `PoolHints`,
  `ClaimSet`.
  Acceptance: formatting a `Backend` reveals host and database but never the
  password; `Grant` TTL clamps to the earliest of grant TTL, token expiry, and
  configured cap.
- [x] `M0.9` `CredentialResolver` trait and its fake.
  Acceptance: the fake resolves configured tenants, returns a typed error for
  unknown ones, and records call counts so singleflight can be tested against it.
- [x] `M0.10` Pool contract: `PoolStats`, `UpstreamGuard`, `PoolError`,
  `UpstreamPool`, and the fake.
  Acceptance: the fake actually tracks acquisitions and actually refuses past
  its cap, rather than recording calls.
- [x] `M0.11` Cluster contract: `MembershipView`, `QuotaLease`, `ClusterDigest`,
  `ClusterCoordinator`, and the fake.
  Acceptance: the fake's `home_node` is stable under rendezvous hashing and
  rehomes only tenants of a departed node; leases expire on the injected clock.
- [x] `M0.12` Config contract: `Config`, `NodeMode`, `ConfigSource`, and the
  fake.
  Acceptance: the fake publishes a new config to watchers; invalid config is
  rejected with a message naming the offending field.
- [x] `M0.13` Route contract: `RouteCtx`, `RouteTarget`, `StmtClass`, `Router`,
  and the fake.
  Acceptance: an unknown statement class routes to primary; a replica behind the
  session watermark is not eligible.
- [x] `M0.14` Cache contract stub: `QueryCache` and its fake.
  Acceptance: the trait compiles and has a fake; no implementation beyond that,
  since this is M9 work.
- [x] `M0.15` Public surface. `lib.rs` re-exports only, `#![warn(missing_docs)]`
  satisfied, crate-level docs.
  Acceptance: `scripts/gates/m0-complete.sh` exits zero.

## M1: protocol and TLS (track A)

`pgprox-proto` is the wire codec in both directions, and `pgprox-tls` is the
rustls setup shared by the listener and upstream connections.

Scope note worth stating up front: the proxy binary does not exist until M6, so
the conformance suite cannot test "the proxy". It tests the codec from both
sides. As a client, driving real Postgres 17 and 18. As a server, accepting real
drivers. Between them that covers every message the codec claims to handle.

- [x] `M1.1` Define M1: this decomposition, and `scripts/conformance.sh`.
  Acceptance: the script exits non-zero now, naming what is missing, and reports
  which drivers ran versus were skipped rather than silently narrowing.
- [x] `M1.2` `pgprox-proto` crate and frame primitives: message type byte,
  length prefix, incomplete-frame handling.
  Acceptance: a frame split across arbitrary byte boundaries reassembles; a
  length larger than the configured maximum is an error, never an allocation.
- [x] `M1.3` Backend messages we inspect: `ReadyForQuery`, `ErrorResponse`,
  `ParameterStatus`, `CommandComplete`, `BackendKeyData`, `NotificationResponse`,
  the `Authentication*` family.
  Acceptance: each decodes from real bytes; everything else passes through as
  opaque frames without being parsed.
- [x] `M1.4` Frontend messages we inspect: `Query`, `Parse`, `Bind`, `Execute`,
  `Sync`, `Describe`, `Close`, `Terminate`.
  Acceptance: statement and portal names are readable, which is what prepared
  statement mapping needs.
- [x] `M1.5` Encoding: `ErrorResponse` from a `ClientError`, `Authentication*`,
  `ParameterStatus`, `BackendKeyData`, `ReadyForQuery`,
  `NegotiateProtocolVersion`.
  Acceptance: an encoded `ErrorResponse` carries the SQLSTATE from the mapping
  and is accepted by a real driver.
- [x] `M1.6` Startup dispatch: `SSLRequest`, `GSSENCRequest`, `CancelRequest`,
  `StartupMessage`, and protocol version negotiation.
  Acceptance: a client asking for 3.2 gets 3.2 or a `NegotiateProtocolVersion`
  down to 3.0; a `CancelRequest` yields the encoded node and secret.
- [x] `M1.7` Session state machine: transaction status tracking and
  extended-query sequence tracking.
  Acceptance: release is permitted only at `ReadyForQuery('I')` with no sequence
  outstanding; a `Sync` missing mid-sequence keeps the session held.
- [x] `M1.8` COPY mode, both directions.
  Acceptance: a session in COPY is never released until the stream ends.
- [x] `M1.9` Fuzz targets for the decoder, with a committed corpus.
  Acceptance: `cargo fuzz` runs both targets; any crash found becomes a unit
  test.
  Left at `[~]` long after it was done, and reviewed in `M14` while listing what
  remained. Three targets exist rather than two, `frame_decode`,
  `message_decode` and `classify`; `scripts/fuzz.sh` runs them; CI runs that for
  300 seconds a target; and a crash uploads the bytes that caused it as an
  artifact, which is what makes "becomes a unit test" possible rather than a
  wish.
  **"A committed corpus" was answered by committing the generator instead**, and
  the substitution is better than the original. `crates/pgprox-proto/examples/
  seed_corpus.rs` is 312 lines that seed the corpus before every run, and
  libFuzzer grows it from there. Committing the grown version would be several
  thousand small files no human reads, replaced by the next run. The acceptance
  wanted reproducible starting inputs, and a generator gives that in a form a
  reviewer can actually read.
  Recorded because the checkbox was wrong in the direction that matters least
  and misleads most: work that is finished but reads as outstanding makes every
  other `[~]` in this file less believable.
- [x] `M1.10` `pgprox-tls`: rustls server and client config, FIPS feature gate,
  certificate loading.
  Acceptance: a FIPS build asserts `fips()` on both configs and refuses to start
  otherwise; there is no way to configure skip-verification.
- [x] `M1.11` Client-side conformance: drive real Postgres 17 and 18 with the
  codec in testcontainers.
  Acceptance: startup, simple query, extended query, and COPY all complete
  against both versions.
- [x] `M1.12` Server-side conformance harness: a minimal server built on the
  codec that real drivers can connect to.
  Acceptance: `psql` completes a session against it.
- [x] `M1.13` Driver matrix: pgx, asyncpg, JDBC, npgsql against the harness.
  Acceptance: each completes startup, a simple query, and a named prepared
  statement; skipped drivers are reported, never silently dropped.
- [x] `M1.14` Close M1. Acceptance: `scripts/conformance.sh 17 18` exits zero.

## M1R: protocol revision (streaming and test breadth)

Raised by review after M2. Named M1R rather than M1.5 because `M1.5` is already
a task ID and IDs are stable.

Three findings, in order of severity.

**The codec cannot stream.** `decode` returns `Incomplete` until an entire frame
is buffered, so a relay built on it must accumulate a whole message before
forwarding a byte. That contradicts ADR 0008: a single large `DataRow` forces up
to 64 MiB of buffering, and a few concurrent large results blow the 500 MB
target the whole design rests on.

**And the cap is an outright bug.** `DEFAULT_MAX_FRAME` applies to every frame
including server `DataRow`s. Postgres field values reach 1 GB, so a legitimate
`SELECT` of a 100 MB `bytea` is rejected by our own decoder.

**The tests are narrow.** Every conformance test and all five drivers exercise
`SELECT 1`, `SELECT $1`, `generate_series` to 50 rows, and one COPY OUT of 100
small rows. Nothing larger than ~100 bytes, no NULLs, no COPY IN, no
pipelining, no multi-statement query, no error mid-stream.

- [x] `M1R.1` Define M1R: this decomposition and `scripts/gates/m1r-complete.sh`.
- [x] `M1R.2` `decode_header` and the per-direction inspect policy. Acceptance:
  a header decodes from exactly five bytes; every tag has a stated policy; the
  policy for `DataRow` and `CopyData` is forward-without-inspection.
- [x] `M1R.3` `FrameRelay`, the streaming state machine. Acceptance: a 100 MiB
  body relays with bounded memory; bytes are emitted before the body ends; a
  prefix-inspected message yields its prefix without buffering the rest.
- [x] `M1R.4` Split the size caps. Acceptance: a 100 MiB `DataRow` passes
  through, an oversized message we must parse is refused, and the two limits are
  separately configurable.
- [x] `M1R.5` Move the conformance server and client onto the relay. Acceptance:
  the existing suite still passes, now streaming.
- [x] `M1R.6` Breadth: values above the old cap, NULLs, multi-statement simple
  query, empty query.
- [x] `M1R.7` Breadth: COPY IN, binary parameter input, error mid-result-stream.
- [x] `M1R.8` Breadth: pipelining, LISTEN/NOTIFY from a real server.
- [x] `M1R.9` Driver depth: prepared statement reuse and a large result per
  driver, rather than `SELECT 1` five times.
- [x] `M1R.10` Close M1R.

## M1F: full protocol coverage

Measured against three references cloned into `reference/` (gitignored): pgdog
(Rust, closest peer), pgbouncer (canonical SCRAM and prepared statements), and
odyssey. The comparison is what makes this list finite rather than a guess.

**Where we are already ahead.** pgdog's `read_buf` reserves the declared length
and reads the whole body before handing it on, using `unsafe set_len`. Our
`FrameRelay` streams and forbids unsafe. Do not regress this to match a
reference.

**What passing bytes through opaquely covers.** `RowDescription`, `DataRow`,
`NoData`, `ParseComplete`, `BindComplete`, `CloseComplete`, `PortalSuspended`
need no decoder for a proxy, and decoding them would cost throughput for
nothing. They are not gaps.

**The real gaps**, in dependency order.

### Group A: message surface

- [x] `M1F.0` Plan M1F: derive the task list by diffing three reference proxies
  rather than guessing it.
  Filed by `M12.10`, long after the commit that did it. The work is real and its
  result is the section header above: pgdog, pgbouncer and odyssey cloned into
  `reference/`, their protocol surface diffed against this one, and the gaps
  below written from that diff. Two findings that shaped the rest of the
  milestone are recorded there too, that this codec already streams where
  pgdog's `read_buf` reserves and reads whole with `unsafe set_len`, and that
  seven message types need no decoder in a proxy and are not gaps.
  What was missing was the entry, not the work. `check-commit-msg.sh` accepted
  `M1F.0:` because it checked the shape of the ID and not whether anything
  answered to it, which `M12.1` fixed and which is how this was found: by
  running the tightened hook over all 321 commits.
- [x] `M1F.1` `EmptyQueryResponse` (`I`). No `Tag` constant exists at all. An
  empty statement yields this instead of `CommandComplete`, so anything counting
  completions is wrong today.
- [x] `M1F.2` `ParameterDescription` (`t`) decoder. Encoded by the harness,
  never decoded. M5's statement mapping needs the parameter count to rewrite a
  `Describe` response.
- [x] `M1F.3` `FunctionCall` (`F`) and `FunctionCallResponse` (`V`). Legacy
  fast-path, still reachable. pgdog models it as `fastpath`. Decide explicitly
  whether to support or refuse it, and record the choice; refusing silently is
  the option that is definitely wrong.
- [x] `M1F.4` Full `ErrorResponse`/`NoticeResponse` field set. Three of about
  twenty are extracted. Add detail, hint, position, internal position, internal
  query, where, schema, table, column, datatype, constraint, file, line,
  routine. Acceptance: a real Postgres error round-trips every field it sent.
- [x] `M1F.5` `Tag` completeness audit. Assert every code in the Postgres
  protocol appendix has a constant and a stated policy, so a future message
  cannot be silently unhandled. Acceptance: a test enumerates them.

### Group B: SCRAM-SHA-256 authentication

ADR 0002 chose SCRAM passthrough for non-JWT clients and it was never built, so
admin tooling and migrations cannot connect. pgbouncer spends 1205 lines here;
this is the largest single gap.

- [x] `M1F.6` SASL message framing: `AuthenticationSASL`,
  `AuthenticationSASLContinue`, `AuthenticationSASLFinal`, and the
  `SASLInitialResponse`/`SASLResponse` frontend forms.
- [x] `M1F.7` SCRAM client-first and server-first messages: nonce generation,
  channel-binding flag, the `n,,` GS2 header. Sans-I/O, so testable against RFC
  5802 vectors.
- [x] `M1F.8` Salted password derivation: PBKDF2-HMAC-SHA-256, `SaltedPassword`,
  `ClientKey`, `StoredKey`, `ServerKey`. Must use the FIPS provider so the FIPS
  build does not diverge.
- [x] `M1F.9` Client proof and server signature verification, in constant time.
  Acceptance: RFC 5802 and RFC 7677 test vectors pass.
- [x] `M1F.10` `scram-sha-256` verifier parsing, for verifying a client against
  a stored verifier rather than a password.
- [x] `M1F.11` Channel binding (`SCRAM-SHA-256-PLUS`). Decide and record: it
  requires the TLS exporter and interacts with the FIPS suite list. Refusing is
  acceptable; refusing without saying so is not.
- [x] `M1F.12` Done in `M6.42`. Was blocked on M6. Wire SCRAM into the auth path as the non-JWT
  branch, selected by a configured static-credential rule. There is no auth path
  to wire into until `pgprox-session` exists, so this was an ordering error when
  written, not work anyone skipped. Move it into M6's decomposition when that
  milestone is planned.
- [x] `M1F.13` SCRAM conformance against real Postgres and all five drivers.
  Acceptance: each driver authenticates with SCRAM through the harness.

### Group C: protocol 3.2

- [~] `M1F.14` **Deferred by ADR 0016**, with triggers for revisiting. 256-bit cancel keys. `ConnId` is 64-bit and `BackendKeyData` is
  two `i32`s, so this is a `pgprox-core` contract change: use the
  `contract-change` skill and expect to touch every crate that holds a `ConnId`.
- [~] `M1F.15` **Deferred by ADR 0016.** Negotiating down is the decision, not a
  placeholder. The spec stays so the work is designed when a trigger fires.
- [~] `M1F.16` **Deferred with M1F.15.** `_pq_.` extension parameters only
  matter to a client negotiating a version we decline.

### Group D: replication and COPY BOTH

pgdog carries a whole logical-decoding subtree. We only track the mode.

- [x] `M1F.17` Decide scope and record an ADR. Physical replication passthrough
  is cheap; logical decoding message types are a large surface that a
  connection proxy may not need at all. Do not build Group D before this.
- [~] `M1F.18` Unnecessary per ADR 0015. `CopyBothResponse` already holds the
  session, and the ADR's pin-for-life rule is the design statement; implementing
  it belongs with the pool at M5, not here.
- [~] `M1F.19` Standby status update and keepalive passthrough, if M1F.17 says
  yes.

### Group E: startup and session parameters

- [x] `M1F.20` `options` startup parameter parsing, including the
  `-c name=value` form. It carries `search_path`, which is part of the cache key
  and therefore correctness-relevant.
- [x] `M1F.21` The replayable session-parameter allowlist as a real type, with
  `SET`, `SET LOCAL`, `RESET`, and `RESET ALL` handling. ADR 0001 named it; it
  does not exist yet.
  Most of this was built in M5 and this task never noticed. `SessionParams`
  handles all four forms plus `DISCARD ALL`, quoting, case-insensitive names
  and replay-only-what-differs, with nineteen tests. What was missing was the
  type: the allowlist was a `&[&str]` threaded through as an argument.
  That is not cosmetic. Two different things consult it, `PinState::
  observe_statement` deciding whether a `SET` pins and `SessionParams::
  observe_statement` deciding whether the same `SET` is recorded for replay,
  and given different lists they disagree without saying so. The shape of that
  bug is a session recorded as movable whose settings are never replayed: a
  client's `search_path` quietly reverts between statements and nothing errors.
  `pgprox_pool::Replayable` is the type, obtainable only from `DEFAULT`,
  `NONE`, or `from_names`, and ADR 0001 now says why it is one.
- [x] `M1F.22` `GSSENCRequest` beyond refusal: confirm the refusal path against
  a GSSAPI-capable client rather than assuming it.
  Attempted in M8 and it needs a KDC, which is the decision this was deferred
  on rather than the work. libpq on this machine has GSSAPI compiled in and
  understands `gssencmode`, but it will not send a `GSSENCRequest` without a
  credential cache: `gssencmode=require` fails client-side with "GSSAPI
  encryption required but no credential cache" before a byte reaches the
  listener, and `gssencmode=prefer` skips GSSAPI silently for the same reason.
  So there is no way to make a real client send the packet without Kerberos
  behind it.
  What that costs: an MIT Kerberos container in the e2e stack, a realm, a
  service principal for the proxy, a keytab, and a `kinit` before the probe.
  Whether the test stack should carry a KDC is a decision rather than a task,
  which is why this one was open. Hand-crafting the packet is what the existing
  unit test already does, and this task existed because that is not the same
  thing.
  Decided, and the answer is no: this proxy does not support GSSAPI encryption,
  and a client that asks is told `N` by a state machine that has never had a
  yes to give. The test that matters is that the refusal is correct and does
  not desynchronise the handshake, which the unit test covers by writing the
  packet directly, and a real client would exercise exactly the same three
  bytes on the way in.
  What a KDC would buy is confidence that libpq's *own* behaviour after the
  refusal is what `pgprox-session` assumes, which is that it falls back to
  `SSLRequest`. That assumption is worth stating rather than testing here: it
  is written on `state.rs:387` and on `shell.rs:457`, and the driver matrix in
  `M1F.24` runs libpq against the proxy on every other path. Standing up a
  realm, a service principal and a keytab in every e2e run to watch a client
  be refused is permanent weight for a path whose whole job is to say no.

### Group F: conformance depth

- [x] `M1F.23` A message-coverage report. Instrument the conformance run to
  record which tags were actually seen in each direction, and fail if a tag with
  a decoder was never exercised. This is what turns "we handle it" into "we
  tested it".
- [x] `M1F.24` Driver matrix against real Postgres, not only the harness. Done
  as `M8.13`, which is this task with a reason rather than a plan: asyncpg
  could not run a parameterised query through the proxy from M6 until M8, and
  `scripts/conformance.sh` stayed green throughout because the harness answered
  a `Flush` the same wrong way the proxy did. All five drivers now pass the
  depth cases against `bin/pgprox` over TLS onto real Postgres, recorded in
  `product/conformance/driver-matrix.md`, and `m1f-complete.sh` checks that a
  matrix exists.
- [x] `M1F.25` Corpus seeding from the references: extract their protocol test
  fixtures into the fuzz corpus, so their accumulated edge cases become ours.
  There are none to extract. pgdog builds its messages in Rust and round-trips
  them, pgbouncer and odyssey drive real servers through their integration
  suites, and none of the three ships a file of wire bytes. What they carry is
  a list of what they thought worth testing, and `seed_corpus.rs` reproduces
  that: 31 frames and 27 message bodies covering the authentication ladder, the
  extended-query sequence, the messages whose length field can disagree with
  their content, and the startup packet.
  The larger finding was next to the task rather than in it. `fuzz/README.md`
  had said since M1 that the targets had never been executed, and running them
  is worth more than seeding them. All three run now, through
  `scripts/fuzz.sh`, and `classify` found two bugs on its first outing. Both
  were in its own oracle: it skipped quotes but not line comments, so
  `---kk...update;` read as DML, and then it did not nest block comments, so
  `/* /* merge */ */` did the same. An oracle that skips less than the thing it
  checks reports the checker's correctness as a bug. The classifier was right
  both times, which is the reassuring part: it is the path that decides whether
  a statement may reach a replica.

### Group H: raised by reviewing the first M1F round

Found by reviewing the work rather than by a gate, which is why they are listed
rather than remembered.

- [x] `M1F.27` A shared Docker readiness helper. The M1.11 probe bug, accepting
  any message as ready including the `57P03` a starting container sends, was
  reproduced verbatim in `scram_live.rs`. The same mistake twice means the fix
  belongs in one place rather than in whoever writes the next probe.
  Acceptance: one helper, both suites use it, and a test asserts it rejects a
  57P03 as not-ready.
- [x] `M1F.28` Correct M1F.12's ordering. It says wire SCRAM into the auth path,
  but there is no auth path until `pgprox-session` exists at M6. It is blocked,
  not merely undone, and the backlog should say which.
- [x] `M1F.29` Record the buffered-handoff hazard where M6 will read it. In
  `scram_live` a helper owned its read buffer locally, so bytes pulled in past
  the handshake were dropped and the session appeared to close. The relay has
  the same hazard at every stage boundary.
  Acceptance: `crates/pgprox-session/AGENTS.md` states it.
- [x] `M1F.30` Prepare the protocol 3.2 contract change as a spec rather than
  starting it. 256-bit cancel keys change `ConnId` and `BackendKeyData`, which
  touches every crate holding one, and `standards/contracts.md` requires
  stopping before a cross-track change rather than discovering the blast radius
  mid-edit.

### Group I: raised by the second review round

- [x] `M1F.31` `pgprox-testkit` is absent from the crate map in
  `product/architecture.md`. A crate map that omits a crate is worse than none,
  because it is trusted.
- [x] `M1F.32` Enforce the crate dependency rule for every crate, not only
  `pgprox-core`. `standards/contracts.md` calls it the thing that makes parallel
  tracks possible, and `m0-complete.sh` checks exactly one crate. Today
  `pgprox-auth` could depend on `pgprox-proto` and nothing would notice.
  Acceptance: a script rejects a seeded sideways dependency, and rejects
  `pgprox-testkit` appearing as a runtime dependency of anything.

- [x] `M1F.33` `cargo deny` has been failing since M1.10 and nothing noticed,
  because it runs only in CI and the milestone gates, not in the pre-commit
  path. A supply-chain check nobody runs is a supply-chain check nobody has.
  Acceptance: it runs on every commit that touches a manifest or the lockfile.
- [x] `M1F.34` `rustls-pemfile` is flagged unmaintained by RustSec. Migrate to
  the PEM support now in `rustls-pki-types`, which is where it went.
- [x] `M1F.35` Workspace path dependencies trip the wildcard ban. They have no
  version by design, so the ban needs `allow-wildcard-paths`, not a version
  invented for a local path.

- [x] `M1F.36` The 3.0 cancel key's low 48 bits are described as a "counter",
  and a cancel key is a bearer token. A sequence number is not a secret. Making
  it random needs no protocol change and no contract change, so it should not
  wait behind M1F.30. State the requirement where the connection is created.

### Group G: close

- [x] `M1F.26` Close M1F. Acceptance: `scripts/gates/m1f-complete.sh` exits zero.

Remaining after five review rounds, all planned work rather than discovered
defects: M1F.15 and M1F.16 are protocol 3.2, gated on the M1F.30 spec's open
question; M1F.21 is the session-parameter allowlist ADR 0001 named; M1F.22 is
confirming the GSSAPI refusal against a real GSSAPI client; M1F.24 runs the
driver matrix against real Postgres rather than only the harness; M1F.25 seeds
the fuzz corpus from the reference proxies.

## M3: cluster (track C)

`pgprox-cluster` holds the membership, quota and placement logic. It needs no
Postgres and no sidecar, so it develops entirely against a deterministic
simulation.

`pgprox-core` already provides `MembershipView` with rendezvous hashing,
`QuotaLease`, `ClusterDigest` and the `ClusterCoordinator` trait. This milestone
builds the real implementation behind that trait.

The invariant everything serves:

> Guaranteed share plus outstanding leases never exceeds the cap, under
> arbitrary partition, leader loss, and simultaneous restart.

Breaching an upstream cap can lock out the operator and take the database down
for every tenant on that host. It is the one property with no graceful
degradation, so it is proven by property test over a simulation rather than
found in staging.

- [x] `M3.1` Define M3: this decomposition and `scripts/gates/m3-complete.sh`.
- [x] `M3.2` `pgprox-cluster` crate and the deterministic simulation: virtual
  clock, an injectable network that can delay, drop, reorder and partition, and
  seeded scheduling. Acceptance: the same seed produces the same run twice.
- [x] `M3.3` Quota arithmetic as a pure function: guaranteed share per node from
  the cap and live membership, and the leasable free pool. Acceptance: the sum
  can never exceed the cap for any membership size, checked exhaustively for
  small N and by property test beyond.
- [x] `M3.4` Leader selection from a membership view, and what happens when it
  changes. Acceptance: every node agrees on the leader given the same view, and
  a new leader waits one full lease TTL before granting from the free pool.
- [x] `M3.5` Lease lifecycle: request, grant, renew, expire, release.
  Acceptance: an unreachable node's leases expire without anyone acting, and a
  lease is never counted after its expiry.
- [x] `M3.6` Tenant reservations with use-it-or-lose-it decay.
  Acceptance: a home node holding an unused reservation loses it after the
  configured rounds, and a non-home node can then claim it.
- [x] `M3.7` Shed decisions and their guard rails: idle threshold, per-tenant
  rate limit, settle window after a membership change, never toward a draining
  node, never a pinned or in-transaction session, global kill switch.
  Acceptance: each guard rail refuses a shed that would otherwise happen.
- [x] `M3.8` Gossip digest encoding, and merging a peer's digest into the local
  view. Acceptance: a digest round-trips, and merging is order-independent so
  two nodes converge regardless of delivery order.
- [x] `M3.9` The `ClusterCoordinator` implementation, wiring the above together.
- [x] `M3.10` The invariant, as a property test over randomized schedules
  including partitions, leader loss and simultaneous restarts. Acceptance: a
  failing seed is committed as a regression case.
- [x] `M3.11` Close M3.
- [x] `M3.12` Forward a quota request to the leader over the gossip transport.
  Done in `M6.16`, which is where the transport it was waiting for arrived.
  Deferred out of M3 deliberately: `pgprox-cluster` needs no socket to be
  tested, and the invariant is a property of the quota rules rather than of a
  message-passing layer. Until this lands, a node that is not the leader gets
  `NoLeader` and falls back to its guaranteed share, which is the safe
  direction but leaves the free pool usable only by the leader. Blocked on the
  gossip transport, so it belongs with M4 or M6 rather than here. Acceptance:
  a non-leader obtains a lease, and the invariant property test still holds
  with the hop in the loop.
- [x] `M3.13` Per-tenant usage in the gossip digest, and the coordinator wiring
  `Reservations` to it. Found reviewing M3: `Reservations::observe` takes the
  home node's usage and `ShedCtx` takes `home_has_headroom`, and neither has a
  source, because `ClusterDigest` carries only whole-node counts. ADR 0004 says
  the digest carries per-tenant usage for homed tenants; it does not. Both
  modules are correct pure functions with nothing able to feed them.
  Acceptance: a peer decays a home node's reservation from gossip alone, and a
  caller can assemble a `ShedCtx` without inventing any field.
- [x] `M3.15` One source of the membership view. `DigestStore::membership` built
  a view with no liveness filtering, so a caller could pick a leader from nodes
  silent for an hour while the coordinator used a filtered one. Removed, so
  `Membership::view` is the only way to get a view.
- [x] `M3.14` Drive the invariant property test through `sim::Network`, so
  gossip is dropped, delayed and reordered rather than delivered directly.
  Found reviewing M3: the schedules partition, but every message that is not
  partitioned away arrives immediately and in order, which is the case least
  likely to produce stale liveness. Acceptance: the invariant holds with loss
  and reordering enabled, and any failing seed is committed.

## M2: auth and sidecar (track B)

`pgprox-auth` turns a client's token into the credentials for its database, by
asking the sidecar and caching the answer.

Scope note: ADR 0017 gives this repository ownership of the sidecar contract and
freezes it at v1. Field numbers are stable; a breaking change means `auth.v2`.

- [x] `M2.1` Define M2: this decomposition, and the `.proto` contract as a
  proposal. Acceptance: the file states its unfrozen status and its versioning
  rules; field numbers are assigned and never reused.
- [x] `M2.2` `pgprox-auth` crate with tonic codegen wired into the build.
  Acceptance: the workspace builds, generated code is excluded from coverage,
  and nothing hand-edits it.
- [x] `M2.3` gRPC client over a Unix domain socket, implementing
  `CredentialResolver`. Acceptance: a `Grant` round-trips proto to Rust with the
  password arriving as a `SecretString`.
- [x] `M2.4` Grant cache keyed by `sha256(token) || startup_db`.
  Acceptance: a hit avoids the RPC, the key is a hash rather than the token, and
  the TTL is the earliest of grant TTL, token expiry, and configured cap.
- [x] `M2.5` Singleflight on the resolve path.
  Acceptance: N concurrent lookups of the same cold key produce exactly one
  underlying call, asserted against the fake's call counter.
- [x] `M2.6` Negative caching for refusals.
  Acceptance: a refused token is not retried on every reconnect, and the
  negative TTL is shorter than the positive one so a revocation reversal is not
  stuck behind it.
- [x] `M2.7` Algorithm allowlist on the JWT header.
  Acceptance: `none` and the `HS*` family are rejected before the sidecar is
  called; the six approved algorithms pass. The proxy still does not verify
  signatures.
- [x] `M2.8` Mock sidecar binary.
  Acceptance: it starts, serves over UDS, and can be told to refuse, stall, and
  return a malformed grant so callers' error paths are reachable.
- [x] `M2.9` Integration tests against the mock over a real socket.
  Acceptance: `cargo nextest run -p pgprox-auth --features integration` passes.
- [x] `M2.10` Close M2.

## M5: pooling and routing (track E)

`pgprox-pool` multiplexes many client sessions onto few upstream connections.
`pgprox-route` decides whether a statement may go to a replica. Together they
are what makes the 100k-downstream to 5k-upstream ratio real rather than
aspirational.

`pgprox-core` already provides the `UpstreamPool` trait, `UpstreamGuard` with
its discard-by-default release, `PoolStats`, `PoolError`, the `Router` trait and
`route::decide`, which is the routing rule as a pure function. This milestone
builds the implementations behind them.

`pgprox-proto` already provides `SessionState`, which answers "may this
connection be released" from the transaction status, the extended-query
sequence and COPY. M5 does not reimplement that. What M5 adds is *pinning*,
which is a different question: not "is this moment safe" but "has this session
used a feature that makes every moment unsafe from now on".

The two properties everything serves:

> No DML-bearing statement is ever classified read-only.

> A connection is released only at a genuine transaction boundary, and one
> released mid-transaction is closed rather than returned.

A misclassification is a stale read, which is a correctness bug from the
tenant's side. A wrong release hands one client a connection sitting inside
another's transaction. Both are proven by property test, and the classifier is
fuzzed because it parses SQL arriving from the internet.

### Layering note

ADR 0011 states as a consequence that `pgprox-pool` gains a dependency on
`pgprox-proto` in order to rewrite statement names, and that M0 settled how.
M0 did not, and the dependency is forbidden by `scripts/check-layering.sh`,
which allows only `pgprox-session` and `bin/pgprox` to compose crates.

Resolved without a contract change, because the work splits along the existing
boundary rather than across it. `pgprox-pool` owns the *mapping*: SQL hash to
global name, which connection holds which name, and the LRU. That is a data
structure over strings and hashes and needs no protocol knowledge at all.
`pgprox-proto` owns the *rewriting*, and already decodes `Parse` and `Bind`
with their statement names. `pgprox-session` joins them at M6, which is exactly
what a composer is for. ADR 0011 is amended in `M5.1` to say so.

- [x] `M5.1` Define M5: this decomposition, `scripts/gates/m5-complete.sh`, and the
  ADR 0011 amendment above. Acceptance: the gate script runs and reports what
  is missing rather than passing vacuously.
- [x] `M5.2` `pgprox-route` and the statement classifier: a token-prefix scan
  from SQL text to `StmtClass`. Acceptance: a property test finds no
  DML-bearing statement classified read-only, and `WITH ... INSERT`,
  `SELECT ... FOR UPDATE`, `SELECT ... FOR SHARE` and `EXPLAIN ANALYZE` are all
  writes.
- [x] `M5.3` Volatile function detection, and `BEGIN READ ONLY` marking a whole
  transaction replica-eligible. Acceptance: a `SELECT` calling a volatile
  function classifies as `Unknown` rather than `ReadOnly`, and an unrecognised
  construct does the same.
- [x] `M5.4` Explicit route overrides: `SET pgprox.route` for the session and a
  leading `/* pgprox:replica */` comment for one statement. Acceptance: a hint
  can admit an `Unknown` statement to a replica and can never admit a `Write`
  or one behind the session watermark.
- [x] `M5.5` Replica state tracking: replayed LSN and health per replica, read
  by the router with no await, plus the session write watermark. Acceptance: a
  replica behind the watermark is never eligible, and a session that has never
  written accepts any healthy replica.
- [x] `M5.6` The `Router` implementation over `route::decide`, with the target
  fixed at the transaction's first statement. Acceptance: the second statement
  of a transaction goes where the first did, whatever its own class.
- [x] `M5.7` A fuzz target for the classifier. Acceptance: it builds and runs a
  short seeded corpus without panicking, and the invariant is asserted inside
  the target rather than only outside it.
- [x] `M5.8` `pgprox-pool` and pin detection: `LISTEN`/`UNLISTEN`, session
  advisory locks, temp tables, `WITH HOLD` cursors, SQL-level `PREPARE`, and
  `SET` outside the replayable allowlist. Acceptance: every trigger is
  detected and carries a distinct reason for `pgprox_pin_total{reason}`, and
  the `_xact_` advisory lock variants do not pin.
- [x] `M5.9` Session parameter tracking and replay: the allowlist, `SET`,
  `SET LOCAL`, `RESET` and `RESET ALL`. Acceptance: a parameter in the
  allowlist is replayed on acquire only when the target connection's value
  differs, `SET LOCAL` is never replayed, and a parameter outside the
  allowlist pins instead.
- [x] `M5.10` The prepared statement map: a global name derived from the SQL
  hash, the per-connection held set, and LRU eviction at a configured cap.
  Acceptance: two sessions preparing identical SQL share one global name, a
  connection that does not hold a statement reports it as needing replay, and
  eviction never reports a statement as held after it is evicted.
- [x] `M5.11` The pool itself: acquire with a deadline, release through the
  guard, per-key pools, and the limit. Acceptance: a guard dropped without a
  clean release closes the connection rather than returning it, and acquiring
  past the limit waits rather than opening.
- [x] `M5.12` Waiters and backpressure: queueing at the limit, deadline
  expiry, and `PoolStats::waiting`. Acceptance: a waiter is woken by a release
  rather than by polling, and one that misses its deadline gets
  `PoolError::Timeout` and stops occupying a slot.
- [x] `M5.13` Idle reap with `min_pool` of zero. Acceptance: an idle connection
  is closed after its configured idle time, and a pool that goes quiet drops
  to zero connections without anyone asking.
- [x] `M5.15` Wire the reaper into `LivePool`. Found reviewing M5: `reap`
  names connections to close and `Pool::close_idle` drops them from the idle
  list, but nothing in the async layer calls either, so a `LivePool` never
  closes an idle connection and its socket is never dropped. The whole of
  M5.13 is unreachable from the only type a caller uses. Acceptance: a pool
  left quiet past its idle timeout drops to zero connections and releases
  their payloads.
- [x] `M5.16` Implement or remove `SET pgprox.pin`. Found reviewing M5:
  `PinReason::Requested` documents an escape hatch for a tenant using a feature
  the pin list has not learned, and no code implements it. The `SET` path
  explicitly skips `pgprox.` parameters, so the variant is unreachable.
  Acceptance: the documented spelling pins the session, or the variant and its
  claim are gone.
- [x] `M5.17` One SQL lexer, in `pgprox-core`. Found reviewing M5: the
  classifier and the pin detector each carry their own scanner for the same
  hazards, and they have already diverged. `pin.rs` does not honour backslash
  escapes inside `E'...'`, so `SELECT E'\'' ; LISTEN c` leaves the session
  unpinned, which is a missed pin and therefore a correctness bug. A third,
  simpler copy of the trivia skipping lives in `params.rs`. Deciding which text
  is SQL and which is data is one rule, and the same argument that puts
  `route::decide` in core puts this there. Acceptance: one implementation, both
  crates on it, and the E-string case pins.
- [x] `M5.18` The replica poller loop. Named in M5's scope in the roadmap and
  missed on the first pass: `Replicas` was built but nothing wrote to it.
  `ReplicaWatch` polls through a `ReplicaProbe` trait, which is where the SQL
  against each replica lives, in the same shape as the pool's `Connector`.
- [x] `M5.19` Take the allocation out of the route decision. Found reviewing
  M5: `route` called `Replicas::states`, which allocates, and for an autocommit
  workload that runs per statement rather than per transaction. The only way to
  feed it from a `ReplicaWatch` was `snapshot`, which copies. The route
  decision is a declared hot path.
- [x] `M5.14` Close M5.

## M4: operations (track D)

`pgprox-config` decides what the node should be doing, `pgprox-observe` says
what it is doing, and `pgprox-admin` lets a human or an agent ask and change it.

`pgprox-core` already provides `Config` with validation, `ConfigError`, the
`ConfigSource` trait and a fake that validates on publish. M4 builds the
providers behind that trait and the two surfaces that read the result.

The property that shapes all three:

> An operator or an agent hits any pod and gets the whole cluster's truth.

Aggregates answer from the local gossip digest at no cost, so hitting the wrong
pod is never wrong. Only drill-downs fan out. See ADR 0007.

### Two decisions taken before writing code

**`pgprox-admin` needs a data source and is not allowed one.** The admin API
reports pools, tenants, clients and cluster state, which live in
`pgprox-cluster`, `pgprox-pool` and `pgprox-session`.
`scripts/check-layering.sh` allows only `pgprox-session` and `bin/pgprox` to
compose crates, and `pgprox-core` has nothing admin-shaped in it. The same
situation as ADR 0011 in M5, and it resolves the same way: a new
`pgprox_core::admin` module with an `Observatory` trait and its DTOs, which the
composition root implements by fanning in, and `pgprox-admin` renders. Purely
additive, so it breaks nothing, and both consumers are unbuilt so there is no
rework. `M4.1` does it under the `contract-change` skill.

**Config is polled, not watched by an event API.** The rule is to watch the
directory rather than the file, because a ConfigMap update swaps a symlink. A
poll re-reads the directory every time and so satisfies that by construction,
where an event watcher has to be pointed at the right inode to begin with. It
also needs no new dependency, and kubelet propagates a ConfigMap change on the
order of a minute, so an event watcher would be reacting instantly to something
that already took sixty seconds to arrive.

- [x] `M4.1` Define M4: this decomposition, `scripts/gates/m4-complete.sh`, and the
  `pgprox_core::admin` contract with its fake. Acceptance: the gate script runs
  and reports what is missing rather than passing vacuously, and the fake
  serves a snapshot without any other crate existing.
- [x] `M4.2` `pgprox-config` and the config document: parsing into `Config`,
  with the document format chosen and its dependency cleared by
  `scripts/check-deps.sh`. Acceptance: a malformed document names the field
  that is wrong, and a document that parses but fails `Config::validate` is
  rejected with the same error a caller would get from the fake.
- [x] `M4.3` The file provider, polling the mount directory. Acceptance: a
  ConfigMap-style symlink swap is picked up, which is the case an event watcher
  pointed at the file misses entirely.
- [x] `M4.4` Hot reload semantics: validate then swap, never publish an invalid
  configuration, and keep serving the last good one. Acceptance: a broken
  document reaching the directory leaves watchers on the previous config and
  surfaces the error, rather than taking the node down or serving nothing.
- [x] `M4.5` The drain overlay with a TTL, for the imperative path. Acceptance:
  a drain requested through the API expires on its own, and a drain in the
  config document does not.
- [x] `M4.6` `pgprox-observe` and the metric registry: every metric named in
  one place, every one carrying `node`. Acceptance: a test enumerates the
  registry and fails on any label that is unbounded, with `tenant` named as the
  example.
- [x] `M4.7` Span and log conventions, with redaction. Acceptance: a span name
  is stable and low cardinality with the tenant in an attribute, and a
  credential cannot reach a log line, a span attribute or a metric label.
- [x] `M4.8` Health endpoints. Acceptance: `/healthz` reports the process is
  alive, `/readyz` fails only for drain, and no load-related condition can make
  it flap.
- [x] `M4.9` The per-tenant series allowlist. Acceptance: a tenant on the
  allowlist gets its own series and one off it is aggregated, and the allowlist
  has a configured ceiling so it cannot become the unbounded label by degrees.
- [x] `M4.10` `pgprox-admin` and the read endpoints over the `Observatory`.
  Acceptance: an aggregate answers from the local view with no fan-out,
  `?scope=local` narrows it, and no response contains a credential or an
  upstream hostname.
- [x] `M4.11` The write endpoints: drain, undrain, and pool reset. Acceptance:
  a drain through the API writes the same desired state the config document
  would, carrying the TTL from `M4.5`.
- [x] `M4.12` The OpenAPI document, generated from the handlers. Acceptance:
  the generated document validates, and it describes every route the router
  actually serves rather than a hand-written list that can drift.
- [x] `M4.13` The `SHOW` parser. Acceptance: `SHOW POOLS`, `SHOW SERVERS`,
  `SHOW CLIENTS`, `SHOW PEERS`, `SHOW QUOTA`, `SHOW TENANTS`, `SHOW CONFIG` and
  `SHOW STATS` each parse, each has a `SHOW LOCAL` form, and an unknown `SHOW`
  is an error rather than an empty result.
- [x] `M4.14` `SHOW` result rendering, PgBouncer-compatible where the command
  exists there. Acceptance: the columns of the shared subset match PgBouncer's
  names and order, so an existing dashboard keeps working.
- [x] `M4.16` A single `SHOW` entry point. Found reviewing M4: `show::parse`
  produced a command and `rows::render` consumed one, and nothing joined them,
  so a caller had to know to call both and the first one to forget the
  not-a-`SHOW` case would break every client that sends a query. The same
  shape as the three gaps M5's reviews found.
- [x] `M4.17` Let the metric registry render its own metadata. Found reviewing
  M4: the registry declared names, kinds and help text and could produce none
  of it, so an exporter would have to type every name again at the call site,
  which is the second source the registry exists to remove.
- [x] `M4.18` Test that the two surfaces agree, and pin which pairs actually
  correspond. Found reviewing M4: ADR 0018's central claim, that the HTTP API
  and `SHOW` cannot drift into different answers, was nowhere checked. Worse,
  `SHOW SERVERS` and `GET /v1/servers` share a word and mean different things,
  and nothing said so.
- [x] `M4.19` Remove the drain contract's ambiguous `Option`. Found reviewing
  M4: `Observatory::set_mode(mode, None)` documented "persists until changed"
  while `DrainState::set(mode, None, now)` applied the default TTL. The same
  absence meant opposite things in two APIs M6 has to wire together.
- [x] `M4.15` Close M4.

## M6: integration

`pgprox-session` and `bin/pgprox` compose the real implementations. Everything
before this milestone was built against fakes on purpose, so M6 is where the
design either fits together or does not.

### The failure mode this milestone inverts

M5 found three modules with good tests and no caller, and M4 found a fourth.
M6 is almost entirely callers, so that failure inverts: the risk here is not an
uncalled module but two modules that each work and do not meet. The check that
catches it is the same one that caught the others, applied in the other
direction: for each seam, name the single place the two sides join, and fail if
there is more than one.

### The two seams left open on purpose

`Connector` and `ReplicaProbe` are traits with fakes and no real implementation,
because implementing either needs a socket and the crates that own them are
sans-I/O. Both land here, in `pgprox-session`, which is where the dependency
rule already allows `pgprox-proto`.

- [x] `M6.1` Define M6: this decomposition and `scripts/gates/m6-complete.sh`.
  Acceptance: the gate script runs against the current tree and reports what is
  missing, rather than passing vacuously.
- [x] `M6.2` `pgprox-session` and the client state machine: startup through
  `ReadyForQuery`, sans-I/O, as a pure function of state and event. Acceptance:
  a client that skips `SSLRequest` under `require_tls` gets an `ErrorResponse`
  saying why rather than a closed socket, and no step touches a socket.
- [x] `M6.3` The token path: a `PasswordMessage` carrying a JWT, resolved
  through `CredentialResolver`. Split from the SCRAM path below while doing it,
  because the two share only the branch that chooses between them and could
  not be committed together with the tree green. Acceptance: the token reaches
  the resolver, a refusal and an unreachable sidecar are distinguishable to an
  operator and identical to the client, and no `Debug` anywhere on the path
  prints the token.
- [x] `M6.4` The static-user SCRAM path, verified against a credential this
  crate does not own. Acceptance: `pgprox-session` gains no dependency on
  `pgprox-auth`, so the seam is a trait the composition root fills, and a
  client that gets the proof wrong is refused with the same message a bad token
  gets.
- [x] `M6.5` Remove `PinReason::Copy` and `PinState::observe_copy`. Found
  writing the relay step: `pgprox-proto` already has `HoldReason::Copy`, which
  is transient and correct, while the pool's version routes a transient
  condition into a mechanism documented as never clearing. Acceptance: a
  session that runs a `COPY` to completion is releasable afterwards, and there
  is one place that says why a connection is held during one.
- [x] `M6.6` The relay step: one client frame in, actions out, with the release
  decision taken from `ReadyForQuery` rather than from the SQL text.
  Acceptance: a connection is never released mid-transaction, never with an
  extended-query sequence outstanding, and never while pinned.
- [x] `M6.7` Session parameter replay and prepared-statement replay on acquire.
  Acceptance: a session that set a replayable parameter observes it on a
  different upstream connection, and a `Parse` the target connection does not
  hold is replayed before the `Bind` that needs it.
- [x] `M6.8` The SASL messages the proxy has to write. Found starting the I/O
  shell: `pgprox-proto` encodes `AuthenticationOk` and cleartext password and
  nothing for SASL, so the SCRAM path M6.4 built has no way onto the wire.
  Acceptance: the three messages round-trip through this crate's own decoder,
  which is the check a hand-written length prefix needs.
- [x] `M6.9` The I/O shell, generic over `AsyncRead + AsyncWrite + Unpin`.
  Acceptance: the whole session runs over `tokio::io::duplex` with no port
  opened, and cancelling the future mid-frame leaves no connection leaked.
- [x] `M6.10` The frontend messages the proxy has to write. Found starting the
  upstream handshake: `pgprox-proto` encodes what a server says and nothing of
  what a client says, so speaking to Postgres meant hand-rolling a startup
  packet, which the conformance test already does in a third place. Acceptance:
  each message round-trips through this crate's own frontend decoder, and the
  conformance client uses these rather than its own copies.
- [x] `M6.11` The upstream handshake, as a state machine. Split from the
  `Connector` below while writing it: the sequence is testable with no socket
  and the socket part is not, and one commit holding both would leave the
  interesting half only reachable through the dull one. Acceptance: every
  authentication method Postgres can ask for is answered or refused by name,
  and an unsupported one says which it was.
- [x] `M6.12` The real `Connector`: dial, drive the handshake, harvest the
  `ParameterStatus` set. Acceptance: the trait's fake and the real
  implementation are exercised by the same test body, so a behaviour the fake
  invents is caught.
- [x] `M6.13` The `ParameterStatus` probe cache, keyed per `(host, db)`.
  Acceptance: a second pool for the same host and database opens no second
  probe connection.
- [x] `M6.14` The real `ReplicaProbe` over `pg_last_wal_replay_lsn()` and
  `pg_is_in_recovery()`. Acceptance: a replica that stops replaying leaves the
  eligible set within one poll interval, and a probe failure is a stale reading
  with an age rather than a silent zero.
- [x] `M6.15` Cancellation across nodes: decode the node from the key, forward
  to the owner, issue the real `CancelRequest` upstream. Acceptance: a cancel
  arriving at a node that does not own the connection reaches the one that
  does, and an unknown key is refused rather than ignored.
- [x] `M6.16` `M3.12`: forward a quota request to the leader over the gossip
  transport. Carried from M3, where it was deferred because it needed a
  transport that did not exist. Acceptance: a node that is not the leader
  obtains a lease, and a request racing a leader change either fails or is
  granted by exactly one leader, never both.
- [x] `M6.17` `bin/pgprox`: the composition root, with the wiring in a lib
  target and `main.rs` doing nothing a test cannot call. Acceptance: the wiring
  is called by a test with fakes, and `main.rs` is the only excluded file.
- [x] `M6.18` The upstream dialer and the pool. Split out while starting the
  live `Observatory`, which cannot report pool statistics for a pool that does
  not exist. Acceptance: a backend that asks for TLS gets it, one that does not
  is refused a plaintext connection only if the document says so, and the pool
  the node holds is the one the `Connector` opens through.
- [x] `M6.19` The live `Observatory`, reading the real components. Acceptance:
  the surfaces-agree suite from `M4.18` passes against the live implementation
  unchanged, since it was written against the contract rather than the fake.
- [x] `M6.20` Let the pool lend a connection out. Found writing the accept
  loop: `with_connection` runs a closure while holding the pool's lock, so a
  relay cannot await on the socket it borrowed, which is the only thing a relay
  does. Acceptance: a session owns its upstream connection for the duration of
  a transaction and gives it back on release, and a connection that is out on
  loan cannot be handed to a second session.
- [x] `M6.21` The accept loop and listener, with TLS and the client connection
  ceiling. Acceptance: a node at its ceiling refuses with a message naming the
  limit, and refusal never takes down connections already established.
- [x] `M6.26` The node's HTTP surface: `/healthz`, `/readyz`, and the admin
  routes over the live `Observatory`. Split out of `M6.22` while starting it:
  the drain sequence is judged on `/readyz` failing first, and nothing answers
  `/readyz`, because `M4.8` built the `Health` type and `M4.10` built a router
  and neither was ever bound. Found the second seam at the same time: `App` and
  `NodeObservatory` each owned a `DrainState`, so a drain through the API and
  the drain a probe reads were different facts. Acceptance: a drain posted to
  the API makes `/readyz` fail on the same node, `/healthz` keeps passing, and
  there is one `DrainState` in the process.
- [x] `M6.27` The run loop: bind the client listener and the admin listener,
  spawn the accept loop and the periodic work, and run until signalled. Split
  out of `M6.22` with `M6.26`: `start` builds a node and returns it, so the
  binary as committed opens no port at all and a drain has nothing to drain.
  Acceptance: the binary serves a query end to end over a real socket, and the
  run loop returns when its shutdown signal fires rather than being aborted.
- [x] `M6.29` The gossip transport, part one: a socket per node exchanging
  digests. Found wiring the run loop: `M3.8` named the digest an API and built
  the merge rule, and nothing ever put one on a wire, so every node believes it
  is alone. A node's own digest also never enters its digest store and
  `GossipCoordinator` exposes no way to read it, which makes `report` and
  `report_tenants` write-only and leaves `Observatory::stats` at cluster scope
  omitting the node that answered. Acceptance: two nodes in one process
  converge on each other's digests, a stale message is refused by version, and
  an oversized one is refused without allocating for it.
- [x] `M6.30` Give `Entropy` a failure channel. Found writing `SystemEntropy`:
  `next` returns a `u64` and the system source can fail, so the only options
  are panicking on a connection path or returning a guessable cancel key.
  Refusing the connection is the third, and the trait cannot express it.
  Acceptance: a failing entropy source refuses the connection with an error
  naming the cause, and no cancel key is ever issued from a fallback.
- [x] `M6.31` The gossip transport, part two: quota requests to the leader.
  `M6.16` built the forwarding rule behind `QuotaTransport` and the composition
  root never implemented it, so every node falls back to its guaranteed share
  and the free pool is unusable. Acceptance: a non-leader obtains a lease over
  the socket, and a request racing a leader change is granted by exactly one.
- [x] `M6.32` The gossip transport, part three: forwarded cancels. `M6.15`
  built the routing and `serve::cancel` drops `Routing::Peer` on the floor, so
  a cancel that lands on the wrong node does nothing. Acceptance: a cancel for
  a connection another node owns reaches that node, and an unknown key is
  refused rather than ignored.
- [x] `M6.33` `pgprox-core` warns on an unused import in the default build.
  `admin.rs` imports `NodeMode` for the fake, which is behind `test-fakes`, so
  a plain `cargo check -p pgprox-core` warns. Nothing caught it because
  `check-crate.sh` runs clippy with `--all-features`, where the import is used.
  Acceptance: the default build is warning-free, and the check that missed it
  covers the default feature set too.
- [x] `M6.35` `check-coverage.sh` reports stale numbers. Its `cargo llvm-cov
  clean --profraw-only` keeps the instrumented build, which is the point, but
  it also keeps object files from a previous run of a different crate, and the
  report then attributes zero coverage to functions the run did execute. Seen
  twice: `pgprox-config` read 85% and 98% for the same tree minutes apart, and
  `pgprox-route` read 94% and 99%. A gate that reports a different number each
  time is a gate nobody can act on, and the direction it errs in is not always
  the safe one. Acceptance: two consecutive runs over the same tree report the
  same number, and the fix does not put the six-minute rebuild back.
- [x] `M6.28` Poll the replicas a grant names, and route from that. Found
  wiring the run loop: `M5.18` built `ReplicaWatch`, `M6.14` built
  `SqlReplicaProbe`, and the session path builds a fresh empty `Replicas` per
  session and polls nothing. Every replica is therefore permanently ineligible,
  which makes `M6.24`'s watermark assertion pass for the wrong reason: no read
  ever reaches a replica at all. Acceptance: a read lands on a replica, and one
  behind the session watermark does not. Second half split out as `M6.34`: a
  read reaches a replica now, and nothing sets the watermark yet, so
  read-your-writes is not enforced.
- [x] `M6.34` The session write watermark. Found finishing `M6.28`:
  `SessionRouter::record_write` exists and nothing calls it, so the watermark
  is always unset and every healthy replica is eligible to every session
  including one that just wrote. The position has to come from the primary,
  which means asking it: `pg_current_wal_insert_lsn()` on the same connection
  before release, on write transactions only. Acceptance: a session that wrote
  does not read from a replica that has not replayed the write, and a session
  that never wrote pays no extra round trip.
- [x] `M6.22` The drain sequence, end to end. Acceptance: `/readyz` fails
  first, gossip announces before any client is closed, in-flight transactions
  finish, and the grace timer force-closes the remainder.
- [x] `M6.36` The relay deadlocks on `COPY ... FROM STDIN`. Found by the first
  pgbench run the stack ever did, during `--initialize`. `pump` reads server
  frames until `ReadyForQuery`, which never arrives during a copy-in: the
  server answers `CopyInResponse` and then waits for the client, while the
  proxy waits for the server. Both sides wait forever and the session is
  wedged, holding an upstream connection. `pgprox-proto` has tracked COPY mode
  since M1.8 and the shell does not use it. Acceptance: a `COPY FROM STDIN`
  completes through the proxy, a `COPY TO STDOUT` still does, and a session in
  copy-in is never released.
- [x] `M6.23` `deploy/` and `scripts/e2e.sh`: three proxy nodes, a primary, two
  replicas, the mock sidecar. Acceptance: the script brings the stack up and
  reports which component failed when it does not, rather than a compose exit
  code.
- [x] `M6.24` The e2e assertions the milestone is judged on: pgbench clean,
  drain with zero failed transactions, no replica read behind the session
  watermark. Acceptance: each assertion fails when its property is broken on
  purpose, verified once per assertion.
### Raised by the first full review round

Every one of these is the failure M6 exists to invert: a module built against
fakes in an earlier milestone, with tests, and no caller in the composition
root. They were found by asking of each crate "what in here does the binary
never touch", which is the same question M5's and M4's reviews asked.

- [x] `M6.37` Spawn the configuration poll loop. `FileSource::run` is the
  loop M4.3 and M4.4 built, and nothing starts it, so a `ConfigMap` edit never
  reaches a running node: hot reload, the last-good-config rule, and a drain
  written into the document are all unreachable from the binary. Acceptance: a
  document rewritten on disk changes what `/v1/config` reports without a
  restart, and a broken one leaves the previous config serving.
- [x] `M6.38` Reap idle upstream connections. `LivePool::reap_idle` exists and
  nothing calls it on a timer, so M5.13's "a pool that goes quiet drops to
  zero" is true of the type and false of the process. A proxy that never
  releases an idle connection holds the database's connection budget for as
  long as it runs. Acceptance: a pool left quiet past its idle timeout drops to
  zero in a running node.
- [x] `M6.39` The node says nothing. There is no tracing subscriber, no span,
  and no log line anywhere in the binary: a refused client, a dead upstream and
  a failed gossip round are all silent. `standards/observability.md` and
  `pgprox-observe::spans` describe what should be emitted and nothing emits.
  Acceptance: a refusal, a drain and an upstream failure each produce one line
  an operator can act on, and no line carries a credential.
- [x] `M6.40` Nothing emits a metric. `pgprox-observe::metrics` is a registry
  of names, kinds and help text with no counter behind it and no exporter, so
  every dashboard M4 designed reads nothing. Acceptance: `/metrics` serves the
  registry's series, the numbers move when the thing they count happens, and
  the per-tenant allowlist from M4.9 is what decides which get a tenant label.
- [x] `M6.41` Terminate client TLS. `run::context` hard-codes
  `TlsPosture::Optional` and the shell refuses a client that asks to upgrade,
  so every JWT crosses the network in cleartext. The mission is tokens over
  TLS and `pgprox-tls` has had the server config since M1.10. Acceptance: a
  client that sends `SSLRequest` gets a TLS session, `require_tls` refuses one
  that does not, and the e2e stack runs with certificates.
- [x] `M6.42` Wire the static-user SCRAM path. M6.4 built it and `serve`
  answers `Credential::Scram` with a refusal, so the admin surface ADR 0002
  promised for non-JWT clients still cannot be reached. This is also `M1F.12`,
  which was blocked on M6 existing. Acceptance: a configured static user
  authenticates with SCRAM and reaches the `SHOW` surface, and an unknown one
  is refused with the message a bad token gets.
- [x] `M6.43` Answer `clients` across the fleet. `NodeObservatory::clients`
  returns `Partial` for any cluster-scoped read because the fan-out was not
  built; the gossip transport that would carry it now exists. Acceptance: a
  cluster-scoped client list returns every node's clients, and one that loses a
  peer is still `Partial` rather than silently short.
- [x] `M6.44` Nothing sheds. M3.7 built the shed decision and its guard rails,
  M6.19 wired the counter that reports sheds, and no code path ever decides to
  shed a client, so tenant rebalancing does not happen in a running fleet.
  Acceptance: a tenant over its share on a non-home node has an idle session
  shed toward its home, and every guard rail M3.7 named still refuses.

- [x] `M6.46` The shed rate limit has no window. `shed_pass` passes
  `recent_sheds: 0`, so the per-tenant-per-minute guard rail M3.7 built can
  never refuse: the rule exists and is fed a number that always admits. The
  other six guard rails are fed real values. Acceptance: a tenant shed more
  than the configured number of times in a minute is refused by
  `ShedRefusal::RateLimited`, and the window is per tenant rather than per
  node.
### Raised by the second review round

Both gates pass, so these were found the same way the first eight were: by
asking of each crate what the binary never touches.

- [x] `M6.47` Nothing replays a session's state onto a new connection.
  `pgprox-session::resume` is M6.7, with `on_acquire`, `replayed` and
  `before_bind`, and `serve::relay` calls none of them. In a transaction
  pooling proxy a session gets a different upstream connection per
  transaction, so a `SET` that was replayable is silently lost at the next
  boundary and a `Parse` the new connection does not hold is never replayed
  before the `Bind` that needs it. Both are correctness bugs a tenant sees as
  their session forgetting things. Acceptance: a replayable parameter survives
  a change of upstream connection, a prepared statement is replayed before the
  `Bind` that uses it, and `SET LOCAL` is still never replayed. Split: the
  parameter half is done here, the prepared-statement half is `M6.49`.
- [x] `M6.49` Prepared statements are not mapped onto the connection that
  serves them. `resume::before_bind` and `pgprox-pool`'s statement map are
  M5.10 and M6.7, and the relay calls neither, so a client that `Parse`s on
  one connection and `Bind`s on another gets "prepared statement does not
  exist" from the server. Needs the name rewriting ADR 0011 puts in
  `pgprox-proto`, which is why it is not the parameter half's commit.
  Acceptance: two sessions preparing identical SQL share one global name, a
  `Bind` whose connection does not hold the statement replays the `Parse`
  first, and a client's own statement name never reaches a server.
- [x] `M6.48` No metric carries a tenant. `pgprox-observe::tenants` is M4.9's
  per-tenant series allowlist with its configured ceiling, and nothing calls
  it: the exporter emits no per-tenant series at all, so the allowlist governs
  nothing and an operator cannot see which tenant is using the connections.
  Acceptance: a tenant on the allowlist gets its own series, one off it is
  aggregated, and the ceiling is what stops the label becoming unbounded.

- [x] `M6.50` The pool does not remember what each connection holds. `M6.49`
  sends `Close` and `Parse` before every `Bind`, because it cannot know
  whether the connection it was just lent has seen the statement, and Postgres
  refuses a second `Parse` under a name it already holds. Correct on a cold
  connection and on a warm one, and a re-parse per transaction is the thing
  prepared statements exist to avoid. `ConnectionStatements` is M5.10 and
  exists; what is missing is somewhere to keep one per pooled connection.
  Acceptance: a `Bind` on a connection that already holds the statement sends
  no extra messages, and one on a connection that does not still works.
### Raised by the third review round

- [x] `M6.51` A client that connects and says nothing holds a slot forever.
  Nothing on the handshake path has a deadline: `negotiate` and
  `authenticate_token` await a read that may never come, and the connection
  counts against `max_client_conns` while it does. Opening the ceiling's worth
  of sockets and sending nothing takes a node out of service with no
  credentials and no traffic, which is the cheapest denial of service there
  is. Acceptance: a client that sends nothing is closed after the configured
  time, the limit is configurable, and a slow but real client is not affected.
- [x] `M6.52` The pool mode a grant asks for is ignored. `PoolHints::mode`
  carries transaction, session or statement pooling, the sidecar sends it, and
  nothing reads it: every tenant gets transaction pooling. A tenant that asked
  for session pooling and silently got transaction pooling loses temporary
  tables and advisory locks between statements, which the pin list catches
  only for the cases it knows. Acceptance: a grant asking for session pooling
  holds its connection for the session, and one asking for transaction pooling
  is unchanged.
- [x] `M6.53` `max_client_conns` is read once at startup. The gate is built
  from the configuration the node booted with, so raising the ceiling in the
  `ConfigMap` does nothing until a restart, which is exactly what an operator
  does when a node is refusing connections. `M6.37` made the document reload;
  this is one of the things that did not follow. Acceptance: raising the
  ceiling admits more clients without a restart, and lowering it refuses new
  ones without closing established ones.

### Raised by the fourth review round

- [x] `M6.54` The proxy cannot make a verified upstream connection.
  `entry::start_with` builds the client configuration from an empty root
  store, so every backend whose grant says `TlsMode::Verified` fails to
  verify: the proxy can reach a database only in the clear. The e2e stack
  uses `disabled`, which is why nothing noticed. `pgprox_tls::root_store_from_pem`
  has existed since M1.10 with no caller. Acceptance: a node told where its
  CA is connects to a TLS-requiring backend, and one told nothing still
  refuses rather than trusting whatever answers.
- [x] `M6.55` The tenant's statement timeout is ignored. `PoolHints` carries
  `statement_timeout_ms`, the sidecar sends it, and nothing applies it, so a
  tenant's cap on its own runaway queries does nothing. It is a `SET` on
  acquire, next to the parameter replay that already happens there.
  Acceptance: a grant naming a statement timeout has it in force on every
  connection the session borrows, and one that names none changes nothing.

### Raised by the fifth review round

- [x] `M6.56` The quota layer does not bound the pool. `LivePool::set_limit`
  and `ClusterCoordinator::request_quota` have no caller in the binary, so a
  node opens up to its configured `PoolConfig::max_size` per pool whatever the
  cluster says its share is. The invariant the whole of M3 exists to protect,
  that guaranteed plus leased never exceeds the cap, is enforced in a ledger
  nothing consults: three nodes each opening fifty connections to a server
  capped at sixty is exactly the failure ADR 0004 calls the one with no
  graceful degradation. Acceptance: a node's pools together never hold more
  than its allowance, a node that needs more asks the leader, and one that is
  refused waits rather than opening.

### Raised by the sixth review round

- [x] `M6.57` A configuration that stopped reloading is invisible. M4.3 built
  `FileSource::last_error` and said in its own documentation that this is what
  `/readyz` and the admin API report, and nothing reports it: the poll loop
  swallows the error and a node serving a stale configuration looks exactly
  like one serving the current document. Not `/readyz`, which by M4.8's rule
  fails only for drain, so it is a log line and a metric. Acceptance: a broken
  document reaching the mount produces one warning an operator can act on and
  a metric that stays wrong until it is fixed.

### Raised by the seventh review round

- [x] `M6.58` A tenant this node stopped serving was never forgotten.
  `ClusterCoordinator::forget_tenant` had no caller, so the tracked set only
  ever grew: a leak with a slow fuse in a proxy built for five thousand
  tenants, and the reservations it held were capacity peers could have used.
  The tick forgets a tenant it reported last time and does not serve now.

### The eighth review round, which found nothing new

Four things in the workspace still have no caller in the binary, and each was
looked at and left alone rather than wired reflexively:

- `SessionStatements::close_all` is what a session's own statement map would
  need if it outlived the session. It does not: the session's map dies with
  it, and the connection's map is bounded by the LRU that `M6.50` now uses.
- `Replicas::lag_behind` feeds `pgprox_replica_lag_bytes`, which is one of the
  four metrics `bin/pgprox/src/metrics.rs` names in `UNSOURCED` with the
  reason: it needs the primary's position at the same instant as the
  replica's, which is the watermark's problem rather than the exporter's.
- `spans::may_record_query` decides whether a query's text may be logged. That
  it is never called is the correct state: queries carry tenant data, and the
  feature exists so the decision has one place to live if it is ever switched
  on.
- `lease::reap` and `membership::reap` are called by `NodeCoordinator::observe`
  inside `pgprox-cluster`, which the tick calls. They are internal, not
  uncalled.

Eight rounds, thirty-four tasks found after the milestone's own gates first
passed. The ratio is the argument for the rule: the gates say the milestone's
stated conditions hold, and asking "what does the binary never touch" is what
says whether the thing behind them is real.

- [x] `M6.25` Close M6.

## M7: scale and performance

The reference workload, the measurement apparatus, allocation budgets on the
declared hot paths, `iai` benchmarks, buffer reclaim, and the connection
harness.

### Order is the point

`plan.md` states it directly: build the measurement before the optimization, or
the optimization is guesswork. So the workload description, the load client and
the scale script come first, then the budgets that turn a claim into an
assertion, and only then anything that changes code for speed. A task here that
makes something faster without a recorded baseline is not done, whatever the
number says.

### What this milestone is judged on, and what it is run at

The roadmap's condition is 100k connections against one node, userspace RSS
under 500 MB, added p99 under 1ms, upstream connections at or under the cap.
That number is unchanged and stays unmet until a run at 100k produces it.

The runs in this milestone are at 1000 connections on a developer machine,
which is enough to show the per-connection slope and to catch anything that
breaks between one connection and a thousand. `scripts/scale.sh` takes the count
as an argument for exactly that reason, and every run is recorded so the slope
is comparable. Extrapolating a number is not the same as meeting it, and the
recorded runs say which they are.

- [x] `M7.1` Define M7: this decomposition and `scripts/gates/m7-complete.sh`.
  Acceptance: the gate runs against the current tree and reports what is
  missing rather than passing vacuously.
- [x] `M7.2` The reference workload document and its parser, in a new
  `pgprox-load` crate: tenant mix, query shape distribution, connection churn,
  transaction size, replica read fraction. Acceptance: the document is a
  committed file, a malformed one is refused with the field named, and nothing
  in the crate touches a socket.
- [x] `M7.3` The sampler: a workload plus a seed yields a deterministic stream
  of statements. Acceptance: two samplers on the same seed produce identical
  streams, and the observed mix converges on the declared distribution.
- [x] `M7.4` The latency histogram and the run report. Acceptance: p50 and p99
  over a known set of samples are the values computed by hand, and the report
  serialises to JSON a script can read without parsing prose.
- [x] `M7.5` `bin/pgload`, the load client: opens N connections, replays the
  sampled workload, prints the report. Acceptance: it runs against a real
  Postgres and against a proxy with the same arguments, its own errors are
  counted rather than swallowed, and `main.rs` holds no logic a test cannot
  reach. Architecture gains it as a stated composer, since it speaks the wire
  protocol and so composes `pgprox-proto`.
- [x] `M7.6` `scripts/scale.sh <connections>`: brings up the stack, measures the
  direct-to-Postgres baseline and the through-proxy run, and reports
  per-connection RSS, added p50 and p99, and the upstream connection count
  against the configured cap. Acceptance: it fails when the cap is breached,
  and a run at 1000 reports four numbers rather than a pass or fail alone.
- [x] `M7.7` Record the runs: `product/perf/` holds one file per run with the
  workload version, the connection count, the machine, and the numbers.
  Acceptance: two runs at different counts are comparable from the files alone,
  and the 1000-connection run is committed.
- [x] `M7.8` Allocation budgets, `pgprox-proto`: `dhat` in an ordinary test,
  asserting counts for frame boundary scanning and the steady-state relay step.
  Acceptance: the budget is a number in `standards/testing.md`, and raising an
  allocation in either path fails the test.
- [x] `M7.9` Allocation budgets, `pgprox-pool`: warm acquire and the
  `ReadyForQuery` release decision. Acceptance: as above, and the claim in
  `standards/testing.md` that these were written allocation-free is either
  confirmed by the assertion or corrected by it.
- [x] `M7.10` Allocation budget, `pgprox-route`: classification plus replica
  eligibility. Acceptance: as above, with the per-statement path asserted rather
  than the session setup.
- [x] `M7.11` Allocation budget, `pgprox-auth`: grant cache lookup on connect.
  Acceptance: as above, and a hit is distinguished from a miss, since only the
  hit is the hot path.
- [x] `M7.12` Allocation budget for gossip digest encode and decode, in
  `bin/pgprox` rather than `pgprox-cluster`: the cluster layer owns the digest
  as a value, the binary owns how it travels. Acceptance: as above, at the
  membership size the reference workload declares rather than at one.
- [x] `M7.13` Instruction-count benchmarks and `scripts/bench.sh`, with a
  committed baseline. Written against `callgrind` directly rather than
  `iai-callgrind`, which pulls two unmaintained crates and fails `cargo deny`;
  a measurement tool is not worth an exception to the supply-chain gate.
  Acceptance: counts are reproducible across two runs on the same tree, and
  the script reports the delta against the baseline rather than a bare
  number.
- [x] `M7.14` `scripts/profile.sh` and the semantic coverage report: replay the
  workload against an instrumented binary, keep execution counts, and emit the
  three lists. Acceptance: the report names functions, and the hot-and-
  under-tested list is non-empty or the report explains why it is not.
- [x] `M7.15` Turn the report's findings into tasks: M7.24 through M7.26 below,
  plus one recorded non-finding. The cold-and-complex list's top entries are
  the TLS-carrying instantiations of `relay`, `serve_client` and `drive`, and
  the generic instantiations of the same functions the replay did exercise:
  the local stack speaks plaintext, so the TLS monomorphisations never ran and
  the list is naming the stack rather than the code. That is a property of
  where the profile was taken, and it is what M7.21 changes.
- [x] `M7.24` The SQL lexer is the top of the optimization queue.
  `pgprox_core::sql::is_word_char` ran 3.6 million times in a 200-connection
  25-second replay, `next` and `skip_trivia` another 1.3 million between them,
  which is roughly ninety token calls per statement, and the route decision
  that drives it costs 7,778 instructions against 20 for a frame scan.
  Acceptance: a baseline before, a number after, and no correctness test
  weakened. The classifier's rule that an unsure answer goes to the primary
  does not move.
- [x] `M7.25` The report's hot-and-under-tested list measures what one replay
  covered, not what the tests cover, and `standards/testing.md` means the
  second. Every crate holds 95% from tier 1, so a function at 18% in the
  report may be fully tested and merely not exercised by this workload.
  Acceptance: the list is cross-referenced against tier-1 coverage, so an
  entry means "hot, and the tests do not reach this either", which is the
  thing worth acting on.
- [x] `M7.40` `Wire::queue` and `serve::told` are tested at the branches that
  were bare: the ordering rule that holds everything after an overflow in the
  overflow, and both of `told`'s error arms. `entry::tls` and
  `entry::static_admin` are left as they are with a reason: they are the
  composition root reading certificates and passwords off disk at start, their
  remaining regions are file-system failures, and a node that cannot read its
  own key fails visibly at boot rather than under load. They are the two
  functions in the repository where an integration test would be testing
  `std::fs`.
- [x] `M7.26` The prepared-statement path runs on every statement and the
  replay reaches little of it: `map_statement_name` 8%, `ready_statement` 18%,
  `statement_of` 29%. It is also the path that deadlocked twice in M6.
  Acceptance: either the workload exercises the extended protocol, which is
  what a real driver uses and what this workload does not currently send, or
  the reason it does not is recorded.
- [x] `M7.16` Buffer reclaim, part one: `Wire` borrows from `BufferSlab` when
  its socket becomes readable and returns when quiescent. Found decomposing:
  the slab has been in `pgprox-core` since M0 with tests, a bound, and no
  caller, and every connection instead holds two `Vec`s for its lifetime.
  Acceptance: a session idle between transactions holds no buffer, and slab
  exhaustion delays a connection rather than allocating past the bound.
- [x] `M7.17` Buffer reclaim, part two: the slab in the composition root, sized
  from config, with its outstanding and idle counts exported. Acceptance: the
  metric moves under load in the scale run, and the per-connection RSS recorded
  before and after M7.16 differ by an amount the commit message states.
  Committed with M7.16: the wire cannot borrow from a slab the composition
  root does not build, so neither half leaves the tree green on its own.
- [x] `M7.18` File descriptor and socket tuning in `deploy/`: `nofile`, the
  backlog, and the `tcp_rmem` and `tcp_wmem` minimums. Acceptance: the scale run
  at 1000 is not limited by a default, and the values are commented with what
  they cost at 100k.
- [x] `M7.20` The workload declares think time, and the load client honours it.
  Found running the first scale run: with no pause between transactions, N
  connections means N requests in flight, so a run at any interesting count
  measures queueing rather than the proxy. The first run at 200 connections
  reported a p50 of 101ms against a direct baseline of 4ms, which is a
  saturated database and not a proxy overhead. It also makes the workload
  wrong in the way that matters most: the design point is 100k connections
  that are idle most of the time. Acceptance: the workload states a think time
  and its distribution, the recorded runs are re-taken, and the added p50 at
  1000 connections is a number a proxy hop could plausibly account for.
- [x] `M7.33` A node never leases past its guaranteed share, so two thirds of
  the upstream cap goes unused. Found by the first scale run against the
  compose stack, which the local one-node stack could not show: with three
  nodes and `guaranteed_fraction: 0.5`, node 1 sits at exactly 10 of 60 while
  600 clients queue behind it, `pgprox_quota_leased` reads 0, and nodes 2 and
  3 hold nothing. At 1000 connections this is 150 refused transactions.
  `run::apply_quota` does ask when `held >= guaranteed + leased`, and its
  result is discarded with `.is_ok()`, so a refusal leaves no log line and the
  failure is invisible from outside. Acceptance: the request's outcome is
  logged either way, the reason it does not lease is named, and a scale run at
  1000 against the compose stack is clean.
- [x] `M7.34` Replica routing works. The conclusion recorded here first, that
  the workload almost never qualifies for it, was wrong, and M7.39 has the
  measurement that overturned it: 32% of statements are served by a replica.
  The evidence it was drawn from was `/v1/pools` sampled during a run, which
  showed the primary at 40 active connections and the replicas at zero or one.
  That is a count of connections in flight at an instant, and a replica read
  on a caught-up replica finishes fast while the primary holds every write and
  every wrapped transaction. A sample of what is busy is not a measure of what
  was served, and reading it as one is exactly the mistake this milestone
  keeps being about.
- [x] `M7.41` The scale stack logged at debug, so the recorded run was partly
  measuring its own logging. Turned on while investigating M7.36 and left on.
  Info by default now, with `SCALE_LOG=debug` for an investigation, and the
  run re-taken: the hop moved from 338us to 351us at p50, which is inside the
  variance either way, and the point is that a measurement run should not be
  writing a line per refusal.
- [x] `M7.42` The semantic coverage report was still the version 2 workload's,
  taken before half the statements went through the extended protocol.
  Regenerated: `map_statement_name` went from 8% to 58% covered by the replay
  and its execution count from 27,000 to 51,000, which is the prepared path
  now being exercised. Found in the review round; a report that describes a
  workload the repository no longer has is worse than no report.
- [x] `M7.39` The watermark is usable as it stands, and the run says so with a
  number: 2,605 of 19,306 statements in the thousand-connection phase went to
  a replica, which is 13%. The workload's own ceiling is 10%: a wrapped
  transaction opens with `BEGIN`, which fixes its target at the primary for
  every statement in it, so only the single-statement reads that are marked
  eligible can go elsewhere. The watermark blocks nothing and no design change
  is warranted. `pgprox_route_total` answers the question from now on, and
  `scripts/scale.sh` reports the share as a delta across the phase rather than
  a running total.
- [x] `M7.35` `scripts/scale.sh --keep` did not keep the stack: the flag set a
  variable and the trap still compared against the old literal, so every run
  tore the stack down and the failure above could not be investigated without
  running it again. Fixed while investigating M7.33; the task is here so the
  fix has a number.
- [x] `M7.36` Three paths ended a client's session without telling it why, and
  the accept loop discarded the reason. `ShellError::Refused` says of itself
  that the client was told before the socket closed; the parameter fetch, the
  pool acquire and the parameter replay each built one without writing
  anything. The login deadline also covered work the client was not doing: a
  client that authenticated and then waited on the proxy for a grant, for
  server parameters or for a pooled connection had its socket dropped in
  silence. Found in a compose scale run, where a handful of clients per
  thousand saw a closed socket and the node's log said nothing at all.
- [x] `M7.37` The listen backlog was the kernel default and a thousand
  simultaneous connections overflowed it: `ListenOverflows` on the node counted
  the drops and the clients saw a socket that closed with nothing on it. A
  reconnect storm after a node restart is the same shape. Found by reading
  `/proc/net/netstat` inside the container after the proxy's own logs came back
  empty.
- [x] `M7.38` A refusal and a failure are different answers, and neither the
  load client nor the scale script could tell them apart. A fatal error is an
  `ErrorResponse` and then a closed socket, so `bin/pgload` reported every one
  of them as "disconnected" and lost the only part that said why; that is what
  sent M7.36 and M7.37 looking for a dropped socket while the proxy was
  answering 53300 correctly. `scripts/scale.sh` now reports a retryable
  refusal as what it is, and still fails on anything else.
- [x] `M7.21` `bin/pgload` speaks TLS behind `--tls-insecure`, and the cost is
  measured: 60 connections for 20s against pgprox-1 in plaintext gives a p50 of
  3,938us, against pgprox-2 over TLS 4,077us, and against pgprox-3 over TLS
  4,237us. Termination costs on the order of 140us at p50 on this stack, which
  is inside the spread between two TLS nodes, so the honest statement is that
  it is small rather than that it is exactly 139us. p99 is 112ms, 108ms and
  127ms respectively: node to node variance, not TLS.
- [x] `M7.22` A one-node stack of local processes, and `scripts/scale.sh
  --local`. Found when Docker Desktop stopped on the development machine
  mid-milestone: every measurement in M7 depended on it, and Postgres was
  installed on the machine all along. The compose stack stays the deployment
  shape and stays what a reported number should come from; every run records
  which stack it was.
- [x] `M7.23` The second of latency the first run reported was the
  measurement's, not the proxy's. A thousand clients offer several times the
  work sixty do, so the database saturates and the queue that forms is in
  front of it; subtracting a sixty-connection baseline from that reported the
  database's queue as proxy overhead. `scripts/scale.sh` now runs the proxy at
  the baseline's connection count as well, and the hop costs 348us at p50 and
  4.3ms at p99. Found by measuring rather than guessing: the proxy's own CPU
  was under half a core while Postgres had 49 of 50 backends active.
- [x] `M7.27` The node does not check its own descriptor limit. A ceiling of
  20,000 clients under a soft `RLIMIT_NOFILE` of 1024 is a node that fails at
  `accept` with something that reads as a network fault. Found writing M7.18.
  Acceptance: the limit and the ceiling are compared at startup and the
  mismatch is a refusal to start or a warning that names both numbers, and it
  is decided which.
- [x] `M7.28` `scripts/bench.sh` runs in CI. Found in the review round: the
  script, the benchmarks and the baseline all existed and nothing ran them, so
  an instruction-count regression would have been caught by whoever happened
  to run it. This repository's own words, from `check-layering.sh`: a rule
  with no gate is a preference.
- [x] `M7.29` Three things in M7's own work that nothing outside a test called:
  `Workload::tenant_count`, `Transaction::writes`, and the workload's
  `cluster_size`, which the gossip budget was supposed to measure at and had
  hard-coded instead. The first two are deleted and the third is now read. The
  same failure M6's first review round found eight times, in code written to
  measure for that failure.
- [x] `M7.30` The documentation caught up with what was built. The `hot-path`
  skill still told a reader to run `iai` benchmarks and a flamegraph script
  that did not exist in that form; `AGENTS.md` listed the per-commit checks
  and none of the measurement ones. Found in the review round, and it is the
  kind of drift that makes a skill worse than nothing: it sends whoever reads
  it to a command that fails. The M7 gate also tightened from grepping for the
  word `dhat` to grepping for `dhat::Profiler`, since a comment satisfied it.
- [x] `M7.31` `_free_port` in `scripts/localstack.sh` was defined and never
  called: written to work around WSL's reserved port ranges, then replaced by
  low default ports and left behind forty minutes later. Found by the review
  round that had just found three of the same in this milestone's Rust.
- [x] `M7.32` CI ran only the M-1 completion gate. Every milestone since has
  had one, each passing on the commit that closed it, and none of them ran
  again afterwards. Found in the review round. M6's and M7's now run on every
  push, where a failure means a regression rather than unfinished work.
- [x] `M7.19` Close the 1000-connection round of M7.
- [x] `M7.43` Close M7. The 100k condition stays as written and stays unmet,
  and the roadmap says what meeting it needs rather than pretending the
  milestone is finished.

## Found after M7 closed

Answering "how many connections would 16 GB and 8 cores hold" needed CPU and
memory per connection, which the milestone had never measured directly. The
measurement found three things.

- [x] `M7.44` `Wire::fill` borrowed a slab buffer before awaiting the client's
  next statement, so every connection held one through its whole think time.
  That is the opposite of what the slab is for: `plan.md` says a connection
  borrows when its socket becomes readable, and an idle connection is supposed
  to cost a socket and a state struct. The read goes into a stack chunk first
  now and the buffer is borrowed only once bytes have arrived.
- [x] `M7.45` `docker compose restart pgprox-1` killed the node every time.
  The entrypoint waits for the sidecar's socket to exist, and a restart leaves
  the previous one behind, so the wait passed instantly and the proxy exited
  unable to connect. The stale socket is removed before the sidecar starts.
- [x] `M7.47` Ten thousand connections, which needed three changes and one
  thing this machine cannot give. The load client ships in the deploy image
  and runs as a `loadgen` service inside the compose network, because the WSL
  host has 4,096 ephemeral ports (44620-48715) and a container has 28,231, so
  ten thousand connections to one address is possible from inside and not from
  outside; it also drops the published-port forwarder out of the path. The
  node under test gets `net.core.somaxconn` of 16384 through a compose sysctl,
  so the 8192 backlog it asks for is not trimmed to 4096. And `bin/pgload`
  takes `--ramp`, because ten thousand connections arriving in the same
  instant each run a transaction before any of them thinks, which measures a
  stampede rather than a steady state.
- [x] `M7.48` `product/perf/workload-idle.yaml`: the reference workload's
  shapes with a think time of 30s to 5min. The reference workload asks what a
  busy tenant costs; this one asks what an open connection costs, which is the
  other half of the design point and the question a connection-count target is
  actually about. Identical in every other field, so a difference between two
  runs has one cause.
- [x] `M7.49` The per-connection cost is the session future, not the buffer,
  and it is 4,704 bytes rather than 11,640. Measured per connection against
  the running node: 8,067 bytes, down from 15,489.
  Measured: `size_of_val` of the future `session()` returns is 11,640 bytes,
  which is 1.1 GB at 100k before a socket, a registry entry or a task, and it
  accounts for nearly all of the 15.5 KB per connection measured at ten
  thousand. The buffer was the obvious suspect and is not the answer: the slab
  peaks at a few hundred buffers outstanding during a 10k run and falls to
  zero, so buffers are under 7% of the process. Rust reserves space in a
  future for everything alive across an await, and what was alive was a 4 KiB
  stack array in `Wire::fill`, the startup negotiation, and the authentication
  exchange. The array is 512 bytes now and reads after the first go straight
  into the borrowed buffer; the startup futures are boxed, so their frames are
  freed when startup ends rather than held for the life of the connection.
- [x] `M7.50` Startup returns rather than falling through into serving.
  `authenticate` hands back a `Ready` and the caller runs the relay loop, so
  the state machine, the SCRAM exchange, the sidecar call and the parameter
  fetch are dropped before a connection settles. The session future is 3,872
  bytes rather than 4,392 and a connection costs 7,281 rather than 7,528.
  A smaller win than the arithmetic promised, and the measurement says why:
  boxing the *serving* half as well took the future to 2,352 and made the
  connection cost 7,920, because a box relocates long-lived bytes to the heap
  and adds an allocator header rather than removing anything. Boxing pays only
  for state that is freed early.
- [x] `M7.51` The grant cache had no caller. `pgprox-auth` has held a caching,
  singleflighting resolver since M2, with its own tests and the allocation
  budget M7.11 measured against its hit path, and the composition root passed
  the sidecar resolver straight through. Every connection made its own gRPC
  call: at a hundred thousand of them, more concurrent streams than one h2
  connection carries, which is the `locally-reset streams reached limit (1024)`
  in the log and the `authentication service unavailable` every client saw.
  The wrap is in `start_with` rather than beside the sidecar connection, so a
  test can reach it, and one now counts what reaches the inner resolver.
- [x] `M7.52` Not a bug: the shed rate limiter already bounds it. A shed
  decision does assume the client can land on another node and nothing checks
  that, but `ShedConfig::max_per_tenant_per_minute` caps the damage at sixty a
  minute per tenant, and the 100k run shed three times in five minutes. The
  churn in that run was the credential path, which is M7.51.
- [x] `M7.53` A hundred thousand connections held on one node: 546 MB of
  userspace, 5,726 bytes each, stable for six minutes with 99,940 of 100,000
  registered. Nine per cent over the roadmap's 500 MB. It measures holding
  rather than serving: the workload thinks for ten to fifteen minutes before
  its first transaction, so almost none were attempted, and the other two
  conditions are still the 1000-connection runs'. Recorded in
  `product/perf/run-2026-07-28-100k-hold.md`.
- [x] `M7.54` The load generator was hitting limits that looked like the
  proxy's. A container has 28,231 ephemeral ports and each generator opened
  20,000 connections, so any churn exhausted it and the failure reads as the
  proxy refusing; the override now gives the generators the whole unprivileged
  range and `tcp_tw_reuse`. And every connection ran a transaction the instant
  it connected, so a hundred thousand arrivals meant a hundred thousand
  transactions at once: a connection now thinks before its first one.
- [x] `M7.46` The proxy spends about 3.2 cores serving 700 statements a second
  at 2000 connections, which is 4.5ms of CPU per statement against an
  instruction count of roughly 10us for the decision path. A `perf` profile of
  the native binary puts 19% in `__memmove_avx_unaligned_erms` and another
  12% in the allocator. The likely cause is `Wire::consume`, which drains
  consumed bytes off the front of the read buffer and therefore moves the
  remainder on every frame, where `FrameRelay` in `pgprox-proto` uses a cursor
  for exactly this reason. Acceptance: the copy per frame is gone or explained,
  and the CPU per connection is re-measured against the number above.
  The copy is gone and the number did not move: 4,242us per statement at 2,000
  connections against the 4.5ms this task recorded. So the memmove was not the
  cost. A profile saying 19% of time is in `__memmove_avx_unaligned_erms` says
  where time goes, not why there is so much of it, and removing one caller
  moved the total by less than the run-to-run spread.
  The cursor stays, because it is correct and cheaper by construction, and it
  paid for its 48 bytes in the session future by boxing the write overflow
  buffer alongside: 5,064 bytes against 5,096 before, under a 5 KiB ceiling
  that was not raised to accommodate it. What it is not is an answer to this
  question, and `M7.55` is.
  `scripts/scale.sh` reports CPU per statement now. The 4.5ms in this task came
  from an ad-hoc `perf` session that left nothing behind, which is why it took
  a milestone and a half to find out it had not changed. Recorded in
  `product/perf/run-2026-07-28-2000-cpu.md`.
- [x] `M7.55` Where the 4.2ms per statement actually goes. 157 seconds of CPU
  across a 30 second phase is 5.2 cores, and at 1,234 statements a second that
  is three orders of magnitude above the instruction count for the decision
  path. That is not a hot loop being slightly slow.
  Two candidates survive `M7.46`'s run and neither is measured. It may be the
  connections rather than the statements: 2,000 tasks each with a socket, a
  timer and a registry entry cost something per wakeup whether or not they have
  work, and a per-statement figure divides all of it by the statements. Or it
  may be the queue: p99 at that connection count is 25 seconds because the
  database is saturated, so the proxy is spending its time on clients that are
  waiting.
  Acceptance: a run whose connection count and statement rate move
  independently says which, and the answer is a number rather than a
  hypothesis. A profile taken before that run is a profile of the wrong thing.
  It is the connections. Two runs at the same statement rate, four times apart
  in connection count: 500 connections did 31,856 statements for 30,330ms of
  CPU, and 2,000 connections against a workload thinking four times as long did
  36,317 statements for 134,020ms. Per statement that is 952us and 3,690us,
  which is not a number. Per connection per second it is 2.02ms and 2.23ms,
  which is the same number twice.
  So the proxy spends about 2ms of CPU per connection per second, near enough
  regardless of what that connection asks for, and one core holds about five
  hundred of them. That is the answer `M7.46` was looking for in the wrong
  place: the memmove was never going to matter because the cost does not scale
  with the thing the memmove was in.
  `product/perf/workload-slow.yaml` is what holds the work still while the
  connections move, and `scripts/scale.sh` takes a `WORKLOAD` now. Recorded in
  `product/perf/run-2026-07-29-connection-cost.md`, with the candidates this
  points at and the caveat that both runs saturate the database.
- [x] `M7.56` The 2ms per connection per second, named. `M7.55` says where the
  CPU goes and not what it is, and the shape of it, a cost per connection that
  barely depends on what that connection does, points at something running per
  connection on a schedule or at contention on something shared. The session
  registry, which every connection touches on every state change and which the
  admin API and the metrics exporter also walk; the timers each connection
  holds; and the scheduler itself at two thousand tasks per worker are the
  candidates, none measured.
  Acceptance: a profile taken *now*, which is the first time one would be
  looking at the right thing, and a number for whichever of those it turns out
  to be. The 100k hold run is the contrast worth keeping in view: idle
  connections cost almost nothing, so whatever this is appears only once a
  fleet is active.
  It is the upstream pool's lock, and none of the three candidates above.
  20.6% of the proxy's CPU is `Mutex::lock_contended`, of which 12.5 points
  come from `LivePool::acquire` and 5.1 from dropping a `WaitGuard`, which is
  the release path. With `acquire` itself, the `HashMap` lookup it does while
  holding the lock, and the `Notify` on both sides, roughly 45% of the process
  is one lock and the wakeups around it.
  That is exactly the shape `M7.55` measured from outside: five hundred
  connections share a sixty-connection pool, so contention is a function of how
  many are queued rather than of what any one asked for. The frame path does
  not appear at all, which is the other half of `M7.46`'s correction.
  Recorded in `product/perf/run-2026-07-29-pool-lock.md`. What to do about it
  is `M7.57`, deliberately not decided here.
- [x] `M7.57` What to do about the pool lock. `M7.56` names the cost and stops,
  because the three obvious answers have different consequences and one of them
  is that there is nothing to fix.
  Sharding `LivePool` by `PoolKey` removes contention between tenants and
  leaves it within one, which for a fleet where a few tenants are hot is most
  of the win for none of the risk. A lock-free or per-worker free list removes
  it altogether and is a rewrite of the code the quota invariant depends on,
  which `scripts/gates/m3-complete.sh` exists to protect. And at 500 connections
  against 60 upstreams the queue is the design working as intended, so the
  contention may simply be what saturation looks like.
  Acceptance: the third is eliminated first, with a run against a database that
  has headroom, because it decides whether the other two are worth attempting.
  That needs a machine this repository does not have, which is the same
  constraint `M7`'s 100k condition ran into.
  `M7.58` found a fourth answer that needs no such machine, and it partly
  answers the third: some of the contention is self-inflicted rather than a
  picture of saturation.
  Answered, and the answer is that neither of the first two is worth doing.
  `M7.58` removed the herd and with it 94% of the proxy's CPU; `lock_contended`
  and `LivePool::acquire` are no longer in the top sixteen of a profile taken
  under the same load, and the sample count over twenty seconds fell from 4,119
  to 161. Sharding by `PoolKey` would not have helped this workload anyway,
  because a scale run has one pool key and the contention was entirely within
  it. A lock-free free list is a rewrite of the code the quota invariant
  depends on, aimed now at 2.25 seconds of CPU rather than 33.
  What is left of the third answer stands: at five hundred connections against
  sixty upstreams the queue is the design working, and the remaining wait is
  the database's. That part still wants a machine with headroom to measure, and
  it is no longer in the way of anything.

## M8: FIPS and release

The FIPS build stage, the driver cipher-suite matrix, the Helm chart, probe and
`preStop` wiring, and the rolling upgrade rehearsal.

### What is already true, and what that hides

`--features fips` is declared on `pgprox`, `pgprox-tls` and `pgprox-auth`, and
`pgprox_tls::server_config` and `client_config` already call `assert_fips`, so
every configuration the process builds is checked rather than only the two the
roadmap names. `check-crate.sh` runs clippy with `--all-features`, so the
feature does compile in tier 1.

What has never happened is a run. The coverage gate is default-features, so no
test has ever executed with the validated module linked, and `ServerConfig::
fips()` returning true was an expectation rather than an observation. M8.2 is
where that changes.

The same applies to deployment. The drain sequence in `plan.md` names a
`preStop` hook and two probes, `bin/pgprox` serves `/readyz` and `/healthz` and
`POST /v1/drain`, and there is no manifest anywhere that wires the three
together. The rehearsal is what proves the wiring, so it comes last.

- [x] `M8.1` Define M8: this decomposition and `scripts/gates/release-check.sh`.
  The gate reports seven failures against the tree it was written on, one per
  task below it. Its first draft passed a check it should not have: looking for
  `feature = "fips"` anywhere in `pgprox-tls` matched the `cfg!` that defines
  `FIPS_BUILD`, so a crate with no FIPS test at all satisfied it. It looks for
  the attribute form now.
- [x] `M8.2` The FIPS build runs, and it needs clang. A test gated on the
  feature asks a real `ServerConfig` and a real `ClientConfig` what they report,
  and `scripts/fips-check.sh` builds and runs it. Both answer true, so the
  assertion the feature exists for is now an observation.
  The compiler is the finding. AWS-LC's FIPS module is delocated, meaning its
  assembly is rewritten into one contiguous text section whose hash is checked
  at startup, and the rewriter refuses any `.data` section in that module. gcc
  15 emits `.data.rel.ro.local` for the module's relocatable read-only tables
  as soon as optimisation is on. So `cargo build --features fips` passed and
  `cargo test --features fips` did not: `[profile.test]` sets opt-level 1,
  cmake-rs turns that into `RelWithDebInfo`, and the same source stops
  delocating. A release build fails for the same reason. clang 21 builds both,
  which is why AWS-LC documents clang for FIPS, and why the script pins the
  compiler rather than taking whatever `cc` is.
- [x] `M8.3` The FIPS image, and a startup line that tells the two apart.
  `docker build --target fips` produces a 148 MB image whose binary carries the
  delocated module, and running it logs
  `"message":"serving","crypto":"aws-lc-rs-fips"`. The default stage builds
  unchanged and logs `aws-lc-rs`.
  Three things the build taught. The FIPS stage sits *before* the default
  runtime stage, because Docker builds the last stage when no `--target` is
  given and putting it after would quietly turn every compose build into a FIPS
  build. The stage needs `make` on top of cmake, Go and clang: without it cmake
  configures and then fails with `CMAKE_MAKE_PROGRAM is not set`, which reads
  like a cmake problem and is a missing package. And the image ships the proxy
  alone, with no mock sidecar, because a mock credential resolver inside a
  validated image is something an auditor has to be told to ignore.
- [x] `M8.4` The cipher-suite matrix, generated rather than typed. All five
  drivers connect to both builds and all five negotiate TLS 1.3, so nothing
  breaks under FIPS. The report says why that is narrower than it sounds: TLS
  1.3's suites are all FIPS-approved, so the restriction `plan.md` expects,
  ChaCha20-Poly1305 dropped and TLS 1.2 confined to ECDHE, was never reached.
  The drivers that would meet it are older builds than this machine has.
  The suite is read from the proxy's log rather than from the driver. Only two
  of the five will tell you what they negotiated, and a matrix with three
  blanks answers the compatibility question badly; the server knows for all of
  them. That needed one debug line on the TLS accept path, which is a question
  a FIPS deployment has to be able to answer anyway.
  Two defects came out of building it rather than out of review. The FIPS and
  default Docker stages shared one cargo target cache mount, and BuildKit runs
  independent stages in parallel, so the default image shipped the FIPS binary
  and logged `crypto=aws-lc-rs-fips`: not a build failure, not a test failure,
  and completely wrong. The script's own check that the two nodes are the two
  builds is what caught it. The second is `M8.11`.
- [x] `M8.5`, `M8.6` The Helm chart, with the drain wiring in it. Committed
  together because the probes and the `preStop` hook are fields of the very
  workload the chart task creates, and a chart landed without them would be
  wrong in exactly the way this milestone exists to fix. `helm lint` is clean
  and a real API server accepts the rendered manifests: `kubectl apply
  --dry-run=server` against a kind cluster, rather than the client dry run
  first written down here, which cannot reach a schema and so cannot catch a
  misspelt field.
  A StatefulSet rather than a Deployment. Gossip addresses a peer by name and
  expects that name to mean the same node after a restart, so a Deployment's
  fresh random name each time would read to the fleet as a node leaving and a
  stranger arriving, churning the quota that node had reserved. The ordinal is
  also where the node id comes from, and that has to be numeric and stable
  because it is encoded into every connection id.
  One number drives the drain. `drain.graceSeconds` is `drain_grace` in the
  config document, the `preStop` sleep, and the TTL on the drain request, and
  `terminationGracePeriodSeconds` is that number plus headroom, because the
  kubelet starts counting when it starts the hook rather than when the hook
  returns.
  Two things the chart does not pretend to set. `net.core.somaxconn` is on the
  kubelet's safe sysctl list and is set; `nofile` is a container runtime
  setting that no pod spec can request, so `NOTES.txt` says how to check it
  instead of the chart quietly leaving it at 1024.
- [x] `M8.7` The rolling upgrade rehearsal: `scripts/rolling-upgrade.sh` takes
  a fleet through a node-by-node restart under load. Acceptance: zero
  transactions lost, recorded in `product/release/` the way a scale run is, and
  a run where a node is killed rather than drained is shown to lose some, so
  the zero means the drain worked rather than that nothing was happening.
  In a kind cluster rather than in compose, which the criterion originally
  said. The drain is four things acting together and three of them exist only
  in a pod spec: the readiness probe that pulls the node out of the Service,
  the `preStop` hook that starts the drain and waits, and the termination grace
  that gives the hook time. A compose restart exercises the fourth alone and
  would report a green run for a chart that wires none of them.
  The result: 21,042 transactions through a rolling restart of all three
  nodes, none lost, 102 clients relocated. The control, one node's container
  SIGKILLed from the node, lost 22 of 21,088 under the same load. Recorded in
  `product/release/rehearsal-2026-07-28.md`.
  Four things had to be fixed before the numbers meant anything, and every one
  of them was a run finding rather than a review finding. `net.core.somaxconn`
  is not on the kubelet's safe list, so every pod was `SysctlForbidden` and the
  chart's comment claiming otherwise was wrong. The credential sidecar as an
  ordinary container races the proxy that cannot start without it, so it is a
  native sidecar now, an init container with `restartPolicy: Always`. The
  workload replays pgbench's schema, so the database has to be seeded before
  the load starts. And the measurement itself, which is `M8.12`.
- [x] `M8.12` "Zero failed transactions" was a target a working drain could
  never hit. The first rehearsal reported 94 failures for a rolling restart
  against 11 for a hard kill, which is backwards, and the reason was the
  measurement: `first_error` said `terminating connection due to administrator
  command`. That is `57P01`, the drain's own signal, sent to clients that are
  between transactions, and it is the code every mainstream driver reconnects
  from. `bin/pgload` counted each one as a failed transaction.
  `pgload` now separates the two. A `57P01` arriving before any statement in
  the transaction has succeeded is a relocation: it costs a reconnect and no
  work. The same code after a statement has succeeded is the force-close at the
  end of `drain_grace`, and that lost something. The report carries both, and
  the rehearsal asserts on the loss while requiring the relocation count to be
  non-zero, because a drain that moved nobody did not run.
  Connect-time refusals are classified the same way: a client reconnecting
  while Kubernetes is still pulling a draining node out of the Service lands on
  it and is told `57P01` again, and counting those as errors is where
  thirty-five of the first run's failures came from.
  Building the control took three wrong answers, all recorded in the script.
  `kubectl exec -- kill -9 1` does nothing: the kernel discards a signal sent
  to PID 1 from inside its own namespace unless the process has a handler, and
  SIGKILL cannot have one, so the restart count stayed at zero while the run
  reported a disruption that never happened. `--grace-period=0 --force` leaves
  the kubelet to terminate the container its own way and some clients still
  left politely. `--grace-period=1` cuts the `preStop` hook short but still
  sends SIGTERM, and the proxy's own shutdown path closes its clients with
  `57P01` before exiting: fourteen relocated, nothing lost. That last one says
  something good about the proxy and nothing about the drain. The control that
  works is `crictl stop --timeout 0` from the node, outside the pod's PID
  namespace.
- [x] `M8.8` MSRV verified rather than declared. A CI job installs whatever
  `scripts/msrv.sh` prints and runs `cargo check --workspace --all-targets` on
  it. Run here first: the whole workspace builds on 1.94.1, so the pin was
  right, which is the answer to have before writing a gate rather than after.
  The version is read from `Cargo.toml` rather than written in the workflow,
  and `release-check.sh` checks that CI *derives* it rather than that the
  number appears there. A literal in two places drifts, and the copy that
  drifts is always the one nobody runs.
  Taken out of order, while the M8.4 stack was building. Nothing depends on
  either.
- [x] `M8.11` `Flush` deadlocked the relay, so asyncpg could not run a single
  extended query through the proxy. Found by M8.4 pointing a driver at the
  proxy that the conformance suite has only ever pointed at the harness, which
  is `M1F.24`'s whole argument.
  `awaits_more` already treats `Flush` as the end of a client sequence, and the
  response pump then reads until `ReadyForQuery`, which a `Flush` never
  produces: the server has answered and both sides wait. asyncpg prepares with
  `Parse`, `Describe`, `Flush` rather than with a `Sync`, so every
  `fetch`/`fetchval`/`execute` with parameters hangs until the client's own
  timeout. The simple protocol is unaffected, which is why nothing caught it:
  the e2e run drives psql and pgbench.
  `pgprox_session::flush::Outstanding` counts what the server owes: every
  extended-query frame the proxy forwards makes it owe exactly one completion,
  and the tags that discharge each kind are fixed. When nothing is outstanding,
  a `Flush` has been answered and the relay goes back to the client. Counting
  rather than a timeout, because a timeout is either too short for a slow
  statement or long enough to be its own hang.
  Verified against the real driver, not only in tests: asyncpg now does
  parameters, prepared-statement reuse, 2,000 rows, a transaction, a
  no-rows result, an `UndefinedColumnError` and a statement after it. The two
  regression tests in `serve.rs` were confirmed to fail without the fix, which
  for a deadlock means a timeout that is the assertion rather than a hung
  suite.
  The relay loop went over clippy's hundred-line limit on the way, so sending
  one frame upstream is now `send_upstream` and the pump's two counters are one
  `Pumping`. Both were arguments the loop was carrying rather than logic it was
  doing.
- [x] `M8.9` The tier 3 workflow. A nightly job, and a `workflow_dispatch` so
  it can be run on demand, running `scripts/fips-check.sh` on a runner with
  cmake, clang, Go and make. `release-check.sh` checks the schedule exists, not
  just the script: a script only a person can remember to run is a script that
  gets remembered on release day.
  The cipher matrix is not in it. It needs a compose stack and five language
  toolchains, and a nightly job that flakes on a `dotnet restore` teaches
  people to ignore the nightly. It stays a pre-release step that a human runs,
  which is what `plan.md` calls it.
- [x] `M8.10` Close M8. `scripts/gates/release-check.sh` exits zero, and so do the
  three runs behind it: `fips-check.sh`, `cipher-matrix.sh` and
  `rolling-upgrade.sh`. The roadmap says what each showed and what it did not.
  One process slip worth recording: `M8.9`'s workflow changes were swept into
  `M8.7`'s commit by a `git add -A`, so its subject does not name them. The
  work is right and the history is one commit coarser than it should be.

### The five tasks carried into this milestone

M7.46 and the four M1F deferrals are open and have been since their milestones
closed. They are worked after M8 closes rather than before it, because M8's
condition does not depend on any of them.

The order they were listed in is no longer the right one. M8 changed the case
for `M1F.24`: pointing asyncpg at the proxy rather than at the conformance
harness found `Flush`, which is the most serious defect this project has
shipped past a milestone gate, and every other driver has only ever met the
harness too. That moves it to the front, and `tests/cipher/` is most of the
harness it needs.

The order is now `M1F.24`, `M7.46`, `M1F.21`, `M1F.25`, `M1F.22`.

## Found after M8 closed

- [x] `M8.13` The driver matrix meets the proxy, not just the harness. This is
  `M1F.24` with a reason rather than a plan: five drivers have been run against
  `conformance_server` since M1 and against the real proxy never, and the one
  time one of them was pointed at the proxy it deadlocked on its first
  parameterised query. A shared misunderstanding between our codec and our
  harness is invisible by construction, and `Flush` is the proof.
  `tests/cipher/` already connects all five to a running proxy over TLS. What
  it does not do is exercise anything: it runs `SELECT 1` and stops, because
  its question was which cipher they negotiate.
  Acceptance: each driver runs the depth cases the conformance suite already
  names, `PGPROX_DEPTH_PREPARED_REUSE` and `PGPROX_DEPTH_LARGE_RESULT` among
  them, against `bin/pgprox` with a real Postgres behind it, and a driver whose
  toolchain is missing is reported as skipped rather than dropped. Failures are
  recorded rather than fixed in the same task: what this finds is its own
  backlog.
  All five pass. What it found was in the probes rather than in the proxy, and
  it is the same lesson: `psql -c 'SELECT $1'` is a case the conformance suite
  has run since M1 and it only works because the harness answers it with a
  canned row. Real Postgres says `there is no parameter $1`, correctly. The
  probe binds a value now, through `\bind` on stdin, because psql reads a `-c`
  string as SQL unless it starts with a backslash.
  This closes `M1F.24`.

## M9: query cache (post-MVP)

`pgprox-cache` behind the trait `pgprox-core` has carried since M0.

### Why this is worth doing now, which is not the reason it was written

The plan filed this as post-MVP throughput work: read traffic dominates, and a
cached read costs nothing. That is still true and is no longer the interesting
part.

`M7.56` measured where the proxy's CPU goes and found 45% of it in one mutex:
`LivePool::acquire`, its release path, and the `Notify` around them. The cost
lands per connection because contention is a function of how many are queued.
A cache hit is a statement that never acquires an upstream connection at all,
so it does not queue and does not contend. That makes this milestone the first
thing to try against the constraint `M7.57` is about, and cheaper than either
of the answers that task lists.

### What a cache may promise, and what it may not

The hazard is the one ADR 0009 is about, arriving from a different direction. A
replica can be behind; a cache entry can be wrong. The difference is that a
replica's staleness is measurable, and `pg_last_wal_replay_lsn()` says exactly
how far behind it is, while a cache entry carries no version of the data it
copied and no way to learn one.

What this proxy can see is its own traffic. A write through this node can
invalidate. A write through another node in the fleet needs gossip. A write
from outside the proxy, a migration, a batch job, an operator with psql, is
invisible and always will be.

So the honest ceiling is bounded staleness, the mode ADR 0009 already offers
tenants who prefer throughput to read-your-writes, and the TTL is the bound.
Everything else the cache does about invalidation is an improvement on that
bound rather than a promise. `M9.2` writes that down before any code assumes
otherwise, because a cache whose guarantees were never stated is a cache
somebody will rely on for read-your-writes.

### Order

The decision first, the store second, and the thing that decides *what* may be
cached before the thing that serves it. A cache that is fast and occasionally
wrong is worse than no cache, and the way this goes wrong is that the hook into
the relay lands before the cacheability rule is finished.

- [x] `M9.1` Define M9: this decomposition and `scripts/gates/m9-complete.sh`. The
  roadmap's condition is `cargo nextest run -p pgprox-cache`, which only says
  the crate's own tests pass. Acceptance: the gate runs against the current
  tree and names what is missing rather than passing vacuously, and the roadmap
  points at it.
  Ten failures against the tree it was written on, one per task below it. Its
  first draft passed a check it should not have: `grep cache` in
  `pgprox-config` matched the comment on `grant_ttl_cap`, which is about the
  grant cache. It looks for `query_cache` now. That is the second time in two
  milestones a gate has been written with a substring loose enough to pass on
  unrelated prose, so it is worth saying out loud: a gate whose check is a word
  rather than a name is green for years while the thing it names does not
  exist.
- [x] `M9.2` ADR: what the query cache may promise. The default (off), the
  scope (one node, not the fleet), the bound (TTL), and what invalidation is
  and is not. Acceptance: an ADR in `product/decisions/` that states the
  staleness contract in the same terms ADR 0009 states its own, and says
  plainly which writes the cache cannot see.
  ADR 0021. The load-bearing sentence is why a cache cannot do what ADR 0009
  does: a replica's staleness is measurable, and `pg_last_wal_replay_lsn()`
  answers it on demand from the replica itself, while a cache entry is a copy
  of bytes carrying no version of the rows behind them. There is no
  `pg_last_wal_replay_lsn()` for a `SELECT` result.
  Four rules come out of it. Off by default. One node rather than the fleet,
  because a partitioned node would keep serving entries whose invalidation it
  never received and the TTL bounds staleness under partition anyway.
  Invalidation on write is an improvement on the bound and may not be called
  read-your-writes anywhere a human reads. And where the cache and read routing
  disagree, routing wins: a session that has written is not served from the
  cache until its transaction ends.
  The crate's `AGENTS.md` and the trait's own documentation say the same thing
  now, because the place this goes wrong is a later reader finding the trait
  before the ADR.
- [x] `M9.3` The crate and the store. `pgprox-cache` implementing `QueryCache`
  with a TTL per entry, a bound on total entries, and per-tenant invalidation.
  Acceptance: it satisfies the same test suite `FakeQueryCache` does, plus its
  own for expiry and eviction; an entry past its TTL is never returned even if
  it is still resident; and the bound is on bytes rather than entries, because
  a cache bounded by count holds an unbounded amount of memory.
  Eighteen tests, 99.65% covered. LRU eviction through a `BTreeMap` keyed by a
  monotonic sequence rather than a scan for the minimum, so the cost of a `put`
  does not depend on how full the cache is. A result larger than the whole
  budget is refused rather than stored, because storing it would evict
  everything and then be evicted by the next insert. A TTL that would overflow
  the clock is refused for the reason the TTL exists: ADR 0021 does not allow
  an entry that never expires.
  The one lock is deliberate and commented as such. `M7.56` found 45% of the
  proxy's CPU in a mutex, so a new one owes an explanation: the pool's is
  contended because callers park on a `Notify` while holding it, and nothing
  here waits. That is a different regime rather than an exemption, and sharding
  by the hash of the key is why no caller knows whether there is one lock or
  sixteen.
  The LRU test failed first time for a reason worth keeping: three 900-byte
  results did not fit a 3,000-byte budget, because `weigh` adds about 115 bytes
  of struct and key to each. It asserts its own setup now, so a future change
  to the accounting fails at the assumption rather than at the conclusion.
- [x] `M9.4` Normalisation, which is the correctness-critical half of the key.
  Two statements differing only in whitespace, comment or letter case key the
  same; two differing in anything the server would treat differently do not.
  Acceptance: a property test over the shape `pgprox-core::sql` already lexes,
  with the property stated as "normalising never merges two statements a
  server would answer differently". Literals are *not* normalised into
  parameters in this task; that is a separate decision and a separate risk.
  The rule is Postgres's own: outside quotes SQL is case-insensitive and
  whitespace separates, inside them neither is true. So a word is lowercased,
  quoted text is copied byte for byte, and a run of whitespace or comments
  becomes one space.
  One space only where the source had trivia, not between every token. Always
  spacing would give `1.5` and `1 . 5` the same key, and although one of those
  is a syntax error, an error is an answer the server gives differently.
  The property is stated against a model rather than an example: the token
  sequence, folded the way the server folds it, is unchanged by normalising.
  Anything normalisation did that a server would notice fails that test.
  The scanner is `pgprox_core::sql`, not a new one. That module exists because
  `pgprox-pool` and `pgprox-route` grew separate scanners that disagreed about
  where an `E'...'` string ends and a session went unpinned. Using it took some
  care, because `Token::Quoted` deliberately does not carry its contents:
  handing them out invites a caller to search them, which is how a tenant's own
  data starts changing how their queries are treated. This module needs the
  bytes only to copy them, so it measures how far the lexer moved rather than
  asking the token what it held, and no contract changed.
  One imprecision is named rather than left to be discovered: `E'x'` and
  `e'x'` key separately, because the introducer is part of the quoted span.
  Separating it would mean parsing inside a region the lexer already decided
  was quoted, which is the second-scanner mistake again. The cost is an extra
  entry for a spelling almost nobody uses, and the direction is the safe one.
- [x] `M9.5` Cacheability: which statements may be cached at all. Read-only by
  the existing classifier, not inside a transaction that has written, not on a
  pinned session, and nothing whose result depends on anything but the
  arguments. Acceptance: a tested function taking the class, the session state
  and the SQL, refusing by default; `pgprox-cache` cannot depend on
  `pgprox-route`, so the class arrives as an argument the way the pin
  allowlist does.
  The task turned out to be a different question from the one the classifier
  answers, and the classifier says so itself: its list covers functions that
  *write*, and its own comment points out that `random()` is volatile and
  perfectly safe to route. Replica-safety asks whether a statement writes.
  Cacheability asks whether the answer is a function of the key. `SELECT
  random()` is read-only, replica-eligible, and turns into a constant for the
  length of the TTL if cached.
  So there are two lists and they are deliberately disjoint. `nextval` writes
  and the class already refuses it, so it is absent here; a test asserts that,
  because two lists with overlapping entries drift and the one that drifts is
  the one nobody remembers to update.
  Refuses by default, and the checks are ordered cheapest first: a session that
  wrote is refused on the fact rather than on a scan of its SQL. Multiple
  statements in one simple query are refused rather than handled, since the
  response is several result sets whose boundaries this crate does not track.
  The honest limit is the same one ADR 0009 records: a denylist of built-in
  names cannot catch a tenant's own `VOLATILE` function. What makes that
  acceptable is not the list being complete, because it is not, but that the
  cache is off until a tenant turns it on and turning it on is a statement
  about their own workload.
- [x] `M9.6` Invalidation on write. A write by a tenant drops that tenant's
  entries on this node. Acceptance: a write through the relay invalidates, the
  test covers a write in a transaction that later rolls back, and the ADR's
  wording that this is best-effort rather than a guarantee is what the code
  comments say too.
  Conservative in three directions, each costing a miss rather than a wrong
  answer. Anything the classifier does not call read-only invalidates,
  including `Unknown`, because that class exists so a construct nobody has
  taught it yet is treated as a write. A `Parse` invalidates without waiting to
  see the statement executed. And a rolled-back transaction invalidates anyway:
  waiting for the commit would buy a better hit rate and would mean detecting a
  commit correctly on every path, and getting *that* wrong means failing to
  invalidate, which is the unsafe direction.
  It happens before the statement is sent rather than after its answer returns,
  or a reader arriving in between would be served an entry the write was about
  to make wrong.
  The whole path is behind `Option<Arc<dyn QueryCache>>`, `None` on every node
  until `M9.8` gives a document a way to say otherwise. A node with no cache
  does not classify either, so the feature is free for anyone who never asked
  for it, and a test asserts the default is off.
  Four tests, and the two that matter were confirmed to fail with the hook
  disabled. The read is what keeps the others honest: if every statement
  invalidated, the write test would pass for a cache that was never used.
- [x] `M9.7` The relay hook. On a hit, the client is answered from the cache
  and no upstream connection is acquired. Acceptance: a test that a hit serves
  the right bytes and the pool records no acquisition, which is the property
  the whole milestone is for.
  It serves the same bytes the client saw the first time, and the fake server
  never hears the second request. A differently spelled statement hits the same
  entry, which is what normalisation bought, and `SELECT random()` is never
  stored, which is what the cacheability rule bought.
  Simple protocol only, and that is a decision rather than an omission. The
  extended protocol's parameter values live in a `Bind`, and `pgprox-proto`
  exposes that message's names and not its parameters. Until it does,
  `CacheKey::params` would be empty for two calls differing only in what was
  bound, so `SELECT $1` with 1 and with 2 would share an entry. A bound
  statement is a miss, which is the difference between a smaller cache and a
  broken one. `M9.12` is the follow-up.
  The session future is what this cost, and it is worth recording because the
  ceiling is now the binding constraint on the feature. Inline, the cache path
  was 152 bytes and put the future over 5 KiB. Folded into one boxed
  `Recording` and with the result dropped before the flush that follows it, the
  future is 5,088 bytes against a ceiling of 5,120. Thirty-two bytes of room
  left, so `M9.9` and anything after it has to find its own.
  Two things were tried and did not work, both worth writing down. Boxing the
  pre-send path to keep it out of the frame needs its borrows taken lazily,
  which Rust will not do: the future is built either way, so the box bought an
  allocation and no bytes. And the relay went over clippy's hundred-line limit
  three times on the way, which is what pushed the pump's tail into
  `read_the_answer` and the pre-send half into `cache_before_sending`.
- [x] `M9.8` Configuration, the half an operator writes: a `query_cache`
  section, and the `pgprox-core` type it resolves to. Split from the wiring in
  `M9.13` because it is a contract change, and the skill's rule that a contract
  change is one commit containing the type, every fake and every call site is
  much easier to hold to when the commit is not also introducing a mechanism.
  The shape is ADR 0021's own: a node-wide byte budget, an operator-controlled
  `ttl_cap`, and a map of tenants that have opted in, each with the staleness it
  accepts. The effective TTL is the smaller of the tenant's and the cap, which
  is exactly how `grant_ttl_cap` bounds a sidecar's TTL. An empty map is off,
  and off is the default: there is one way for the cache to be off rather than
  two, because a separate `enabled` flag disagreeing with an empty tenant list
  is a bug with no right answer.
  A size takes a unit for the reason a duration does. `max_bytes: 64` meaning
  bytes when the operator meant megabytes is `drain_grace: 500` again, so the
  unit is required and both `MB` and `MiB` are accepted with their real
  meanings rather than one of them being refused during a deploy.
  Acceptance: a document with no `query_cache` section resolves to a
  configuration that serves no tenant, a tenant asking for a day gets the cap,
  and every bad spelling names its field.
- [x] `M9.9` Observability, the number and where it comes from. `CacheView` in
  `pgprox-core::admin`, an `Observatory::cache` with a default so it is
  additive, the fake, the node's implementation reading the store, and the
  `pgprox_cache_*` metrics rendered from it.
  One view behind all three surfaces, which is the property `pgprox-admin`
  already protects between `SHOW` and HTTP: two readings of the same question
  that can differ are two numbers an operator has to reconcile during an
  incident.
  The acceptance's harder half is that a cache doing nothing is
  distinguishable from one that is off, and counters cannot say it: both are
  zeroes. What says it is how many tenants the store is configured for, so
  that is in the view and exported as a gauge.
  Split from the surfaces in `M9.14`, because this half is a `pgprox-core`
  contract change and that one is two parsers.
  Acceptance: `/metrics` distinguishes a node with no `query_cache` section
  from one whose tenants are simply idle, and every counter the store keeps has
  somewhere to appear.
  Done. `pgprox_cache_tenants` is the metric that carries the distinction and
  it is emitted on every node including the ones where the cache is off, since
  an absent series and a zero one are different facts to an alert and only the
  second is what "off" means.
  Two things this pulled in. `NodeObservatory::new` went past clippy's
  seven-argument limit, so it takes a `NodeParts` now, which is the same fix
  the exporter's `Sources` already is and for the reason written on it. And the
  view's tenant count is read from the live document rather than from the
  store, so the number an operator sees and the number the store is holding to
  cannot be two things.
- [x] `M9.14` Observability, the two surfaces: `SHOW CACHE` and
  `GET /v1/cache`, both reading `M9.9`'s view. `SHOW CACHE` is `pgprox` only,
  since `PgBouncer` has no such command, so its columns are this repo's to
  choose; the HTTP one needs its OpenAPI entry, which `check-drift.sh` gates.
  ADR 0021 says the output has to say "bounded staleness" rather than anything
  warmer, and this is the surface it meant.
  Acceptance: both answer, both agree, and the drift check passes.
  Done. `promise` is a column and a field rather than a comment, because ADR
  0021 asks for the words on every surface an operator reads and a cache that
  described itself as nothing at all is one somebody fills in for themselves.
  `SHOW LOCAL CACHE` is the same answer as `SHOW CACHE` rather than an error,
  which is correct here and now pinned by a test rather than being a property
  nobody meant.
- [x] `M9.10` Does it help. Measured against the reference workload with
  `scripts/scale.sh`, and against `M7.56`'s finding specifically: a hit avoids
  an acquire, so the question is whether contention falls. Acceptance: a
  recorded run in `product/perf/`, and the number is reported whichever way it
  comes out.
  It helps by about 7%, on both median latency and CPU per statement, over five
  matched pairs whose two sets do not overlap. It does not change the shape:
  the p99 did not move and the pool's share of the profile is flat at ~49%,
  because contention tracks how many callers are queued and 89% of statements
  still queue. `M7.57` is still the task that matters for 100k active
  connections, and this is more evidence for it rather than less.
  The 11% share is two ceilings in the workload rather than in the cache. Half
  of every run is the extended protocol, which is all miss until `M9.12`, and
  30% of statements are writes, which empties the tenant's cache roughly every
  other lookup. See `product/perf/run-2026-07-29-cache.md`.
- [x] `M9.16` A cache hit is a statement that went somewhere, and nothing
  counts it. `record_statement` runs after the relay's `decide`, and a hit
  returns before that, so `pgprox_route_total` misses every one.
  Found while taking `M9.10`'s measurement, where it made the arithmetic wrong
  in the direction that flatters: the cache-on runs reported 8.7% fewer
  statements and slightly worse CPU per statement, when what had actually
  happened was that eight thousand statements per run were served and not
  counted. A denominator missing its best cases is worse than no denominator.
  `route="cache"` as a third value rather than a new metric, because the
  question `pgprox_route_total` answers is "where did the statements go" and
  the cache is now one of the places. A third counter on `RouteCounts` rather
  than a third `RouteTarget`, because the target is what the router chose and a
  hit never reached the router: adding a variant there would be a `pgprox-core`
  contract change to describe something the routing layer does not do.
  Acceptance: a scale run with the cache on reports as many statements as one
  with it off, plus or minus the noise, and the share the cache served is
  readable from `/metrics`.
  Done. `PGPROX_MOCK_TENANT` came with it: the mock sidecar derives a tenant
  from the token's first eight bytes, which is right for auth tests and useless
  for a config document that has to name one, and the first cache-on run
  measured nothing for exactly that reason.
- [x] `M9.11` Close M9, which needs `M9.13` as well as everything before it.
  Acceptance: `scripts/gates/m9-complete.sh` exits zero.
- [x] `M7.58` One connection freed wakes every waiter. `LivePool::release`
  calls `Notify::notify_waiters`, which wakes *all* of them; at five hundred
  clients against sixty upstream connections that is roughly four hundred and
  forty tasks woken to hand out one connection. Each takes the pool mutex, asks
  `Pool::acquire`, is told to wait, builds and drops a `WaitGuard` for two more
  acquisitions of the same mutex, and parks again.
  That is a thundering herd, and it is what `M9.10`'s profile is a picture of:
  `lock_contended` at 18.7%, dropping a `WaitGuard` at 4.5%, `poll_notified` at
  4.1%, `notify_waiters` at 2.2%, and the pool's `HashMap` entry lookup at 4.8%
  with a `PoolKey` clone and a sip hash on every one of those four hundred and
  forty passes.
  `notify_one` is the right primitive for the two paths that free exactly one
  thing: `release`, and `SlotGuard::drop` giving a reserved slot back.
  `set_limit` keeps `notify_waiters`, because raising a cap can admit many at
  once and there is no count to notify.
  The correctness question to answer before touching it is whether a
  notification can be lost. `acquire_inner` registers interest before it checks,
  which closes the race the comment on that line names, and `tokio::Notify`
  passes a notification on when a notified waiter is dropped before polling.
  What changes is fairness: today the whole herd re-races, so a waiter that
  keeps losing loses to the crowd; with `notify_one` it can lose to a barging
  newcomer instead. The deadline bounds that either way and `give_up` reports
  it, but a test has to say so rather than a paragraph.
  Acceptance: a test shows one release wakes one waiter, a test shows no waiter
  is stranded when the woken one loses the connection to a newcomer, and a
  matched pair of scale runs says what it was worth.
  It was worth 15.7x of CPU per statement, 687us to 43.7us, and half the p99.
  `lock_contended` and `LivePool::acquire` are gone from the top of the profile
  entirely, and the sample count over twenty seconds under the same load fell
  from 4,119 to 161. The median went up 12%, which is FIFO service replacing a
  race, and is the same fact as the tail collapsing. See
  `product/perf/run-2026-07-29-thundering-herd.md`.
  The test almost did not work. The queue length after a release is identical
  either way, so the first version asserted on that and passed against the herd.
  `LivePool::futile_wakeups` is what made the property visible, and it reads 7
  against 0 for one release with eight waiters.
- [x] `M9.12` The extended protocol, which `M9.7` left out on purpose. A bound
  statement is a miss today, because `CacheKey::params` would be empty for two
  calls differing only in what was bound and `SELECT $1` with 1 and with 2
  would share an entry.
  What it needs is for `pgprox-proto` to read a `Bind`'s parameter values,
  which it has never had a reason to do. That is a change to the codec, the
  most exposed parser in the process, so it is its own task rather than a
  detail of the hook: a `Bind` carries a count and then that many length-
  prefixed values, and a length the decoder trusts is how a malformed message
  becomes an allocation.
  Acceptance: the codec can read them, the fuzz corpus gains a `Bind` with
  parameters, and the target exercises the new reader. `M9.17` is the half that
  reaches `CacheKey::params`. Worth doing only if `M9.10` says the cache helps:
  most drivers use the extended protocol, so a cache that cannot serve it is a
  cache most traffic misses. It says 7%, with the ceiling roughly doubling.
  Done, and split, because the two halves are different work. The codec half is
  `frontend::bind_parameters`, read on demand rather than as a field on the
  `Bind` variant: every extended-protocol statement sends one, so a `Vec` in
  the variant is an allocation on a path that currently makes none, on every
  node, for a feature that is off by default. The one caller that wants the
  values has already decided to build a cache key.
- [x] `M9.17` The extended protocol reaches the cache key. `M9.12` gave the
  codec a way to read a `Bind`'s parameters; this is what carries them to
  `CacheKey::params`.
  The work is state, not parsing. A cache key needs the SQL and the parameter
  values together, and the relay sees them in three separate messages: `Parse`
  carries the SQL, `Bind` carries the values and names a portal, and `Execute`
  names the portal and is the point the statement runs. Something has to hold
  the portal's SQL and values between the `Bind` and the `Execute`.
  The constraint that decides the shape is the session future, which is 5,064
  bytes against a 5,120 ceiling. Fifty-six bytes will not hold a map of
  portals, so this is a boxed allocation on the sessions that use it and
  nothing on the ones that do not, the way `M9.7`'s `Recording` is.
  Two things found while sizing it, both of which make it larger than "read the
  parameters and put them in the key".
  The response shape is not the same. A simple `Query` is answered with rows, a
  `CommandComplete` and a `ReadyForQuery`, and `M9.7` caches all of it. An
  `Execute` is answered with rows and a `CommandComplete` and no
  `ReadyForQuery`: that arrives in answer to the `Sync` that follows. So a
  served `Execute` leaves a `Sync` the client still expects an answer to, and
  the session may hold no upstream connection to send it to. Either the `Sync`
  is answered locally, which means the relay is now deciding transaction
  status for itself, or the hit has to be given up when one follows. That is a
  decision to make before any code, and it is the same class of thing as the
  `Flush` deadlock: a message with no terminator whose absence nobody noticed.
  Portals also have to be forgotten. An unnamed portal is destroyed by `Sync`
  and a named one by `Close`, and a map that only grows is a leak for the
  length of a session, which for this proxy is measured in days.
  Acceptance: two bindings of `SELECT $1` key separately and are served
  separately, a `Sync` after a served `Execute` is answered correctly, a portal
  is forgotten when the protocol says it is, a session that never binds
  allocates nothing new, and the session future stays under 5 KiB.
  **Scope narrowed to the decision, which is what the entry itself said had to
  come first.** ADR 0022 records it and `M9.18` through `M9.23` are the work.
  Two of the three problems named above turned out to be the smaller two.
  A hit at the `Execute` has already taken a connection, because a `Parse` or a
  `Bind` is forwarded as it arrives and forwarding acquires. So the obvious
  shape saves the database's execution and none of the pool work that `M7.56`
  measured, which is the reason this milestone was moved up at all. And the
  answer to a sequence is not a function of the SQL and the parameters: a
  `RowDescription` appears only if the client asked for one, so bytes recorded
  from one driver's framing desynchronise another's.
  What is decided: an opted-in tenant's sequence is withheld from the upstream
  until the client ends it, what is cached is the statement's answer with
  nothing of the sequence in it, and a hit is assembled from the frames the
  client actually sent. Withholding starts only from an idle session, which is
  what makes a locally generated `ReadyForQuery('I')` true rather than hopeful.
  Anything not covered replays the withheld frames upstream and carries on.
  The acceptance criteria above are superseded in one place: a map of portals
  is not needed, because a withheld sequence ends at the `Sync` that closes it
  and there is only ever one. Forgetting is what the machine does at the end of
  every sequence rather than a rule about named portals.
- [x] `M9.18` A cache hit inside a transaction reports the wrong transaction
  status. Found while designing `M9.17` rather than by running anything, which
  is why it is filed rather than folded in.
  `cache_key` asks the cacheability rule about the session, and `SessionFacts`
  carries whether it wrote and whether it is pinned. It does not carry whether
  a transaction is open. So `BEGIN; SELECT 1;` on an opted-in tenant can be
  served an entry another session stored while idle, and that entry ends in a
  `ReadyForQuery('I')`. The client is told its transaction ended while the proxy
  goes on holding a connection with an open one on it.
  Storing has the same hole from the other side: a read inside a transaction can
  see rows only that transaction can see, and its answer ends in `'T'` rather
  than `'I'`, so it is neither safe to publish nor the right shape to store.
  `SessionFacts` gains `in_transaction` and `NotCacheable` gains the variant to
  refuse it with. The constructor is `#[non_exhaustive]` on purpose so a field
  added here is a compile error at every call site, which is the whole reason it
  is a constructor.
  Acceptance: a `SELECT` inside a transaction is neither served from the cache
  nor stored in it, with a test that fails before the fix. `M9.23` depends on
  this: withholding a sequence is only sound from a session with no transaction
  open, and the invariant has to exist before anything rests on it.
- [x] `M9.19` The key can carry what a `Bind` carries. `CacheKey::params` is
  `Vec<Vec<u8>>` and has been empty since M9.7, and it cannot hold what the
  extended protocol needs: a SQL `NULL` is not a zero-length value, and
  `Vec<Vec<u8>>` cannot tell them apart. Two bindings of `SELECT $1`, one with
  `NULL` and one with the empty string, would share an entry.
  It becomes `Arc<[u8]>` holding the parameter section of the `Bind` exactly as
  it arrived, length-prefixed, with `-1` meaning null the way the wire already
  says it. One allocation per key rather than one per parameter, cheap to hash,
  and it distinguishes null by construction rather than by a rule somebody has
  to remember. `pgprox-proto` grows the accessor that hands out that slice,
  since `bind_parameters` already walks it and proves it well formed.
  A `pgprox-core` DTO change, so the trait, the fake, `pgprox-cache`, its
  accounting of key bytes and every construction site move in one commit.
  Acceptance: a key built from a `Bind` with a null and one built from a `Bind`
  with an empty value are not equal, the store's byte accounting counts the new
  shape, and nothing in the workspace still names the old one.
- [x] `M9.20` The withheld sequence, as a state machine with no I/O in it. A new
  module in `pgprox-session`, which is where a per-session protocol machine
  belongs and which already depends on everything it needs.
  It is fed the frames the relay decodes and answers what to do with each:
  withhold it, replay everything withheld and carry on, or that the sequence is
  now complete and here is the key material. It holds the frames as they will go
  upstream, which is after the statement-name rewrite, because a replay has to
  send what would have been sent.
  The cacheability verdict arrives as an argument rather than being reached for,
  the way `SessionFacts` does: this crate may not depend on `pgprox-cache`.
  What it withholds is a `Parse`, a `Bind`, a `Describe` of a portal and an
  `Execute` with no row limit. What replays is everything else, named the safe
  way round: the machine lists what it can hold and anything unlisted is a
  replay, so a message nobody has thought about does not get swallowed.
  Acceptance: unit tests for each frame it withholds and each of `Flush`, a row
  limit, a statement `Describe`, a second `Execute`, a simple `Query` and a
  `Terminate` mid-sequence; the SQL comes from the `Parse` in the sequence or
  from what the session prepared earlier; the parameters come from the `Bind`;
  and the machine is empty again after the `Sync`. Nothing uses it yet.
- [x] `M9.21` A hit, assembled from the frames the client sent. The other half
  of `M9.20` and the same module: given a withheld sequence and a stored
  payload, produce the bytes the client is owed.
  A `ParseComplete` for the `Parse`, a `BindComplete` for the `Bind`, the
  payload's `RowDescription` for the `Describe`, the rows and the
  `CommandComplete` for the `Execute`, a `ReadyForQuery('I')` for the `Sync`.
  The payload is split rather than replayed whole, because a client that sent no
  `Describe` must not be handed a `RowDescription` it never asked for.
  Acceptance: byte-for-byte assertions on the assembled output for a sequence
  with a `Describe` and one without, a payload whose `RowDescription` is absent
  is refused rather than assembled around, and a malformed payload cannot make
  the assembler read past its end.
- [x] `M9.22` One payload shape for both protocols. `M9.7` stores the whole
  answer to a simple `Query`, `ReadyForQuery` and all, and ADR 0022 makes the
  stored payload the statement's answer instead: the `RowDescription`, the rows,
  the `CommandComplete`. So the recording filters what it keeps, and the simple
  protocol's hit path grows the `ReadyForQuery('I')` it now has to generate.
  This is what lets one entry serve both protocols, which matters because the
  reference workload asks the same questions both ways.
  Depends on `M9.18`: generating that `ReadyForQuery` is only true for a session
  with no transaction open.
  Acceptance: a simple query is still served correctly from its own entry after
  the change, the stored bytes no longer contain a `ReadyForQuery`, and a hit
  inside a transaction is still refused rather than answered with `'I'`.
- [x] `M9.23` The relay withholds, serves and records. The wiring, and the last
  of the code: the machine from `M9.20` behind an `Option<Box<..>>` in the
  session, the assembler from `M9.21` on the hit path, the recording from
  `M9.22` on the miss path, and the hit counted the way `M9.16` counts one.
  Acceptance: two bindings of `SELECT $1` are served separately and neither is
  served the other's answer, a hit takes nothing from the pool and the session
  ends holding no connection, a `Flush` mid-sequence is answered exactly as it
  is today, a session that never binds allocates nothing new, and the session
  future stays under 5 KiB.
- [x] `M9.25` A replayed sequence left the connection's statement record wrong.
  The first thing `M9.24` ran found it: 1,083 errors in a thirty-second run,
  `prepared statement "pgprox_..." already exists` and then `does not exist` as
  the two sides diverged further. Nothing in the test suite had caught it.
  `M9.20` stored the held frames after the statement-name rewrite, on the
  reasoning that a replay has to send what would have been sent. The rewrite is
  one way. A `Parse` stored under this proxy's own global name decodes to a
  statement name that no session's map contains, so at replay `statement_of`
  found nothing, and the connection's record of which statements it holds was
  never updated. The server held the statement and the proxy believed it did
  not, so the next `Bind` on that connection prepared it again.
  The fix is to hold the client's own bytes and map the name at replay, which is
  what the relay loop does for a frame arriving now.
  What let it through is more interesting than the bug. The fake upstream
  answered every `Parse` with a `ParseComplete`, including a second one for a
  name it had already prepared, so no test in the file could tell a correct
  statement record from a wrong one. Postgres refuses that with `42P05`, and the
  fake now does too. That is the crate rule about fakes behaving like the real
  thing, and this is what it is for.
  Acceptance: a sequence replayed onto a connection that already holds its
  statement is not prepared twice, with a test that fails before the fix.
- [x] `M9.26` The other half of `M9.25`, which never landed, and the two harness
  gaps that hid it.
  `M9.25` was two changes: hold the client's own bytes, and map the statement
  name at replay. Only the second was applied. The tree was green and the run
  went from 1,083 errors to 207, which read as progress and was not: the same
  divergence, surfacing as a refusal instead. `map_statement_name` at replay was
  handed an already-rewritten body, so for a sequence carrying a `Parse` it
  inserted an alias keyed by the global name and papered over itself, and for a
  sequence with no `Parse` in it there was nothing to alias and the frame was
  refused. Every driver sends `Bind`, `Execute`, `Sync` once it believes a
  statement is prepared, so that is most extended-protocol traffic.
  Two things in the harness had to change before a test could see it. The fake
  extended server answered the position query the proxy asks the primary for
  after a write with a bare completion, so `relay.wrote()` never cleared and
  nothing after a write in that fake's world was cacheable: the test reached
  neither the hit nor the replay. And the ordering matters, because a sequence
  that replays inserts the alias that hides the bug, so the test has to fill the
  entry through the simple protocol, take a hit on the extended one, and only
  then force a miss. That the two protocols can share an entry is `M9.22`.
  Acceptance: a `Bind`, `Execute`, `Sync` with no `Parse` in it is replayed
  rather than refused, with a test that fails on the half-applied fix.
- [x] `M9.27` A simple query could be served rows with nothing describing them.
  Found while reading `M9.24`'s first clean numbers: the hit rate had fallen from
  `M9.10`'s 39% to 25%, and looking for why turned up something worse than a
  lower hit rate.
  `M9.22` made both protocols store "the statement's answer" and called it one
  shape. It is not. Whether a payload holds a `RowDescription` is the server's
  choice, not the proxy's: one comes back for every simple query with rows, and
  for an `Execute` only when a `Describe` asked. The reference workload asks the
  same SQL both ways with no parameters, so the keys collide, and an entry stored
  by a sequence that sent no `Describe` was served to a simple query which is
  always owed one. The client got `DataRow`s with nothing describing them, which
  no driver can read.
  `assemble_simple` is the simple protocol's half of `M9.21`'s assembler: it
  refuses a payload with no description rather than trimming or inventing one.
  Sharing now runs one way, which ADR 0022 records.
  Nothing caught it because both fake servers answered a `SELECT` with a
  completion and no description, which is a shape no Postgres produces. That is
  the third time in this milestone that a fake being kinder than the real thing
  hid a defect, and this one had been storing the wrong payload shape since
  `M9.7`. The fakes now send a description for anything that returns rows, six
  tests that counted frames now read to the terminator instead, and one of them
  asserts the description is there.
  Acceptance: a simple query is not served a sequence's description-less
  payload, with a test that fails before the fix.
- [x] `M9.24` What it was worth. A matched pair of scale runs with the cache on,
  the way `M9.10` did it, recorded in `product/perf/` and reflected in the
  roadmap's M9 section.
  The number to watch is not the hit rate. `M9.10` served 11% of statements at a
  39% hit rate and moved the median 7%, and said the pool and its wakeups were
  flat at about half the profile either way. Half the workload is extended, so
  the question this run answers is whether serving that half without acquiring a
  connection changes that share or only the total again.
  Acceptance: two runs whose sets do not overlap, the hit rate and the share of
  statements served recorded, and an honest sentence about the pool's share of
  the profile whichever way it went.
  It went the other way. Three matched pairs put the cache 7.8% *worse* on the
  median at five hundred connections, sets not overlapping, serving 3% of
  statements at a 26% hit rate. Recorded in
  `product/perf/run-2026-07-30-extended-cache.md`.
  A third configuration separated the two costs, because the pair cannot: opted
  in with a 64-byte budget, so every lookup and every withheld sequence happens
  and nothing can be stored. That is 1% worse rather than 8%, so two thirds of
  the cost is the hits themselves. Throughput is identical across all three,
  pinned by the database, so the cache cannot make the fleet do more work: what
  serving a statement instantly does is return that client to the queue sooner,
  which lengthens it for everyone still in it.
  `M9.10`'s +7% was measured when the pool lock cost 687us per statement and the
  proxy was a bottleneck in front of the database. `M7.58` removed that. The
  cache's own cost did not change; what it was buying shrank fifteenfold
  underneath it. The sentence about the pool's share is now that there is no
  pool share worth talking about, which is the answer this task was asked for
  even though it is not the one it expected.
- [x] `M9.13` Configuration, the half the node obeys. `M9.8` gives an operator
  a way to say it; this is what makes saying it change anything, and it is
  where the hot-reload acceptance lives.
  Three pieces. The store's settings become live rather than constructor
  arguments, so a `reconfigure` can raise the budget, lower it and evict down
  to it, and drop the entries of a tenant that has just been taken out of the
  list. The composition root builds a store once and the tick loop reconfigures
  it, next to `gate.set_ceiling`, which is the same shape and the same reason:
  an operator changing a limit is usually doing it while the node is misbehaving.
  And the relay needs a gate cheaper than building a key. Today it is
  `context.cache.is_none()`, which stops working the moment the store is always
  present, so `QueryCache` gains a defaulted `serves(&TenantId) -> bool`. It is
  not async and it is checked before `normalize` allocates, because off is the
  default and therefore every node's hot path.
  This is also where `store_answer` stops using `grant.ttl`. That is a
  credential's lifetime standing in for a staleness bound, which are two
  unrelated numbers that happened to both be durations; the configured TTL
  replaces it.
  Watch the session future: `M9.7` left 32 bytes under the 5 KiB ceiling.
  The Helm chart and `deploy/config/` grow the section here rather than in
  `M9.8`, because a chart that writes a setting the node ignores is worse than
  a chart that does not mention it yet.
  Acceptance: a running node with no `query_cache` section caches nothing, a
  document adding a tenant makes it start caching without a restart, and one
  removing that tenant drops what was held for it.
  Done. The session future came in at 5,064 bytes rather than 5,088: dropping
  the grant from `store_answer` also dropped it from `read_the_answer`, which
  is 24 bytes back in every session. The hot-reload test was checked by
  deleting the `reconfigure` call and watching it fail, because a test of a
  wiring change that passes either way is the shape this milestone has already
  shipped twice.
  The e2e stack keeps the cache off, with the section written out and commented
  in `deploy/config/config.yaml`. Turning it on there would change what M6's
  three properties measure rather than add a check: a cached read is a
  statement the database never sees. `M9.10` turns it on against the scale
  workload, which is where the question is whether it helps.

## M10: the claims nothing enforces

- [x] `M10.0` Decompose it, which is a task here because the commit-msg hook
  wants a task ID on every commit and a milestone's plan is a commit. The five
  below, the roadmap section, and the completion condition.
- [x] `M10.1` CI runs every milestone gate, and notices the next one that is not
  wired. Eleven `scripts/m*-complete.sh` exist and CI's milestone job runs three:
  M-1, M6 and M7. The other eight passed on the commit that closed their
  milestone and nothing has checked them since, which makes them a record of what
  was once true rather than a gate. All eight are Docker-free and run in seconds,
  and `scripts/gates/release-check.sh` is the same story for M8.
  The wiring is the easy half. The half that matters is that adding an
  `m11-complete.sh` and forgetting to wire it must fail something, so
  `scripts/check-drift.sh` grows the assertion: every milestone gate in
  `scripts/` is named in `.github/workflows/ci.yml`. A gate nobody runs is worse
  than no gate, because the roadmap cites it as evidence.
  Acceptance: CI names every `m*-complete.sh` and `release-check.sh`, the drift
  check fails when one is missing, and all of them pass on this commit.
  All eight passed, so wiring them added no failures and the gap was only ever
  that nothing would have said. The drift check was watched failing on all eight
  before the workflow was touched.
- [x] `M10.2` The codec is fuzzed by something other than memory.
  `pgprox-proto/AGENTS.md` says a malformed frame must not take down a node and
  that this is "fuzzed, not assumed". `scripts/fuzz.sh` exists and no scheduled
  job runs it, so the most exposed parser in the process is fuzzed exactly as
  often as somebody remembers to.
  It goes in the nightly job the FIPS build already uses, with a time budget
  rather than a target count, because a fuzzer on a shared runner is measured in
  minutes and not in executions. The corpus that finds something is committed, so
  the next run starts where the last one stopped rather than from nothing.
  Acceptance: the scheduled job runs `scripts/fuzz.sh` with a bounded duration,
  the script takes that duration as an argument, and a crash leaves the input
  that caused it rather than a line in a log nobody reads.
  One criterion was wrong as written: it said a *committed* reproducer, and CI
  cannot commit. What it can do is upload the artifact, which is what a human
  then commits alongside the fix, so that is what the job does.
  The script already took a duration and already seeded its corpus from the
  committed generator, so the whole gap was scheduling. Run here at 20 seconds a
  target before committing: all three clean, including the `Bind` parameter
  reader `M9.19` added and the target `M9.19` extended to walk its output.
- [x] `M10.3` Mutation testing, which `standards/testing.md` says already runs.
  It says `cargo-mutants` runs nightly against the pure state machines and that
  surviving mutants are treated as missing tests. There is no script, no job, and
  the tool is not installed. Either the sentence goes or the thing exists.
  M9 is the argument for the thing existing. Three of its defects were invisible
  because a fake answered something Postgres refuses, and one fix went in
  half-applied and green. Each is exactly what a surviving mutant looks like: a
  line whose removal changes nothing any test can see.
  `scripts/mutants.sh` runs against the sans-I/O crates the standard names, with
  a timeout per mutant and a baseline file of survivors that are accepted with a
  reason each. New survivors fail the script; the baseline is a list nobody may
  grow without saying why.
  Acceptance: the script exists, runs, and records a baseline; the nightly job
  calls it; and `standards/testing.md` describes what runs rather than what was
  intended.
  The first run: 950 mutants across the four crates, 720 caught, 137 unviable,
  89 alive. An 89% kill rate against line coverage of 96% to 99%, which is the
  gap the standard's sentence about coverage being a floor is about.
  Two things the tool found about the harness rather than the code. Each job
  copies the tree, and this repo's `target-coverage` is 6 GB, so on a machine
  whose `/tmp` is a tmpfs the run dies with `ENOSPC` twenty minutes in: the
  copies go on the real disk. And the stale-entry check compared the baseline's
  every crate against whichever crate was measured, so running one crate warned
  about all the others, which is a warning about nothing.
  The gate was checked both ways: it passes with the baseline as recorded, and
  removing one entry fails it by name.
- [x] `M10.4` The survivors worth killing, in `pgprox-cache`. `M10.3` produced
  the list of 89; this is one crate's ten, and the split is per crate because
  eighty-nine triages is not one commit and because each crate's survivors are
  about a different property.
  The rule for triage, so it does not become a scoreboard: a survivor earns a
  test if the mutation it survived would be a wrong answer to a client or a
  bound the code claims to hold and does not. A survivor that only changes a log
  line or an error's wording goes in the baseline with that as its reason.
  All ten here are the byte accounting, which this crate's own rules call the
  thing that makes it bounded: `weigh` survives having a `+` replaced by a `*`
  three times over, because the only assertion on it is that a big entry weighs
  more than a small one. That is the shape of a property tested by inequality
  when it is arithmetic.
  Acceptance: every `pgprox-cache` survivor is killed by a test or in the
  baseline with a reason that is not `untriaged`, the crate's coverage gate still
  holds, and the mutation run for it is clean.
  Ten became one. Six fell to four tests that assert the byte accounting as
  arithmetic rather than as an inequality, which is what let a `+` become a `*`
  three times over. Three more fell to two tests written after reading the exact
  line each mutant replaced: one asserts the recency index holds one place per
  entry, which is the invariant a no-op `next_seq *= 1` breaks, and one asserts
  that a TTL which overflows the clock is refused, which is a guard nothing had
  ever reached.
  The last is `Inner::remove -> None`, a timeout: the suite detects it by
  hanging, which the tool reports as a survivor, and there is no assertion that
  improves on a hang. That is its reason in the baseline.
  Coverage went from 98.9% to 99.06%, which is the smaller half of what the six
  tests were worth.
- [x] `M10.6` The survivors in `pgprox-route`, eight of them, five of which are
  timeouts. Same rule as `M10.4`.
  Two were real and both are now tests. `begins_read_only_transaction` joins its
  `SET` arm with `&&`, and replacing that with `||` made a bare `SET` open a
  transaction; nothing noticed because no case in the table was a `SET` that is
  read only and is not a transaction. `SET SESSION CHARACTERISTICS AS
  TRANSACTION READ ONLY` is exactly that, and it is a statement a real
  application sends. And `ReplicaWatch::is_empty` returning true unconditionally
  survived everything, because the only assertion on it was for a watch with no
  replicas: it now has one for a watch with two, which is what decides whether a
  grant's replicas are polled at all.
  The other six stay, five as hangs and one as an equivalent mutant. The
  equivalent one is worth the sentence it gets in the baseline: `i < len` becomes
  `i <= len`, and because `bytes[i..]` at the end is an empty slice rather than
  out of bounds, the loop exits one iteration later with the same answer.
- [x] `M10.7` The survivors in `pgprox-proto` that are about bounded inspection,
  nine of the eighteen. Split from the rest because they are one property: how
  much of a message the proxy is willing to hold, which is the crate's answer to
  a malformed frame not taking down a node.
  Five are the prefix sizes themselves, where `8 * 1024` becoming `8 + 1024`
  survived everything: the existing test records *which* messages are inspected
  and never how much of them. Four are `FrameRelay::buffered`, which is what an
  operator reads to know a session is not holding a gigabyte and which nothing
  asserted at all. Two are the header boundary, where `1 + LEN_PREFIX` could
  become `1 * LEN_PREFIX` and a four-byte prefix would be read as a header whose
  length came from bytes that had not arrived.
  Acceptance: the prefix sizes and the relay's byte count are asserted as
  numbers, and the crate's mutation run has no survivor outside the baseline.
  Nine killed by two tests. The header boundary was scoped into this task and
  should not have been: asserting that four bytes do not complete a *message*
  does not discriminate, because a message completes when its body arrives and
  not when its header does. Those two moved to `M10.10`, which is the honest
  place for them.
  The run also produced a survivor that was caught the time before, in
  `Reader::cstr`, and the gate failed on it as designed. It is not a regression:
  the mutant makes a proptest fail and then shrink, and whether the shrinking
  fits inside the per-mutant timeout depends on machine load. Worth knowing about
  this repo's baseline in general, since a timeout here means slow rather than
  undetected.
- [x] `M10.10` The other nine `pgprox-proto` survivors: `conn_id_from_key`,
  `row_description`, `untagged`, `push_body`, `push_header`,
  `SessionState::on_frontend` and `Startup::options`.
  `push_header` is the interesting one and the reason it needs a task rather than
  a line: `1 + LEN_PREFIX` becoming `1 * LEN_PREFIX` makes four bytes a complete
  header, whose length field is then read from a byte that has not arrived. The
  test that kills it has to assert where the *next* message starts, not that this
  one is unfinished. Same rule as `M10.4`, and filed apart from `M10.7` because
  they are six unrelated functions rather than one property, and because at least
  one of them, `push_body`'s `> 0` becoming `>= 0`, looks equivalent and has to
  be shown to be rather than assumed.
  Three killed, six equivalent, and the ratio is the finding. The three are both
  halves of a `RowDescription` column that no test read: `typlen` and `typmod`
  are each -1, and dropping either unary minus writes 1, which moves every byte
  after it by nothing and changes what a client makes of them entirely. The
  existing test counted forward to the type OID and stopped, so the four fields
  after it were never looked at; the new one asserts all eighteen bytes. The
  third is `untagged`'s length prefix, where `out.len() - len_at` becoming `+`
  is wrong for every buffer that is not empty when the message starts, and every
  test in that module started from an empty one. `pgprox-session` appends
  several messages into one buffer, so this was reachable, not theoretical.
  The six are equivalent mutants and each carries its argument in the baseline.
  The two `push_header` ones are the interesting pair, because this task's own
  guess about them was wrong: the boundary cannot be moved to four bytes,
  because `decode_header` returns `Ok(None)` below five on its own first line
  and the fall-through path returns the same `RelayOutcome` the early return
  would have. What the tool actually found there is a guard duplicated in caller
  and callee, which is a fact about the code rather than a missing test. The
  others are `|` against `^` on operands with no bit in common, `> 0` against
  `>= 0` on a `usize` guarding a no-op, an empty match arm that exists to hold
  the comment above it, and a `continue` for a case the `split_once` below it
  drops anyway.
  Acceptance: every proto entry in the baseline says why, `untriaged` appears
  nowhere in that section, and the crate's run has no survivor outside it.
  351 mutants, 7 surviving, all seven in the baseline with reasons. Coverage
  98.52%. `pgprox-proto` is finished; `pgprox-session` is the last crate.
- [x] `M10.8` The survivors in `pgprox-session`, fifty-three, ten of them
  timeouts. Same rule, and the largest list, which is consistent with it being
  the most correctness-critical crate and the one M9 kept finding defects in.
  Split on reading the list, the way `M10.4` and `M10.7` were. The current run
  reports fifty-five, two more than the baseline, both of them timeouts that
  come and go with machine load. They fall into four groups that have nothing
  to do with each other, and one commit that touched all four would be one
  commit nobody could review: this task takes the sixteen about what the crate
  reports about itself, and `M10.11` through `M10.13` take the rest.
  The sixteen are `Registry::len` and `is_empty`, `PgConnector::known`,
  `ParameterCache::is_empty`, `SqlReplicaProbe::len` and `is_empty`,
  `TokenAuth::is_done`, `ScramAuth::is_done`, the three `Debug` impls, and the
  four in the `StaticCredentials for Arc<T>` blanket impl.
  Acceptance: each of those is asserted by a test that fails without it.
  Eighteen, not sixteen. The re-run turned up two more of exactly this shape in
  `state.rs`, `Handshake::is_closed` and `is_awaiting_credential`, neither of
  which was in `M10.3`'s baseline. They were checked by hand rather than
  assumed: with `is_closed` returning true unconditionally the whole suite
  passes, so it is a missing test today whatever it was then.
  Five tests, four additions to existing ones. Three findings worth the task:
  The `Debug` impls had a test each asserting what they must *not* print, and a
  `Debug` that prints nothing passes that. Redaction has two halves and only one
  was being checked, so an impl could lose the field an operator actually reads
  and no test would notice. The assertions now name what has to appear as well.
  `is_done` was asserted true at the end of both exchanges and never false
  before, so a machine that reported itself finished with no grant in hand
  survived. That is the shape of a session admitted without authenticating,
  which is worth more than the mutant that pointed at it.
  The `StaticCredentials for Arc<T>` blanket impl had no test at all. It is the
  path production takes, since the composition root shares one set of keys
  across every session, and the bare impl is the one the tests took. Forwarding
  that returned `None` would have refused every login on a real node while the
  whole suite stayed green.
  Six timeouts in `shell.rs` that `M10.3` did not see are now in the baseline
  rather than killed, because they belong to `M10.13` and because the reason is
  the same one for all six. `flush -> Ok(())` was reproduced by hand: five probe
  tests wait for bytes that are never written and run past sixty seconds, so the
  suite does detect it, by not terminating. Which of these appear in a given run
  depends on how loaded the machine is, which is the effect already recorded for
  `Reader::cstr`.
- [x] `M10.11` The `relay.rs` and `flush.rs` survivors, eight: `Relay::wrote`
  both ways, `record_write`, `released`, `forward_without_routing`,
  `Outstanding::sent` twice and `discharge`. One property rather than eight
  functions: whether a connection is safe to hand back. `wrote` is what decides
  a pool release, and `Outstanding` is what decides when a pipelined sequence is
  over. `M9.26` already found one defect here by way of a fake server that was
  too polite, so these are the ones to read closely rather than to rubber-stamp.
  Acceptance: all eight killed, or a written reason for any that cannot be.
  All eight, by five tests, and none of them equivalent. The pattern in every
  one is a value that other tests set up and none of them read:
  `wrote` had no assertion at either end. Stuck at true it pins a session to
  the primary for good; stuck at false it sends the next read to a replica that
  has not replayed the write yet, which is the one wrong answer this project's
  routing exists to avoid. `record_write` clearing it had nothing checking that
  either, so the flag could have been write-only in both directions.
  `released` is half of a pair whose other half, `acquired`, was checked
  everywhere. A release the relay does not record leaves it believing it still
  holds a connection, so the next statement goes out with no acquire.
  `forward_without_routing` decides `acquire` as the negation of what is held,
  and dropping the negation is invisible unless a test sends a frame with no
  SQL in it, which none did: every test here started with a `Query`.
  The two `Outstanding::sent` arms were the same shape as the proto findings.
  `Bind` appeared only as the thing before an `Execute`, and an `Execute`'s own
  completion settles that sequence whether the `Bind` was counted or not, so
  dropping the `Bind` arm changed nothing any test could see. `Close` was never
  sent by any test at all. Both now have a test that sends one on its own and
  requires its own completion tag to settle it.
- [x] `M10.12` The `sequence.rs` survivors, ten. The M9 machine, and the newest
  code in the crate: `feed`'s size ceiling three ways, `may_hold`, `begins`,
  `split`, `is_empty`, `assemble_simple` and the `Frames` iterator bound. The
  ceiling ones matter most, because `MAX_HELD_BYTES` is what stops a client
  holding a proxy's memory by never finishing a sequence.
  Acceptance: all ten killed, or a written reason for any that cannot be.
  Eight killed by four tests, two equivalent.
  The ceiling had a test that fed a body far over the limit, which every
  arithmetic mistake in the comparison survives: too large is too large whether
  the terms are added, subtracted or multiplied. The boundary is where it is
  decided, so the new test feeds the frame that exactly fits and the one a byte
  past it, and asserts `MAX_HELD_BYTES` as a number besides, since `64 * 1024`
  becoming `64 + 1024` moves the limit without moving anything measured
  relative to it. That is `M10.7`'s finding in a different file.
  `assemble_simple` had no test in this crate at all. Only the binary called
  it, so the whole simple-query hit path was assembled by a function whose
  output nothing asserted; replacing its body with `Ok(())` wrote nothing to
  the client and passed. It now has the same byte-for-byte test its extended
  sibling already had.
  `split`'s bound was the one worth the most. The first attempt at a test did
  not kill it, because a truncated payload that follows a `CommandComplete` is
  refused by the check for anything after the completion and never reaches the
  bound at all. The stub has to come before the completion, and then the mutant
  reads a length field out of bytes that are not there: an index out of bounds
  on a node serving everybody else, rather than the `Unservable::Malformed` the
  original returns.
  The two equivalents are both guards duplicated by a later check, which is the
  same pattern `M10.10` found in `pgprox-proto`. `may_hold`'s `Parse` arm tests
  `frames.is_empty() && step == Step::Nothing`, and those are the same boolean
  at every reachable state, so `&&` and `||` agree. `begins` guards the first
  frame of a sequence, and letting everything through only defers the refusal
  to `may_hold`, which with an empty buffer accepts a `Parse` or a `Bind` and
  nothing else. Both arguments are in the baseline where a reader can disagree
  with them.
- [x] `M10.13` The `shell.rs` survivors, eighteen, ten of them timeouts. The
  wire buffer: `fill`, `fill_held`, `consume`, `compact`, `borrow`, `reclaim`,
  `queue`, `flush` and `is_buffered`. Ten hangs in one file is itself the
  finding to explain before writing anything, because a mutant that hangs is
  usually an index that stops advancing, and the honest question is whether the
  suite detects it or merely fails to terminate. `standards/testing.md` says a
  timeout is a survivor here; whether that is the right call for this file is
  part of the task.
  Also the three loose ends that are not the wire buffer: `probe::text_row`'s
  `at + 4` bound, `run_replica_query` and `cancel::send`.
  Split again on reading it. Ten of the eighteen are the read cursor, and one
  test covers all ten by driving `consume`, `compact`, `buffered`,
  `is_buffered` and `reclaim` directly rather than through a socket. The
  interesting states are the ones `reclaim` exists to make unreachable from
  outside, so a test that drives the wire only through its public reads can
  never reach them; driven directly they are ordinary assertions about four
  bytes. `HELD_READ` is in here too, for `M10.7`'s reason: `16 * 1024` becoming
  `16 + 1024` changes a documented per-connection cost and nothing measured
  relative to it notices, so both read sizes are asserted as numbers now.
  Four of the ten became kills and six did not, which answers the question this
  task was filed to ask and not in the direction it expected. The four that
  died are the ones whose mutants leave the read loop working: `compact` twice,
  `is_buffered`, and the constant. The six that live are `buffered` three ways,
  `consume` twice and `reclaim`, and the new test does fail every one of them.
  It never gets to say so. Those mutants stop the read loop making progress, so
  some other test in the suite hangs, and `cargo mutants` runs the suite under
  `cargo test`, which has no per-test timeout. One hung test is one hung run,
  and the run reports a timeout with no verdict at all.
  So a timeout here does not mean "detected by hanging". It means the run was
  abandoned before the assertion that would have named the defect could be
  read. That is worth more than the six mutants: every timeout in this file's
  baseline, and in `pgprox-route`'s and `pgprox-cache`'s, is a verdict nobody
  has actually seen. `M10.16` is the fix, and it is a change to the runner
  rather than to any test.
- [x] `M10.14` The last three real gaps, and the last task in M10's mutation
  work. `M10.16` took this from six to four and then `M10.15`'s one loose end
  joined it, so the list is `borrow`'s deadline both ways, `fill_held`'s two
  `start + n` arithmetic mutants, and `probe::text_row`'s bound.
  The deadline pair is the interesting one and the reason this is a task rather
  than a line: `Instant::now() + BUFFER_WAIT` becoming a minus, and
  `now >= deadline` becoming `<`, both change behaviour only when the slab is
  exhausted. The test fixture sizes its slab small so that state is reachable,
  and nothing reaches it, so the retry loop that absorbs a burst as latency has
  never run in a test. That is `ADR 0008`'s claim untested.
  `probe::text_row` was filed as an `at + 4` bound, twice, and it is not one:
  the mutant is `len < 0` becoming `len <= 0`, which reads an empty string as a
  SQL NULL. The bound is already covered by
  `a_truncated_row_is_rejected_rather_than_panicking`. Reading the replacement
  text rather than the function name would have caught that both times it was
  written down.
  After this the baseline is equivalents only, every one with an argument.
  Three tests, five mutants, and each one had the same cause: a test that
  covered one ending of a two-ended thing.
  `borrow` had a test that holds the slab empty for good, so the retry loop
  only ever ran out. Both mutants, the deadline computed backwards and the
  comparison inverted, reach that same refusal, so neither was visible. The new
  test frees the buffer while the read is waiting, which is `ADR 0008`'s claim
  and the loop's other ending, and it had never run. No sleep is needed: the
  read is polled first and gets as far as an empty slab, so the drop lands
  while it waits rather than before it asks.
  `fill_held` resizes to make room, reads, and trims back. Both arithmetic
  mistakes leave the frame decodable, which is why nothing noticed. One
  over-trims and shows up as bytes that were never sent; the other
  over-allocates, and that one is the buffer growing past what the slab lent,
  which is the thing this type exists to stop. Asserted as the buffered bytes
  and as a bound on the capacity after one read.
  `text_row` had `a_null_field_is_not_an_empty_one` and not its converse. The
  existing test pairs a NULL with a non-empty value, so `len <= 0` reading an
  empty string as NULL passed it. A length of zero is an empty string and only
  -1 is NULL, and that distinction is the reason the function has a doc comment.
- [x] `M10.15` The last twelve, which had nothing in common but being left:
  six `shell.rs` hangs `M10.8` baselined, six cursor mutants `M10.13` had
  written assertions for, and three loose ends elsewhere. Filed as waiting on
  `M10.16`, on the grounds that a list of mutants nobody can see a verdict for
  is not a list worth working.
  `M10.16` emptied it. Eleven of the twelve are caught by tests that already
  existed, and the twelfth, `probe::text_row`, moves to `M10.14`. Nothing was
  written for this task and nothing needed to be, which is the outcome it was
  filed to find out about.
- [x] `M10.16` Make a hung test a failed test, so a mutation timeout means what
  the standard says it means. `cargo mutants` runs the suite under `cargo test`,
  which has no per-test timeout, so one test that never returns costs the whole
  per-mutant run its verdict. `M10.13` found this the hard way: it wrote
  assertions that fail six mutants and the run reported all six as timeouts
  anyway, because another test hung first.
  The change is `--test-tool=nextest` in `scripts/mutants.sh` and a
  `slow-timeout` with a `terminate-after` in a nextest profile, so a hung test
  is killed and reported as a failure. The workspace already runs nextest under
  `cargo llvm-cov`, so this adds no tool.
  The terminate-after has to sit well under `MUTANTS_TIMEOUT`, which is sixty
  seconds for the whole suite, and well above the slowest honest test. Both
  numbers want measuring rather than picking.
  Then re-triage every timeout in `product/mutants-baseline.txt`, across all
  four crates and not just this one. `pgprox-cache`'s one, `pgprox-route`'s
  five and `pgprox-session`'s twelve are all verdicts nobody has seen, and the
  reasons written beside them say "detected by hanging", which is now known to
  be a claim about the runner rather than about the suite.
  Acceptance: a mutation run in which no outcome is a timeout for a reason
  other than a genuinely slow test, and a baseline whose remaining entries say
  what they mean.
  Thirty-seven baseline entries became fourteen, and not one test was written.
  `pgprox-cache` is clean outright, `pgprox-route` is down to its one
  equivalent, `pgprox-proto` to six and `pgprox-session` from twenty-four to
  seven. No outcome in any of the four crates is a timeout any more.
  The per-test cap is ten seconds, against a suite whose slowest test is 0.207s
  and whose whole run across the four crates is 0.321s. Both numbers were
  measured rather than picked, and forty-eight times the slowest honest test
  leaves nothing legitimate at risk while sitting well inside the sixty seconds
  the whole per-mutant run gets. Only the mutation run uses the profile: an
  ordinary run and the coverage gate should not be killing tests, and a test
  that hangs while somebody is working on it is a thing they notice.
  Sixteen of the twenty-three that fell were in `shell.rs`, including all six
  `M10.13` had already written assertions for. Those assertions were correct
  the whole time and had never once been allowed to report.
  What is left is the honest list: eleven equivalents with arguments, and three
  real gaps that `M10.14` owns. `M10.15` closes empty, its one
  remainder moving to `M10.14`.
  Worth keeping from this: a category the tool reports is not a finding until
  you have read what it is a category of. Every one of those entries had a
  reason written beside it, several of them by me, and the reasons were fluent
  and wrong.
- [x] `M10.5` What the cache is worth on a workload it is for. `M9.24` measured
  the reference workload and named this as the cheapest thing left, because that
  workload answers a different question: 30% of its statements are writes, two
  thirds are inside a `BEGIN`, and only 27% reach a lookup at all.
  A second workload document, versioned the way `workload.yaml` is, with a
  read-heavy mix and single-statement transactions. It is not a friendlier
  version of the same thing and must not be tuned until the answer improves: it
  is the shape of a tenant that would opt in, chosen once and then measured.
  The prediction to record before running, so the run can contradict it: below
  saturation the queueing effect `M9.24` found disappears, because there is no
  queue to move work to the back of. If the median improves at 500 connections
  too, then `M9.24`'s explanation is wrong and that is worth more than the
  improvement.
  Acceptance: a committed workload document, a matched pair against it whose sets
  do not overlap, and the prediction recorded before the numbers.
  A quarter off the median, 794us to 600us, sets not overlapping; the hop at
  matched load down from 430us to 25us; CPU per statement 16% better rather than
  5% worse. 27% of statements served at a 57% hit rate.
  Two of the three predictions were right and the first was optimistic: the
  addressable share went to 64% rather than 80%, because a tenth of transactions
  being four statements inside a `BEGIN` eats a third of all statements, none of
  them cacheable.
  What it does not settle is `M9.24`'s queueing explanation, and the reason is
  worth its own task. This workload does not saturate anything: its median is
  three orders of magnitude below the reference workload's and its throughput is
  three times higher. A run with no queue cannot confirm a claim about queues.
  `M10.9` is that test.
- [x] `M10.9` Test the queueing explanation directly. `M9.24` said the cache
  regressed the median because the database was saturated, throughput was pinned
  by it, and a statement answered instantly returned its client to the queue
  sooner. `M10.5` improved the median on a workload that saturates nothing, which
  is consistent with that and is not a test of it.
  The test is `workload-cached.yaml` at a connection count high enough to
  saturate, found by walking the count up until the median stops tracking the
  direct baseline. If the cache still helps there, the explanation is wrong and
  the reference workload's regression has another cause, which matters more than
  either number.
  Acceptance: a connection count at which the read-heavy workload saturates,
  recorded with how it was found, and a matched pair at that count.
  It saturates between one and two thousand connections, four times what the
  reference workload needs, and the walk says so three ways at once: throughput
  stops rising (53,597 then 106,890 then 136,798 then 135,705 as the count
  doubles and doubles again), the median leaves the direct baseline by a factor
  of 144, and the direct baseline itself never moves off 320us.
  The pair ran at two thousand. The cache is **17.5% worse on the median**, sets
  not overlapping, so `M9.24`'s explanation stands as stated rather than as
  incomplete. The prediction written before the run named both readings, and
  this is the one that leaves the claim intact.
  The falsification that mattered did not fire. Throughput with the cache on is
  4.2% higher on the means, which would have said the database does not pin it,
  and the sets overlap by 330 transactions out of 136,000. By the standard
  `M9.24` applied to its own hop figure that is not a result, and it is not
  claimed as one. It is left open in the run document rather than resolved,
  because three pairs cannot separate 4% from noise and settling it is a
  different task.
  The size is the part worth keeping. `M9.24` served 3% of statements and cost
  7.8% of the median; this serves 36% and costs 17.5%. Serving more made it
  worse, which is the mechanism's own signature rather than a surprise.
  And the finding that outlives both: whether the cache helps is not a property
  of the workload. The same document is 24.4% better at five hundred connections
  and 17.5% worse at two thousand. ADR 0021 says opt-in per tenant; this says an
  operator also has to know where their fleet sits against its database.
  Recorded in `product/perf/run-2026-07-31-saturation.md`.
- [x] `M10.17` Write `scripts/gates/m10-complete.sh`, which the roadmap has named as
  this milestone's completion condition since the milestone was filed and which
  does not exist. Every other task in M10 is done and the milestone cannot be
  called complete, which is precisely the failure mode M10 is about: a claim
  with nothing that fails when it stops holding. It was found by trying to run
  the gate rather than by reading the file.
  The roadmap already says what it checks: every milestone gate that does not
  need Docker runs in CI, the fuzz target runs on a schedule rather than only by
  hand, mutation testing exists as a script with a recorded baseline, and
  `standards/testing.md` describes what actually runs.
  Some of that is already enforced elsewhere. `scripts/check-drift.sh` asserts
  that every `scripts/m*-complete.sh`, `release-check.sh`, `fuzz.sh` and
  `mutants.sh` is named in `.github/workflows/ci.yml`, which is the first check
  in a different place. The gate should assert what is not covered rather than
  restate it, and should say where the overlap is.
  Acceptance: the script exists, runs without Docker, fails if any of the four
  claims stops holding, is named in CI like its siblings, and passes.
  Eleven checks, and it found two things on its first run rather than passing
  the way a gate written to fit its subject would.
  `m10-complete.sh` was not named in CI, because the check that every milestone
  gate is named there counts the gates it finds on disk and this one had just
  appeared. So the gate's own first act was to notice its own absence, which is
  the behaviour worth having.
  And the `untriaged` check failed against the baseline's header, which explains
  what `untriaged` was and why it is no longer allowed. A check that reads its
  own documentation as a violation cannot be fixed without deleting the
  explanation, so it reads entries rather than comments now.
  Verified by breaking it rather than by running it once: an `untriaged` line
  added to the baseline and `--test-tool=nextest` removed from `mutants.sh` both
  fail it, and both were put back.
  It deliberately does not re-check what `scripts/check-drift.sh` already
  asserts. What it adds is the part drift cannot see: that the tier 3 jobs are
  on a schedule, that the baseline is reasons rather than names, and that a hung
  test is killed rather than read as a survivor.

## M11: the gaps the completed milestones name

Ten milestones are complete and each one wrote down what its own numbers do not
say. This milestone works that list. Nothing here is a feature; every task is a
claim some milestone made and then qualified in its own words.

Three of the qualifications cannot be worked here at all, and they are recorded
as blocked rather than filed, because a task nobody can start is not a plan:

- **A complete 100k run** needs the load generators on their own machines, a
  database that can absorb the offered load, and a real network between the
  three. Every latency number in this repo is loopback and is therefore a floor.
  M7 says so already.
- **The interactive half of `M-1.17`**, in ADR 0012: running a real task under a
  second agent tool and recording what changed. No second tool is installed and
  the judgement is a human's.
- **The plan's three open items for M0**: the sidecar `.proto` sign-off, the
  upstream `max_connections` reserve per server class, and whether any tenant
  needs `LISTEN`/`NOTIFY` at scale. All three need an owner outside this repo.

- [x] `M11.0` Plan M11: read what each completed milestone says its numbers do
  not cover, separate what can be measured here from what needs hardware or a
  human, and file the first as tasks and the second as blocked.
  Four filed, three blocked, and a fifth filed for the gate: `M10.17` found that
  a milestone whose completion condition does not exist cannot be closed, so
  `M11.5` writes `scripts/gates/m11-complete.sh` rather than leaving it to be
  discovered at the end.
  The blocked three are worth naming rather than dropping. Each is a real gap
  and none of them is work: they need three machines, a second agent tool, or an
  owner outside this repo. Filing them as tasks would put entries in a backlog
  that nobody could ever start, which reads as progress not made rather than as
  progress not possible.
- [x] `M11.1` Settle the throughput question `M10.9` left open. That run found
  the cache 4.2% ahead on transactions at saturation, with sets overlapping by
  330 out of 136,000, and declined to claim it. It matters because `M9.24`'s
  whole explanation rests on throughput being pinned by the database: if the
  cache raises it, something else is pinning the fleet and the explanation is
  wrong at its root.
  Three pairs cannot separate 4% from noise. This needs enough pairs to say yes
  or no, and the count should be argued from the spread the six existing runs
  show rather than picked. Same workload, same connection count, same machine.
  Acceptance: a number of pairs justified before running them, and a verdict
  that says either "throughput rises and here is by how much" or "the difference
  is inside the noise and here is the bound".
  Eight pairs, argued from `M10.9`'s six runs before any were run: d is 1.66, so
  six per arm gives 80% power and eight gives 90%, and 90% was taken because a
  null result is the outcome that leaves `M9.24` standing and a weak null is
  worth little.
  **Throughput rises. Eight pairs out of eight.** 4.11% on the means, 95% CI
  from +1.14% to +7.08%, paired t 3.28 on 7 df, sign test p = 0.008. The
  unpaired sets still overlap, which is exactly why `M10.9` was right not to
  claim it and why the pairing rather than the count is what settled it.
  So `M9.24`'s premise is false as stated: the fleet does more work with the
  cache on, on a database saturated by every other measure.
  Its mechanism survives and now explains both numbers at once. The 36% served
  from memory return to the queue sooner and the 64% that reach the database
  wait longer, which is the median regression, 16.6% here on eight pairs against
  `M10.9`'s 17.5% on three. What `M9.24` missed is that served statements are
  nearly free rather than merely reordered, so they add completions on top. The
  workload splits in two and the median statement is in the slower half.
  The confound is stated rather than waved away: the control runs first in every
  pair, so a machine warming up would produce this. The direct baseline is the
  control for it and does not move, 314us against 315us. That is evidence, not
  proof, and a re-run should alternate which arm goes first.
  `M9.24`'s document is left as written. Recorded in
  `product/perf/run-2026-07-31-throughput.md`.
- [x] `M11.2` The TLS 1.2 restriction FIPS mode imposes, which `M8` never
  reached. `scripts/cipher-matrix.sh` says it in its own comments: FIPS drops
  ChaCha20-Poly1305 and restricts TLS 1.2 to ECDHE suites with extended master
  secret, and a matrix with no TLS 1.2 row in it has not tested the restriction
  it was written for. Every driver on the machine negotiated TLS 1.3, whose
  suites are all approved, so the restriction was never exercised.
  Pin at least one driver to TLS 1.2 and record what each build accepts. The
  interesting cell is a suite the default build takes and the FIPS build
  refuses; if there is no such cell the claim is wrong and that is the finding.
  Acceptance: a matrix with TLS 1.2 rows for both builds, and either a
  demonstrated difference or a written statement that there is none.
  **Demonstrated.** TLS 1.2 with ChaCha20-Poly1305 is taken by the default build
  and refused by the FIPS build. That is the restriction, exercised for the
  first time, and every other row in the matrix is still TLS 1.3.
  Pinned on the client rather than on the proxy. Giving the proxy a maximum
  version knob would be adding a production surface so a test could reach a
  state, and the state is reachable from outside: libpq uses OpenSSL and OpenSSL
  reads `OPENSSL_CONF`. Two probes, `psql-tls12-aes` and `psql-tls12-chacha`,
  differing only in the suite they ask for.
  The AES probe is the control and it is the reason this is an experiment rather
  than a row. FIPS approves ECDHE with AES-GCM, so it has to be taken by both
  builds; if it were refused, the FIPS build would be broken rather than
  restrictive and the ChaCha row would mean nothing. The script asserts both
  outcomes rather than recording them, because a probe broken by its own
  OpenSSL config would produce "refused on both", which reads as a difference
  and is not one.
  One defect found in the harness while reading its output: the protocol table
  showed `TLSv1_2` for the refused handshake, because version negotiation
  succeeds before suite negotiation fails, so a refusal still logs a protocol.
  A cell reading `TLSv1_2` beside a `**refused**` suite says the connection
  worked. Both tables mark refusals now.
- [x] `M11.3` What happens when a fleet at its connection cap loses a third of
  itself, which `M8` says its rehearsal does not cover. That rehearsal is three
  nodes on one machine losing one node, and it lost 22 of 21,088 transactions.
  What it does not say is what happens when the survivors are already at their
  cap, which is where shedding has to work and where `M4`'s shed path has never
  run under real pressure.
  Acceptance: a run at the cap with a node killed outright, the shed path shown
  to fire, and the transaction loss recorded next to the existing figure.
  **The acceptance criterion is unsatisfiable, and finding out why is the
  result.** The shed path cannot fire at the cap. It is refused there
  deliberately: `pgprox_cluster::shed::decide` returns
  `Keep(NoHeadroomAtHome)` when the tenant's home node has no room, and
  `crates/pgprox-cluster/src/shed.rs` has carried a test for exactly that case
  since M3. Shedding is a rebalancing mechanism and rebalancing needs somewhere
  to rebalance toward; closing a client so it reconnects to a node that is also
  full is churn, which the module's own header calls the thing worse than the
  fan-out it was trying to reduce.
  So M8's sentence, "it does not say what happens when a fleet at its connection
  cap loses a third of itself, which is where shedding has to work", is wrong in
  its second clause. The cap is where shedding is designed *not* to work.
  Two things follow. The sentence in the roadmap is corrected rather than left
  to mislead the next reader. And the real question it was reaching for is still
  open and is now filed with the right mechanism named: what happens to the
  clients displaced by a dead node when every survivor is full is a question
  about admission and quota, not about shedding. `M11.6`.
  Worth recording about method: this was settled by reading the decision
  function and its tests, in a few minutes, after `kind` turned out not to be
  installed and the run looked expensive. The expensive experiment would have
  produced a shed count of zero and no explanation for it.
- [x] `M11.4` What pinning costs multiplexing, which ADR 0001 calls an open
  question and hands to the plan. The question the plan asks needs a tenant
  population nobody here has. The question that can be answered here is the one
  underneath it: how the upstream connection count and the median move as the
  share of sessions holding a pin rises.
  A workload document with a `LISTEN` fraction, run at several values of it, so
  the curve is measured rather than reasoned about. `pgprox_pin_total` is
  already instrumented by reason, which is what makes this cheap.
  Acceptance: a workload knob, a curve over at least three values, and a
  statement of where multiplexing stops paying for itself.
  Split on inspection, before starting, the way `M10.8` and `M10.13` were split
  once their size was visible. The knob is not a knob: `pgprox_load::workload`
  has `Kind` as `Read | Write`, and a pinning workload needs a third variant, a
  client that issues `LISTEN` and holds the session, and the crate's 95% gate on
  top. That is a commit. The curve is a separate commit and a separate hour of
  runs.
  Correcting the first version of this note, which said a schema version bump
  was needed on every workload document. It is not. `SUPPORTED_VERSION` exists
  because "a version that changed meaning without changing its number would
  silently invalidate every recorded run", and adding a variant changes nothing
  about what a version 3 document means: one that never says `kind: listen`
  means exactly what it meant before. The bump is for changes that reinterpret
  existing files and this is not one, so `workload.yaml` and its two derivatives
  stay at 3 and the runs recorded against them stay comparable.
  The hazard to watch instead is that `Kind` is compared with `==` rather than
  matched exhaustively, in three places across `workload.rs` and `sampler.rs`.
  A new variant therefore compiles clean and is silently excluded from all
  three, which is the opposite of what an enum is usually good for. Each site
  needs a decision written down, and `sampler.rs:143`, which decides replica
  eligibility, is the one that matters: a pinned session belongs on the
  primary.
  Done, and smaller than either scope note. `Kind::Listen` with the reasoning
  for why it is neither a read nor a write: calling it a write would move a
  watermark that has not moved, and calling it a read would make it eligible for
  a replica the notifications never reach. No version bump, so the three
  committed workload documents stay at 3 and every run recorded against them
  stays comparable.
  `bin/pgload` needed no change at all. It never branched on `Kind`; the only
  mention of it there is a test helper. A `LISTEN` statement is a statement with
  that SQL, which is why this turned out to be a crate change and not a client
  one.
  Two tests, and the second is the one worth having. The sampler already had an
  invariant that no write is marked replica-eligible; extending it to `Listen`
  looked like the work and is vacuous, because no fixture holds a `Listen`
  statement. The tests that matter parse one and assert it is neither of the
  other two, and refuse a document of nothing but `LISTEN`, which would
  otherwise slip past the rule requiring a read that `replica_read_fraction`
  rests on.
  `M11.7` has what it needs.
  This task keeps the knob. `M11.7` takes the curve, and cannot start until this
  one lands.
- [x] `M11.5` Write `scripts/gates/m11-complete.sh` before the milestone needs
  closing rather than after. `M10.17` is the argument: M10's gate was named in
  the roadmap from the day the milestone was filed and did not exist, and
  nothing noticed until every task was done and the milestone could not be
  closed.
  What it checks follows from the four tasks: a recorded verdict on the
  throughput question, a cipher matrix with TLS 1.2 rows, a shed-at-cap run, and
  a pinning curve. Like `m10-complete.sh` it should assert what other checks
  cannot see rather than restate them, and it goes in CI beside its siblings.
  Acceptance: the script exists, fails when any of the four is missing, is named
  in CI, and fails today because none of them is done yet.
  Taken out of backlog order, ahead of `M11.4`, which is what its own text asked
  for: written before the milestone needs closing rather than after. `M11.4`
  had also grown a code change across `pgprox-load` by the time it came up, and
  a gate is the better thing to have in hand first.
  Nine checks. Seven pass and two fail, on `M11.4`'s pinning curve and
  `M11.6`'s admission run, which is the gate doing its job while the milestone
  is open rather than a gate that only exists at the end.
  Two of the nine check behaviour rather than artefacts, which is worth more
  than a file test: that `shed::decide` still refuses on headroom, since
  `M11.3`'s finding and `M11.6`'s premise both rest on it, and that the roadmap
  has not regrown the clause `M11.3` corrected.
  In CI as `continue-on-error` while the milestone is open, so the job stays
  green and the gate stays visible. A gate nobody runs until the end is exactly
  what `M10.17` was about.
- [x] `M11.6` What happens to the clients a dead node displaces when every
  survivor is full. `M11.3` was filed as a question about shedding and is not
  one: shedding is refused at the cap by design, so the mechanism actually under
  test is admission.
  There is no client connection cap in the configuration at all, deliberately;
  `max_client_connections` appears in this repo only as the example of a
  misspelled key that must be rejected. The cap that exists is the upstream pool
  quota, leased per tenant by the cluster. So the question is what the survivors
  do when the displaced clients arrive and the quota is already fully leased:
  whether they are refused with `53300`, whether they queue, and how long the
  leases take to be reissued after the membership change.
  Acceptance: a run on the compose stack, which has three nodes and needs no
  `kind`, with the fleet at its upstream quota and one node killed outright;
  what the displaced clients are told, recorded; and the transaction loss
  recorded next to `M8`'s 22 of 21,088.
  The mechanism is already settled by reading, which sharpens what the run has
  to look for. A displaced client is *accepted* by a survivor, because there is
  no client connection cap; it then waits for an upstream connection when it
  issues its first statement, and `Waiters::give_up` decides what it is told:
  `53300 too_many_connections` when the pool is at its limit, `57014
  query_canceled` when the pool has headroom and the wait merely expired. The
  comment there says why the two are separate: "one says the server is full, the
  other says this node is. Reporting the cap when the pool has headroom would
  send them to the wrong place."
  So the run is not asking whether clients are refused. It is asking **which of
  those two they get**, and the interesting outcome is `57014` while the fleet is
  genuinely full, because that is the operator sent to the wrong place at exactly
  the moment they can least afford it.
  Two things follow for the design. The run needs the client-visible SQLSTATE
  captured, not just a failed-transaction count, so `pgbench` output alone is not
  enough. And it needs the pool's own view at the same instant, since the
  distinction between the two errors is a property of the pool at the moment of
  giving up rather than of the fleet over the run.
  `scripts/e2e.sh` already kills `pgprox-3` under load, in
  `prove_drain_check_catches_losses`. That is the harness to extend rather than
  a new one to write; what it lacks is the saturation before the kill and the
  SQLSTATE after it.
  Done, in `scripts/admission.sh` and `deploy/docker-compose.fleet.yml`,
  recorded in `product/perf/run-2026-07-31-admission.md`.
  A new script rather than an extension of `scripts/e2e.sh`, against what the
  note above says. Two reasons, both found once the shape was visible. The e2e
  run is `M6`'s gate and its assertions are about the shipped configuration;
  this run needs a different client cap and a DNS alias, so putting it there
  would change what the gate measures for the sake of a measurement that is not
  the gate's. And it takes four minutes across two arms, which is not a thing to
  add to a check people run before committing.
  **What a full fleet tells the clients a dead node displaces is nothing. They
  are served, in 0.13 seconds.** One `psql` through the alias at +2s, +5s, +10s,
  +20s and +29s from the kill, with a control at -1s, and all six are served in
  0.12 to 0.16 seconds. Neither `53300` nor `57014` reaches a client at any
  point. There is no admission decision to make: the client cap is not binding,
  so the survivor accepts, and the statement's wait behind 400 queued callers is
  milliseconds because the transactions are milliseconds. The 30-second acquire
  deadline is two orders of magnitude from being reached.
  So the question this task inherited from `M11.3` has the same shape as the
  answer `M11.3` gave: the mechanism it was reaching for is not the one under
  pressure. Neither shedding nor admission is what a fleet at its cap tests.
  What the run found instead is two things about capacity and about what an
  operator sees.
  **A three-node fleet that loses one runs the database at 50 of its 60 cap, for
  as long as the node stays dead.** Not measured as a limit, derived as
  arithmetic and then confirmed by `pg_stat_activity`: three nodes times ten
  guaranteed is thirty reserved, thirty leasable, and the survivors take all
  thirty. The dead node's guaranteed ten is held for a node that no longer
  exists. That is the design working, and the fleet cannot report it, because
  `/v1/servers` says headroom is zero, which is true of the leasable pool and
  not of the cap.
  **And `/v1/servers` reports 89 connections against a cap of 60.** The cluster
  view sums what every node last gossiped and a killed node never gossips again,
  so its last reading stays in the sum. Eight seconds after the kill the
  survivors' new numbers arrive and the corpse's 39 are still being added to
  them. No cap is breached: the database holds 50 and never exceeds 60 in any
  sample of any run. It is a reporting defect rather than a quota defect, and
  that distinction only exists because the run asks the database. `M11.9` files
  the defect.
  Transaction loss, next to `M8`'s 22 of 21,088 (0.10%): **454 of 47,743
  (0.95%)**, with the control at 1 of 43,298. Nine times the share and not
  because admission behaves worse, but because a node at the cap is carrying
  in-flight work when it dies: 363 of the 454 went down with the node.
  Two methodological mistakes, both the same mistake, both worth naming.
  The load client's default connect timeout is 30s and the proxy's
  `ACQUIRE_TIMEOUT` is 30s, so the first run recorded 112 clients saying
  "startup did not finish within 30s" and not one SQLSTATE. A client that gives
  up when the server does measures its own patience. And even at twice the
  server's deadline, a thousand clients reconnecting at once describe the queue
  behind them rather than the node in front, which is why the single-client
  probe exists and why it is the row that answers the task.
- [x] `M11.7` The pinning curve itself, once `M11.4` has given the load
  generator a statement kind that pins. At least three values of the pinned
  share, each a matched run, reading `pgprox_pin_total` by reason and the
  upstream connection count against the median.
  The question is where multiplexing stops paying for itself: ADR 0001 says a
  fleet whose tenants all use `LISTEN`/`NOTIFY` collapses back to session
  pooling, and the curve is what says at what share that starts to bite.
  Acceptance: three or more values, matched runs, and a stated crossing point
  or a statement that there is none inside the range measured.
  First run done, and it is not the curve. Recorded in
  `product/perf/run-2026-07-31-pinning.md` because a run that fails to answer
  its question is worth keeping when it says why.
  The intended y-axis is flat by construction: upstream peak is 60 in all four
  arms including the control, because 60 is the pool's cap and 150 clients reach
  it with no pinning at all. `scripts/pinning.sh`'s own header says the run sits
  "well under its cap", and the control arm proves that false. The x-axis is
  compressed for the same reason: pinned sessions go 0, 60, 60, 71, because once
  sixty sessions pin they own the pool outright.
  What it did measure is worth keeping. **Pinning is paid for in refused work,
  not in connections**, because there is no headroom for connections. Transactions
  fall 9.5%, 38.8% and 47.1% against the control, and every error is `53300 too
  many connections`: 0, 57, 90, 270 across the arms. That is ADR 0001's "collapses
  back to session pooling" observed, with the SQLSTATE an operator would see.
  The `high` arm's p50 is 63% *lower* than the control's, which reads as an
  improvement and is the opposite: that arm refused 270 transactions and a median
  over the work an arm kept is a median over the faster half. The harness prints
  that warning beside the table rather than leaving it to be inferred.
  The re-run needs a connection count low enough that the pool is demand-driven,
  so upstream has somewhere to rise from, and probably shorter sessions so the
  pin count does not saturate at the pool size.
  First run: three points, no baseline, so no curve yet.
  `scripts/pinning.sh` gained a guard first, because its first run reported
  `ok` for three arms that had measured nothing at all: peak 0 upstream
  connections, 0 samples, 0 pins. An arm like that contributes a zero to the
  curve and a shape to the picture, and the harness said it was fine. The guard
  fails an arm with no samples, no peak, or no pin where the document declares
  one, and fails the control if it pins anything.
  It caught something on the next run, which is the finding to chase. The
  control ran perfectly by every other measure, 150 connections for 120s,
  44,832 transactions and no errors on `workload.yaml`, and was rejected
  because `pgprox_pin_total` rose while it ran. That document contains no
  `LISTEN`. Either something else in it pins, or the counter counts something
  the script's own comment says it does not, and until that is known the
  x-axis of this curve is not what it claims to be.
  The three pin arms did produce points, held connections against pins:
  low 71/63, mid 73/73, high 60/76. Two things about them to check rather than
  publish: `high` holds *fewer* connections than `low` and `mid`, which is the
  wrong direction for the hypothesis, and peak equals mean exactly in all three
  arms, which means every one of the 32 samples read the same number and is a
  smell rather than a measurement.
  Split on inspection, before starting, the same way `M11.4` was once its size
  became visible. The curve needs workload documents that do not exist, and a
  committed workload document is a measurement baseline: three of them, with
  the reasoning for the weights, plus the test that keeps them parsing, is a
  commit. The runs are another.
  What the knob has to be is not obvious and is worth writing down before
  either commit. A `LISTEN` pins a session for the rest of its life, so any
  weight large enough to notice pins nearly every long-lived connection and the
  share of sessions that *ever* pin saturates almost immediately. What varies
  continuously is the share of a session's life spent pinned, which is set by
  how far into that life the first `LISTEN` falls. A connection here lives
  about 670 statements, so a per-statement probability q puts the first pin
  around statement 1/q and the time-averaged pinned share is what the curve is
  in. That makes the useful range of q roughly 0.0002 to 0.02, which needs the
  other weights scaled up so an integer weight can express it.
  `M11.10` takes the documents, `M11.7` keeps the curve and cannot start until
  they land.
  Second run, at forty clients, and it is the curve. Recorded in
  `product/perf/run-2026-07-31-pinning-curve.md` with the counts in
  `product/perf/curve-2026-07-31-pinning.tsv`.
  Upstream 14, 30, 39, 40 against pins 0, 24, 36, 40, and no arm saw an error.
  **There is no crossing point, because the cost is linear.** A pinned session
  takes an upstream connection for good and gives back the share it was already
  consuming, which at the control's 2.857 clients per connection is
  `upstream = 14 + 0.650 * pins`. Both constants come from the control arm, so
  the model has zero free parameters against the three pinned arms, and it fits
  to R^2 = 0.9937. No knee, no threshold, no safe pinned share.
  The `high` arm is the degenerate case exactly: 40 clients, 40 pins, 40
  upstream, peak equal to mean equal to the client count, one connection per
  client. ADR 0001's "collapses back to session pooling" is not an analogy.
  It also corrects what the first run concluded. That run said pinning is paid
  for in refused work, which is true at saturation and is a statement about a
  pool with no headroom. With headroom, **throughput is flat**: 17,041, 17,111,
  17,170, 16,894 transactions, a 1.6% spread with no ordering, while upstream
  nearly tripled. The two compose: pinning consumes connections at 0.65 each,
  and the first run's `53300`s are what follows once that reaches the cap.
  The latency columns are not a result and the document says so. One run per
  arm and they do not order, p50 1,455, 2,260, 2,181, 1,806 and p99 36,099,
  30,499, 23,799, 87,199, so the fully pinned arm reads better than the mixed
  ones on the median and two pinned arms beat the control on the tail. The
  connection columns are the measurement.
  Two guards came out of it, both for failures the first run walked past. The
  script now fails a control arm that peaks at the cap, since from there no arm
  can rise and the curve is flat by construction. And `scripts/gates/m11-complete.sh`
  no longer globs for `product/perf/*pinning*.md`, which passed on a document
  whose own title says it is not the curve; it reads the recorded counts and
  requires three arms, a control below the cap, a control that pinned nothing,
  and a y-axis that moved. All four negative cases were checked by feeding it a
  bad file, not by assuming.
- [x] `M11.8` The load client records what clients were told, by SQLSTATE.
  Split out of `M11.6` on inspection, before starting, which is what `M11.4`
  did once the size of its own scope note became visible.
  `Report` counts failures and quotes the first one. `M11.6` needs the
  distribution: how many clients were refused at the door by the node's own
  gate, how many were refused by a full upstream pool, how many timed out
  waiting for one, and how many lost a connection outright. Those are four
  different operator responses and today they are one number.
  Keyed by SQLSTATE, with the message kept alongside, because `M11.6`'s two
  interesting outcomes share the code `53300` and differ only in what the
  message names. Unbounded growth is the hazard to avoid: the map is keyed by
  code, of which there are a handful, rather than by message.
  Acceptance: the report has a per-SQLSTATE breakdown that sums to `errors`
  plus `relocations`, a test that a run against a target answering a known code
  reports that code, and `pgprox-load` and `bin/pgload` both still hold 95%.
  Done, and the one test that failed before it passed is the finding.
  **The two `53300`s cannot be told apart from the client side, and that is the
  security posture working rather than a gap.** The test was written asserting
  that the message naming `primary:5432` and a cap of 60 reached the report; it
  reached "too many connections, please retry" instead.
  `ClientError::client_message` is vague on purpose, and its doc comment says
  why: an untrusted client must not learn upstream hostnames or the connection
  cap. So a node refusing at its own client ceiling and a fleet refusing at its
  upstream cap send the same code and the same sentence.
  That does not weaken the breakdown, it relocates half of the answer. The code
  distribution is a client-side fact and says how many were refused against how
  many timed out, which is the `53300` against `57014` distinction the scope
  note called the interesting one. Which refusal produced a given `53300` is a
  node-side fact, and `M11.6` has to read the node's own view for it: the
  ceiling refusal logs `refused a client: at the connection ceiling` at warn,
  and the pool refusal does not.
  Messages are kept anyway, capped at eight distinct ones per code, because a
  run against Postgres directly sees the server's own vocabulary and because
  `57014` carries the wait in its text, which is unbounded. The cap drops
  messages and never failures: merging keeps the count of the ones it had no
  room for, which has its own test.
  Relocations are recorded here beside errors rather than left out. `57P01` is
  something a client was told, and a document that says what clients saw with
  the drain code missing from it would have a hole exactly where a drain is.
- [x] `M11.9` The cluster view keeps counting a node that is gone. Found by
  `M11.6` rather than by review.
  `/v1/servers` reports `in_use` as the sum of what every node last gossiped.
  A node killed outright never gossips again, so its last reading stays in that
  sum with nothing to expire it. Eight seconds after a kill the survivors have
  leased up, their new numbers arrive, and the corpse's are still underneath:
  89 against a cap of 60, and it stays there. `/v1/stats` has the same shape,
  reporting 1,326 clients where there were 1,000.
  Nothing is over-subscribed. `pg_stat_activity` says 50 and never above 60 in
  any sample of any run, so the quota accounting is right and the view is
  wrong. That is the reason this is worth fixing rather than urgent: what it
  costs is an operator's trust in the number, at exactly the moment they are
  looking at it because something died.
  The fix is a liveness question rather than an arithmetic one, and the
  liveness machinery already exists: `pgprox-cluster` has membership with a
  heartbeat, and `M11.6`'s logs show `some peers did not answer the gossip
  round` firing 54 times during the window. So a peer that has not been heard
  from for longer than its lease is already knowable, and the view is summing
  without asking.
  Acceptance: a node that has been silent past some stated threshold stops
  contributing to `in_use` and to the client count, a test that pins the
  threshold's behaviour on both sides of it, and the arithmetic in
  `product/perf/run-2026-07-31-admission.md` re-checked against the fixed
  view.
  **Two corrections to the note above, both found by reading before running,
  and both change what the run is looking for.**
  The first sentence of it is wrong. There *is* a client connection cap:
  `max_client_conns`, defaulting to 10,000 and set to 200 in the e2e stack's
  document, enforced by `serve::Gate` and refused after the handshake with
  53300. What appears in this repo only as a misspelled key to be rejected is
  `max_client_connections`, which is the wrong name for the real knob rather
  than evidence that no knob exists.
  So a displaced client has three outcomes, not two, and two of them are 53300.
  The gate's refusal carries `ServerId::new("this node", 0)`, so it renders as
  "upstream this node:0 is at its connection cap of 200"; the pool's carries the
  real server, "upstream primary:5432 is at its connection cap of 60". The
  message is what separates a node that is full of clients from a fleet that is
  full of upstream connections, and both arrive at the client as the same
  SQLSTATE. A run that recorded only codes would merge them.
  The second correction is about what the run can record at all, and it is why
  this task is now blocked on `M11.8`. `pgprox_load::Report` carries `errors`,
  `relocations` and `first_error`, and nothing that says how the errors divide.
  `first_error` is one string from whichever client failed first, which cannot
  answer "which of those two do they get" when the answer is a mixture. That is
  a crate change with a coverage gate on it, so it is a commit of its own, the
  same way `M11.4`'s knob was.
- [x] `M11.10` The pinning workload documents, so `M11.7` has something to run.
  Three variants of the reference workload differing only in the weight of one
  `LISTEN` statement, chosen so the share of a session's life spent pinned
  spans roughly a tenth to nearly all of it. The reference document itself is
  the zero point, so the curve has four values.
  Weights scaled by ten so the small end is expressible: 600/100/250/50 against
  a `LISTEN` weight of 1, 2 and 20. That is the same mix as the reference, since
  scaling every weight by the same factor changes no proportion, plus a
  per-statement `LISTEN` probability of about 0.1%, 0.2% and 2%.
  No schema version bump, for the reason `M11.4` established: a version 3
  document that never says `kind: listen` means exactly what it meant before,
  and the bump is for changes that reinterpret existing files.
  Acceptance: three documents that parse, the crate's committed-document test
  extended to cover them rather than left naming only the reference, and
  `pgprox-load` still at 95%.
  Done. `workload-pin-low`, `-mid` and `-high`, with `LISTEN` weights of 1, 2
  and 20 against the reference mix scaled by ten.
  Four tests rather than one, and two of them are the ones worth having. That
  the three documents differ from the reference and from each other in exactly
  one weight, since a curve where two things moved is not a curve in one
  variable. And that the sampled stream actually carries `LISTEN` at the
  declared rate, asserted relatively rather than absolutely because the three
  rates differ by a factor of twenty and one absolute tolerance suiting the
  largest would pass a smallest of zero. A document whose `LISTEN` never
  reached the stream would produce a run with no pins in it, which reads as
  "pinning costs nothing" and is the reference workload wearing another name.
  `M11.4` left one invariant vacuous and this fills it. The sampler has a rule
  that no `LISTEN` is ever marked replica-eligible, and it could not be checked
  against anything, because no committed document held a `LISTEN` statement.
  It is checked against `workload-pin-high` now.
  One fact about Postgres was checked before committing rather than after,
  because half of every document's statements go through the extended protocol
  and a `LISTEN` that could not be parsed there would break half the run.
  SQL-level `PREPARE ... LISTEN` is a syntax error; protocol-level `Parse` of
  the same statement is fine, which is the path the load client uses.
  Fixed, and it is three lines because the design had already decided where
  they go. `DigestStore` says in its own module comment that it will not filter
  by liveness, and gives the reason: one source of the view means a caller
  cannot pick the wrong one. `Coordinator::forget` drops a node from both the
  liveness map and the digest store, and is called on an explicit leave
  announcement. `Membership::reap` drops dead nodes from liveness alone.
  So the hole is precisely the case with no announcement. A node killed
  outright is reaped from liveness and its digest is never mentioned to
  anybody, and every cluster-scoped sum keeps adding it. `reap` now returns the
  nodes it dropped and `observe` forgets their digests in the same breath,
  which keeps the single source of liveness the module comment asks for.
  `reap`'s own doc comment said "housekeeping only: `view` and `alive_count`
  already ignore them, so skipping this cannot make a dead node count". The
  first half is still true and the last clause was not, for the one consumer
  that does not go through `view`. It says so now.
  The test that matters is the one that fails without the fix, and it had to be
  written with digests that report a non-zero count: the crate's usual
  `digest_for` helper reports zeros, and a sum over zeros reads the same
  whether or not a corpse is in it. Two tests either side of the behaviour, so
  a reap that dropped a live node's digest is caught too, which is the same
  defect pointing the other way and worse: it reports headroom that is not
  there.
  `scripts/gates/m3-complete.sh` still passes, which is the check that matters most
  here: the quota invariant is proven over randomized schedules with partitions
  and leader loss, and this changes what those schedules do to the store.
  One refinement the first version needed, and `bin/pgprox`'s own suite is what
  found it. A node ages out of its own liveness on purpose, so that one whose
  loop has wedged stops leading, and forgetting digests on reap therefore made
  a node forget *itself*: `run::tests::a_client_whose_tenant_belongs_elsewhere_is_shed`
  gossips for forty-four seconds without ticking, so the local node went dead
  in its own view and dropped its own tenant usage, and the tenant it homed
  read as having no headroom. That is the defect the `heartbeat` comment
  already describes, arriving from the other direction. The local node is
  skipped, with its own test, because ageing out of liveness is a statement
  about the loop and not about the data.
- [x] `M11.11` Close M11. All eleven tasks are done, so the section header, the
  status table and the milestone summary have to say so.
  Filed after the fact rather than before, which is the wrong order and is
  recorded here rather than tidied away: the commit hook checks that a subject
  matches the task pattern, not that the task exists, so `M11.11:` passed with
  nothing behind it. The entry exists now because the rule is one task, one
  commit, and a commit whose task is invented in the subject line is a commit
  with no task.
  The summary says what the four measurable questions found rather than that
  they ran, because a milestone that records only its own completion is the
  thing this whole milestone was written to correct. Two of the four changed
  the answer that raised them: `M11.1` found the cache raises fleet throughput
  4.11% where `M10.9` declined to claim it did anything, and `M11.3` found that
  shedding cannot fire at the connection cap at all. In both, reading the code
  beat running the experiment.
  The status table also had a blank line between the `M10` and `M11` rows,
  which splits it into two tables in any renderer.

## M12: the gates that count files

- [x] `M12.0` Plan M12: audit what every gate actually asserts.
  The audit rather than the suspicion, because `M11` was a milestone of
  suspicions that turned out to be half wrong and the half that was right was
  only visible after reading. Every `compgen -G` in a gate was run against the
  working tree and the match count compared with what the check then claims.
  Nine result globs across four gates. Two are honest, `m9-complete.sh`'s ADR
  glob and `m7-complete.sh`'s benches glob, where the pattern and the claim are
  the same thing. The rest report a conclusion the pattern cannot support.
  Also audited: every `fail` reachable from a pipeline's right-hand side, which
  is the shape that made `M11.7`'s first replacement print `FAIL` and exit 0.
  Zero sites today. That is worth a lint rather than a shrug, because the reason
  it is zero is that it was found by accident once.
  The milestone is those findings and nothing else. No task here is a feature.
- [x] `M12.1` `check-commit-msg.sh` accepts a task ID with no task behind it.
  Its own comment says the subject "references the backlog task so history stays
  traceable to the plan", and it checks only that the ID is well formed.
  Found by committing `M11.11:` when no `M11.11` existed. The hook passed. The
  entry was filed afterwards, which is backwards, and `M11.11` says so.
  Acceptance: the hook resolves the ID against `product/backlog.md` and fails
  when it is absent, with a negative test that proves it fails. The pattern
  check stays, because an ID that is absent and an ID that is malformed are
  different errors and should read differently.
  Care needed on two cases the pattern currently carries for free: the
  mechanical subjects `Merge`, `Revert`, `fixup!` and `squash!` must stay
  exempt, and a task's own filing commit necessarily adds the entry it
  references, so the check has to read the backlog as staged rather than as
  committed.
  Done. The hook resolves the ID against `product/backlog.md` read from the
  index, `git show :product/backlog.md`, so a task's own filing commit resolves
  and an entry left unstaged does not. Verified both ways directly: the same
  subject fails against the working tree alone and passes once the entry is
  staged. The fallback to the working tree says which source it used, because a
  check that silently degrades to a weaker check is this milestone's subject.
  **`M11.11` was the third instance, not a one-off.** Running the tightened hook
  over all 321 commits rejects two more: `M1F.0: plan full protocol coverage
  against three reference proxies` and `M-1.18: close M-1, unblock M0`. Neither
  task is in the backlog. Both are the same shape as `M11.11`, a milestone's
  planning or closing commit inventing an ID for work the backlog never listed,
  which is why the check now exists. `M12.10` reconciles the two entries.
  The negative test is `tests/gates/negative.sh`, which is the more important
  half. Six cases: three that must fail, including the well-formed-but-absent
  ID that motivated the task, and three that must keep passing, a real task and
  the mechanical `Merge` and `Revert` subjects git writes itself. It asserts
  exit codes, never output, for the reason in its header: `M11.7`'s bug printed
  the right message in red and exited 0.
  It is wired into CI ahead of the twelve gates, and into `check-drift.sh`'s
  wired-into-CI list, so dropping it fails the pre-commit hook. `M12.7` extends
  it to the rest of the gates.
- [x] `M12.2` `m7-complete.sh` counts every run document in the repo and calls
  the total scale runs. It reports "a scale run is recorded (16 file(s))" from
  `product/perf/run-*.md`, of which five are scale runs and eleven are cache,
  admission, throughput, saturation and pinning documents. The check passes with
  none of the five present.
  Its own comment already names the right assertion: "a recorded
  1000-connection run is what M7 asks for". So assert that, not a file count.
  Acceptance: the check identifies a run at a stated connection count, and a
  negative test shows it failing when only unrelated run documents exist.
  Done. The check reads each `run-*.md`, keeps the ones whose title declares a
  scale run, pulls the connection count out of the run's own summary table, and
  requires the largest to reach `M7`'s thousand. It reports "3 run(s), the
  largest at 100000 connections" where it used to report sixteen files.
  **The count in `M12.0` was wrong and the check is what corrected it.** That
  entry said five of the sixteen were scale runs, by eye. Three declare
  themselves so. The two others, `run-2026-07-28-100k-hold.md` and
  `run-2026-07-28-2000-cpu.md`, measure at a connection count without being
  scale runs, which is a real distinction and not one an eye reading filenames
  makes reliably. The roadmap is corrected rather than left, because a number
  and the check that measures it cannot both stand.
  The gate takes `PGPROX_PERF_DIR` so the four negative cases hand it a
  purpose-built directory rather than moving the real documents aside. A test
  that mutates the tree it tests leaves the tree broken when interrupted, and
  this suite runs in CI. The cases: an empty directory, a directory of run
  documents that are not scale runs, which is the actual regression, a scale run
  below the required count, and the shape that must still pass.
  One bug in the test itself, worth recording because it is the same class the
  milestone is about: `set -euo pipefail` turned a document written without a
  connection count into a silent abort, since the `[[ ]] &&` was the writing
  group's last command and returned 1. The suite stopped after one case and
  reported nothing wrong about the rest.
- [x] `M12.3` `m9-complete.sh` globs `product/perf/run-*cache*.md` and reports a
  recorded run. Three files match. The same shape as `M12.2` and it needs the
  same treatment, with the difference that M9's claim is a number with a sign:
  the cache costs 7.8% of the median and, by `M11.1`, raises fleet throughput
  4.11%. A check that reads the file can check the numbers are still there.
  Acceptance: the recorded figure is read rather than the filename matched, and
  a negative test proves the check fails without it.
  Done, and the check is stronger than the acceptance asked for. Rather than
  hard-coding 7.8%, it reads the figure out of the roadmap's own `M9` row and
  requires a recorded cache run to contain it. The tie then holds in both
  directions: re-measure the cache and the gate fails until the roadmap is
  updated, edit the roadmap's number and it fails until a run supports it.
  Neither can drift from the other quietly, which is the failure `M9` and
  `M11.1` between them already demonstrated is possible.
  Reports "the roadmap's 7.8% is backed by a recorded run
  (run-2026-07-30-cached-workload.md)" where it used to report a file count.
  Four negative cases: a cache run that does not record the figure, which is the
  regression; a roadmap figure the runs no longer support; a roadmap row that
  states no figure at all; and the tie intact.
  One cost worth naming before `M12.7` runs into it. The suite now takes 37
  seconds, nearly all of it `m9-complete.sh` invoking `check-crate.sh` and so
  `cargo fmt` and `clippy`, four times over. Extending this to twelve gates by
  running each gate whole, several times each, will not stay in that budget.
  `M12.7` has to either run each gate once per broken artefact with the
  expensive checks skipped, or accept a tier-3 runtime and say so.
- [x] `M12.4` `m11-complete.sh` globs `product/perf/*admission*.md`. One file
  matches and the check reports what a full fleet tells displaced clients, which
  is a claim about `53300` that the filename cannot carry.
  Acceptance: assert the SQLSTATE the run recorded, with a negative test.
  Done. The check requires a run that names both `53300` and `57014`, because
  `M11.6`'s entire result is which of the two refusals the pool distinguishes a
  displaced client sees. A run naming one has answered half the question and a
  run naming neither has answered none of it, whatever the filename says.
  Three negative cases: no admission run, a run naming no SQLSTATE, which is the
  regression, and a run naming only `53300`. Only the admission check reads
  `PGPROX_PERF_DIR`, so the gate's other checks keep reading the real artefacts
  and a failure in these cases is this check and nothing else.
  **It caught a wrong claim on its first reading, in prose written one task
  earlier.** `M11.11`'s roadmap summary said `M11.6` "measured what actually
  happens to displaced clients, which is `53300`". The run's own headline is the
  opposite: the answer is nothing, they are served in about a seventh of a
  second, and neither `53300` nor `57014` reaches a client at any point. The
  roadmap is corrected and says where the error came from, rather than being
  quietly rewritten.
- [x] `M12.5` `m1f-complete.sh` globs `product/decisions/*protocol-3-2*` and
  `product/decisions/*replication*` and reports that scope is "a recorded
  decision rather than an omission". An ADR that decided the opposite, or an
  empty file with the right name, passes both.
  Acceptance: each check reads the ADR's status and decision, with a negative
  test for each.
  Done. Both go through one `adr_decided` helper that finds the ADR, reads its
  `Status:` line, and requires it to be accepted. It reports which file and what
  status, so the evidence is in the output rather than implied by a green tick.
  The convention it relies on was checked rather than assumed: all twenty-two
  ADRs in `product/decisions` carry a `Status:` line and all twenty-two are
  accepted, so requiring it adds no new burden. `0012` reads "accepted, with one
  item outstanding", which is why the test is a prefix match rather than
  equality.
  Four negative cases: neither ADR present, empty files with the right names,
  which is the regression, an ADR still marked proposed, and both decided.
  This is the task that found `M12.11`. Proving the check fails needs four
  invocations of `m1f-complete.sh`, and each was 84 seconds because the gate
  runs the whole workspace coverage gate, so the suite timed out before it
  could report. `M12.11` was split out and done first.
- [x] `M12.6` A lint for `fail` reachable inside a pipeline subshell, so the
  near miss in `M11.7` cannot come back. The counter `fail` increments lives in
  the parent, the right-hand side of a pipeline is a subshell, so such a gate
  prints `FAIL` and exits 0. A gate that cannot fail is worse than no gate.
  Zero sites today, which is the reason to write it now rather than a reason
  not to: nothing found the one that existed except luck.
  Acceptance: the lint is wired into `check-drift.sh` alongside the other
  repo-wide rules, it flags a deliberately planted site, and it does not flag
  `|| { fail ...; }`, which is a brace group in the current shell and is the
  dominant idiom in `scripts/`.
  Done, and proven against the real bug rather than a lookalike. `M11.7`'s
  broken version was reconstructed in `m11-complete.sh` and the lint flagged all
  five of its `fail` calls with the line the pipeline opened at, and the tree
  went back to green when it was reverted.
  The rule arms on a pipe that is not `||` or `|&` followed by a block opener,
  and disarms on the line that closes the block. That is a heuristic and is
  written down as one: it is the rule that catches the shape this repo actually
  produced, not a shell parser. It scans `scripts/*.sh` and `tests/gates/*.sh`
  through `PGPROX_SHELL_ROOTS`, so the negative cases plant files in a temp
  directory instead of writing a deliberately broken script into `scripts/`.
  Three cases: the exact `| { read; case; fail }` shape, a `| while read` loop
  with a `fail` in its body, and the `|| { fail ...; }` idiom, which must keep
  passing. That last one is not a formality. It is how most of `scripts/`
  reports failure, and a lint that flagged it would be switched off within a
  day, so the whole rule turns on the distinction between a pipe and an or.
  `shopt -s lastpipe` was considered and rejected in the comment: it needs job
  control off and covers the last stage only, so it would replace a visible
  rule with an invisible one.
  The lint's first version flagged its own fixtures, which is a true positive on
  text that is an example of the bug rather than the bug. The fix is that a
  heredoc body is data and not code, so the rule now tracks heredoc terminators
  and skips their contents. Moving the fixtures somewhere unscanned would have
  worked too and would have been worse: it would leave the lint unable to read
  the one file most likely to contain the shape it looks for. Re-verified after
  the change by replanting the real bug, which it still catches, all five
  lines.
- [x] `M12.7` Prove each gate can fail. Every `mN-complete.sh` is trusted to
  report a milestone's completion and not one of them has ever been observed
  failing on this tree.
  The method is the one that found `M11.7`'s subshell bug: break the artefact,
  run the gate, assert a non-zero exit. Not the output, the exit code, because
  the bug that motivates this printed the right output.
  Sized on inspection rather than guessed: twelve gates, so this is a harness
  plus a table of one broken artefact per gate, and it is a commit only if the
  harness is small. If it is not, it splits by gate and this entry says so.
  It did not need to split. The harness is one loop, because there is a method
  that breaks every gate's artefacts at once: copy `scripts/` into an empty
  directory. `lib.sh` derives `REPO_ROOT` from its own location, so the copy
  looks out at a tree with no crates, no `product/`, no `deploy/`, and every
  check has something to object to. Nothing in the real tree is touched.
  Thirteen gates covered, the twelve `mN-complete.sh` plus `release-check.sh`,
  which is M8's. The loop globs, so a gate added later is covered without anyone
  remembering to add it.
  What this proves and what it does not, stated plainly. It is a floor: every
  gate exits non-zero when the thing it checks is absent. It is not a proof that
  each individual check inside a gate can fail. The targeted cases for `M12.2`
  through `M12.6` do that for the five checks this milestone rewrote, and the
  rest of the checks in those gates have the floor and nothing more.
  The loop had a bug worth keeping, because it is this milestone's own subject
  one level up. Its first version reported all thirteen gates exiting 0 while
  all thirteen were exiting 1, because it read `$?` in the same `printf` as
  `$(basename ...)`, and the command substitution runs first and replaces it.
  A harness reporting success for a failure it had measured correctly is
  `M11.7`'s bug wearing different clothes. The comment in the file says so, so
  the loop does not get rewritten that way.
- [x] `M12.8` Write `scripts/gates/m12-complete.sh` before the milestone needs
  closing, which `M10.17` and `M11.5` both establish as the order. It has to
  avoid being an instance of its own subject: no check in it may glob for a
  filename and report a conclusion.
  Seven checks, and none of them looks for a file. The first runs
  `tests/gates/negative.sh`, because that suite is the milestone's deliverable
  and a gate that described it rather than ran it would be the exact defect.
  Two run `check-commit-msg.sh` against a fake ID and a real one, so the check
  is on behaviour rather than on the source. One resolves every task ID in
  history against the backlog in a single pass. One plants the pipeline-subshell
  shape in a temp directory and requires the lint to object. One reads `ci.yml`
  for a discarded failure. The last asserts the four globs this milestone
  removed have not regrown, which is the device `M11.3` used for its roadmap
  sentence, and is the only one that is a pattern match rather than a run.
  Two things went wrong writing it, both worth keeping.
  Adding the gate made `check-drift.sh` fail, because a new `mN-complete.sh`
  that is not named in `ci.yml` is a gate nobody runs. That rule was written in
  `M10.1` and it fired correctly on the first new gate since.
  And the `continue-on-error` check failed on `M12.9`'s own comment explaining
  why the flag was removed. It matched the word instead of the construct, which
  is this milestone's defect one layer up, in the gate written to detect it. It
  strips comments now.
  It is covered by its own `M12.7` floor without anything being added, because
  that loop globs: fourteen gates now, up from thirteen.
- [x] `M12.9` CI runs the `M11` gate with `continue-on-error: true`, so it
  cannot fail the build. That was right while the milestone was open, which is
  what the comment beside it says, and `M11` closed in `M11.11`.
  A gate that cannot fail is this milestone's subject, so leaving it is not an
  oversight to fix quietly but an instance of the thing being fixed.
  Acceptance: the step enforces, and the comment explaining why it did not is
  replaced rather than left to confuse the next reader.
  Done. The flag is gone and the comment now says why it was there and why it
  is not: writing the gate early meant watching it go green a check at a time,
  which stopped being a reason when the milestone closed.
  Taken out of backlog order, immediately after `M12.1`, because both are the
  same defect. `M12.1` fixed a check that could not fail on a bad input; this
  fixed a check whose result was discarded. A gate that cannot fail and a gate
  whose failure is ignored are the same gate.
  No other step in `ci.yml` carries `continue-on-error`, checked rather than
  assumed. The `M11` gate reads committed artefacts and greps `shed.rs`, with
  no Docker or network, so enforcing it does not make CI depend on a stack.
- [x] `M12.10` Two commits in history reference tasks the backlog never had:
  `M1F.0` and `M-1.18`. Found by `M12.1`, by running the tightened hook over all
  321 commits rather than by review.
  Both are the shape `M11.11` was: a milestone's planning or closing commit
  inventing an ID for work that was real and never listed. The work exists and
  is described in the commits themselves.
  Acceptance: both entries filed, marked done, each saying that it was written
  after the fact and how it was found. Not backdated and not disguised, because
  a backlog that hides its own gaps is worth less than one that records them.
  Done. Both entries are filed in their own milestone's section, in position,
  each saying that `M12.10` wrote it and that the hook accepted the original
  commit because it checked the shape of the ID rather than whether anything
  answered to it.
  `M1F.0`'s work is visible without the entry, because its result is the M1F
  section header: the three references, the diff, and the two findings that
  shaped the milestone. `M-1.18` closed M-1 with one item outstanding, and that
  item is still outstanding and still marked `[~]` on `M-1.17`, so the closure
  reads honestly now as it did then.
  With these two filed, every task ID in all 321 commits resolves. That is
  checked by the hook rather than claimed: running it over the full history
  rejects nothing.
- [x] `M12.11` The negative suite cannot afford to run the gates. Found by
  `M12.5`, which needed four invocations of `m1f-complete.sh` at 84 seconds
  each because that gate runs the whole workspace coverage gate.
  `M12.3` already recorded the shape of this and put it against `M12.7`. It
  arrived two tasks earlier, which is the usual reason to deal with it now
  rather than at the end.
  Nine gates delegate to `check-crate.sh` and `check-coverage.sh`, so the fix
  has one change point rather than nine: those two scripts honour a skip flag
  and the suite sets it. What the negative cases exercise is a gate's own
  logic, and re-running `cargo` once per case adds nothing to that. CI runs
  both scripts in tier 1 regardless, so nothing goes unchecked.
  Acceptance: the suite runs in seconds, the flag is loud when it fires, and
  `check-drift.sh` fails if CI or the pre-commit config ever sets it, because a
  knob that turns off the coverage gate is exactly the kind of thing this
  milestone exists to be suspicious of.
  Done. `PGPROX_SKIP_DELEGATED_CHECKS` makes both scripts exit 0 before doing
  anything, announcing itself on stderr, and `tests/gates/negative.sh` exports
  it. The suite went from over five minutes to 57 seconds, and the remaining
  time is the gates' own work rather than cargo.
  The guard is the part that matters and it is verified, not asserted: adding
  the variable to `ci.yml` makes `check-drift.sh` exit 1 with the reason, and
  removing it makes it exit 0 again. Both were run.
  The honest risk is stated rather than hidden. This is a switch that turns off
  clippy and the 95% coverage gate. It is safe because CI runs both in tier 1
  independently of any milestone gate, so the milestone gates re-running them
  was duplicated work rather than the only coverage of them, and because the
  drift check refuses to let the variable appear in `ci.yml` or the pre-commit
  config at all.
- [x] `M12.12` Close M12. Filed before the commit that does it, which is the
  order `M11.11` got wrong and `M12.1` now enforces.
  The status table and the section header say so, and the section says what the
  milestone found rather than that it ran.

## M13: the non-negotiables that nothing enforces

- [x] `M13.0` Plan M13: audit the seven non-negotiables against the scripts that
  are supposed to enforce them.
  One rule at a time, checked by running things rather than by reading
  `AGENTS.md` and believing it. Three hold, four do not, and the table is in
  `product/roadmap.md`.
  The sharpest is rule 2. `COVERAGE_MIN=10 scripts/check-coverage.sh
  pgprox-route` prints `ok coverage (pgprox-route): 99.65% >= 10%` and exits 0.
  The gate announces its own weakened threshold and passes.
  One of the four defects was introduced by `M12.2`, which added
  `PGPROX_SCALE_MINIMUM`, in a milestone about checks that do not check. Filed
  as a finding rather than fixed quietly, because the useful fact is that this
  class is easy to reproduce while looking straight at it.
- [x] `M13.1` The three pass/fail thresholds can be lowered from the
  environment: `COVERAGE_MIN` at 95, `BENCH_TOLERANCE` at 5 and
  `PGPROX_SCALE_MINIMUM` at 1000.
  The distinction that matters, and the reason this is not "remove every
  override": most `${X:-n}` defaults in `scripts/` are run parameters, and
  overriding a connection count, a duration, a seed or a port is exactly what
  they are for. These three decide whether a check passes. Those are different
  things and only the second kind is a threshold.
  Acceptance: the three become constants that no environment can move, with a
  negative test showing the override no longer takes effect, and `check-drift.sh`
  refuses to let a pass/fail threshold be reintroduced as a settable default.
  Care needed: `check-coverage.sh` legitimately needs a way to run at a
  different figure while writing tests. If that stays, it has to be an argument
  that reports loudly and cannot be reached by an exported variable, so a stray
  environment does not silently weaken CI.
  Done, and the care turned out to be unnecessary. No replacement knob is
  needed, because `check-coverage.sh` already prints the measured percentage:
  anyone who wants to know where a crate stands reads that number rather than
  moving the line it is compared against. All three are plain constants.
  The drift rule refuses to let any of them come back as `${NAME:-n}`, and it is
  scoped to pass/fail thresholds by name rather than to every settable default,
  because most `${X:-n}` values in `scripts/` are run parameters and overriding
  a duration, a seed or a port is what they exist for. A rule that flagged those
  would be turned off rather than obeyed.
  Three cases in the negative suite: the property itself, that an exported
  `COVERAGE_MIN` does not reach the gate; a reintroduced settable threshold,
  refused; and a run parameter, left alone.
  Two mistakes while writing it, both caught rather than reasoned away.
  The rule first flagged `lib.sh` on the comment explaining why `COVERAGE_MIN`
  is now a constant. That is `M12.8`'s mistake exactly, matching text that looks
  like the thing rather than the thing, made again one milestone later. It
  strips comments now.
  And moving the `SHELL_ROOTS` definition put it after the subshell lint that
  uses it, so that lint scanned an empty list and passed. `check-drift.sh`
  reported all-green while one of its rules had stopped looking at anything.
  `tests/gates/negative.sh` caught it, which is the first time the suite has
  failed on a regression rather than on a deliberately broken artefact, and is
  the clearest argument for `M12` that this milestone could have produced.
- [x] `M13.2` Nothing detects a deleted test. Rule 2's second half.
  Sized on inspection before starting: a count is the obvious thing and is the
  wrong thing, because a commit that deletes one test and adds another passes it
  while doing exactly what the rule forbids.
  Acceptance: a check that names what disappeared, not one that compares totals.
  If that turns out to need a committed inventory of test names, this splits:
  the inventory and its drift check are one commit, the enforcement another.
  It did not need an inventory and so did not split. `git` already holds the
  previous state: the check reads test names out of `HEAD:<file>` and out of
  `:<file>`, the staged version, and reports the set difference by name. A
  committed inventory would have been a second copy of something git already
  stores, and a file that has to be updated by hand is a file that gets updated
  by hand until someone updates it wrongly.
  A rename reads as one removal and one addition, which is what it is, and is
  the case a count passes while the rule is being broken. Tested.
  The escape hatch is a line in the commit message, `Removes-test: <name>`.
  Deleting a test is ordinary work when what it covered is gone; what the rule
  forbids is deleting one to make a check pass, and no script reads intent. What
  a script can do is refuse to let it happen silently, and a commit message is
  the one place a declaration travels with the change forever. Deliberately not
  an environment variable and not a flag, for `M13.1`'s reason: a switch is
  something a later run sets by accident, a message is written once by hand.
  The extractor was checked against the tree rather than trusted: 1,666 test
  attributes in the workspace and 1,666 names extracted, so it loses none.
  Five negative cases, each in a throwaway git repository so the real tree is
  never staged against: an undeclared removal, a declared one, a rename, a
  deleted file that held tests, and added tests, which must not be objected to.
  One limitation, stated rather than left to be discovered. This runs at the
  `commit-msg` stage, so it is a local hook and CI cannot run it: CI has no
  commit message to read. A PR-level equivalent would have to walk the commits
  on the branch. Not done here, and not pretended to be done.
- [x] `M13.3` Rule 7, credentials never reach a log, is a repo-wide claim held
  up by one unit test in one crate.
  Acceptance: a check that covers the claim's actual scope. What that means has
  to be settled first and written down: candidates are a lint against passing
  secret-bearing types to a logging macro, a test per crate that holds one, and
  a run of the e2e stack grepping its logs for the token it authenticated with.
  The last is the only one that tests the claim end to end and it is the
  slowest, so the decision is which of the three the rule actually needs.
  Settled by reading the design rather than by preference. `SecretString`
  carries every credential and cannot be printed: `Debug` and `Display` both
  render `[redacted]`, and it has no `PartialEq`, `Deref`, `AsRef<str>` or
  `From` back to `String`, each left out on purpose. So there is exactly one
  route to a real value, `expose()`, named that way because it greps.
  That makes the leak path one shape, and `expose`'s own documentation already
  states the rule: never pass it to a formatter that reaches a log, a span
  attribute, a metric label, or an error variant. Nothing enforced that
  sentence. `scripts/check-secrets.sh` does, over the whole workspace, in 0.24
  seconds, so it runs on every commit and in CI rather than per crate.
  A field-type rule was considered and rejected: requiring every field named
  `token` or `password` to be a `SecretString` false-positives on the generated
  gRPC request type, whose `token` field is a plain `String` by construction.
  **The first version of the lint caught nothing and reported the workspace
  clean.** It used `` in the macro pattern, which is a GNU extension this awk
  does not have, so `opens` was false on every line. It printed
  `ok no exposed credential reaches a formatter (133 file(s))` and exited 0.
  That is `M12`'s defect written by the person who spent a milestone on it, in
  the commit that adds a security check. It was found by planting a leak and
  watching nothing happen, which is the only reason this task is not a lie.
  Three negative cases: a single-line leak, a multi-line `tracing::info!` leak,
  which is how tracing is usually written and which a line-based rule misses,
  and the safe uses that must not be flagged.
  The end-to-end half is `M13.8`, split out on inspection: this lint cannot see
  a value exposed into a local and formatted three functions later.
- [x] `M13.4` Rule 5 says business logic is sans-I/O and `check-layering.sh`
  checks the crate dependency rule, which is a different property.
  Acceptance: either a check for the stated property, or `AGENTS.md` reworded so
  the rule says what is enforced. Deciding which is the task, and the decision
  goes in the entry with its reasoning.
  The property is checkable, so it is checked rather than reworded, and the
  decision came out of an audit rather than a preference.
  `product/architecture.md` gives the mechanical shape: "The I/O shell that
  wraps it is generic over `AsyncRead + AsyncWrite + Unpin`." So a concrete
  socket type named inside a library crate is the violation and the generic
  bound is not. The tree already satisfies that completely: `pgprox-session`
  holds the entire I/O shell and names no concrete socket type anywhere.
  The clock half is stronger than expected. Across every library crate there are
  109 `now()` calls and **all but six are in test code**. Of the six, four are
  inside `pgprox-core/src/clock.rs`, which is the injection point that exists so
  nothing else reads a clock, and two are `tokio::time::Instant::now()` in
  `pgprox-session/src/shell.rs`'s buffer-wait deadline.
  Those two are not violations and the distinction is earned, not excused.
  `tokio::time` is the runtime clock, which `#[tokio::test(start_paused = true)]`
  makes virtual, so it costs nothing in determinism, and the tests that drive
  that path do pause time. The rule is about non-determinism, and a clock the
  test controls is not a source of it.
  So the rule already held everywhere. What was missing was anything that would
  notice if it stopped, which is the same finding `M12` kept producing.
  Three exceptions, each named with a reason rather than listed: `src/bin/*` is
  a composition root, `pgprox-auth/src/client.rs` is the sidecar adapter ADR
  0003 chose, and `pgprox-core/src/clock.rs` is the one place allowed a clock.
  `bin/` is not scanned at all, because holding the concrete types the libraries
  are generic over is what a composition root is for.
  `AGENTS.md` is corrected too: rule 5 credited nothing, and the checks list now
  names `check-sans-io.sh` and `check-secrets.sh`. 0.12s over 86 files, so it
  runs on every commit and in CI.
  One bug worth keeping: an apostrophe in a comment inside the single-quoted awk
  program closed the quote, and bash then tried to parse awk. It is the third
  time in two milestones that the shape of a check broke on quoting or on a
  regex dialect rather than on its logic.
- [x] `M13.5` Rule 6 says a core trait change updates the trait, every fake,
  every implementation and the ADR in one commit. `m0-complete.sh` checks that
  every public trait has a fake, which is the static half and not the rule.
  Acceptance: a check on the commit, since that is what the rule is about. It
  has the same shape as `M12.1`'s: read the staged change, and if it touches a
  trait in `pgprox-core`, require the fakes and an ADR alongside it.
  Done, with two of `standards/contracts.md`'s six items enforced and the other
  four left to the skill and to review, deliberately and with the reason stated
  in the script. Every implementation and the ADR are mechanical; call sites and
  dependent specs are not distinguishable from ordinary edits, and a rule that
  guessed at them would be routed around.
  The fakes item needed no check: every fake in this repo lives in the same file
  as its trait, so the trait file being staged already implies it.
  **It fires on the trait's method set, not on the file.** Editing a doc comment
  on a trait is not a contract change and must not demand an ADR. A rule that
  demanded one would be noise and would be switched off, which is a worse
  outcome than any single missed violation, so the check compares the `fn`
  signatures inside each `pub trait` block between `HEAD` and the index and
  stays quiet unless that set actually differs. Tested.
  Implementor matching is `impl Trait for` or `impl pgprox_core::module::Trait
  for` and no other path, because `impl pb::credential_resolver_server::
  CredentialResolver for MockSidecar` is the generated gRPC service, a different
  trait sharing a name. Matching it would have demanded an unrelated file in
  every `CredentialResolver` change.
  Four cases: a trait grown with its implementor left behind, the implementor
  staged but no ADR, the whole change, and a doc comment alone.
  Same limitation as `M13.2` and stated the same way: it reads the index, so it
  runs at pre-commit and CI cannot run it, having nothing staged. A PR-level
  equivalent would diff against the base branch. Not done, not pretended.
- [x] `M13.6` Whatever remains unenforceable gets said plainly in `AGENTS.md`.
  Rule 3, never claim a test passes without having run it, is the likely
  candidate: it is a rule about honesty in reporting and it may have no script.
  A rule that cannot be scripted should sit under a sentence that says so,
  because a false claim about enforcement is worse than an honest claim about
  intent, and this whole milestone exists because one sentence claimed seven.
  Acceptance: the sentence introducing the non-negotiables matches the audit,
  and `m13-complete.sh` checks the ones that are claimed to be enforced.
  Done for the wording; the gate is `M13.7`.
  The sentence said "Each is enforced by a script, not by good intentions" and
  four of the seven had no script or had the wrong one credited. It now says six
  of seven, names the script beside each rule, and marks rule 3 as the one that
  cannot be enforced. It also records what the audit found, so the next reader
  learns that the sentence was once wrong rather than trusting the new one on
  the same faith.
  Rule 3 was the candidate and it is the right one. Nothing can check a claim
  against an intention: "never claim a test passes without having run it" is a
  rule about what you say. It stays in the list, marked, because it is what the
  other six rest on. A green gate reported by someone who did not run it is
  worth less than no gate, which is a sentence this session has earned the right
  to write: `M13.3`'s first lint reported the workspace clean while matching
  nothing at all.
  Rules 6 and 7 are marked partial rather than done, which is the same honesty
  in smaller print. `check-core-contract.sh` holds two of `contracts.md`'s six
  items, and `check-secrets.sh` holds the static half of rule 7 with the
  end-to-end half filed as `M13.8` and not pretended to exist.
  A drift rule keeps the credits honest: every `scripts/*.sh` AGENTS.md names
  must exist and be executable. Thirteen named today. It cannot tell whether a
  script checks the right thing, which is what went wrong with rule 5, but a
  named script that is not there is the same failure with less ambiguity.
- [x] `M13.7` Write `scripts/gates/m13-complete.sh`, before the milestone needs
  closing, as `M10.17`, `M11.5` and `M12.8` all establish.
  Under `M12.8`'s constraint as well: no check may match a filename or a word
  where it can run something and read an exit code.
  Ten checks, and eight of them plant a violation and require the rule to
  object. That is the only way to know a rule is awake rather than absent, and
  this milestone earned the point the hard way: `M13.3`'s first lint reported the
  whole workspace clean while its pattern matched nothing.
  Two are prose checks and could not be anything else. A gate cannot read
  `AGENTS.md` and judge it, so it checks that the specific wrong sentence, "Each
  is enforced by a script", has not come back, and that rule 3 is still marked
  as having no script. That is `M11.3`'s device, used for the same reason.
  Two more assert the tree itself is clean rather than that the rule works,
  which is a different claim and is worth stating separately: a rule that
  objects correctly to a planted violation tells you nothing about whether the
  real tree has one.
  Covered by `M12.7`'s floor with nothing added, because that loop globs.
  Fifteen gates now, up from fourteen.
- [x] `M13.8` The end-to-end half of non-negotiable 7: run the stack and grep
  its own logs for the token it authenticated with. Split out of `M13.3` on
  inspection, before starting, once the difference between the two became clear.
  `M13.3`'s lint proves the one route `SecretString` leaves open is not taken
  through a formatting macro. It cannot prove a credential never reaches a log:
  a value exposed into a local and formatted three functions later is invisible
  to it, and so is anything a dependency prints.
  Acceptance: `scripts/e2e.sh` authenticates with a known token, and afterwards
  every proxy node's log is searched for it and for the backend password the
  sidecar handed back. Needs Docker, so it is tier 3 and cannot join the
  pre-commit path.
  Done, and it passes: no service logged the client token, the token's signature
  segment, or the backend password, after a run that served every node, ran
  pgbench clean at 160 tps both ways, drained a node with zero failed
  transactions and did 25 write-then-read rounds.
  Three strings rather than one. The whole token, its signature segment alone
  because a log that truncates a JWT still leaks the part identifying the
  session, and the backend password read out of `deploy/docker-compose.yml` so
  it cannot drift from the value the stack actually uses. The service list comes
  from `docker compose config --services`, so a node added later is searched
  without anyone remembering to add it.
  **The check has a positive control, and it is the reason to believe the
  result.** The same three greps run first against a line that does contain each
  secret, and the assertion fails outright if any of them comes back clean,
  because then a clean result on the real logs would mean nothing. That guard
  exists because `M13.3` shipped a lint one task earlier that reported the whole
  workspace clean while matching no line at all.
  Both were run: the first pass without the control, then the control added and
  the whole thing re-run, so the recorded result comes from a search known to
  work rather than from one that had never found anything.
- [x] `M13.9` Close M13. Filed before the commit that does it, which is the
  order `M12.1` enforces and `M11.11` got wrong.
  `AGENTS.md` rule 7 also still said the end-to-end half was "not built yet",
  which stopped being true in `M13.8` one commit earlier. A list that names its
  own gaps has to stop naming the ones it has closed, or it decays into the
  thing this milestone was about from the other direction.

## M14: the crates mutation testing never reached

- [x] `M14.0` Plan M14: audit which crates mutation testing covers against the
  criterion the script states.
  Measured rather than assumed. `mutants.sh` targets "the crates whose logic is
  a pure state machine" and lists four; `M13.4` proved every crate under
  `crates/` is sans-I/O and now enforces it, so the criterion selects all
  fourteen and the list selects four.
  49,725 lines and 857 tests across ten crates have never had a mutant run at
  them, against 37,536 lines and 576 tests that have. Counted, not estimated:
  `cargo mutants --list` gives 280 mutants for `pgprox-cluster`, 273 for
  `pgprox-pool` and 536 for `pgprox-core`.
  Ordered by what a surviving mutant would mean rather than by size.
  `pgprox-cluster` first: it holds the quota invariant that is M3's completion
  condition and the roadmap's headline safety claim.
- [x] `M14.1` Mutation testing for `pgprox-cluster`, 280 mutants.
  The quota invariant, guaranteed plus leased never exceeding the cap, is the
  strongest claim this project makes and 156 tests assert around it. Whether any
  of them would notice the invariant breaking is untested.
  Acceptance: the run completes, every survivor is either killed by a new test
  or carries a written equivalence argument in the baseline, and `mutants.sh`
  passes for the crate.
  Expect this to split if the survivor count is large. One commit per group of
  survivors that share a cause is better than one commit that rewrites a crate's
  test module, and `M10` set that precedent.
  It split. The run found **22 surviving mutants**, 19 distinct baseline keys,
  and they fall into five groups by cause. One task each, `M14.11` to `M14.15`.
  Numbered that way rather than `M14.1a` to `M14.1e`, because
  `check-commit-msg.sh` accepts a letter on the milestone, as in `M1F.1` and
  `M1R.2`, and not on the task number. Widening the pattern to fit a naming whim
  would be the wrong direction: `M14` has seven top-level tasks, so 11 to 15 are
  unambiguous and still read as belonging to `M14.1`.
  | group | file | count | what it is |
  | --- | --- | --- | --- |
  | 11 | `lease.rs` | 4 | the quota invariant: `grant`, `reap`, `holders` |
  | 12 | `service.rs` | 6 | coordinator accessors and a match guard |
  | 13 | `coordinator.rs` | 3 | `heartbeat`, `has_quorum`, `home_draining` |
  | 14 | `digest.rs` | 3 | `is_empty` and two `view_hash` arms |
  | 15 | `sim.rs` | 6 | the deterministic simulator's RNG and network |
  Closed by the full-crate run: **281 mutants, 2 surviving, both in the
  baseline with an argument**, and `scripts/mutants.sh pgprox-cluster` passes.
  Nineteen of the twenty-two survivors were killed by 14 tests. Two are
  equivalent with a stated expiry. The twenty-second, `home_draining`, turned
  out to be reachable only through an inconsistency in `gossip`, which is now
  `M14.16`.
  What the crate holding this project's headline safety claim did not have,
  before this: any test that would notice the quota ledger treating a grant as
  live one instant past its expiry, any test that went through the coordinator's
  own façade rather than around it, any test that would notice a node's second
  heartbeat being discarded, and any test that would notice its simulator
  degenerating to a single schedule.
- [x] `M14.11` The lease ledger's four survivors, which are the quota invariant.
  Three of the four are one shape: **expiry is exclusive**. A grant is live
  while `expires_at > now`, so at exactly `expires_at` it is gone. Every reader
  was written that way and nothing pinned it, so `>` could become `>=` in
  `grant`, `holders` and `reap` with all 156 tests in the crate still passing.
  That instant is reachable rather than theoretical: grants expire at
  `now + ttl`, and a caller computing its next deadline the same way lands
  exactly on it.
  The fourth is `reap` replaced by `()`. It survived because `reap` is
  housekeeping and every other reader filters expired grants out already, which
  the function's own doc comment says. So no answer the type gives changes. What
  does change is that a long-lived leader carries every grant it ever made, and
  that property had no observer.
  Rather than accept it as equivalent, this adds a `#[cfg(test)] fn tracked()`
  returning the map length. A test-only accessor is not a widened public API,
  and "no test can see it" is a reason to give the property an observer when the
  property is real, not a reason to write it into the baseline. The baseline is
  for mutants no test *could* kill; this was one no test *did*.
  Three tests. All 38 mutants in `lease.rs` are now caught, none missed, and the
  crate holds 99.73% coverage with fmt and clippy clean.
  `scripts/mutants.sh` also had to be fixed to get here. It copies the build
  tree once per worker into `TMPDIR`, and `/tmp` on this machine is a 16 GB
  tmpfs against a tree of about 29 GB, so six workers exhausted it and the run
  died partway with `No space left on device` after paying for the build. The
  copies go to the repo disk now.
- [x] `M14.12` The gossip coordinator's six survivors in `service.rs`:
  `track_tenant` and `forget_tenant` replaced by `()`, `view_hash` by `1`,
  `cluster_usage` and `cluster_clients` by `0`, and the `!matches!(err,
  QuotaError::NoLeader)` match guard by `false`.
  The accessors are what `/v1/servers` and `/v1/stats` are built on, and
  `M11.9` was a bug in exactly that accounting, so a mutant that makes them
  return zero unnoticed is worth more than its size suggests.
  All six are one gap. Every method here is a one-line delegation to
  `NodeCoordinator`, that type is thoroughly tested, and nothing went through
  the façade. So the crate had 156 tests and no test that would notice
  `cluster_clients` returning zero.
  Four tests. `service.rs` now has 32 mutants, 24 caught and 8 unviable, none
  missed. The crate holds 163 tests and its coverage gate.
  `track_tenant` and `forget_tenant` needed the same treatment `reap` did in
  `M14.11`: a `#[cfg(test)]` observer of the tracked set. Their effect reaches
  the outside only through reservation decay several gossip rounds later, which
  is too far to pin without a fragile test, and whether a tenant is tracked
  decides whether its reservation ever decays. A reservation that never decays
  strands capacity, so the property is real and deserved an observer.
  Three wrong assumptions, all caught by running the tests rather than by
  reading them back:
  The tenant test first asserted through `report_tenants`, which replaces the
  reported usage outright and never consults the tracked set, so forgetting a
  tenant does not stop it being reported.
  The exhaustion test asked for the whole cap and got half: `guaranteed_fraction`
  is 0.5, so half is held back as guaranteed shares and only the rest is leased.
  Then it tried to exhaust the pool by asking twice from the same node, which
  cannot work, because a holder's renewal replaces its own grant rather than
  adding to it. That is a documented property of `grant` and it took a failing
  test to remember it. The fix is to have another node hold the leasable half
  through `serve_request`.
- [x] `M14.13` `coordinator.rs`: `heartbeat`'s `+=` to `*=`, `has_quorum`'s `>`
  to `>=`, and `home_draining` to `false`.
  `has_quorum` decides whether this node may act as leader at all, and its
  boundary is the same shape `M14.11` found in the ledger.
  Three tests. `coordinator.rs` now has 55 mutants, 44 caught and 11 unviable,
  none missed.
  `heartbeat`'s `self_version += 1` becoming `*=` is worse than it looks,
  because the counter starts at 0: multiplying leaves it 0 forever, and
  `DigestStore::merge` treats an equal version as stale. The node's first
  heartbeat would land and every later one would be dropped, so its own store
  would describe it as it was at startup for the life of the process, through a
  drain and through every change in its connection count. Every cluster-wide
  total is a sum over that store.
  `has_quorum`'s `alive * 2 > fleet` becoming `>=` is the difference between a
  majority and a tie. In a fleet of two, one live node would believe it had
  quorum, and both halves of a partition would grant against the same cap.
  **`home_draining` took longer to understand than to test, and the
  understanding is the finding.** `home_node` hashes over `active()`, which
  excludes draining nodes, so a drain rehomes its tenants the moment it is
  announced and the home can never be draining. On that reading the function is
  dead code and the mutant is equivalent.
  It is not, because `gossip` updates liveness from a digest the store may
  reject. An out-of-order Active message can put a node back into `active()`
  while the store holds its newer Draining digest, and that disagreement is
  exactly what the guard is for. The test constructs it and asserts the
  `MergeOutcome::Stale` on the way, so it documents the mechanism rather than
  just exercising it.
  Whether `gossip` should behave that way is a separate question and a design
  decision rather than a test, filed as `M14.16`.
- [x] `M14.14` `digest.rs`: `is_empty` to `true`, and the `NodeMode::Active` and
  `NodeMode::Draining` arms deleted from `view_hash`. A view hash that ignores
  whether a node is draining is a view hash that says two different clusters are
  the same, which is what gossip convergence rests on.
- [x] `M14.15` `sim.rs`: the deterministic simulator's `Rng::next_u64` (`^=` to
  `|=`, `<<` to `>>`) and `Network::reachable` and `send`.
  This one needs a decision before it needs tests, and the decision is the
  interesting part. `sim.rs` is test infrastructure: mutating the RNG does not
  break a test, it changes which schedules the simulator explores. The tests
  still pass because they assert invariants that hold under any schedule, which
  is what they are for. So these mutants are not missing assertions about
  behaviour, they are a weakened search that nothing would notice.
  Acceptance: either tests that pin the generator and the network model as the
  contracts they are, or baseline entries arguing why a weaker search is not a
  defect. Do not kill them with an assertion that merely re-states the RNG.
  Pinned, not baselined, and the reasoning is the task. A weakened search is
  worse than it sounds rather than better: this crate's headline claim is that
  the quota invariant holds across a randomized schedule set including
  partitions, and that rests entirely on this file actually randomizing and
  actually partitioning. Degrade the generator to a near-constant and every
  property test still passes while exploring one schedule. Nothing fails, and
  the evidence quietly stops being evidence.
  All 61 mutants in `sim.rs` are now caught, none missed, none baselined.
  Four tries, and the failures are the useful record.
  The first attempt asserted the generator produced distinct values and that
  `below` covered its range. Every mutant survived it, because only one of the
  four xorshift operations is mutated at a time and a three-quarters-intact
  xorshift still looks random. Distinctness is a property of almost any
  generator; being *this* generator is not.
  What killed all four is a golden vector, for the reason `pgprox-auth` uses
  published vectors for SCRAM: the only thing that pins an algorithm is its
  output. A simulator is evidence only if it is reproducible, so two runs on
  different machines exploring the same schedules is the actual requirement.
  `reachable` fell to asserting the half the existing tests never asked: that
  pairs which were *not* partitioned stay reachable. `==` becoming `!=` makes
  almost everything unreachable, which still satisfies "a partition drops
  messages".
  The last one was mine to misread. I recorded line 215 as the dropped counter
  and wrote a test for the drop rate path; it was the reorder branch,
  `delay += below(..)` becoming `*=`. A product is a different distribution that
  looks plausible and collapses to zero whenever either draw is zero, silently
  un-reordering that message. The test re-derives the whole schedule from a
  generator seeded the same way, replicating the draws `send` makes in order,
  so it states the rule rather than blessing the current output. The drop-rate
  test stayed anyway: it covers the second `dropped += 1`, which nothing asked
  about either.
- [x] `M14.2` Mutation testing for `pgprox-pool`, 273 mutants. The pool state
  machine, whose refusal and pinning behaviour `M11` spent four tasks measuring
  from the outside without ever asking whether its tests would notice a change.
  The run found **274 mutants and 9 survivors**, in two groups by cause.
  | group | files | count | what it is |
  | --- | --- | --- | --- |
  | 21 | `params.rs`, `pin.rs` | 4 | pure parsers, character rules nothing pinned |
  | 22 | `statements.rs`, `live.rs` | 5 | counters and accessors nobody asserted |
  Closed by the full-crate run: **274 mutants, 0 surviving**, nothing added to
  the baseline. Nine survivors, nine killed, eight tests.
  `pgprox-pool` is in `mutants.sh`'s crate list now, so it cannot drop out of
  coverage the way it was never in it.
- [x] `M14.21` The parsers: `quote`'s two `||` alternatives for `.` and `-`,
  `unquote`'s `&&` chain, and `Replayable::names` replaced by an empty iterator.
  These decide how a `SET` value is rendered when it is replayed onto a new
  upstream connection, which is the mechanism that makes transaction pooling
  transparent. Getting the quoting wrong is not a crash, it is a session that
  silently comes back with a different setting.
  Three tests. Both files are now fully covered: 127 mutants, 117 caught, 10
  unviable, none missed.
  `quote`'s guard is a chain of alternatives, so turning any `||` into `&&`
  makes it unsatisfiable, since no character is alphanumeric *and* an
  underscore. Every value would then be quoted. Nothing noticed because no test
  ever asked for a bare value containing a dot or a dash, which are exactly the
  two alternatives the surviving mutants sat on. The test names a value for
  each, and one with all four kinds at once.
  `unquote` needs length, a leading quote and a matching trailing one, all
  three. Any `&&` becoming `||` accepts a value that is not actually quoted and
  slices a character off each end anyway, so `'utf8` becomes `utf`. The test
  walks the unpaired, mismatched and too-short cases, which is where the
  difference lives.
  `Replayable::names` returning an empty iterator is the quietest of the three.
  Its one caller walks it to reset every replayable parameter a previous session
  left behind before the connection is handed on, so an empty iterator is a
  connection returned to the pool still carrying the last session's settings,
  which is the precise failure the replay mechanism exists to prevent. The test
  makes enumeration and membership agree, and checks that an empty list still
  enumerates as empty so the assertion is about contents rather than about the
  method always returning something.
- [x] `M14.22` The counters and accessors: `SessionStatements::len` to `1`,
  `ConnectionStatements::is_empty` to `true`, `prepare_for`'s `tick += 1` to
  `*=`, `LivePool`'s hand-written `Debug` to a default, and `futile_wakeups` to
  `0`.
  `tick` is the one that matters: it starts at zero, so `*=` freezes it and
  every held statement carries the same use time, which is the LRU order the
  eviction policy runs on. `futile_wakeups` is the counter `M7.58` added to
  measure the thundering herd it fixed, so a constant zero makes that
  measurement unfalsifiable.
  Five tests. Both files fully covered: 97 mutants, 63 caught, 34 unviable.
  `tick` was the interesting one and the eviction test had to be built rather
  than written. Statement names are hashes of the SQL, so the deciding case,
  where the statement that is touched again sorts *before* the one that should
  be evicted, cannot be written by hand. The test searches for such a pair and
  says why it needs one. Only then do use order and name order disagree, and
  only then does a frozen tick pick the wrong victim.
  **`futile_wakeups` is the sharpest of the nine.** The existing test asserts
  the count *is* zero, which a constant zero satisfies exactly, and that test is
  the whole measurement `M7.58` rests on: waking one waiter per released
  connection rather than all of them. A frozen counter makes that assertion
  unfalsifiable, which is worse than not having it, because it still reads as
  evidence. A futile wakeup is a waiter that wakes and finds nothing, so ringing
  the doorbell without releasing anything produces one on purpose, and the test
  also checks the counter accumulates rather than latching at one.
  The `Debug` impl is hand-written so a socket is never printable and no payload
  can reach a log by construction. It could be replaced wholesale with an empty
  rendering, so `LivePool` would have printed as nothing in every diagnostic
  that ever formatted it. The test asserts the rendering changes with the
  contents, not just that it contains the right words.
- [x] `M14.3` Mutation testing for `pgprox-core`, 536 mutants. Every contract
  and every fake. `mutants.sh` opens by arguing that M9 hid three defects behind
  a fake that answered something Postgres refuses, which makes the fakes the
  most valuable thing in this milestone to point a mutant at.
  The run found **537 mutants and 58 survivors**, far more than the other two
  crates combined, across eight files. Split by file, since the files are the
  causes here: a lexer, a hash, a set of trait defaults and a set of fakes are
  four different kinds of gap.
  | group | file | count |
  | --- | --- | --- |
  | 31 | `sql.rs` | 18 |
  | 32 | `cluster.rs` | 10 |
  | 33 | `admin.rs`, `cache.rs` | 7 |
  | 34 | `config.rs`, `ids.rs`, `buf.rs`, `error.rs` | 10 |
  Closed: **537 mutants, 4 surviving**, all four in the baseline with an
  argument. Fifty-four killed by twenty-six tests.
  A process note worth keeping, because it cost a run. `cargo-mutants` copies
  the tree once per worker, so editing any file while a run is in flight
  produces copies that do not build: a verification of `sql.rs` came back
  "1 missed, 27 caught, 103 unviable" because `cluster.rs` and `admin.rs` were
  being edited underneath it. The number was meaningless and was thrown away
  rather than read. Finish the edits, then verify once.
- [x] `M14.31` The SQL lexer, 18 survivors and the largest group in the
  milestone. `trim_leading_space`, `is_string_introducer`, `word_end`,
  `block_comment_end`, `single_quoted_end`, `double_quoted_end` and
  `is_dollar_tag`, which between them decide where a statement's first word
  ends and what counts as a string.
  This is the crate's most load-bearing pure function: the statement classifier
  and the pin detector are both lexical scans over it, so getting a quote or a
  dollar tag wrong is a write classified as a read, or a `LISTEN` inside a
  string literal that pins a session that never asked.
  Ten tests. Fifteen killed, three accepted as equivalent with arguments.
  **The instructive failure is one of my own tests.** The first round put the
  doubled quote in `'a''b'` at exactly offset 2, which is the single value where
  `i += 2` and `i *= 2` agree, so both mutants survived a test that looked
  thorough. Moving the escape to any other offset kills both. A test that
  exercises a line is not a test that constrains it.
  Two more needed a case nobody had written: an underscore *after* the first
  character of a dollar tag, since the earlier test only ever put one first
  where a different clause handles it; and `u'abc'` without the ampersand, where
  turning `&&` into `||` makes any `u` word skip a byte and the string is never
  consumed, so the rest of the statement is read as SQL.
  Three are equivalent and each argument names the loop that absorbs it: the
  line-comment `+ 1` that the next `trim_leading_space` would have done anyway,
  the `is_ascii` guard that only chooses which of two identical answers to
  compute, and the `< len` bound that slices with a range rather than an index.
  The `true` form of that guard is *not* equivalent and is killed by a
  non-breaking space, which is why the entry says so.
- [x] `M14.32` `cluster.rs`, 10 survivors, including three in `stable_hash` and
  `MembershipView::is_home_for` to `false`.
  `stable_hash` is rendezvous hashing. Its own comment says `DefaultHasher` was
  rejected because it is not stable across Rust releases, since two nodes on
  different compilers would disagree about which node owns a tenant. Three
  mutants of the mixing function survived, so nothing pins the value it produces
  and the property that comment is about is unchecked.
  Nine of the ten, in fact: every `^` in the SplitMix64 finalizer could become
  `&` or `|`, and every `>>` could become `<<`.
  A golden vector, for the same reason `pgprox-auth` uses published vectors for
  SCRAM and `M14.15` used one for the simulator's generator. Every property a
  test might assert instead, that different inputs differ or that the output
  looks spread, holds for almost any mixing function including all nine
  mutants. The only thing that pins a value is the value.
  The tenth is `is_home_for` to `false`, which is how a node decides whether it
  owns a tenant and therefore drives reservations and shedding. Every existing
  test asked `home_node` directly. The new one asks from the owning node and
  from two that do not own it, and asserts the two methods agree, which is the
  invariant that makes it safe for a caller to use either.
- [x] `M14.33` `admin.rs` and `cache.rs`, 7 survivors in trait defaults and in
  the fakes themselves, including `FakeObservatory::stats`, `FakeQueryCache::
  is_empty` and `Observatory::config_is_current`.
  These are what `mutants.sh`'s own header is about: a fake that answers
  something the real thing would not is how `M9` hid three defects.
  Six killed, one accepted.
  **`Observatory::cache`'s mutant is textually equivalent**: the default body is
  `CacheView::default()` and the mutant is `Default::default()`, which in a
  position returning `CacheView` resolves to the same call. The two programs
  differ in spelling and not at all, so it goes to the baseline with that said
  plainly rather than a test written trying.
  `config_is_current` defaulting to `false` is the sharpest of the rest.
  Nothing overrides it, so the default is what every caller gets, and
  `bin/pgprox/src/metrics.rs` exports `u32::from(!observatory.config_is_current())`
  as a staleness gauge: the mutant makes every healthy node report its
  configuration as stale for ever, which is an alert that fires on a good fleet.
  **`MAX_TTL`'s existing clamp test compares the result against the constant
  that produced it.** `assert_eq!(applied, FakeObservatory::MAX_TTL)` passes for
  any value of `MAX_TTL` at all, so `4 * 60 * 60` becoming `4 * 60 + 60` was
  invisible. Naming the duration is what makes the assertion mean something.
  The `tenants` filter's `==` becoming `!=` returns exactly the complement:
  local scope would list the tenants this node does *not* home, which in a
  two-node fleet is the same length as the right answer. The test asserts both
  scopes so the filter is what is under test rather than an empty list.
  The `Stats` builder uses `..Stats::default()`, so deleting the `waiting` field
  leaves it at zero and still compiles. Waiting clients are the queue behind a
  full pool, which is the first number an operator reads when latency climbs.
- [x] `M14.34` `config.rs`, `ids.rs`, `buf.rs` and `error.rs`, 10 survivors.
  `ConfigSource::is_healthy` survived being replaced by both `true` and `false`,
  which means nothing calls it through the trait at all. `ConnId::counter` and
  the `Lsn` parser are identity handling, and `ClientError::client_message`
  being replaceable by a literal means no test reads what a client is told.
- [x] `M14.4` The remaining seven crates, or a written decision about which stay
  out and why. `pgprox-testkit` at 296 lines and `pgprox-tls` at 968 are small;
  `pgprox-admin`, `pgprox-auth`, `pgprox-config` and `pgprox-observe` are not.
  Acceptance: `mutants.sh`'s list matches its stated criterion, or the criterion
  is reworded to match the list with a reason. What must not survive is the
  present state, where the header says one thing and the array says another.
  Counted first: 662 mutants across the seven. Run in two batches, the four
  small ones together and then the three large.
  **First batch done: `tls` 16, `testkit` 13, `observe` 63, `config` 80.**
  Eleven survivors, nine killed by six tests, two argued.
  `testkit` was clean without any work, which is worth recording as the one
  crate in this milestone that needed nothing.
  `FileSource::is_healthy` survived both `true` and `false` even after a test
  was written for it, because the test never called it. `FileSource::new`
  returns `Arc<Self>` and `pgprox-core` implements `ConfigSource for Arc<T>`,
  so `source.is_healthy()` resolves to the trait method on the `Arc` with no
  deref rather than the inherent method with one. That inherent method has no
  caller anywhere in the workspace: it is public API shadowed into
  unreachability by a blanket impl in another crate. It is now called
  explicitly, with the reason in the test, because someone reading
  `source.is_healthy()` would reasonably believe they were calling it.
  With `M14.34`, that makes three separate places the config staleness signal
  was untested: the trait default, the `Arc` forwarding, and the implementation.
  `max_series` could return `0` or `1` because the check on it is an upper
  bound, `max_series() <= CEILING`, which any small constant satisfies. That
  check is the cardinality budget for the whole metric surface, so a constant
  made the budget unfalsifiable.
  **Second batch: `auth` and `admin`, both now at zero survivors.**
  `pgprox-auth` had 15, and 12 were in `bin/mock_sidecar.rs`. Binaries are now
  excluded, for the reason `M13.4` already established when it exempted the same
  paths from the sans-I/O rule: a binary is a composition root or a fixture, not
  logic with unit tests, and `scripts/e2e.sh` asserts this one end to end.
  Mutating it reports that a binary with no unit tests has none.
  The exclusion did nothing at first and the survivor count is the only reason
  that was noticed. `cargo-mutants` matches a glob containing a slash against
  the entire path, so `src/bin/**` never matched
  `crates/pgprox-auth/src/bin/mock_sidecar.rs`. `**/src/bin/**` does, checked
  with `--list` before being trusted: 12 matches against 0. A flag that runs is
  not a flag that does something, which is this milestone's own lesson one layer
  out from the code.
  The three real ones: a cached authentication decision could outlive its TTL by
  an instant, the same boundary `M14.11` found in the quota ledger and here
  admitting a client against a token the sidecar has stopped vouching for; the
  `DeadlineExceeded` arm could be deleted, which keeps the variant and loses the
  message that distinguishes a silent sidecar from one that answered; and
  `UNIX_EPOCH + Duration` could become `-`, putting every token expiry the same
  distance before 1970 so every valid grant reads as long expired.
  `pgprox-admin` had 5. Three are the `active > 0` predicate behind the `state`
  column of `SHOW SERVERS`, where `>= 0` makes every pool active and `== 0`
  inverts it; every existing test used a pool in one state only.
  The other two are `DEFAULT_DRAIN_TTL`, and they are **the third instance of an
  assertion compared against the constant that produced it**. This crate's own
  check is `assert_eq!(ttl, Some(DEFAULT_DRAIN_TTL))`, which passes for any
  value. Its documented partner in `pgprox-config` asserts that crate's own
  literal and says in a comment that admin mirrors it, but the two cannot see
  each other, which is exactly why the value is duplicated. A constant described
  as held together by a test was held by neither side. Both now pin the literal
  independently, which is the only way a duplicated constant can be paired.
  **Third batch: `pgprox-load`, 219 mutants and 43 survivors**, the largest
  single group in the milestone and the one it would have been worst to skip.
  Thirty in `report.rs`, eleven in `sampler.rs`, two in `workload.rs`: the code
  that computes every percentile, count and SQLSTATE tally that `M9`, `M10` and
  `M11` drew conclusions from. A wrong percentile there fails nothing; it
  changes what this repository believes about itself. `M11.1` overturned
  `M10.9`'s throughput claim on numbers this code produced.
  Thirty-seven killed by nine tests, eight accepted with arguments.
  Twenty-four of the survivors were in `Histogram::bucket` and
  `Histogram::upper_edge`, which are inverses across three resolution bands, and
  every one of them moved a boundary by one. No spot check finds that reliably,
  so the test asserts the two agree across all 25,800 buckets, that each band
  starts where the last ended, and that the edges rise without repeating: a
  mutant that flips a subtraction can leave them non-monotonic while each value
  still looks plausible, and a percentile read off a non-monotonic table is
  nonsense that reports as a number.
  That round-trip walks `0..BUCKETS - 1` and so never reached the overflow
  bucket, which is exactly where two more mutants lived. Naming
  `upper_edge(BUCKETS - 1)` kills both.
  **The two I nearly wrote off are the ones worth recording.** `roll < units`
  and `<=` disagree for exactly one value in a million, so no distributional
  test separates them: the expected counts differ by a single draw. The
  temptation was to baseline them as unreachable in practice, which is a
  convenience argument and not an equivalence argument, and this file is for
  mutants no test *could* kill. They are reachable, because the draw order is
  fixed and `sampler.rs`'s own comments treat it as a contract: each fraction
  gets its own draw so that changing one does not shift the other's stream. The
  test replays that order to find the exact roll the first statement will make
  and sets the fraction to it. Confirmed by applying both mutants by hand.
  The eight accepted are argued rather than asserted, and three of the arguments
  are proofs: the band guards are equivalent because the bands are seamless by
  construction, checked at all three boundaries; two `unwrap_or` fallbacks
  cannot be reached because `usize::try_from(u64)` cannot fail on a 64-bit
  target, with re-triage noted for 32-bit; `weights.len() - 1` cannot be reached
  because `point` is drawn below the total; and the share tolerance boundary is
  not representable in `f64`, since hitting it needs `total` to be exactly
  `1.0 + 0.001_f64`, which needs more than 53 significant bits. Checked
  numerically rather than reasoned: 0.0009999999999998899, then
  0.001000000000000112, with nothing in between.
  With this, all fourteen crates are mutation tested. `mutants.sh`'s header and
  its crate list finally say the same thing.
- [x] `M14.5` `product/plan.md`'s M0 open items are stale. Item 1 says the
  sidecar `.proto` "needs sign-off from whoever owns the sidecar"; ADR 0017
  decided this repository owns it and the file is marked `STATUS: FROZEN`. Item
  3 asks what happens if a large fraction of tenants use `LISTEN`/`NOTIFY`, and
  `M11.7` measured it: 0.650 upstream connections per pinned session, linear,
  no threshold.
  Acceptance: each item says what has since been decided or measured and points
  at it, and the ones that genuinely need an owner outside this repo stay open
  and say so. Item 2, upstream `max_connections` per server class, is one of
  those and must not be dressed up as resolved.
  Done, and item 2 turned out to be two questions rather than one, which is why
  reading it beat assuming it. It asks for the values *and* for caps to be
  configurable rather than guessed. The second half is built: `max_connections`
  and `guaranteed_fraction` are fields on the config document. The values per
  server class are not knowable here and stay open. It is the only one of the
  three still open, and it says so.
  Item 1 was settled differently from how it was asked. The premise was that the
  sidecar is the one interface this project does not control; ADR 0017 decided
  to control it, and the `.proto` carries `STATUS: FROZEN`. The discipline that
  survives the premise is recorded rather than dropped: field numbers never
  reused, fields never removed, sidecar owners agree before the Rust side moves.
  Item 3's consequence half is `M11.7`'s curve, and the answer is milder than
  the question feared. "The pool sizing model needs revisiting" becomes one
  extra term, because the cost is linear with no knee. The population half needs
  real tenants and stays open inside the item rather than as a separate entry.
  Taken out of backlog order, ahead of `M14.1` to `M14.4`, because those are
  mutation runs that own the machine for a long stretch and this needed none.
- [x] `M14.6` Write `scripts/gates/m14-complete.sh`, before the milestone needs
  closing. Under `M12.8`'s constraint: run things, do not match filenames.
  Five checks.
  The list is compared against the crates that exist rather than against its own
  header, because the header is prose and the milestone exists precisely because
  prose and array disagreed. Fourteen of fourteen.
  A failed baseline is planted as an outcomes file and read back, then the guard
  itself is confirmed present, because `M14.4` found `mutants.sh` reporting
  "1 mutants, 0 surviving" and "all checks passed" for a crate whose baseline
  had failed to build. A gate for this milestone that did not check that would
  be repeating the mistake it exists to record.
  Every baseline entry is required to carry a reason, which is the file's own
  rule: the list may not grow without somebody writing down why. Twenty-three
  entries, all reasoned.
  And a real run, on `pgprox-testkit`: one crate rather than fourteen, because
  the sweep is tens of minutes and this sits in CI beside twelve other gates.
  Testkit is the smallest and was the only crate in the milestone that needed no
  work, which makes it the cheapest thing that still exercises the whole path.
  It also corrected a stale claim in `ci.yml`, which described the mutation job
  as "nine hundred of them across the four sans-I/O crates". That was true when
  it was written and stopped being true in `M13.4`, which made every crate
  sans-I/O and enforced it.
- [x] `M14.16` `gossip` takes a node's mode from a digest it then rejects as
  stale. Found by `M14.13`, while working out why `home_draining` could return
  `false` unconditionally with nothing noticing.
  ```rust
  pub fn gossip(&mut self, incoming: VersionedDigest, now: Instant) -> MergeOutcome {
      self.liveness.heard(incoming.digest.node, incoming.digest.mode, now);
      self.digests.merge(incoming)   // may return Stale and keep the newer digest
  }
  ```
  `heard` is unconditional and `merge` is not, so an out-of-order message can
  put a node back into `active()` as Active while the store holds its newer
  Draining digest. The two then disagree about the same node's mode.
  That disagreement is the only thing that makes `home_draining` reachable, and
  therefore the only thing that makes `shed`'s `HomeDraining` guard reachable,
  because `home_node` hashes over `active()` and excludes draining nodes: a
  drain normally rehomes its tenants before anything can shed toward them.
  The question is which of the two is right, and it is a design decision rather
  than a test. Hearing *from* a node is evidence it is alive whatever version it
  sent, so passing the message to `heard` is defensible. Taking the *mode* from
  a digest being discarded as stale is harder to defend: it lets an old message
  undo a drain announcement in the view while the store still knows better.
  Acceptance: a decision, recorded, and whichever way it goes the consequence is
  followed through. If the mode should come only from an accepted digest, then
  `home_draining` becomes unreachable and the `HomeDraining` guard and its
  `ShedReason` have to go with it, or be documented as defence in depth that
  cannot fire. If the current behaviour is right, say why in `gossip` so the
  next reader does not file this again.
  Not urgent: no test failed and no run misbehaved. It is a latent
  inconsistency, found by asking why a mutant survived.
  **Decided: the mode comes only from an accepted digest.** Contact and mode
  are different things. Hearing from a node is evidence it is alive whatever
  version it sent, and recency of contact is the right ordering for that. Its
  mode is not reachability, it is content the sender asserts, and content is
  ordered by the sender's own version, which is exactly what `merge` has just
  ruled on. `Membership::heard_without_mode` records contact alone, and `gossip`
  merges first so it knows which to call.
  Scope of the bug, established before deciding rather than assumed. Nodes never
  relay a peer's digest: `outgoing()` returns this node's own, with a
  monotonically increasing version. So a stale mode needs two of a node's own
  messages arriving out of order, and it un-drains that node in one receiver's
  view until its next round re-asserts the drain. Real, narrow, self-healing
  within a round, and during that round a shutting-down node is back in
  rendezvous hashing and can be homed.
  **It broke an existing test, and that is the finding.**
  `a_leader_that_loses_office_stops_granting` delivered version 2 twice, so its
  second digest was stale and the store rejected it: the drain reached the view
  only through the side-channel this task closed. The test passed for a reason
  it did not intend, and it went green again by bumping the version, which is
  what a node announcing a drain actually does.
  The consequence is followed through as the acceptance asked. `home_draining`
  is now unreachable, because the view and the store agree and `home_node`
  excludes draining nodes, so its `-> false` mutant is equivalent by
  construction and goes to the baseline with that argument and a re-triage
  condition. The `HomeDraining` guard in `shed` stays: the property is then
  enforced structurally *and* checked, and removing it would change the public
  `ShedReason` for no behavioural gain.
- [x] `M14.7` Close M14. Filed before the commit that does it, which is the
  order `M12.1` enforces.
  The status table and the section say the milestone is complete, and the
  section says what the runs found rather than that they ran.

## M15: the protocol crate under a second reading

- [x] `M15.1` The inspect cap that bounds nothing. `DEFAULT_MAX_INSPECT` is
  documented as "largest message body the proxy will buffer in order to read
  it", with the reason stated: bytes parsed are held per connection, so at 100k
  connections their limit must be small. `FrameRelay` never reads it. For
  `Inspect::Whole` it sets `want_inspect = header.body_len`, which is bounded
  only by `max_frame`, and that is 1 GiB.
  Client-reachable without authenticating: `Sync`, `Flush`, `Terminate` and
  `CopyDone` are all `Inspect::Whole` on the frontend side, and the relay takes
  the declared length on trust. A `Sync` claiming 8 MiB makes the relay hold
  8 MiB; the same frame claiming 1 GiB makes it hold 1 GiB.
  Acceptance: a relay carries an inspect cap, the cap defaults to
  `DEFAULT_MAX_INSPECT`, and a test drives a `Whole`-policy tag past it and
  asserts what is held. The existing `complete` flag already reports a
  truncated inspection, so the parser side needs no new signal.
  Found because `DEFAULT_MAX_INSPECT` has no caller anywhere in the workspace.
  **The cap alone was half a fix, and the test that was supposed to confirm it
  is what said so.** `Vec::clear` keeps its allocation, so capping the peak
  leaves the capacity in place for the life of the connection: the attack goes
  from "a gigabyte per connection while the frame is in flight" to "a megabyte
  per connection, permanently", at a cost of one frame each. At 100k
  connections that is the same problem in a smaller font. The relay now
  releases anything above `RETAINED_INSPECT`, 8 KiB, once the message that
  needed it is over, and keeps everything below it so the ordinary path still
  allocates nothing. Both halves have a test, including the cost side.
- [x] `M15.2` A failed COPY holds the connection for the session's life.
  `SessionState` clears `copy` on a frontend `CopyDone`/`CopyFail` and on a
  backend `CopyDone`, and on nothing else. When the server rejects a COPY it
  sends `ErrorResponse` and then `ReadyForQuery`, and a client that has been
  told its COPY failed has no reason to send `CopyDone`. The hold never lifts.
  Acceptance: `ReadyForQuery` ends COPY mode, for the same reason it already
  ends an extended sequence: the server does not send one until it is back in
  normal command processing, so copy mode cannot outlive it. A test drives the
  failed COPY IN sequence and asserts the connection comes back.
  pgbouncer clears `copy_mode` on both `ErrorResponse` and `CommandComplete`
  (`src/server.c`, "ErrorResponse and CommandComplete show end of copy mode").
  **Not copied literally, and the reason is worth keeping.** Clearing at the
  `ErrorResponse` would release while the `ReadyForQuery` answering it is
  still in flight, because the release test here runs on every server frame and
  `tx_status` is still whatever the last `ReadyForQuery` said. pgbouncer can
  clear there because it tracks readiness in a separate flag. That is
  `a_sync_alone_does_not_permit_release` in the other direction, and the new
  test asserts the intermediate state as well as the final one.
- [x] `M15.3` `DISCARD ALL` deallocates the server's prepared statements and
  nothing tells the maps. `ParamCache::observe_statement` handles `DISCARD ALL`
  and `RESET ALL` by clearing the parameter cache. The statement maps have the
  matching operations, `ClientStatements::close_all` and
  `ConnectionStatements::forget_all`, and neither has a caller outside its own
  tests. After a client runs `DISCARD ALL` the proxy still believes every
  mapped statement is prepared on that connection, so the next `Bind` sends a
  global name the server has just dropped.
  Acceptance: the same observation point that clears parameters clears the
  statement maps, and a test runs `DISCARD ALL` and then binds.
  pgbouncer does this on the `CommandComplete` tag, checking for `DEALLOCATE
  ALL` and `DISCARD ALL` by name (`src/server.c`).
  **Both maps, not one.** Clearing only the session map would leave the
  connection claiming to hold the global name, and since the global name is
  derived from the SQL, a client that re-parses the same statement gets the same
  name back and the connection would skip the `Parse`. There is a test for
  exactly that sequence.
  Read from the client's SQL rather than the server's `CommandComplete` tag,
  which is where pgbouncer reads it. The two differ only when a `DEALLOCATE
  ALL` is rolled back, where both over-clear, and over-clearing costs a
  re-prepare while under-clearing produces "prepared statement does not exist"
  on a connection the proxy thought was warm.
- [x] `M15.4` `cstr` scans one byte at a time. `Reader::cstr` finds its
  terminator with `iter().position(|b| *b == 0)`, which is a scalar loop, and
  it is on every hot path this crate has: the SQL in `Query` and `Parse` up to
  the 64 KiB prefix, every `CommandComplete` tag, both strings of every
  `ParameterStatus`, and every field of every error. `rewrite.rs` does the same
  scan twice more.
  Acceptance: `memchr` on those scans, already in the lock file and MIT so the
  supply-chain gate has nothing to say, plus a microbenchmark in
  `benches/hot_paths.rs` that measures the scan and a before/after instruction
  count recorded in `product/perf/`.
  **Measured: 2168 to 460 instructions on the long scan, 4.7x.** The short-string
  case, eight fields of an `ErrorResponse`, improves by 14.3% rather than by
  anything like that, because at five bytes the work is call overhead and the
  UTF-8 validation that follows. Both shapes are benched separately for exactly
  that reason, and three unchanged benches are the control.
  See [run-2026-08-01-cstr-scan.md](perf/run-2026-08-01-cstr-scan.md).
- [x] `M15.5` The header copy on every frame. `FrameRelay::push_header` copies
  the five header bytes into `header_buf` and clears it again, on every message,
  including the overwhelmingly common case where all five are already contiguous
  in the caller's slice. The partial-header path is what the buffer exists for
  and it is the rare one.
  Acceptance: the contiguous case decodes in place, the split case still works
  byte for byte (`a_message_split_at_every_boundary_relays_identically` is the
  test that says so), and the instruction count moves in the direction claimed.
  **Measured: 197 to 169 instructions, -14.2%**, on the path every byte of every
  result set takes. The first version of the pinning test was wrong rather than
  the code: it pushed the tail of a split header once and expected the body to
  be consumed too, which is not what `push` promises. Driven through `drive` it
  passes, and the contract is the one that was already documented.
- [x] `M15.6` The crate says it never allocates and it does. `lib.rs`: "Nothing
  here allocates at all: frames borrow from the caller's buffer." Untrue of
  `bind_parameters`, of `startup::decode`, of everything in `rewrite`, of
  `FrameRelay`, and of `select_sasl_mechanism`, which builds a `Vec<&str>` of
  the offered mechanisms in order to search it. Of those, only the last is
  gratuitous, and `Startup::option` re-parses and re-allocates the whole option
  list on every lookup.
  Acceptance: `select_sasl_mechanism` allocates nothing, `option` does not
  build a list to throw it away, and the sentence in `lib.rs` says what is
  actually true. The rule it is trying to state is real; it is the scope that
  is wrong.
  **A second finding came out of it.** `select_sasl_mechanism`'s rule is that
  our preference order decides and the server's does not, and
  `SUPPORTED_SASL_MECHANISMS` holds one entry, so the test asserting that rule
  could not fail: every ordering of a one-element list is the same ordering.
  That is an `M14` shape, an assertion compared against the thing that produced
  it. The selection is now split into a private form taking an explicit
  preference list, and the rule is stated against a list long enough to have an
  order.
- [x] `M15.7` `Reader` adds client-controlled lengths without checking. `i32`,
  `i16` and `bytes` all compute `self.pos + n` and rely on the slice index to
  refuse the result. `bytes` takes its `n` from the wire: a `Bind` parameter
  length and a `ParameterDescription` count both reach it. On a 64-bit target
  the sum cannot wrap, so this is hardening rather than a live bug, and it is
  worth the two lines because the fuzz target cannot reach a 32-bit overflow on
  the machine it runs on.
  Acceptance: `checked_add`, and the truncation error rather than a panic.
  The test passes `usize::MAX`, which is the one input that separates a checked
  add from an unchecked one on this target, and also asserts the cursor did not
  move: a failed read that consumed bytes would desynchronise every field after
  it.


### M15 round two

The first pass fixed what it found. This is the second reading, which the
milestone said it would do and which found three more.

- [x] `M15.9` The pre-authentication path is bounded by the relay cap. A client
  sends a startup packet before it has proved anything, and
  `shell::negotiate` reads it with `read_untagged(.., DEFAULT_MAX_FRAME)`, which
  is 1 GiB. `Wire::fill` grows its buffer 16 KiB at a time until the declared
  length arrives, so an unauthenticated client can make the proxy hold whatever
  it is willing to send, at one byte for one byte, on as many connections as it
  can open. The password message is the same: `authenticate_token` reads it
  with `read_tagged(.., DEFAULT_MAX_FRAME)`, and that also runs before the
  client has authenticated.
  Postgres itself does not allow this. `MAX_STARTUP_PACKET_LENGTH` is 10000
  bytes, and pgbouncer refuses anything over `cf_max_packet_size` at the header.
  Neither number is 1 GiB, and 1 GiB is not a startup packet.
  Acceptance: a cap for the handshake, sized from what a startup packet and a
  JWT actually are rather than from what a `DataRow` may be, with a test that
  drives an oversized declared length and asserts the client is refused rather
  than served.
  `M15.1` was the same mistake one layer down: a documented bound with no
  caller. This is a bound that was never written.
  **The cap is a parameter rather than a second default**, so every one of the
  eleven call sites had to state which stage it is in. That is the point: a
  default is what let this sit unnoticed, and the compiler asking the question
  once is worth more than a constant nobody reads. The upstream reads and the
  authenticated relay loop keep `DEFAULT_MAX_FRAME`, deliberately: a client's
  own `Bind` or `CopyData` can legitimately be large, and narrowing those is a
  different question from this one.
  32 KiB rather than Postgres's 10000, because a startup packet carrying a long
  `options` string and a JWT with a full claim set both have to fit. The number
  that matters is that it is not a gigabyte.
- [x] `M15.10` A count and the list it counts can disagree.
  `encode::row_description` writes `i16::try_from(columns.len())` saturated to
  `i16::MAX` and then writes every column. `encode::data_row` and
  `encode::negotiate_protocol_version` have the same shape. Past the saturation
  point the count says one thing and the bytes say another, and a client reads
  the following message from the middle of this one.
  Unreachable from the current callers, which is why it is worth two lines
  rather than an argument: `encode_frontend::bind_with_parameters`, in the same
  workspace and written for the same reason, already does it correctly with
  `values.iter().take(count)`. Three of the four got it wrong and one got it
  right, so the fix is to apply the pattern that is already here.
- [x] `M15.11` `standard_conforming_strings` pins a session that could be
  replayed. It is one of the five parameters pgbouncer tracks by default, it is
  an ordinary GUC reproducible by re-issuing the `SET`, and it is absent from
  `REPLAYABLE_NAMES`, so a session that sets it is pinned for its lifetime.
  The list's own comment says additions are a promise and should be rare, and
  that is the right instinct. It does not apply here: the criterion the list
  actually uses is "can this be reproduced by re-issuing the `SET`", and
  `bytea_output` and `intervalstyle` are already on it and are exactly as rare.
  Its absence is an omission rather than a decision.
  Acceptance: on the list, with the replay test that covers the others.
- [x] `M15.12` The mutants my own tests let live. A mutation run over
  `pgprox-proto` after the first seven tasks found three survivors in code this
  milestone wrote, and one of them is the shape `M14` catalogued and this
  milestone quoted: `the_buffer_a_large_message_needed_does_not_outlive_it`
  asserts `capacity() <= RETAINED_INSPECT`, which is the constant that produced
  the number, so `8 * 1024` becoming `8 + 1024` passes it. Writing about that
  failure mode in `M15.6` did not stop me committing it in `M15.1`.
  The other two are equivalent and go to the baseline with arguments. Two
  existing baseline entries are now caught, because `M15.5` and `M15.6`
  rewrote the functions they were about, and a baseline that keeps an argument
  for a mutant that no longer survives is a baseline nobody trusts.
  Acceptance: the constant is asserted against something that is not itself,
  the two equivalents carry arguments, the two stale entries are gone, and a
  clean run agrees.
  **The clean run agrees: 359 mutants, 6 surviving, every one of them in the
  baseline with an argument.**
- [x] `M15.13` A capacity reserved from a number a peer sent.
  `probe::text_row` starts with `Vec::with_capacity(count)` where `count` is the
  column count read from the `DataRow` it is about to parse. A three-byte
  message claiming 32767 columns reserves 32767 `Option<String>`, which is
  about 786 KB.
  `frontend::bind_parameters` has the same shape and refuses it in a comment
  that says why: "Not `with_capacity`. The count is the client's and the values
  have not been read yet, so reserving on it is a nine-byte message asking for
  thirty-two thousand pointers." One crate wrote that down and another did the
  thing it warns about.
  Smaller than it sounds and worth fixing anyway. The peer is an upstream
  Postgres the sidecar named rather than a client, `i16` caps the reservation,
  and a probe runs per connection attempt rather than per statement. What makes
  it worth two lines is that this is the only `DataRow` the project parses, its
  answer has two columns, and the rule it breaks is one the workspace already
  states.
  Acceptance: no reservation from an unread count, and a test that a large
  declared count with no columns behind it is refused rather than reserved.
- [x] `M15.8` Close M15. Filed before the commit that does it, and after the
  readings the milestone promised, which between them found five more.

## M16: the streaming relay nothing streams through

- [x] `M16.1` Measure what the data path actually holds. `FrameRelay` exists to
  read a five-byte header, ask `inspect_policy` how much of the body is needed,
  and forward the rest as it arrives. Its module header says why: "`decode`
  needs a whole message before it returns one, so a relay built on it must
  accumulate an entire body before forwarding a byte. A single large `DataRow`
  would then hold up to a gigabyte, and ADR 0008's whole premise is that an idle
  connection costs roughly 200 bytes."
  The relay loop in `bin/pgprox/src/serve.rs` is built on `decode`. Every server
  frame goes through `read_tagged(&mut body, DEFAULT_MAX_FRAME)`, which waits
  for the whole message and copies its body into a `Vec`, and `forward` then
  copies it again into the write buffer. `FrameRelay` has no caller anywhere
  outside its own module, its tests and its benches; the only other mention of
  it in the workspace is a comment in `shell.rs` referring to it for a different
  reason.
  So the thing the crate says must not happen is what the proxy does, and the
  code written to prevent it was never wired in.
  Acceptance: a number, not a reading. A test or a scale run that drives one
  large result through a real session and reports peak RSS per connection
  against the same result driven through `FrameRelay`. `M7`'s 100k figure was
  measured with small rows, so it does not answer this.
  Filed before the fix, because the fix is a rewrite of the most
  correctness-critical loop in the project and it should be justified by a
  measurement rather than by a module header.
  **Measured: 16,777,216 bytes against 0.** One 16 MiB `DataRow`, the same
  bytes down both paths. Zero rather than five because a `DataRow` is
  `Inspect::None`, so the relay reads the header, learns it has nothing to
  inspect, and forwards every byte without copying one.
  See [run-2026-08-01-streaming.md](perf/run-2026-08-01-streaming.md).
### What the pump uses a body for

Established before designing the change, because three of the four uses turn out
not to need one.

| use | needs the body |
| --- | --- |
| `backend::decode` for `relay.on_server` | only for inspected tags; `DataRow` decodes to `Opaque(tag)` without reading it |
| swallowing `ParseComplete`/`CloseComplete` | no, the tag decides |
| `pumping.owed.received(tag)` | no, the tag decides |
| cache recording | yes, and `belongs_in_payload` includes `DataRow` |

So the streaming path is available whenever the tag is `Inspect::None` and the
session is not recording for the cache. That is every `DataRow` and every
`CopyData` of every uncached statement, which is the traffic this is about.
Recording stays on the buffering path and is bounded by the cache's own limit,
which it must have anyway or it would cache a gigabyte.

There is a second copy nobody has counted: `forward` re-encodes the tag, the
length and the body into the write buffer, so a 16 MiB row is held twice.
Streaming removes both.

- [x] `M16.2` A wire can move a body without holding it. The enabling piece,
  and the reason it is its own task is that `Wire` owns both a borrowed read
  buffer and a borrowed write buffer, and streaming crosses two wires: read from
  the upstream one, write to the client one, in bounded chunks, flushing as it
  goes so the write side does not become the buffer the read side stopped being.
  Acceptance: the primitive, its tests, **and its caller**. Not landed
  separately. A streaming primitive with no caller is the defect this milestone
  exists to fix, and adding a second one would be a joke at the project's
  expense.
- [x] `M16.3` Stream the server-to-client pump. `M16.2`'s caller, in full:
  header, then `inspect_policy`, then either the current path or the streamed
  one, with COPY, the swallow counter, the cache recording and the `Flush`
  terminator all still behaving. The tests that cover those today are the
  acceptance criteria; none of them may be relaxed.
- [x] `M16.4` Stream the client-to-server direction, in the COPY loop.
  The same shape, on the side where the sender is the untrusted one.
  `copying` read every client frame whole, and a `COPY ... FROM STDIN` is
  nothing but those frames. It now reads the header and streams a `CopyData`
  body straight upstream; everything that ends a copy is small and is still
  read whole, because the caller's loop needs the frame.
  Safe without a cancellation-point argument beyond the one already in the
  function's own header: nothing races this loop, which is why it is one-way.
  Covered end to end rather than by a new fixture. `pgbench --initialize`
  loads 100,000 rows with `COPY pgbench_accounts FROM STDIN` through the proxy,
  and `scripts/e2e.sh` runs it. The mechanism has its own unit tests from
  `M16.2`.
- [x] `M16.6` Stream the prefix-inspected client messages. What `M16.4` leaves:
  the main relay loop still reads a whole `Bind`, and `inspect_policy` says only
  its first 4 KiB matters, so a client binding a 100 MB parameter has 100 MB
  held for a name at the front. Same for `Query` and `Parse` past 64 KiB.

  **Designed but not attempted, and the reasons are specific rather than
  general caution.** Reading it through turned up four hazards the earlier
  sketch missed, and each one is a place a mistake desynchronises a session
  rather than costing memory.

  1. `forward` computes the length prefix from the buffer it is given, not from
     the header that arrived. Hand it a 4 KiB prefix of a 100 MB `Bind` and it
     announces a 4 KiB message and then streams 100 MB behind it. Every
     forwarding path needs the true `body_len` threaded through.
  2. `send_upstream` can decline to forward at all. `Statement::AlreadyPrepared`
     queues a synthetic `ParseComplete` and returns false, so the frame never
     goes upstream, but its tail is still on the client's socket and must be
     drained or the next header is read from inside this body. `ClientAction::
     Answer` and `Close` are two more paths with the same problem.
  3. Rewriting changes the length. `rewrite::bind_statement` replaces the
     statement name with a global one of a different size, so the forwarded
     header cannot be copied from the one that arrived; it is
     `body_len - prefix_read + rewritten_prefix_len`. That arithmetic is
     correct in one place and wrong in every other, and there is no test today
     that would catch it being wrong.
  4. The destination is not known until after routing, and routing needs the
     prefix. So the order has to be read prefix, route, acquire, forward header
     and prefix, stream tail, which means the tail sits on the socket across an
     acquire that can block on the pool. That is defensible, and it is a change
     in where backpressure appears.

  The cache is a fifth constraint rather than a hazard: `bind_parameters` needs
  the whole body to build a key, so streaming has to be off whenever the cache
  is on for the tenant.

  A safe subset exists and is worth naming: the tags that are neither rewritten
  nor cached, which is `Query`, `FunctionCall`, `CopyData` and `CopyFail`. Those
  avoid hazards 1 and 3 entirely. It leaves the headline case, a large `Bind`
  parameter, exactly where it is, which is why it was not taken as a
  consolation.

  Severity, stated so the priority is arguable rather than assumed: this needs
  an authenticated client holding a valid grant for a tenant, and the memory is
  one message in flight rather than retained. `M15.9`, which was the same shape
  from an unauthenticated client, was the urgent one and is done.

  The `read_header` half is already available and already safe: it consumes
  only after five bytes decode and its only await is the fill before that, so
  it can go inside the `select!` without the drain branch being able to drop it
  mid-frame. The pair is what must not straddle a cancellation point, and it
  would not.

  **Done, and the design pass found a fifth hazard that set the scope.**
  `pin_reason` scans *every* statement in a query's SQL rather than the first,
  because the simple query protocol allows several in one message and its own
  comment says `SELECT 1; LISTEN c` would otherwise go through unpinned. A
  truncated scan is a missed pin, and `pgprox-pool`'s own rules say a missed pin
  hands one client another client's state. `Parse` has a second reason: its
  global name is derived from the SQL, so two long statements sharing a prefix
  would collide on one name. **`Query` and `Parse` are read whole whatever the
  policy says**, and there is a test whose message says why.

  That pointed at the right target rather than away from it. `Bind` carries
  parameter values, not SQL: nothing scans them, and the rewrite touches only
  the two names at the front. So the case that matters most, a 100 MB `Bind`
  parameter, is also the one that is safe, and it now costs 4 KiB.

  The four hazards above are handled rather than avoided. `forward_header`
  takes the true length, prefix plus tail, instead of the buffer's.
  `AlreadyPrepared` drains its tail even though only a `Parse` reaches it and a
  `Parse` is never streamed, because the reason it is unreachable lives in
  another function. The rewritten prefix's length is computed, not copied. The
  tail waits on the socket across the acquire, which is backpressure rather
  than a buffer. And a name longer than the prefix re-reads rather than
  refusing the client, through `Wire::append_body`.

  Nothing streams while the node has a cache, because a `Bind` key is built
  from its parameter values. Coarse on purpose: whether *this* statement is
  cacheable is not known until it is decoded, and erring toward reading cannot
  be wrong.

  Validated where it can actually disagree: conformance across psql, pgx,
  asyncpg, JDBC and npgsql against Postgres 17 and 18, e2e with prepared
  statements, drain and watermark, and 443 mutants over `pgprox-session` with
  two survivors, both pre-existing and argued.

  One scare worth recording. The first e2e after the change reported 146 tps
  against a 166 to 179 range. Two re-runs gave 181.6 and 178.7, so it was
  machine contention rather than a regression. Chased rather than assumed,
  because a 17% drop on the main path would have mattered.

- [x] `M16.5` Re-measure. **16,777,216 bytes held becomes 512**, for the same
  16 MiB `DataRow` through the pair the pump now uses. 512 is `FIRST_READ`, the
  stack chunk a quiet connection reads into, so the largest piece held is a read
  rather than a message; a busy connection reads into the 16 KiB borrowed buffer
  instead, which is the same answer with a different constant.
  `M16.1`'s original comparison is kept alongside it rather than replaced,
  because it is what says the gap was real.
  The 100k half is not done and is blocked on the same three machines and real
  network as `M7`'s full run. It is named in the roadmap as such rather than
  claimed. `e2e.sh` reported 160.5 tps before and 174.8 after, which is worth
  almost nothing as a performance claim on one machine with pgbench's tiny rows,
  and is quoted only because it is the run that says nothing broke.
- [x] `M16.7` `M15.3`'s fix has no caller. `resume::observe_statement` clears
  both statement maps when a client runs `DISCARD ALL`, it has tests, and
  nothing in the proxy calls it. The desync `M15.3` set out to close is still
  there.
  Found while reading `serve.rs` for `M16.6`, which is the point: this
  milestone exists because `FrameRelay` had no caller, `M15.1` because
  `DEFAULT_MAX_INSPECT` had none, and `M15.3` because two clearing functions
  had none. The fix for the third added a fourth, in the same session, by the
  same hand, after writing the sentence "a streaming primitive with no caller
  is the defect this milestone exists to fix".
  Writing the rule down is not the same as following it. That is the finding,
  and it is worth more than the bug.
  Acceptance: called from the relay loop, with the connection map that the
  statement will actually run on, and a check that says so rather than a test
  of the function in isolation. Its signature takes the two memory structs and
  the proxy holds the two maps, which is part of why it did not fit anywhere.
- [x] `M16.8` A test in `pgprox-tls` fails about one run in three.
  `test_cert` builds its directory from the process id alone, and every test in
  a binary shares one process, so all of them write `cert.pem` and `key.pem` to
  the same two paths. `cargo test` runs them on parallel threads, so one
  truncates a file another is reading, or pairs a certificate with a different
  test's key.
  It shows as `a_server_config_builds_from_a_certificate_and_key`,
  `a_mismatched_certificate_and_key_are_rejected` or
  `a_root_store_holds_exactly_what_was_added` failing, apparently at random.
  Found while verifying something else, and confirmed to predate it by
  stashing: three runs at `HEAD~1` failed once too.
  This matters more than one flaky test. Non-negotiable 3 is that a claim a
  test passed is only worth what the run behind it was worth, and a suite that
  fails a third of the time trains everyone to re-run it. That is how a real
  failure gets re-run until it goes away.
  Acceptance: a directory per call, and a run of the crate's tests repeated
  enough times to say the flake is gone rather than hiding.
  **Twelve consecutive clean runs**, against a failure rate of roughly one in
  three before. Twelve rather than three because at one in three, three clean
  runs happen thirty per cent of the time by luck.
- [x] `M16.9` Nothing checks that a thing written is a thing reached. Four
  findings in two milestones were correct code with no caller: `FrameRelay`
  (`M16`), `DEFAULT_MAX_INSPECT` (`M15.1`), the two statement-clearing
  functions (`M15.3`), and the function `M15.3` added to call them (`M16.7`).
  The last was committed one commit after writing that a primitive with no
  caller is the defect the milestone exists to fix, which says as clearly as
  anything can that the rule needs a script rather than a reader.
  Acceptance: a check that names the symbols which must reach production code
  and fails when one does not, wired into CI and into `negative.sh` like every
  other gate, seeded with all four.
  **Its own first two versions had the defect in miniature, and its first real
  run found a fifth instance.** Version one counted any mention, so a `pub use`
  re-export read as a caller. Version two filtered imports and still passed
  `FrameRelay`, on two doc comments that mention it by name. Prose about a thing
  is the least reliable evidence anything calls it, since prose is exactly what
  says it should. Both versions would have passed throughout the milestone in
  which nothing called it, which is the definition of not a check, and both are
  planted in `negative.sh` rather than described.
  The fifth instance is `FrameRelay` itself, still reached from nowhere after
  `M16.3`, because that task solved streaming with `Wire::read_header` and
  `take_body` in the I/O shell rather than with the type built for it. Recorded
  with the marker rather than hidden, and `M16.10` settles it.
- [x] `M16.10` Two implementations of one idea, and only one is used.
  `FrameRelay` reads a header, applies `inspect_policy`, and forwards the rest.
  `Wire::read_header` and `Wire::take_body` now do the same job in the I/O shell
  and are what the proxy calls. The first is sans-I/O and tested and
  benchmarked; the second is used.
  Neither answer is free. Deleting `FrameRelay` removes about twenty tests and
  a benchmark and orphans `DEFAULT_MAX_INSPECT`, whose only non-import caller is
  the file that would go, so `M15.1`'s bound would need a new home. Rewiring the
  pump onto it means `Wire` driving a push-based state machine from a pull-based
  read, which is a different shape and would re-open work that `e2e.sh` and
  `conformance.sh` have just signed off.
  Filed rather than guessed at, and `check-wired.sh` will not let it be
  forgotten: the marker naming this task is what keeps that check green, and it
  fails the moment the marker stops matching reality in either direction.
  **Decided: `FrameRelay` stays, and stays unwired.** Surveying it rather than
  assuming changed the answer. It has four consumers, all tests, and one of
  them is `conformance_client`, which relays real Postgres responses through it
  byte for byte against Postgres 17 and 18. `budgets.rs` holds it to zero
  allocations and `hot_paths` benchmarks it. That is a reference implementation
  and a test oracle, not rot.
  And it cannot be the production path. `Wire` frames against a socket and
  hands out borrowed slices; `FrameRelay` is push-based and copies what it
  inspects into its own buffer. Routing the pump through it would reintroduce
  the copy `M15.5` removed and the buffering `M16.3` removed.
  **What was actually wrong was the rule being written out twice**, once in
  `FrameRelay::push_header` and once in `wanted_body`, which is one edit away
  from a proxy that buffers more than the component documenting the bound.
  `inspect_budget` is now that rule, in `pgprox-proto` beside the policy it
  reads, and both call it. Two tests: what the rule says, and that the relay's
  own buffering matches it rather than merely staying under the cap.
  The `check-wired.sh` entry is rewritten from a deferred decision into a
  standing argument, which is what that marker is for.
- [x] `M16.11` The pump buffers an inspected body against the relay cap.
  `M16.3` streams every uninspected message, which is the bulk. What it left is
  the other half of the same sentence: for a message something *does* read, the
  pump still does `read_body_into(&mut body, header.body_len)`, and
  `header.body_len` is bounded by `DEFAULT_MAX_FRAME`, which is 1 GiB.
  `inspect_policy` says how much is actually wanted, and it is 8 KiB for an
  `ErrorResponse` and one byte for a `ReadyForQuery`.
  This is `M15.1` again, one layer up. `FrameRelay` was given the cap and the
  pump was not, because the pump does not use `FrameRelay`, which is `M16.10`.
  Reachable from a server rather than from a client, so it needs a compromised
  or hostile upstream rather than anyone who can open a socket. Worth fixing
  because the whole point of two caps is that the parsed one is small.
  Acceptance: the buffered part is bounded by the inspect policy and by
  `DEFAULT_MAX_INSPECT`, and the rest of the body is streamed like any other
  tail, so the bytes forwarded are unchanged.
  A recording session is the exception and keeps reading whole: a truncated
  cache entry is a wrong answer rather than a smaller one. That the cache has a
  bound of its own is assumed here and not checked, which is a separate task if
  it turns out to be false.
- [x] `M16.12` The two primitives `M16.2` added are barely tested. A mutation
  run over `pgprox-session` after the pump rewrite: 440 mutants, seven new
  survivors, all seven in `Wire::read_header` and `Wire::read_body_into`.
  Six are in `read_body_into`, including one that replaces its whole body with
  `Ok(())`. A function whose body can be deleted without a test noticing has no
  test. It is reached from the pump, which is in `bin/` and is not mutated, and
  from nothing in its own crate.
  The seventh is `read_header`'s `1 + LEN_PREFIX` becoming `1 * LEN_PREFIX`,
  four rather than five, which consumes one byte too few and starts every body
  one byte late. `M10.7` found the same mutant in `FrameRelay` and a test was
  written for it there; the new copy of the logic did not get one.
  That is the milestone's own subject twice over: a thing written and not
  reached, and a lesson learned in one place and not carried to the next.
  Acceptance: tests in the crate that owns them, killing all seven or arguing
  each survivor.
  **All seven killed. 440 mutants, 2 surviving, both pre-existing and argued.**
  The last one took three attempts and the reason is the finding underneath the
  finding. `n - body.len()` and `n + body.len()` are the same number on the
  first pass, because `body.len()` is zero, so they can only disagree on a
  second one: the first read has to come up short *and* the second has to have
  more waiting than is still wanted. Two tests passed without producing that
  and proved nothing. Sizing the duplex to exactly the first piece forces the
  writer to block and the reader to come back, and the mutant then takes 206
  bytes for a 200-byte body and eats the next frame's header.
  Verified by applying the mutation by hand and watching the test fail, rather
  than by inferring it from coverage.

## M17: the assumptions the last two milestones wrote down

- [x] `M17.1` A recording session holds the whole answer, unbounded. `M16.11`
  let a session recording for the query cache keep reading whole bodies, on the
  stated assumption that the cache bounds its own entries. The assumption is
  true where it was checked and false where it matters: `Store::put` rejects an
  entry larger than the budget, but the pump accumulates every frame into
  `recording.frames` for the entire answer and only offers it at the end. A
  500 MB result is 500 MB held and then thrown away.
  So the cache's guard protects the cache and nothing protects the proxy, which
  is the same shape as `M15.1`, where a documented bound guarded a structure
  nothing used.
  The pump cannot ask the cache for its budget: it holds
  `Arc<dyn QueryCache>` and the trait has `get` and `put`. Adding to that trait
  is a core-contract change, and it is also the wrong answer. The number wanted
  here is not the cache's budget, which is one global figure for a store; it is
  how much this session is willing to hold speculatively, which is spent per
  connection and multiplied by a hundred thousand. Two resources, two guards.
  Acceptance: recording stops at a bound and the session falls back to the
  streaming path for the rest of the answer, with a test that a large answer is
  forwarded intact after recording gives up.
  Found by checking an assumption I wrote into a commit message rather than by
  reading the code again.
- [x] `M17.2` The binaries mutation testing never reached. `scripts/mutants.sh`
  lists the fourteen crates under `crates/`. `bin/pgprox` and `bin/pgload` are
  packages with their own lib targets, they are held to the same 95% coverage
  gate, and no mutant has ever been run at either.
  This is `M14`'s subject one level out, and it got worse during `M16` rather
  than better: `wanted_body`, `client_body_wanted`, `read_client_body`,
  `record_frame`, `stream_body`, `forward_header` and `MAX_RECORDED_ANSWER` are
  the decisions that milestone turned on, and every one of them is in
  `bin/pgprox/src/serve.rs`. Three mutation runs during that work tested
  `pgprox-proto` and `pgprox-session` and found seven survivors between them,
  which is the reason to think the untested half is not clean either.
  `M14.4` settled which crates stay out of the list and wrote down why. The
  binaries were never in that argument; they were not considered.
  Acceptance: both binaries in the list, the survivors killed or argued, and
  `m14-complete.sh`'s check that the list matches its own criterion updated to
  cover them, since it currently walks `crates/*/` and would not notice.
  **134 survivors: 109 in `pgprox`, 25 in `pgload`.** Milestone-sized, like
  `M14`, so it is decomposed below by where the code came from rather than done
  in one commit.
- [x] `M17.3` The mutants in the code `M16` and `M17.1` wrote. Seven of the
  `serve.rs` survivors are in functions added during the streaming work, which
  is the half of it never mutated because `bin/` was not in the list:
  `read_client_body`'s guard, all four of its mutants, so the re-read fallback
  is unconstrained; `record_frame`'s bound at exactly the limit; `stream_body`'s
  per-chunk flush, whose whole reason is to keep the write side bounded and
  which nothing asserts; and `forward`'s length arithmetic, where
  `body.len() + 4` becoming `* 4` survives.
  `forward` is the one to note: `M16.3` added `forward_header` and tested its
  length, and the older `forward` beside it has computed the same length since
  M6 with nothing checking it.
  **Six of the seven killed, each verified by applying the mutation by hand and
  watching a suite fail rather than by inferring it from coverage.** The
  seventh, `tail > 0` becoming `tail >= 0`, is equivalent: `tail` is a
  `usize`, so the left half is always true and the guard collapses to the
  decode check, after which the only difference is a zero-length `append_body`
  and returning a zero that was already zero. It carries a re-triage condition.
  The `stream_body` one needed a property rather than an assertion: flushing
  per chunk produces the same bytes either way, so what had to be stated is
  that a peer with a window smaller than the body still receives it, which
  queue-then-flush cannot do.
- [x] `M17.4` **80 survivors remained after the first round. Six are argued in
  the baseline and the rest are killed.** Stated that way rather than as a
  running total against the original 86, because the two figures do not add:
  each run re-tests what the previous one fixed, and a mutant that was never
  reached is absent from a missed list exactly as a caught one is. The rest of
  `bin/pgprox`: originally 86 survivors across `run.rs`, `observatory.rs`,
  `metrics.rs`, `gossip.rs`, `entry.rs`, `wiring.rs`, `sessions.rs`,
  `replicas.rs`, `admin.rs`, `http.rs`, `drain.rs`, `dial.rs`, `logging.rs` and
  `main.rs`. Pre-existing, and untouched by this review.
  The first round did the `M14` move: pure decisions extracted out of async
  orchestration so they can be tested at all. `run.rs` gave up
  `descriptors_are_short`, `wants_more_quota` and `share_per_key`; `metrics.rs`
  gave up `count_by_state` and `count_by_tenant`.
  The second round is the same move at scale, plus the scaffolding this entry
  used to say was needed. `run.rs` gave up `pools_for`, `drain_step`,
  `tenants_to_forget`, `parse_descriptor_limit` and two log-gate predicates;
  `observatory.rs` gave up `local_in_use`, and four open-coded `active + idle`
  became `PoolStats::total`, which is the arithmetic core already owns and
  already tests; `serve.rs` gave up `Pumping::swallow_one`, one rule that had
  been written out twice with five mutants living across the two copies;
  `entry.rs` gave up `static_admin_from`, for the reason `parse_descriptor_limit`
  was split, an environment read this workspace cannot fake.
  **Two defects rather than missing tests.** `Sessions::set_pinned` incremented
  its counter outside the `if let`, so a pin was counted for a client that had
  already gone: `pgprox_pin_total` could climb while `SHOW CLIENTS` showed
  nothing pinned. Its sibling `shed` has had a test asserting the opposite since
  it was written, and nothing had ever asked `pins` the same question. And
  `apply_quota` locked, cloned and filtered the whole pool map twice for every
  configured server, every tick; folding both loops into `pools_for` made that
  one read, which was not the goal and is the better outcome.
  **Three tests that did not test what they are named.**
  `a_cancel_for_a_held_query_reaches_the_server` asserted only that the session
  finished, which it does whether or not anything is cancelled, so replacing
  `cancel` with `Ok(())` survived; it now runs against the catcher and asserts
  the key arrives. `a_server_name_tls_cannot_verify_is_refused_before_dialling`
  asserted the error kind and not the reason, so deleting the arm that resolves
  the name before connecting survived: the dial still failed, later and for a
  different reason. And every `TlsMode::Verified` test in `dial.rs` asserted a
  *failure*, so nothing had ever proved an upstream TLS connection can succeed
  and the arm that performs the handshake could be deleted whole.
  **A flaky test caught before it landed, not after.** The new test for the peer
  table took the first connection its listener accepted, and the node's own
  gossip round dials that address once a tick: it failed about one run in eight
  with a digest where the cancel should have been. Found by running it twelve
  times rather than once, which is the habit `M17.5` earned.
  **A claim of mine that the final run falsified.** The baseline entry accepting
  the *trait default* for `CancelSink::clients` said the overriding
  implementation on `Context` "is a separate mutant and is caught". It was not:
  the full run reported it missed, and returning an empty list there makes
  `SHOW CLIENTS` at cluster scope report every peer's sessions and none of this
  node's. Fixed rather than reworded, so the entry is true now. Writing "and
  the other one is caught" into a baseline entry is asserting a measurement,
  and this one had not been taken.
  **What is argued rather than tested**, in `product/mutants-baseline.txt`:
  `main`, which cannot be called from a unit test; `CancelSink::clients`, whose
  mutant is textually the trait default; `read_client_body`'s `>` against `>=`,
  which differ only at `tail == 0` where both return `Ok(0)`; `Drain::settled`'s
  `<` against `<=`, which differ only when a real monotonic clock reads exactly
  the deadline and even then only by one extra poll, and which survived ten
  hand-run passes of the whole suite; `terminated`,
  which needs a real signal delivered to the test process; and
  `Options::static_admin`, whose only distinguishing input is an environment
  variable this workspace cannot set without `unsafe`.
  **The last one was scaffolding, and the move is what fixed it.** `reset_pool`
  deleting `idle_timeout: Duration::ZERO` needed a pool holding a real idle
  upstream connection, which needs a backend that completes the Postgres
  handshake. That fake existed in `serve.rs`'s own test module where nothing
  else could reach it, so it is now `bin/pgprox/src/fakepg.rs`, a `#[cfg(test)]`
  module both files use. A move rather than a second fake: `serve.rs` drives
  seventy tests through it and duplicating fifty lines of protocol into
  `observatory.rs` would have been two fakes to keep in step.
  The test turns on the difference between the two configs rather than on the
  reset doing something. A connection released a moment ago has been idle for
  nothing, so a reset with a zero timeout closes it and the default thirty
  seconds keeps it. An operator asking for a reset, being told "0", and finding
  the pool still holding the connections they wanted gone is the failure; the
  number reported is the one they read.
  **A consequence to state rather than leave to be discovered.** `M17.2` put
  both binaries in `scripts/mutants.sh`, and CI's nightly `mutants` job runs it
  with no arguments, so that job is red until this task closes. That is the
  correct state: they were absent, the absence was the defect, and a nightly
  reporting untested decisions is worth more than one reporting nothing. `M11`
  ran a gate under `continue-on-error` while its milestone was open and `M12`
  called that a gate that cannot fail, so hiding this behind a flag is the one
  option that is not available. `M17.7` is the separate finding this round's
  measurement produced, and the final full run confirms its diagnosis from the
  other side: 568 mutants at `--timeout 300 --jobs 4`, **zero timeouts**, and
  all nine `pools_for` mutants that the sixty-second run called timeouts came
  back caught.
- [x] `M17.5` `pgload`: 25 survivors, mostly in `run.rs`. The load generator,
  whose numbers `M7` and `M11` drew conclusions from.
  Eighteen killed, seven argued. The kills came from two directions. Pure
  decisions came out of a spawned task: `ramp_delay`, `connection_seed`,
  `Refused::is_relocation` and `Refused::told` are functions with their own
  tests now, and the seed one matters because reproducible across a run and
  distinct within it is the pair the whole generator rests on.
  The other direction was fakes that did not exist, and that is the part worth
  keeping. `Fake::Refusing` refuses every connection, so a run against it ends
  in `NoConnection` with no report to read, and `Fake::Working` never refuses.
  Neither could produce a run containing both a refusal and a report, so the
  connect-failure counters had no observable effect at all. `RefusingOnce`
  closes that. Separately, every fake failed statements with `53300`, so a
  `57P01` on an already-connected client never happened, and that is the shape a
  rolling restart makes: `DrainingEveryOtherStatement` closes that one.
  **The real finding is that this crate's mutation results are not
  deterministic.** Its tests drive real sockets, real sleeps and real deadlines,
  fourteen uses of `TcpListener`, `tokio::time::sleep` and `Instant::now` in one
  file, so a mutant that perturbs timing is caught or missed depending on
  machine load. Three consecutive runs of an unchanged tree caught and missed a
  different subset each time; the ramp guard was missed, then caught, then
  missed again.
  I read the first of those samples as fact and wrote two baseline arguments
  that the second run contradicted. The arguments were not wrong, the
  measurement was, and reading one sample as a result is the mistake worth
  recording. The seven entries now say so, and what they cover is a branch
  around arithmetic that is itself tested.
  A limitation of the harness, found the same way: `mutants.sh` reports a
  baseline entry as "now caught" when it is absent from the missed set, and a
  mutant that was never tested is also absent from it. One run tested 123 of
  124. The warning is weaker than it reads.
- [x] `M17.6` `conformance.sh` leaks every container it starts. 548 Postgres
  containers were running when this was found, all named
  `pgprox-conformance-*`, all started in the preceding two hours, load average
  above eight on an otherwise idle machine.
  The cause is a subshell, which is this project's most repeated shell mistake.
  `PG_PORT="$(start_postgres "$version")"` runs the function inside a command
  substitution, so the `PG_CONTAINER=` it assigns is set in a child and lost.
  The parent's copy stays empty, and both the explicit `stop_postgres` and the
  `trap ... EXIT INT TERM` then have nothing to remove. The comment above the
  trap says "Containers must not outlive a failed or interrupted run", and none
  of them were being removed even on a clean one.
  `M12.6` is the same family: a `fail` called inside a pipeline subshell,
  reported and discarded. `check-drift.sh` gained a lint for that shape and it
  looks for `fail`, not for a lost assignment.
  **A correction, because the first version of this entry overstated it.** It
  said the leak corrupted every performance number taken today. That was a
  guess dressed as a conclusion, and removing all 548 containers refuted it:
  tps stayed at 101 and 102 across two clean runs. The machine is running work
  that has nothing to do with this repo, several `java` processes at about half
  the CPU between them and another project's MySQL, and that is the larger term.
  What can be said is narrower and still worth saying. The leak is real, 548
  idle Postgres servers is real, and it contributed. What cannot be said is how
  much, or that it explains the 146 reading I chased earlier and attributed to
  contention. The honest conclusion is that **wall-clock tps from `e2e.sh` on
  this machine is not a measurement**, and every number quoted from it in this
  session should be read as "the run completed", not as throughput.
  The memory figures are unaffected and this is why they were chosen: 16 MiB to
  512 bytes and 100 MB to 4 KiB are counted, not timed. `bench.sh` opens with
  that argument, callgrind over wall clock, and this is the second time in one
  session that argument has earned its place.
  Acceptance: the container name survives the call, the trap removes every
  container the run started rather than the last one assigned, and a check that
  a completed run leaves none behind. The last part is the one that would have
  caught this, because the leak was invisible from inside a passing run.
- [x] `M17.7` A mutant reported as a timeout is not a mutant that survived.
  `M17.4` measured seven files and got back twelve missed and **three
  timeouts**, in `pools_for`. `scripts/mutants.sh` counts a timeout as a
  survivor, which is correct in principle: a run that was abandoned proved
  nothing. So all three would have needed a test or a baseline entry.
  All three are caught, and each fails a test in **four milliseconds**. Applied
  by hand, one at a time, against the whole suite: `==` to `!=` and `+=` to
  `-=` both fail `a_servers_pools_are_its_own_and_its_count_includes_the_waiters`,
  and returning `(vec![], 0)` fails `the_pools_are_held_to_what_the_cluster_layer_allows`.
  What ran out was the *whole-suite* budget, `MUTANTS_TIMEOUT`, which is 60
  seconds. That number was chosen in `M10.13` against "a suite whose slowest
  test is 0.207s and whose whole run is 0.321s across the four mutated crates".
  `bin/pgprox` is 242 tests and 2.9 seconds on an idle machine, and this run had
  six mutation jobs building and testing in parallel at a load average of 23.
  Sixty seconds is no longer forty-eight times the slowest honest test. It is
  about twenty times the honest *suite*, before contention.
  **The per-test cap is what detects a hang, and it still works.** `M10.13` put
  `slow-timeout = { period = "5s", terminate-after = 2 }` in
  `.config/nextest.toml` precisely so a hung test is killed at ten seconds and
  counted as a failure, which catches the mutant and names the test. Given that,
  the whole-suite budget is only reached when nextest itself wedges, so it is a
  backstop rather than a detector, and a tight backstop converts machine load
  into false survivors. A generous one costs nothing when nothing hangs, because
  the budget is only consumed by a hang.
  This is the same defect `M10.13` found, in the same file, from the other
  direction: that task raised a per-test cap because timeouts were hiding real
  kills, and this one is timeouts *inventing* survivors. Both make a timeout
  mean something other than what the baseline file's entries claim it means.
  **And there is a third face, which is worse than either.** Closing `M17.4`
  turned up `drain.rs`'s `Drain::settled` reporting `< -> <=` as **caught** in
  one full run and **missed** in a targeted one, on identical code. Run against
  the whole suite ten times by hand, the mutant survived ten times. So the kill
  was the anomaly, and the mechanism is the per-test cap doing its job on the
  wrong input: under six-way mutation load an honest test exceeds ten seconds,
  nextest terminates it and reports a failure, and cargo-mutants reads any
  failure as the mutant having been caught. A timeout that invents a survivor
  makes the gate cry wolf. A cap that invents a *kill* makes the gate report
  success for a mutant nothing detected, which is the failure `M12` spent a
  milestone on and the one this whole file exists to prevent.
  That raises the stakes on the fix: the per-test cap cannot simply be raised
  either, because a genuinely hung test then costs the whole-suite budget
  again. What the two numbers have to be is derived from a measured suite under
  the parallelism the run actually uses, and neither of the current two is.
  **Confirmed from the other side while `M17.4` was closing.** A full run of the
  same crate at `--timeout 300 --jobs 4` tested 568 mutants with zero timeouts,
  and all nine `pools_for` mutants came back caught. The only variables were the
  budget and the parallelism, so the timeouts were the budget.
  **The acceptance this task was filed with does not work, and finding that out
  is part of the task.** It asked for the three `pools_for` mutants to report
  caught rather than timed out under the same parallelism. Re-run at the old
  sixty seconds and the same six jobs, all three reported caught. The failure
  depends on what else the machine is doing, so it passes before the fix as
  readily as after, and a criterion that cannot fail is the thing `M12` spent a
  milestone on. Nothing here can be demonstrated by reproduction.
  **One of the two measured numbers was wrong because of a test `M17.4` wrote,
  and it had to be fixed before the derivation meant anything.** The slowest
  test in `bin/pgprox` was `a_cancel_for_a_peers_connection_is_forwarded_from_a_running_node`
  at 4.4 to 5.4 seconds, and it is one commit old. Its fake peer accepted the
  gossip connection and never answered, so every round the node made against it
  waited out `PEER_TIMEOUT`, and the node was inside one when the shutdown
  landed. Making the fake answer with a digest, which is what a peer does,
  took it to 0.04s. Deriving a cap from a suite whose slowest member is slow
  for a fixable reason would have baked that mistake into the constant.
  What can be demonstrated is the margin, so that is what this task produces.
  Measured on this machine with `cargo nextest run -p pgprox`, idle against six
  concurrent suites, six being what `MUTANTS_JOBS` asks for:
  slowest test 2.85s against 6.66s, whole suite 2.93s against 7.17s. The
  ten-second per-test cap was 1.5x the worst honest test rather than the 48x
  `M10.13` derived it as, and the real run is worse than that measurement
  because each worker is also building.
  **The mechanism is verified at the new numbers rather than assumed.** A test
  that sleeps for two hundred seconds, run under the `mutants` profile, is
  terminated at 60.01s and the run reports `test run failed`, which is what
  `cargo mutants` reads as a kill. So the property `M10.13` built still holds:
  a hang becomes a failure rather than a timeout, and it does so well inside
  the three hundred seconds the suite gets.
  Acceptance: both numbers are re-derived from a measured suite under the
  parallelism the run uses, with the measurement written beside them; the
  per-test cap's second edge is documented where the cap is set, because
  nothing said that a terminated test is reported as a *kill*; and a full run
  of `pgprox` at the new settings reports no timeouts and no verdict that
  disagrees with `M17.4`'s hand-checked results.
  Done. The per-test cap is sixty seconds, nine times the worst honest test
  under six-way parallelism and twenty-one times the worst idle; the suite
  budget is three hundred, four times the cap plus the worst loaded suite. Both
  carry their measurement in the file that sets them, and
  `standards/testing.md` now says what a terminated test is reported as, which
  it did not. `scripts/mutants.sh pgprox` then ran 571 mutants with **zero
  timeouts** and seven survivors: the six in the baseline and `reset_pool`,
  which is `M17.4`'s open item. Every verdict agrees with what `M17.4` checked
  by hand, which is the only comparison available given the defect is not
  reproducible on demand.
  The stale sentence in `standards/testing.md` went with it: it named "the four
  sans-I/O crates" as what the nightly mutates, which has been wrong since
  `M14` added ten and `M17.2` added the binaries.

## M18: what the deployment story assumes

- [x] `M18.0` Plan M18. `M17` closed the last survivor and the backlog went dry.
  What surfaced was not more code but three claims about deployment that nothing
  checks, found by answering a question about replacing gossip with the
  Kubernetes API for membership.
  Filed as three tasks and one blocked note. The ADR reconciliation goes first
  because the other two are read against it: a spec written against a document
  that describes a system nobody built would inherit the error.
  `M16` and `M17` also get their roadmap rows here, which they never had. That
  is not bookkeeping: `M10.17` established that a milestone whose completion
  condition does not exist cannot be closed, and both closed without one.
- [x] `M18.1` ADR 0004 describes a system that was not built. It says "SWIM
  gossip over UDP using `foca`, seeded from headless Service DNS. One-second
  protocol period, sub-second failure detection." `bin/pgprox/src/gossip.rs` is
  TCP carrying newline-delimited JSON, addressed by a peer list passed as
  `--peer node=host:port`, with `PEER_TIMEOUT` at two seconds and a 1 MiB read
  cap. There is no `foca` in any `Cargo.toml` and no `UdpSocket` anywhere in the
  workspace.
  Everything the ADR decided *above* the transport is intact and is what the
  property tests hold: the guaranteed share divided by a configured fleet size
  rather than a live count, the free pool leased by the lowest active node, the
  majority requirement, the `ttl + suspect_after` takeover wait. Only the
  transport paragraph is fiction. So this is a correction, not a reversal, and
  the ADR keeps its number and status.
  Acceptance: ADR 0004 describes the transport that exists, with the round shape
  and the two constants; the sentence that made the claim carries a note saying
  what it used to say and that no code ever matched it, because an ADR that
  quietly changes is worse than one that is wrong; and a check that the names it
  cites can be found, so the next drift is caught rather than read.
  Done. The file is renamed to `0004-pairwise-gossip-with-leader-leases.md`,
  because the slug said SWIM too; the number is the identity and does not move,
  and the one link to it is updated. The old sentence is quoted as a blockquote
  rather than paraphrased.
  Two differences got stated rather than left to be inferred: this is all-to-all
  and not SWIM, which is O(N) messages per node per round against SWIM's O(1),
  and it is fine only at the three to five pods the Context names; and discovery
  is static, so nothing learns of a node it was not told about.
  The check is in `check-drift.sh` and matches one construction, ``using `x` ``,
  which two ADRs use: `0003` names `tonic` and is right, `0004` named `foca` and
  was not. It skips blockquoted lines, and that is not a convenience: `0004` now
  quotes the sentence it was wrong about, so a rule that read blockquotes would
  fire on the record of its own finding and the only way to quiet it would be to
  delete the record. All three cases are planted in `tests/gates/negative.sh`,
  including that one, because a gate nobody has watched fail is a claim.
- [x] `M18.2` Nothing separates finding peers from trusting them. The peer table
  is `--peer` flags rendered by a shell loop in the StatefulSet template, and
  membership is derived from digest arrivals. Both are correct and neither is
  swappable, so a deployment that wants the Kubernetes API to supply peers has
  nowhere to put it.
  The seam is discovery, not membership, and the distinction is the whole task.
  Liveness must stay first-party: `membership.rs` counts a peer alive from
  digests that *arrived*, which is what makes a one-way network failure safe,
  and an API server is a third party that would tell a partitioned leader the
  fleet is healthy. That is the two-leaders case ADR 0004's majority rule exists
  to prevent, and `pgprox-cluster`'s stated invariant is that partitions cause
  under-subscription and never over-subscription.
  A spec rather than an implementation, because this crosses `pgprox-core`,
  `pgprox-cluster` and the binary, and non-negotiable 6 puts a trait change and
  every implementor in one commit. Writing it out first is also what `M16.6`
  credits with finding five hazards before any code existed.
  Acceptance: a spec directory with the trait, its default static implementation,
  what a Kubernetes implementation may and may not influence, and an ordered task
  list whose entries each leave the tree green. It must state the one rule that
  keeps the swap safe: an external source may mark a node draining sooner than
  gossip would, and may never mark one alive that gossip has not heard from.
  Done, in `specs/2026-08-02-peer-discovery-seam/`. `PeerSource` is shaped like
  `ConfigSource` deliberately: a watch receiver, an `is_healthy` the `Arc` impl
  must forward rather than default, and a `run_loop` the composition root starts
  without knowing which source it holds. A second mechanism for "a thing that
  changes while a node runs" would be a second set of mistakes, and `M14.34`
  already found both mutants of that `is_healthy` surviving once.
  The rule is in the trait's own doc comment, not only in the spec, because the
  spec is not what somebody reads while changing the code.
  Three things are out of scope with the reason recorded. `fleet_size` stays
  configured: ADR 0004 records that a node cut off from its peers would
  otherwise see `N = 1` and award itself the whole guaranteed total, which is
  the first correction its property test forced. Liveness stays first-party.
  And the node id stays the StatefulSet ordinal, which is the real blocker on
  "pods and a Service" rather than gossip: it is encoded into every `ConnId` so
  a cancel landing on any pod routes to the owner, and moving to a Deployment
  changes what clients see on the wire. That is a separate and larger spec, and
  none of these tasks bring it closer.
  The task list is not filed in this backlog yet. Filing tasks for unscheduled
  work puts entries nobody can start, which is what `M11.0` said about the three
  blocked items.
- [x] `M18.3` A milestone can close with no completion condition.
  `check-drift.sh` walks `scripts/m*-complete.sh` and requires each to be named
  in CI, which is the wrong direction: it checks that existing gates run, not
  that every milestone has one. `M16` has a prose condition in the roadmap and
  no script. `M17` has neither and closed anyway, across seven tasks and three
  commits, with nothing objecting.
  `M10.17` found that a milestone whose completion condition does not exist
  cannot be closed, and `M12` spent a milestone on gates that cannot fail. This
  is both at once: the gate exists, it passes, and the thing it was supposed to
  prevent happened underneath it.
  **The acceptance this was filed with was wrong, and the count was too low.**
  It asked for a rule that every milestone has a `scripts/mNN-complete.sh`.
  Six rows lack one, not two: `M1`, `M2` and `M8` as well. All three have a
  completion condition and it is not called that. `M1`'s is
  `scripts/conformance.sh 17 18`, `M2`'s is a `cargo nextest` invocation
  against `pgprox-auth`, and `M8`'s is four scripts led by
  `scripts/gates/release-check.sh`. A rule demanding the naming convention would have
  failed all three, and `M12.8` says what happens next: a check people route
  around is worse than no check.
  So the rule is the one that is actually true. Every milestone in the status
  table needs a section, that section needs a fenced `bash` block, and every
  `scripts/...` path inside it has to exist. That last part catches the other
  direction, a gate renamed out from under the roadmap, which nothing checked
  either.
  Acceptance: `check-drift.sh` fails when a milestone in the status table has no
  section, when its section names no command, and when it names a script that is
  not there; it does not fail when a milestone points at something other than an
  `mNN-complete.sh`; all four are planted in `tests/gates/negative.sh`, because
  a gate nobody has watched fail is a claim; and `M16`, `M17` and `M18` get the
  gates they never had, with `M16`'s reporting the blocked 100k half rather than
  asserting it passes.
  Done. `M17` had no roadmap section at all, so it gained one. `M18`'s own gate
  is part of the milestone rather than an afterthought, which is what `M18.0`
  said it would be.
  One thing the review caught before it shipped: `m17-complete.sh` shelled out
  to `cargo mutants --list`, and CI installs that tool for the nightly sweep and
  not for the milestone job, so the gate would have failed in CI while passing
  here. It now reports the listing as unchecked when the tool is absent, which
  is the trade `M16`'s gate makes for its blocked half. The milestone job's
  whole argument is that sixteen gates cost less than the coverage job, and a
  tool install for one listing works against it.
- [x] `M18.4` Close M18. The gate exists and passes, so the status row can stop
  saying open and the section can say where it got to.
  Filed as its own task for the reason `M15.8` was: closing a milestone is a
  claim about the whole of it, and bundling that claim into the last piece of
  work makes it look like a side effect of that piece rather than a judgement
  about all of them.
  Acceptance: `scripts/gates/m18-complete.sh` passes, the status row says complete,
  and the section records what the milestone found rather than what it planned.

## M19: a seam for peer discovery

- [x] `M19.0` Plan M19, from `specs/2026-08-02-peer-discovery-seam/`. The spec
  wrote the task list and deliberately left it unfiled; this files it.
  The gate exists from this commit rather than from the last one. `M18.3` made
  a milestone with nothing to run a failure and `M18.0` said a completion
  condition is part of the milestone, so an open milestone's gate cannot wait
  until the end. It checks what has landed, which today is the spec, and gains
  a check as each task lands. That keeps it green while CI runs it.
- [x] `M19.1` `PeerSource`, its static implementation, its fake, and the ADR.
  One commit, because non-negotiable 6 says a `pgprox-core` contract arrives
  whole and `scripts/check-core-contract.sh` refuses the alternative.
  Shaped like `ConfigSource` deliberately: a watch receiver, an `is_healthy`
  the `Arc` impl forwards rather than defaults, and a `run_loop` the
  composition root starts without knowing which source it holds. A second
  mechanism for a thing that changes while a node runs would be a second set of
  mistakes, and `M14.34` already found both mutants of that trait's
  `is_healthy` surviving once.
  Additive, so it is green on its own. The risk is that it stays additive,
  which is what `scripts/check-wired.sh` exists to catch, so `product/wired.txt`
  gains `PeerSource` in this commit with the `?` marker naming `M19.2`.
  Acceptance: the trait, `StaticPeers`, `FakePeerSource` that can publish and
  can go stale, the `Arc` forwarding impl, and an ADR recording the
  discovery/liveness split. Tier 1 tests per the spec's `tests.md`, including
  the negative `is_healthy` case that mutant survived on `ConfigSource`.
  Done, as ADR 0023. One thing the spec did not anticipate: `run_loop`'s default
  could not be tested with a timeout, because this crate depends on tokio only
  for `sync` and the time driver would be a dependency added for one test.
  `config.rs` had already solved that by polling the future by hand with a noop
  waker, so this matches it rather than inventing a second answer, which is the
  same reasoning that made `PeerSource` look like `ConfigSource` in the first
  place.
- [x] `M19.2` `run_with_peers` takes the source, and `entry.rs` builds a
  `StaticPeers` from the `--peer` flags. The signature changes and the three
  consumers still read the table once, at the top of the function, so nothing
  behaves differently.
  Separated from `M19.3` on purpose: it is the widest diff and the least
  thinking, and reviewing it beside the semantic change would hide the semantic
  change.
  Acceptance: every existing test passes unchanged, and `wired.txt`'s `?`
  marker for `PeerSource` goes.
  Done. All 254 `pgprox` tests pass with no edit to any of them beyond the three
  call sites that construct a node, which is what "nothing behaves differently"
  had to mean. The read still happens once, at the top of `run_with_peers`, with
  a comment saying so and naming `M19.3`: the point of splitting was to keep the
  widest diff away from the semantic change, and leaving the temporary read
  unmarked would have made the next commit look smaller than it is.
- [x] `M19.3` The three consumers read the current table rather than a copy.
  `GossipTransport`, `NodeObservatory` and `Context`. The `OnceLock` on
  `set_peers` goes, and its doc comment is replaced rather than deleted: it says
  a second call would mean two answers to who is in the fleet, which was right
  when the answer could not change and is the reasoning a future change will
  repeat.
  Acceptance: a cancel for a node added after `Context` was built is forwarded
  to it; the observatory's fan-out reaches a peer added after construction; and
  a quota request goes to a leader whose address changed.
  Done, and each test publishes *after* the consumer was built, which is the
  only shape that can tell a source from a copy: publishing first would pass
  against either. The cancel one was checked against the old behaviour by
  putting an empty table back in `deliver`, and it fails, so it is testing the
  seam rather than the routing `M6.30` already fixed.
  The `OnceLock` stayed. What it holds is now the source rather than the answer,
  which keeps the property its comment claimed, that there is exactly one thing
  being asked, while letting the answer change. Replacing the comment rather
  than deleting it: it was right when it was written, and the reasoning is what
  a future change will repeat.
- [x] `M19.4` The simulation gains a peer table that changes mid-run.
  This is the task that would catch a future change letting discovery feed
  liveness: a table that grew during a partition would let both sides reach
  quorum, and the cap invariant would break in `pgprox_cluster::sim` rather
  than in production.
  Acceptance: the simulation can add and remove peers while a run is in
  progress, and the property that guaranteed plus outstanding leases never
  exceeds the cap still holds. Plus the assertion the whole seam exists for: a
  source publishing a node nothing has gossiped with does not move quorum.
  Done, and the route there is the part worth keeping. The first version of
  `gossip_over_peers` breached the cap at seed 7 and reduced to a deterministic
  130 against 100. That was filed as `M19.5` and looked like a serious defect in
  the one property with no graceful degradation. It was a defect in the
  simulation: the function sent one way, and a gossip exchange is two. See
  `M19.5` for what that cost and what it says about the model.
- [x] `M19.5` **The cap breach `M19.4` found was a modelling error, and this is
  the correction.** No defect. `gossip_over_peers` sent the initiator's digest
  to its target and nothing back, which models a node heard by nobody while
  hearing everybody. Under that model node 1 keeps its whole view, never stops
  believing it leads, and grants beside the node that replaced it: two leaders,
  one free pool, 130 permitted against a cap of 100 with no network faults at
  all.
  **The transport cannot produce that state.** One connection carries both
  digests. `gossip::speak` sends this node's and reads the peer's back;
  `gossip::answer` merges what arrived and replies with its own. The test
  `two_nodes_learn_about_each_other_in_one_exchange` has asserted exactly that
  since `M6`, and its name is the whole argument. A peer table decides who
  *starts* an exchange; an exchange is symmetric. So a peer table cannot make
  liveness one-way, and that is the property that makes discovery safe to hand
  to a deployment rather than a thing to hope about.
  The reduction is kept and inverted: `a_peer_table_cannot_make_liveness_one_way`
  now asserts that node 1 stays in every view, that exactly one node believes it
  leads, and that greed does not move the cap. Both it and the property test
  were checked against the one-way model and both fail there, so neither passes
  vacuously.
  **What this cost, and the lesson that is not "be careful".** The finding was
  written up, filed, and reported as a serious pre-existing defect in the
  cluster layer before the transport was read. Everything in that write-up was
  true of the model and none of it was true of the system. `M17.5` learned that
  one mutation sample is not a result; this is the same shape one level up, a
  simulation is not the system, and a new simulation is a claim about the system
  that needs checking before its output is believed.
  What made it recoverable is that the reduction was deterministic and had zero
  network faults. A randomized seed-7 failure alone would have sent somebody
  looking at the quota ledger.
  `membership.rs` said "a node that can still send but no longer receives ages
  its peers out and steps down", and the mirror case genuinely is not written
  down anywhere. It is not a gap: the transport makes it unreachable. That is
  now recorded in `gossip_over_peers` beside the model it corrects, because the
  next person to model gossip will reach for one-way sends for the same reason
  this did.
- [x] `M19.6` A `pgload` test fails about one run in three, on a clean tree.
  `run::tests::a_drain_mid_run_is_a_relocation_rather_than_an_error` failed two
  of six consecutive runs with no change to `pgload` or `pgprox-load` in the
  working tree. Tripped over by `M19.4`'s workspace-wide run, which is the only
  reason it was seen: nothing in this session had touched that crate.
  **This is a different finding from `M17.5`'s, and worse.** That task recorded
  that `pgload`'s *mutation verdicts* are unstable, because its tests drive real
  sockets, sleeps and deadlines, and it argued seven baseline entries on exactly
  that basis. An unstable mutation verdict costs an argument. A test that fails
  one run in three costs every CI run a coin flip, and the first response to a
  red build that passes on re-run is to stop reading red builds.
  `M16.8` is the precedent and the shape to copy: a `pgprox-tls` test that
  failed about one run in three, diagnosed rather than retried.
  Acceptance: the test passes twenty consecutive runs, and the commit says what
  made it flaky rather than what was changed to stop it. If the cause is a
  deadline that the machine decides, the fix is a paused clock or an assertion
  that does not depend on ordering, not a longer timeout: `M17.7` is what
  happens when a timing constant is widened without deriving it.
  Done, and it was neither a clock nor a timeout. The fake sent its `57P01` at
  every other *statement*, counted by one atomic that four connections shared,
  and twenty percent of the workload's transactions are wrapped in `BEGIN` and
  `COMMIT`. When the counter landed inside one of those, the client had already
  had a statement succeed, so `Failed::work_lost` was true, `is_relocation()`
  was false, and the run counted an error. The test asserting `errors == 0` was
  therefore asserting something the fake did not guarantee, and the scheduler
  picked. The fake now refuses only between transactions, which is what a
  draining node does; the lost-transaction case it used to produce by accident
  is asserted on purpose by
  `client::tests::a_shutdown_after_a_statement_has_run_is_a_loss_rather_than_a_relocation`.
  Measured rather than estimated: at five times the exposure the old fake
  failed eight runs out of eight and the new one passed eight out of eight, and
  at the committed one second the test passed twenty consecutive runs and the
  whole `pgload` suite six.
  The production code was right throughout. `M19.5` was the same mistake in the
  other direction, so the two go together: a fake that models the wrong thing
  produces a finding about the system that is really a finding about the fake.
- [x] `M19.7` A `pgprox` test passes only because nextest gives it a process.
  `logging::tests::installing_twice_is_not_a_panic` asserts that its own call to
  `logging::init` is the first one in the process, and justifies that with a
  comment saying "this is the only caller of `init` in the test binary". That is
  not true. `entry::tests::a_bad_configuration_path_fails_before_the_runtime_does_anything`
  calls `run_with`, which builds a runtime and blocks on `serve`, and `serve`'s
  first line is `crate::logging::init()`. Two tests in one binary install the
  process-wide subscriber and only one of them may win.
  It is invisible under the gate and deterministic outside it. The tier 1 job
  runs the suite through `cargo llvm-cov nextest`, which gives every test its
  own process, so the whole workspace is green and has been. Plain
  `cargo test -p pgprox --lib` failed eight runs out of eight on this machine,
  which is what a developer or an agent reaching for the obvious command gets.
  Found by `M19.6`, which ran the workspace under `cargo test` rather than under
  nextest while looking for a different flake.
  The second half is the name. "fails before the runtime does anything" is what
  the entry test is called, and the runtime does two things before that failure:
  it starts, and it installs logging for the rest of the process.
  Acceptance: `cargo test -p pgprox --lib` passes twenty consecutive runs, the
  comment in `installing_twice_is_not_a_panic` says something that is true of
  the binary as it stands, and whatever makes it true is not "run it under
  nextest". Note that `INSTALLED` is already the idempotence the module set out
  to provide, so the assertion worth keeping is about `init`'s two return values
  rather than about which caller came first: `M17.4` added `assert!(first)`
  because `init` returning a constant `false` had survived mutation, and that
  mutant has to stay dead.
  Done. `crate::logging::init()` moved out of `entry::serve` and into
  `main.rs`, which is where a process-wide install belongs and which no test
  can reach, so the test binary has one caller again and the comment is true of
  the binary rather than of an intention. `cargo test -p pgprox --lib` passed
  twenty consecutive runs, against eight failures out of eight before.
  `M17.4`'s mutant was checked rather than assumed: `init` made to return a
  constant `false` fails this test three runs out of three, deterministically
  now instead of whenever the ordering allowed it.
  The gate is `scripts/gates/m19-complete.sh`, and it is the untargeted command on
  purpose. Every other run of these tests hides this: nextest gives each test
  its own process, and the gate's own `run_test` uses `--exact`, so both are
  blind to a second caller by construction. What the gate runs is
  `cargo test -p pgprox --lib`, whole and in one process, and it returns 1 with
  the defect put back.
  The entry test was renamed to
  `a_bad_configuration_path_is_a_startup_error_through_the_entry_point`. It was
  called `..._fails_before_the_runtime_does_anything`, and the runtime started
  and installed logging before that failure. A name that says a thing does not
  happen is where nobody looks for the thing happening.
- [x] `M19.8` Close M19. The gate exists and passes, so the status row can stop
  saying open and the section can say where it got to.
  Filed as its own task for the reason `M15.8` and `M18.4` were: closing a
  milestone is a claim about the whole of it, and bundling that claim into the
  last piece of work makes it look like a side effect of that piece rather than
  a judgement about all of them.
  Acceptance: `scripts/gates/m19-complete.sh` passes, the status row says complete,
  and the section records what the milestone found rather than what it planned.
  In particular it records that two of the eight tasks were corrections of
  claims this milestone itself made, because a section that reads as seven clean
  steps would be the same fiction `M18.1` deleted from ADR 0004.

## M20: the protocol layer against pgbouncer, pgcat and odyssey

- [x] `M20.0` Plan M20, and give it a gate that passes from this commit.
  A fourth reading of the protocol layer, against three implementations rather
  than against the crate's own header. `M15` read `pgprox-proto` against its
  rules and against `pgbouncer`; this reads the whole path a frame travels,
  including `pgprox-session` and the relay in `bin/pgprox`, and adds `pgcat`
  and `odyssey` because the first two readings only ever had one outside
  opinion to check against.
  Same rule as `M19.0`: the gate exists from the first commit and gains a check
  as each task lands.
- [x] `M20.1` **A client `Close` of a prepared statement poisons the pooled
  connection.** A protocol `Close` is rewritten to this proxy's global name and
  forwarded, so the server really does deallocate the statement. Neither map
  hears about it. `ConnectionStatements` goes on claiming the connection holds
  it, so the next `Bind` of that SQL takes the already-prepared path and names
  a statement that is gone.
  Reproduced before it was reported: `26000 prepared statement
  "pgprox_533e5fdc2f41216f" does not exist`, from a client that did nothing
  unusual. `Parse`, `Bind`, `Execute`, `Sync`, `Close`, then the same statement
  again is what every driver with a statement cache does when it rotates one.
  **It outlives the session that caused it.** The connection goes back to the
  pool still mis-recorded, so the next session to bind that SQL on it fails the
  same way, until the connection is reaped or the entry evicted.
  This is `M15.3`'s finding through the door that fix left open. That task
  wired `DISCARD ALL` into both maps and wrote that under-clearing produces
  "prepared statement does not exist on a connection the proxy thought was
  warm". The protocol form of the same operation was never wired.
  Why four readings missed it: the extended-protocol fake answered `Close` from
  its `_ =>` arm, as though it were a simple query, and answered every `Bind`
  with `BindComplete` without checking. `M9.24` added the `42P05` arm for
  `Parse` with the note that "the proxy's record of what a connection holds is
  only correct if something notices when it is not", and left the other two
  halves unmodelled.
  Acceptance: the fake models `Close` and refuses a `Bind` for a statement it
  does not hold, and both new tests fail without the fix.
  Done. `resume::on_close` drops the statement from both maps, called from
  `send_upstream` for the same reason the `DISCARD ALL` half is called there:
  it is the first point where the connection the frame is about to reach is
  known, and the name has already been rewritten into the outgoing frame by
  then, so dropping the session's entry cannot leave it untranslatable.
  Checked rather than assumed: with `on_close` made a no-op, the sans-I/O test
  and the end-to-end one both fail; the guard test for an unknown name passes,
  as a test asserting that nothing happens should.
  The fake now removes a closed name and answers `26000` for a `Bind` naming
  something it does not hold. That change alone breaks no other test in the
  file, which is the measure of how blind it was.
- [x] `M20.2` **`options` from the startup packet is parsed, stored, and
  dropped.** `Startup::options` splits `-c name=value` out of the startup
  packet, `StartupInfo::options` carries the result, and its only reader in the
  workspace is the test beside it. Nothing applies it to the upstream
  connection and nothing puts it in the cache key.
  So a client connecting with `options=-c search_path=tenant_acme` runs every
  statement under whatever `search_path` the pooled connection happens to
  carry. `SessionParams` would replay it, and only ever learns of a
  `search_path` set by a `SET` the client sent afterwards.
  `startup.rs` says of this parameter: "That makes this correctness-relevant
  rather than cosmetic: `search_path` is part of the query cache key, because
  the same SQL resolves to different tables under different paths. See ADR 0007
  and the cache module." The key is built from `session.params`, which this
  never reaches.
  pgbouncer's answer is the loud one and worth copying rather than the quiet
  one: `set_startup_options` refuses the connection outright with "unsupported
  startup parameter in options: %s" unless the parameter is tracked or listed
  in `ignore_startup_parameters`. It would rather drop the client than let it
  believe its `search_path` took effect. The same is true of plain startup
  parameters, which this proxy also accepts and forwards none of: only `user`,
  `database` and a hard-coded `application_name=pgprox` go upstream.
  Acceptance: a client that sets a runtime parameter at connect either gets it,
  or is refused and told which parameter. Silently ignoring it is the one
  option this task exists to remove.
  Done, and it gets it in both branches, which is better than what was filed.
  `Relay::on_startup_settings` records every setting so the existing replay puts
  it on whichever connection the session borrows, and a setting outside the
  replayable allowlist pins the session with `PinReason::UnreplayableSet`
  instead of refusing the client. pgbouncer refuses because it has no way to
  keep a setting it cannot track; this proxy does, and a setting arriving at
  connect is the same thing as the `SET` of one arriving later. Refusing would
  have been giving up a capability the design already has.
  The scope is `options` only. Plain startup parameters raise a question this
  task should not answer in passing, and are `M20.7`.
  It cost 72 bytes and there were 72. Carrying the settings as a `Vec` through
  `Ready::Tenant` and borrowing them across the relay loop put the session
  future at exactly 5120 bytes against a 5 KiB ceiling, so
  `one_session_costs_less_than_the_slab_buffer_it_no_longer_holds` failed. The
  threshold did not move: the settings are a boxed slice, moved into the relay
  rather than borrowed by it, and dropped before the loop, which is what keeps
  them out of every one of a hundred thousand connections.
- [x] `M20.3` **A `_pq_.` protocol extension is accepted by saying nothing.**
  `encode::negotiate_protocol_version` takes an `unrecognized` list, and every
  caller outside the fuzz seed corpus passes `&[]`. `negotiate_version` decides
  from the version integer alone, so a client that asks for 3.0 plus
  `_pq_.something` gets no `NegotiateProtocolVersion` at all and is entitled to
  conclude the extension was accepted. It is not: the parameter is not
  forwarded upstream either.
  pgbouncer sends `NegotiateProtocolVersion` when the version is unsupported
  **or** when any `_pq_.` parameter was not recognised, which is the condition
  the protocol actually specifies.
  Acceptance: an unrecognised `_pq_.` parameter produces a
  `NegotiateProtocolVersion` naming it, at any accepted version.
  Done. `Startup::extensions` reads them by their reserved prefix, `negotiate`
  takes whether there were any, and `Reply::negotiate` carries a `Negotiation`
  with both the minor and the names so the encoder's `unrecognized` argument
  finally has something in it.
  Both halves were wrong and each was checked separately: with the decision
  made from the version alone, the handshake test and the wire test fail; with
  the encoder handed an empty list again, the wire test fails. Either fix on
  its own still tells a client its extension was accepted.
  `negotiate_version` is gone rather than kept as a one-argument wrapper. It
  would have been a default argument wearing a function name, and the only
  caller that wanted it is the conformance server, which answers a harness that
  sends no extensions and can say so itself.
- [x] `M20.4` **Nothing says goodbye to an upstream connection.**
  `encode_frontend::terminate` exists and its only callers are tests. A reaped
  connection is dropped, so Postgres sees the socket close rather than a
  `Terminate`, and logs it.
  This matters here more than it would elsewhere: `min_pool` is 0 and the idle
  timeout is 30 seconds, on purpose, so reaping is the steady state rather than
  an exception. Every reap is a log line on the database that looks like a
  client that crashed.
  pgbouncer sends `{PqMsg_Terminate, 0, 0, 0, 4}` from `disconnect_server`.
  This project's own load client already does the same thing and says why:
  "a proxy that saw every churned connection as an abrupt disconnect would be
  measured on its error path rather than on its close path". The proxy does not
  extend its own courtesy to the database.
  Acceptance: a connection closed by the reaper, by `max_lifetime`, or by a
  drain sends `Terminate` first.
  Done for the reaper, which is what `max_lifetime` and the admin reset go
  through too. `Upstreamed::goodbye` writes it and `dial::retire` calls it for
  everything `reap_idle` hands back.
  `reap_idle` returns the payloads now rather than a count, and that is the
  design rather than a convenience: it decides under a `std::sync::Mutex`, and
  the rule at the top of `live.rs` is that the lock is never held across an
  await. So the reaper decides under the lock and the caller says goodbye
  outside it.
  **Only on a clean close.** A connection discarded mid-transaction is in a
  state nobody knows: if the server is in COPY-in it reads `CopyData`,
  `CopyDone` or `CopyFail` and nothing else, so a `Terminate` there is a
  protocol error rather than a courtesy. pgbouncer draws the same line with
  `disconnect_server`'s `send_term` argument.
  Two things worth keeping from doing it. The first negative control passed and
  was worthless: cargo had not rebuilt, and the run printed no `Compiling`. A
  control that does not recompile proves nothing, and the second one, which
  did, failed as it should. The second is that the test waits by parking rather
  than by `yield_now`: on a current-thread runtime the I/O driver only runs when
  the runtime parks, so a yield loop never delivers the read. The interval is
  what a failure costs, not what a pass waits for, and widening it would fix
  nothing.
- [x] `M20.5` **An idle pooled connection is never read.** `Pool::idle` is a
  `VecDeque<Connection>` and nothing polls it: every `tokio::spawn` in
  `pgprox-pool` is in a test. So anything the server sends on a connection
  between borrowers stays in the socket for whoever borrows it next, which will
  read it as the answer to its own frame.
  Two things arrive that way. An asynchronous message: `NoticeResponse`,
  `ParameterStatus`, `NotificationResponse`. And the end of the connection:
  `pg_terminate_backend`, `idle_session_timeout`, a failover, a restart. The
  second is the common one, and what a client sees is its query failing for a
  connection that was already dead when it was handed over.
  pgbouncer runs its packet loop on servers in `SV_IDLE` for exactly this, and
  `release_server` will not hand over a server with anything outstanding.
  Verified structurally rather than measured: what was checked is that no such
  poller exists, not what a client sees when it happens. The task should start
  by making it happen.
  The 30-second idle reap bounds the window, which is why this is filed below
  the ones that need no window at all.
  Acceptance: a connection the server closed while it was idle is not handed to
  a client, and an async message that arrived while idle is consumed rather
  than delivered as somebody's reply.
  Done, and the second half is answered by discarding rather than by consuming.
  `Upstreamed::unfit` polls the socket for readability at the moment the pool
  hands it over: a healthy idle connection has nothing to say, so this costs
  nothing in the common case, and anything readable is either the close or a
  message nobody asked for. Which of the two it is would cost a parse and the
  answer is "discard it" either way, so it is not asked. `fit_connection` takes
  another, up to four times, because the condition that kills one connection
  kills the whole warm pool and a client with one retry meets the second corpse.
  pgbouncer keeps such a connection instead, by running its packet loop on
  servers in `SV_IDLE`. That is the better answer for a proxy with an event loop
  over every server; this one holds idle connections in a map with nothing
  watching them, and a check on borrow is the cheap half of the same guarantee.
  It was reported as costing a client a failed query. It is worse than that:
  with the check disabled the client does not get an `ErrorResponse`, it gets
  its socket closed with `UnexpectedEof`, which every driver reports as a
  network fault against the proxy.
  Not covered: the branch that gives up after four attempts. It is a bounded
  loop's backstop and reaching it needs four stale connections queued in one
  pool, which the harness cannot arrange without the timing being the test.
- [x] `M20.6` **The unnamed prepared statement is turned into a named one.**
  `map_statement_name` rewrites every `Parse` to `pgprox_<hash of sql>`,
  including `Parse` of the unnamed statement. The unnamed statement's contract
  is that the next `Parse` of it replaces it and that it does not persist; the
  rewritten one persists until it is closed or evicted.
  So a driver using the unnamed statement for one-shot queries, which is what
  it is for, has each distinct query become a named statement on the pooled
  connection, occupying a slot under `per_connection_cap` and costing a `Close`
  round trip when the LRU evicts it.
  pgcat carries an `anonymous()` on `Parse`, `Bind`, `Describe` and `Close` and
  excludes those from its rewriting for this reason.
  Acceptance: an unnamed `Parse` is forwarded as unnamed, and the tests say
  what the connection holds afterwards.
  Done. An unnamed `Parse`, `Bind`, `Describe` and `Close` all travel with the
  name they arrived with, and the session still records the SQL so a `Bind` of
  it landing on a connection whose unnamed statement is something else gets a
  `Parse` first. Re-parsing is always legal for this one: it replaces rather
  than collides, so there is no `42P05` to avoid the way there is for a name.
  The connection tracks it outside `held`, because putting it there would let
  something the server does not keep evict something it does, and because
  `per_connection_cap` is counting persistent statements.
  It is a hash rather than the SQL, and that is not an optimisation. This
  struct lives inside `Upstreamed`, which the session holds across every await
  in the relay loop, so a `String`'s twenty-four bytes are twenty-four bytes per
  connection at a hundred thousand of them:
  `one_session_costs_less_than_the_slab_buffer_it_no_longer_holds` failed by 56
  and there were 16. Zero means "nothing here", and SQL that hashes to zero
  reads as absent, which sends a `Parse` that was not needed rather than
  skipping one that was.
  The test that matters is the unit one on the rewrite, not the sequence: both
  behaviours produce a working sequence, and the difference is which name left
  this process.

- [x] `M20.7` **Plain startup parameters are accepted and forwarded nowhere.**
  `M20.2` did the `options` half, which is where `search_path` lives and where
  the correctness case is. The other half is the parameters a client sends
  directly in the startup packet: `client_encoding`, `DateStyle`, `TimeZone`,
  `application_name`, `extra_float_digits`. `StartupInfo` does not even carry
  them; `state.rs` keeps `user`, `database` and `options` and drops the rest.
  The upstream startup packet is `user`, `database` and a hard-coded
  `application_name=pgprox`.
  It was left out of `M20.2` because `application_name` is a real question and
  should not be answered in passing. This proxy sets its own upstream on
  purpose, so a DBA reading `pg_stat_activity` sees which process holds the
  connection, and `probe.rs` lists it as the one parameter deliberately not
  reported back to the client. Honouring a client's `application_name` reverses
  that. pgbouncer honours it and has `application_name_add_host` for the
  operability half; this proxy has the tenant and user visible in the pool key
  already, which may make the trade different here.
  Note that the two halves disagree until this lands: `options=-c
  application_name=x` is honoured as of `M20.2` and `application_name=x` is
  not.
  Acceptance: the question above is answered in the commit rather than
  implied, `StartupInfo` carries what the client sent, and a client's
  `client_encoding` either reaches the server or pins.
  Done, and the answer is that the client's `application_name` wins. A
  connection actively serving a tenant showing that tenant's application is the
  more useful of the two facts available to a DBA, and which node holds a
  connection is already in the pool key. pgbouncer honours it too. `probe.rs`'s
  separate rule stands and is a different rule: `pgprox` is still not reported
  back to a client as its own application name, and the upstream startup packet
  still says `pgprox` for a connection nobody has claimed.
  `StartupInfo::options` became `StartupInfo::settings`, one field holding both
  forms in the order they apply: plain parameters first, then the ones from
  `options`, so `options` wins a disagreement. One field rather than two
  because they are one question, and because two cost twenty-four bytes of
  session future that were not there.
  The excluded four are `user`, `database`, `options` and `replication`, each of
  which has a meaning the protocol gives it rather than a value a session
  carries, plus the reserved `_pq_.` prefix that `M20.3` answers.
- [x] `M20.8` **`replication` is ignored rather than answered.** A client
  asking for a replication connection gets an ordinary one, and finds out when
  `IDENTIFY_SYSTEM` fails oddly. pgbouncer checks the parameter before anything
  else and routes the connection differently.
  Nothing in this proxy could serve one: a replication connection is a session
  by definition, and `CopyBothResponse` pins, so the honest answers are to
  refuse it by name or to pin it from the startup packet. Ignoring it is the
  one that produces a confusing failure a long way from its cause.
  Acceptance: a client that asks for replication is told something true at
  connect.
  Done, and it is refused rather than pinned. Pinning was the other candidate
  and it would make the thing half-work: the connection this proxy opens
  upstream carries no `replication` parameter of its own, so the backend on the
  far side is not a walsender whatever this side does with the session. Saying
  no is the only answer that is true.
  `ClientError::Unsupported` is a new variant and `0A000
  feature_not_supported` a new code in `standards/error-handling.md`. It is the
  client being right and the proxy being unable, which is a different thing
  from `08P01`, where the client is wrong, and a client told the difference
  knows whether to fix its request or its expectations. It is also the one
  error whose detail reaches the client: what it carries is this proxy's own
  statement about its own capabilities, not anything derived from a credential,
  a tenant or an upstream, so the rule about what an error must not contain is
  not bent by it.
  The value is the question rather than the parameter's presence: `false`,
  `off` and `0` are ordinary clients, and `true`, `on`, `1` and `database` are
  not, which is how Postgres reads it and what libpq sends.

### What was checked and found sound

Recorded because a review that only lists what is wrong reads as a list of
everything that was looked at, and this was not.

`SessionState`'s release rule against `pgbouncer`'s `server->ready`: sound, and
`Outstanding` is the same idea as pgbouncer's `outstanding_requests` list,
arrived at independently and covering `Flush`, which is the case that hangs.
SCRAM channel binding: `SCRAM-SHA-256-PLUS` is refused by name with the right
argument, which is that this proxy terminates TLS and the binding a client
would verify is to the proxy. A duplicate `Parse` under a name the connection
holds is answered locally rather than provoking `42P05`. A pinned session's
connection is discarded rather than returned, because `UpstreamGuard` defaults
to `Discard` and only a transaction boundary marks it clean, so a `LISTEN`
registration cannot outlive the session that made it. `GSSENCRequest` is
recognised so it can be refused rather than read as a version.
- [x] `M20.9` Close M20. The gate exists and passes, so the status row can stop
  saying open and the section can say where it got to.
  Filed as its own task for the reason `M15.8`, `M18.4` and `M19.8` were:
  closing a milestone is a claim about the whole of it, and bundling that claim
  into the last piece of work makes it look like a side effect of that piece
  rather than a judgement about all of them.
  Acceptance: `scripts/gates/m20-complete.sh` passes, the status row says complete,
  and the section records what the reading found rather than what it planned.
  In particular it records which of the findings came from reading a second and
  third implementation and which came from the hunt for the first one, because
  those are different arguments for doing this again.

## M21: the driver matrix does not cover what M20 changed

- [x] `M21.0` Plan M21, from what running the existing suites turned up, and
  give it a gate that passes from this commit.
  **`scripts/driver-matrix.sh` already runs all five drivers against
  `bin/pgprox` over TLS onto a real Postgres.** It has since `M8.13`, and
  `tests/proxy-drivers/_env.sh` says why: asyncpg deadlocked on its first
  parameterised query from M6 to M8 and `scripts/conformance.sh` was green
  throughout, because the harness answers a `Flush` the same wrong way the
  proxy did.
  I proposed building that suite before checking whether it existed. It did.
  The plan named the wrong milestone and this one is what was actually found.
  Two things, both from running what is here rather than from reading it.
  **First: the matrix passes and covers none of what `M20` changed.** Its five
  depths are both wire protocols, a prepared statement reused, a result larger
  than one segment, a transaction, and an error with a statement after it. The
  five behaviours `M20` added are a protocol `Close` and a re-prepare, the
  unnamed statement, a `search_path` from `options`, an `application_name` from
  the startup packet, and a refused `replication` connection. No driver probes
  any of them.
  **Second: the report is committed evidence with a date on it and nothing
  notices when it rots.** `product/conformance/driver-matrix.md` said
  "Generated on 2026-07-28" until this commit, thirteen milestones and one
  wire-behaviour change later. `m1f-complete.sh` checks the script exists and
  the report exists; neither check would fail on a report that predates
  everything it is evidence about.
  The refreshed report is in this commit, and running it is what produced both
  findings. Every driver still passes: `M20` broke nothing the matrix covers,
  which is a result about `M20` rather than an absence of one.
- [x] `M21.1` A stale matrix report says so. The report carries a date and the
  repository knows when the proxy last changed, so a report generated before
  the code it describes is mechanically detectable.
  ~~Acceptance: a gate fails on a report older than the newest commit touching
  `bin/pgprox`, `crates/pgprox-session` or `crates/pgprox-proto`~~, and passes
  on the report as regenerated. `M18.1` is the shape: evidence that describes a
  tree that no longer exists is worse than no evidence, because it is quoted.
  **That acceptance criterion was wrong and this corrects it.** Regenerating the
  report needs Docker, a built proxy image and five driver toolchains. A gate
  that failed on any proxy commit until someone ran all of that would be red
  from the first edit and permanently red in CI, which has none of it.
  `check-core-contract.sh` names what it would become: a rule people route
  around, and it kept only the halves that can be met.
  So the report records the newest commit touching those three paths at
  generation time, the gate **fails** when that line is absent or names a
  commit the repository does not have, and reports staleness as a count with
  the commit subjects behind it. A date could never be checked because nothing
  knows what the code looked like on it; a commit can.
  Named rather than counted alone: "three commits behind" is a number, and
  "behind M20.4 and M20.6" is what says whether it matters.
- [x] `M21.2` The statement-cache rotation, in every driver that has one.
  A protocol `Close` followed by re-preparing the same SQL is `M20.1`'s exact
  reduction, and pgx, JDBC and npgsql all produce it when their caches evict.
  Verified by hand against pgx during `M21.0`: prepare, `DeallocateAll`,
  prepare again, which passes. Nothing runs it.
  Acceptance: the probe for each driver that keeps a server-side statement
  cache rotates it, and the case fails against the tree before `M20.1`.
  Done, and the acceptance criterion earned its keep. The first version used
  pgx's `DeallocateAll` and npgsql's `UnprepareAll`, and **both passed with
  `M20.1` reverted**: those send `DEALLOCATE ALL` as SQL, which the proxy has
  handled through `deallocates_everything` since `M15.3`. Two of the three
  probes were testing a fix from five milestones ago while claiming to test
  this one. Only JDBC fired, because its case is a cache eviction and that is a
  real protocol `Close`.
  So pgx deallocates one statement by name and npgsql unprepares one command,
  and all three now fail against a proxy built without `M20.1`: pgx reports
  `26000 prepared statement "pgprox_7aed9f0df11d4c23" does not exist`, npgsql
  aborts inside `FetchPreparedStatement`, and JDBC says the statement does not
  exist. All three pass with it.
  The JDBC case is the one worth keeping for its shape: nobody calls anything.
  `preparedStatementCacheQueries=2` and three distinct statements make the
  cache evict on the third, which is how a real application produces this
  sequence.
- [x] `M21.3` The unnamed statement, in the drivers that use it.
  pgx reaches it through `QueryExecModeExec` and asyncpg through its own
  one-shot path. `M20.6` changed what goes on the wire for it and the only
  assertion is a unit test on the rewrite.
  Acceptance: a driver runs a one-shot parameterised query more than once and
  the session survives it.
  That criterion was too weak and this says so rather than meeting it. Both
  behaviours produce a working sequence, which `M20.6`'s own commit says: the
  difference is which name left the process. A probe that only ran queries
  would have passed against the rewrite it exists to catch, which is `M21.2`'s
  mistake in a new place.
  So pgx counts what the server was left holding. Three one-shot queries inside
  a transaction, so they land on one connection, then
  `pg_prepared_statements`: zero with `M20.6`, three without it, measured by
  building the image both ways.
  Two things the counting got wrong first. Matching `name LIKE 'pgprox\_%'`
  counted the named statements this same probe legitimately prepares and said
  four when it meant zero, so it matches a marker in the SQL instead. And the
  count query's own text contained that marker, so it counted itself and said
  four where three was true; the pattern is split as `'%unnamed' || '_probe%'`
  so the statement cannot match itself.
  asyncpg's case stays a behaviour check rather than a count. Its one-shot path
  is the one that deadlocked from M6 to M8, and what is worth asserting there
  is that it still completes.
- [x] `M21.4` The startup path, through a driver rather than through psql alone.
  `search_path` from `options`, `application_name` from the startup packet, and
  a `replication` connection refused by name. All three verified by hand during
  `M21.0` and none of them runs.
  psql is the right driver for the first two, since what is under test is the
  packet libpq builds. The refusal deserves a second driver: a client that
  cannot start is the one case where every driver reports differently.
  Acceptance: each of the three fails when its fix is reverted.
  Done, and all three were checked by building the proxy image with the fixes
  removed rather than by reasoning. Without them, psql reports `search_path`
  as `"$user", public`, which is the server default the session used to run
  under, and pgx reports that a replication connection was accepted.
  The refusal is asserted on the message rather than only on the failure. From
  outside, a stack that is down, a token that expired and a feature that is not
  offered all end as "connection failed", so a case that only checked for an
  error would pass for two wrong reasons.
  And both drivers check that an ordinary connection still works afterwards. A
  proxy refusing everything would satisfy every refusal assertion here.
- [x] `M21.5` Close M21. Filed as its own task for the reason `M18.4`, `M19.8`
  and `M20.9` were.
  Acceptance: the gate passes, the status row says complete, and the section
  records that the milestone began by proposing to build something that already
  existed.

## M22: the mutants nobody has swept since M17

- [x] `M22.0` Plan M22, and give the baseline the provenance the matrix report
  got in `M21.1`.
  `product/mutants-baseline.txt` was last written by `M17.4` on 2026-08-01, and
  eighteen commits have landed on the mutated crates since: all of `M18`,
  `M19`, `M20` and `M21`. Everything those milestones added has never been
  mutation tested. `resume::on_close`, `Relay::on_startup_settings`,
  `startup::negotiate`, `extensions`, `settings` and `replication`,
  `Upstreamed::unfit` and `goodbye`, `dial::retire`,
  `ConnectionStatements::note_unnamed` and `holds_unnamed`,
  `ClientError::Unsupported`, and the whole `PeerSource` seam.
  **And nothing notices the baseline is stale.** Four gates read its contents,
  `m10`, `m14`, `m15` and `m17`, and none of them asks whether it describes the
  tree it is a claim about. That is `M21.1`'s finding in the file four gates
  depend on, so it gets the same answer: the baseline records the newest commit
  touching the crates it covers, and the gate reports how far behind it is.
  Coverage says a line ran; this says the line mattered. `M17`'s sweep of
  `pgprox` found two real defects, a pin counted for a client that had gone and
  a lock taken twice per server per tick, and neither was visible to any test.
- [x] `M22.1` Sweep `pgprox-session` and argue every survivor.
  First because it is where `M20`'s real defect lived and where its biggest
  behaviour change is: `on_close` and `on_startup_settings` are both new
  decision logic with nothing but line coverage behind them.
  Acceptance: `scripts/mutants.sh pgprox-session` passes, every accepted
  survivor carries an argument rather than `untriaged`, and any mutant that
  turns out to be a missing test gets the test rather than an entry.
  455 mutants, 6 surviving, 4 of them new and every one in code `M20` added
  five commits earlier. `goodbye` replaced with `()` survived, meaning the
  `Terminate` write could be deleted; `unfit` survived returning nothing,
  `false` and `true`, and all three answers surviving is a function with no
  test at all.
  Both are exercised end to end by the probes `M20.4` and `M20.5` added, and
  both of those live in `bin/pgprox`. A cross-crate integration test does not
  discharge a crate's own obligation, and this crate's `AGENTS.md` is the one
  that names mutation testing and calls itself the most correctness-critical
  code here. So four tests, in the crate where the bytes are produced.
  The two survivors that remain are the pre-existing `sequence.rs` pair, both
  argued equivalent: the replacement yields a program no test can distinguish.
  Down to 2 surviving out of 455, coverage 97.75%.
- [x] `M22.2` Sweep `pgprox-proto`. `M20.3` added `negotiate`, `extensions` and
  `settings`, and `M20.8` added `replication`, all of which are predicates
  whose wrong answer is a client told something untrue at connect.
  Acceptance: as `M22.1`.
  378 mutants, 6 surviving, none of them new. The prediction that opened this
  task was wrong: the four predicates `M20` added are all killed, and the shape
  `M22.1` found is absent here.
  The difference is where their tests live. `M20.3` and `M20.8` put theirs in
  `startup::tests`, in this crate, beside the functions. `M20.4` and `M20.5`
  put theirs in `bin/pgprox`, and that is exactly the pair whose mutants
  survived. Same milestone, same author, one week apart; the only variable is
  whether the crate tested its own decision.
  The six that survive are the ones already argued: three comparison operators
  in `FrameRelay`'s buffer arithmetic, the `^` in `conn_id_from_key`, the SASL
  preference `<=`, and an arm of `SessionState::on_frontend`. No baseline entry
  is stale, which the sweep checks: it warns on an accepted mutant that is now
  caught, and warned about none.
- [x] `M22.3` Sweep `pgprox-pool`. `M20.6` added `note_unnamed` and
  `holds_unnamed`, and the second is a comparison with a sentinel, which is the
  shape a mutant survives most easily.
  Acceptance: as `M22.1`.
  284 mutants, zero surviving, and no baseline entries at all. The sentinel
  comparison in `holds_unnamed` was the thing to watch and it is killed, along
  with everything else in the crate.
  Third data point for what `M22.2` found: `M20.6` put its tests in this crate,
  beside the functions, and its mutants die. The two that survived in `M22.1`
  are still the only pair whose tests were written somewhere else.
- [x] `M22.4` Sweep `pgprox`. The largest of the four and the one `M17.4` spent
  a milestone on: 571 mutants and eighty minutes then, and `M20` and `M21` have
  added to `serve.rs` since.
  Acceptance: as `M22.1`.
  590 mutants, 10 surviving, 4 new and all four in `M20.6`'s unnamed-statement
  code. Down to 6 of 592 after the tests.
  **`M22.2`'s rule does not explain these, and that is worth saying: these had
  tests in this crate.** In-crate is necessary and not sufficient. The check
  that survived lived inside `ready_statement`, which takes an `Upstreamed` and
  therefore a socket, so only an end-to-end test could reach it and none
  covered the branch.
  The one that matters is `holds_unnamed` replaced by `true`. That mutant is a
  session which moved connections binding against whatever the previous
  borrower left unnamed: not an error the client sees, the wrong query's rows.
  It survived every test in the file.
  Fixed by the rule `M22.7` had just written down. `prepare_unnamed` takes the
  statement map and no socket, so the decision is testable where it is made.
  The other three were `unnamed_statement` returning `None`, which makes the
  whole branch dead, and the `Describe`/`Close` guard going `false`, which puts
  this proxy's global name on a `Describe` of a statement the server knows as
  the unnamed one.
- [x] `M22.5` Sweep `pgprox-core`.
  Filed as one task for twelve crates and split here, because twelve commits
  all naming `M22.5` would satisfy the commit-msg gate and break the rule it
  exists to serve. A crate whose sweep changes code gets its own task; the ones
  that come back quiet share one, since recording eleven clean sweeps is one
  coherent change that reverts as one thing.
  560 mutants, 5 surviving, 1 new, and the new one was neither a missing test
  nor machine contention.
  `is_word_char` replaced by `true` came back as `Timeout`, which
  `survivors_of` counts as a survivor by an explicit choice `M17.7` argued for.
  Its three siblings, `false`, `&&` and `!=`, were all caught, which is what
  made "missing test" the wrong reading: the function is tested, and there is
  an assertion that `!is_word_char('(')` which a constant `true` fails in
  microseconds.
  **What it was hiding is a latent hang.** `Lexer::next` guards its word arm
  with `is_word_char` and then calls `word_end` to decide how far to advance,
  and `word_end` does not call `is_word_char`: it restates the same rule inline
  over bytes, because it is the innermost loop of the route decision at 3.6
  million calls per replay. So the guard trusts one implementation and the
  advance uses the other. Disagree about one character and the guard accepts
  it, `word_end` returns zero, `advance(0)` consumes nothing, and `next` spins
  forever on a live connection.
  That is the hazard `pgprox-pool`'s `AGENTS.md` warns about by name, "do not
  write another SQL scanner", occurring inside the scanner between two halves
  of one rule.
  Fixed with a `debug_assert!` that the lexer consumes something, which
  documents the invariant where it can be violated, costs nothing in release,
  and turns the hang into a caught mutant because tests run in debug. Not a
  test, because a hanging test cannot assert; not a baseline entry, because the
  mutant was pointing at something real. Down to 4 of 560, all baselined.
- [x] `M22.8` Sweep the eleven that remain: `pgprox-route`, `pgprox-cache`,
  `pgprox-cluster`, `pgprox-admin`, `pgprox-auth`, `pgprox-config`,
  `pgprox-observe`, `pgprox-load`, `pgprox-tls`, `pgprox-testkit`, `pgload`.
  `M19`'s seam touched `pgprox-cluster`, so that is the one with new logic; the
  rest are re-baselines that should be quiet, and are worth running because
  "should be quiet" is a prediction rather than a fact.
  One commit if they are all quiet. Any crate whose sweep changes code takes
  its own task, for the reason `M22.5` was split out of this one.
  Acceptance: as `M22.1`, per crate.
  All eleven quiet. 1,283 mutants, 21 surviving, every one already argued and
  no new survivor anywhere. The prediction held, which is worth recording as a
  result rather than as an absence: `pgprox-cluster` was the one with new logic,
  `M19`'s `PeerSource` seam, and its tests were written in the crate that owns
  it, which is the third time that has been the difference.
  `pgprox-load` and `pgload` carry fifteen of the twenty-one between them, all
  argued by `M17.5` on the grounds that their tests drive real sockets, sleeps
  and deadlines, so a verdict there is unstable by construction. Nothing about
  that changed.
- [x] `M22.6` Close M22. Filed as its own task for the reason `M18.4`, `M19.8`,
  `M20.9` and `M21.5` were.
  Acceptance: the gate passes, the status row says complete, and the section
  records what four milestones of unswept code turned out to contain, including
  if the answer is nothing.
- [x] `M22.7` Write down where a test has to live, because three sweeps
  measured it. `M22.1`, `M22.2` and `M22.3` are one experiment with the
  variable isolated by accident: six functions added by one milestone in one
  week by one author, four of them tested in the crate that owns them and two
  tested only from `bin/pgprox`. The four are killed and the two survived every
  mutant of themselves, including `unfit` surviving all three of its possible
  answers.
  `standards/testing.md` says a surviving mutant is a missing test and says
  nothing about where the test goes, so the rule that decided this outcome is
  not written anywhere. `pgprox-session`'s own `AGENTS.md` implies it and that
  did not stop the same milestone getting it wrong twice.
  Acceptance: the standard says that a crate's decisions are tested in that
  crate, that an end-to-end test elsewhere does not discharge it, and cites the
  measurement rather than asserting it. Documentation only.

## M23: the streaming question M16 left open, at the scale one machine has

- [x] `M23.0` Plan M23, and give it a gate that passes from this commit.
  `M16` moved both bulk directions onto the streaming relay and measured one
  16 MiB `DataRow` costing 16,777,216 bytes held on the old path and zero on
  the new. That is one connection in a unit test. Its completion condition asks
  for "the same 100k run with a result set large enough that the difference
  would show", and the 100k half is blocked on three machines.
  **The connection count is not what makes the difference visible; the row size
  is.** `M7`'s 100k run used pgbench's tiny rows, so a proxy holding every row
  entire would have looked identical. A pair of runs at one connection count,
  differing in nothing but the statements, answers the memory question on one
  machine.
  Acceptance: a large-result workload derived from `workload.yaml`, a run
  document with the pair, and a gate that checks the two workloads differ where
  they claim to and nowhere else.
- [x] `M23.1` A second connection count, because one pair cannot tell a
  per-connection cost from a constant.
  Filed as "the workload and the measurement", and `M23.0` committed both:
  its gate checks for them, so it could not pass without them. That is the
  merge the green-tree rule outranks the one-task-one-commit rule for, and it
  should have been said in that commit's message rather than here.
  What was left is the part that mattered. At 200 connections the large
  workload cost 8,581 more bytes per connection and this document said so. At
  600 the difference is **-403**, which is to say less than the measurement's
  own variability, and a cost that disappears when you look harder was never a
  cost. The 8,581 was fixed overhead landing differently across two runs at a
  count where fixed overhead still dominates.
  The two readings have opposite meanings, which is why one pair was not
  enough: a per-connection cost that grows with the count is something
  accumulating, and that is precisely what streaming exists to prevent.
  Acceptance: the numbers are recorded with what they do not say, which is the
  100k target, the latency figures, and the per-connection constant at small
  connection counts.
- [x] `M23.2` Close M23. Filed as its own task for the reason `M18.4`, `M19.8`,
  `M20.9`, `M21.5` and `M22.6` were: closing a milestone is a claim about the
  whole of it, and bundling that claim into the last piece of work makes it
  look like a side effect of that piece rather than a judgement about all of
  them.
  Acceptance: the gate passes, the status row says complete, and the section
  records what was measured, what the second pair corrected, and which part of
  `M16` is still blocked and why. The last of those matters most: this
  milestone narrows an open question and a section that read as closing it
  would be worse than no section.

## M24: a reading of every crate, and the nine things it found

- [x] `M24.0` Plan M24, and give it a gate that passes from this commit.
  A read of all sixteen crates against correctness, completeness, design,
  performance and test quality. Nine findings, filed below in the order of what
  they cost rather than the order they were found.
  Four of them are one shape: **a decision that reads SQL, taken by a scanner
  that is not the shared one.** `pgprox-pool` and `pgprox-route` both carry a
  written rule against exactly that, and `pgprox-pool/src/params.rs` has its
  own scanner anyway.
  Acceptance: the roadmap has an M24 section and a status row, this list is
  written, and `scripts/gates/m24-complete.sh` exists, is named in CI, and passes on
  this commit by checking what has landed rather than what is planned.
- [x] `M24.1` A `SET` after a semicolon is neither replayed nor pinned.
  `SessionParams::observe_statement` calls `ParsedSet::parse`, which reads the
  first statement of the string and stops. `PinState::observe_statement` reads
  every statement. So `SET statement_timeout='5s'; SET search_path=tenant1`
  records the timeout, does not record the search path, and does not pin,
  because both names are on the replayable list and pinning is decided per
  name. The session is then moved to a connection carrying somebody else's
  `search_path`, and nothing errors.
  This is the bug `Replayable`'s own doc comment says the type exists to
  prevent: "a session recorded as movable whose settings are never replayed".
  It guards the list and not the parse.
  Acceptance: a test that a `SET` after a semicolon is recorded, a test that a
  string mixing a replayable and an unreplayable `SET` still pins whichever
  order they arrive in, and both failing before the fix.
- [x] `M24.2` A `SET` whose parameter name is quoted is neither replayed nor
  pinned. `SET "search_path" = tenant1` is valid Postgres. `params.rs` keeps the
  quotes, so the name misses the allowlist and is not recorded; `pin.rs` reads
  `statement_words`, which drops a quoted token entirely, so the statement has
  one word and `set_pin_reason` returns before it looks at anything. Neither
  half of the promise holds.
  Acceptance: the setting is either recorded or the session is pinned, tested
  both ways, with the test failing on the current build.
- [x] `M24.3` A schema-qualified advisory lock does not pin.
  `SELECT pg_catalog.pg_advisory_lock(1)` takes a session-level lock.
  `is_session_advisory_lock` matches `starts_with("pg_advisory")` against words
  from `statement_words(sql, true)`, which joins a qualified name into one
  token, so the word is `pg_catalog.pg_advisory_lock` and the prefix does not
  match. The classifier gets this right on the same text, because it reads raw
  tokens rather than joined ones. Two readings of one function, disagreeing.
  A missed pin here returns a connection holding a session lock to the pool.
  Acceptance: the qualified form pins, the unqualified form still pins, the
  `_xact_` forms still do not, and the first of those fails before the fix.
- [x] `M24.4` The query cache key omits the database and the role.
  `CacheKey` is tenant, normalized SQL, parameters and `search_path`. A grant
  resolves to a `Backend { server, database, user }`, and `PoolKey` carries all
  three because they vary within a tenant. Two sessions of one tenant on two
  databases, or as two roles, share an entry: the same SQL against a different
  database is a different table, and against a different role is a different
  set of rows under RLS.
  The type's own doc says "Every field is part of the key. Dropping one is how
  a cache starts returning another tenant's data."
  A `pgprox-core` DTO, so non-negotiable 6 applies: the type, every
  construction, every fake and an ADR in one commit.
  Acceptance: two grants differing only in database do not share an entry, two
  differing only in user do not either, and both fail before the fix.
- [x] `M24.5` The grant cache stops admitting once it is full, permanently.
  An entry leaves `CachingResolver` only when the same key is looked up again
  and found expired, or on `clear()`. Tokens rotate, so a key is rarely looked
  up twice past its expiry. After 100,000 distinct tokens the map is at
  `capacity`, every entry in it is dead, and `store` refuses every new one for
  the life of the process. Every connection then makes a sidecar RPC, on the
  path this crate's `AGENTS.md` calls a declared hot path.
  Acceptance: a test that fills the cache with entries that then expire and
  shows a new one is admitted, failing before the fix.
- [x] `M24.6` No upper bound on the SCRAM iteration count.
  `ScramError::BadIterationCount` documents itself as "absent, zero, or
  absurd" and `parse_server_first` checks only for zero. The count comes from
  the peer, and `ScramKeys::derive` runs PBKDF2 for that many rounds inline on
  the dial path, so `i=4294967295` is a connection attempt that occupies a
  runtime worker for hours.
  Acceptance: a count past the bound is refused with `BadIterationCount`, the
  bound is a constant no environment can move, and the RFC 7677 vectors at
  4,096 still pass.
- [x] `M24.7` A prepared statement's global name is a 64-bit hash of the SQL,
  and nothing checks the SQL. `GlobalName::for_sql` is FNV-1a with a bijective
  finalizer, so colliding the name is colliding FNV-1a-64, which is cheap to do
  on purpose. `ConnectionStatements` keeps the name and not the text, so a
  second `Parse` that collides is answered `AlreadyHeld` and the client's
  `Bind` runs the first statement instead.
  Contained to one tenant's own pool, since `PoolKey` carries the database and
  the role, so this is wrong answers rather than a crossing. It is still wrong
  answers with nothing to see.
  **The acceptance this was filed with was wrong and is replaced.** It asked
  for "a constructed collision rather than an argument". Constructing an
  FNV-1a-64 collision is a meet-in-the-middle search of roughly 2^32, which is
  not a unit test, and no collision was constructed. Saying so is the point:
  the criterion would otherwise have been quietly met by something weaker.
  Acceptance: the name is 128 bits from two independent passes rather than one
  repeated, with a test that fails for `(h << 64) | h`, which every other test
  in the file would pass; the name fits an identifier Postgres will not
  truncate, which nothing checked; and the half that was **not** fixed says so
  with the measurement that decided it.
- [x] `M24.8` `LivePool` never forgets a pool key. `keyed` and `doorbells` gain
  an entry per `PoolKey` and lose one never: `reap_idle` closes the connections
  and leaves the `Pool`. A node that has served a tenant that no longer exists
  holds its pool until the process ends.
  Small per key and unbounded in the number of keys, which is the shape this
  project rejects elsewhere.
  Acceptance: a pool with nothing open, nobody waiting and nothing checked out
  is forgotten by the reaper, and one with any of those three is not.
- [x] `M24.9` Certificate hot reload is claimed twice and does not exist.
  `product/architecture.md` gives `pgprox-tls` "rustls setup, FIPS feature
  gate, cert hot-reload" and the crate's own `AGENTS.md` repeats it. Nothing in
  the workspace re-reads a certificate: `server_config` is called once from
  `entry.rs` and the `ServerConfig` it returns is fixed for the life of the
  process. A cert-manager rotation therefore serves an expired certificate
  until somebody restarts the pod.
  This is `M13`'s subject arriving from the other side: not a rule with no
  script, but a capability with no code and two documents asserting it.
  Acceptance: the listener picks up a rewritten certificate without a restart,
  a rewritten file that does not parse leaves the previous one serving rather
  than taking the listener down, and the gate runs the test rather than looking
  for the file.
- [x] `M24.10` Close M24. Filed as its own task for the reason `M18.4`,
  `M19.8`, `M20.9`, `M21.5`, `M22.6` and `M23.2` were: closing a milestone is
  a claim about the whole of it, and bundling that claim into the last piece
  of work makes it look like a side effect of that piece rather than a
  judgement about all of them.
  Acceptance: the gate passes, the status row says complete, and the section
  records the four findings that shared one cause, the two that were fixed
  only in part and say which part, and the one whose acceptance criterion this
  milestone had to withdraw.

## M25: the query cache against pgpool-II, and the three things it has that we do not

- [x] `M25.0` Plan M25, and give it a gate that passes from this commit.
  A comparison of `pgprox-cache` against pgpool-II's `memqcache`. Most of it
  came out well: the per-answer cap fires while the answer is still streaming
  rather than after it is assembled, the opt-in is per tenant rather than
  global, and the TTL is a contract rather than a default of never.
  Two places pgpool is ahead are already written down as known limits and stay
  that way: it consults `pg_proc` for volatility where `cacheable.rs` matches a
  denylist of built-in names, and it invalidates by table OID where this
  invalidates a whole tenant. `cacheable.rs` and this crate's `AGENTS.md` each
  say so; neither is a finding.
  Three are findings, all about the same constant. `MAX_RECORDED_ANSWER` is
  pgpool's `memqcache_maxcache` and behaves differently from every other bound
  in the query cache: it is invisible, unsettable, and unrelated to the budget
  it interacts with.
  Acceptance: the roadmap has an M25 section and a status row, this list is
  written, and `scripts/gates/m25-complete.sh` exists, is named in CI, and passes on
  this commit under `M24.0`'s rule: every task the backlog ticks must be named
  in it.
- [x] `M25.1` An answer abandoned for being too big is counted nowhere.
  `record_frame` drops the recording at `MAX_RECORDED_ANSWER` and increments
  nothing. `get` has already counted a miss, so a tenant whose results all sit
  just over a megabyte sees a 100% miss rate with no reason attached, while
  `rejected` stays at zero because it only counts the check inside `put`.
  The two are different failures with different fixes: `rejected` says raise
  the budget, this says the answers are too big for the cache to be the right
  tool. Reporting neither is worse than reporting either.
  A proxy-side counter rather than one on the store, because the store never
  sees this answer. `RouteCounts` is the pattern and its module comment is the
  argument: a count belongs where the decision is made and in the metric that
  answers the question an operator is asking.
  Acceptance: an abandoned answer moves a counter, `SHOW CACHE` and
  `pgprox_cache_total{result="abandoned"}` both report it, and the test fails
  on a build where the counter is not incremented.
- [x] `M25.2` The per-answer cap is a constant while the budget it interacts
  with is configuration. `query_cache.max_bytes` is in the document and
  reloads live; `MAX_RECORDED_ANSWER` is 1 MiB in a `const` and no
  configuration reaches it. So an operator who raises the budget to a gigabyte
  still cannot cache a five megabyte result, and nothing they can read says
  why.
  pgpool has `memqcache_maxcache` for exactly this and defaults it to 400 KB.
  **The existing comment argues the cap is not the cache's, and it is right.**
  It bounds a per-session buffer held while an answer is in flight, multiplied
  by however many sessions are recording at once, where `max_bytes` is one
  figure for one store. Two resources, two guards. That argues against the
  store owning the number, not against an operator setting it.
  Acceptance: `query_cache.max_entry_bytes` reaches the recorder through the
  tick loop the way `max_client_conns` reaches the gate, the default is the
  constant it replaces, and a test shows a raised value caching an answer the
  default refuses.
- [x] `M25.3` Nothing checks the two limits against each other. A
  `max_entry_bytes` above `max_bytes` is a node that records answers to the
  cap and then rejects every one of them at `put`: work done, memory held,
  nothing stored, and two counters that each look explainable on their own.
  pgpool documents the same interaction between `memqcache_maxcache` and
  `memqcache_cache_block_size` and leaves it to the operator to get right.
  Configuration validation is the place this project puts that kind of thing,
  beside the six checks already in `Config::validate`.
  Acceptance: a document setting the pair that way is refused with a reason
  naming both fields, and the boundary where they are equal is accepted.
- [x] `M25.4` Close M25. Filed as its own task for the reason `M18.4` through
  `M24.10` were: closing a milestone is a claim about the whole of it.
  Acceptance: the gate passes, the status row says complete, and the section
  records which of pgpool's advantages were fixed, which two are still open
  and why they are limits rather than findings.

## M26: what the query cache costs, measured for the first time

- [x] `M26.0` A bench and a baseline for the query cache, because there is
  none. `run-2026-07-29-cache.md` measured what the cache is *worth* end to
  end and nothing has ever measured what it *costs* per call. The store's own
  module docs say that if a profile finds its single lock, the answer is to
  shard by the hash of the key; a profile cannot find anything without a
  number to compare against, and `scripts/bench.sh` ran three crates and not
  this one.
  Acceptance: `crates/pgprox-cache/benches/hot_paths.rs` covers the paths a
  statement takes, `bench.sh` runs it, the five counts are in
  `product/perf/baseline.json`, and this milestone has a gate wired into CI.
- [x] `M26.1` A write walks every entry on the node and compares a string per
  entry. `invalidate_tenant` filters `entries.keys()` by `&key.tenant ==
  tenant`, which is an `Arc<str>` comparison, so it is a string compare per
  entry across every tenant's entries, then clones each match.
  **The measurement, at 4,096 entries across 64 tenants, invalidating a tenant
  that holds nothing: 198,283 instructions.** That is 48 times a hit and 124
  times a miss, and it is linear in the whole node's entry count rather than
  the tenant's. `M9.10` counted 10,700 invalidations against 20,000 lookups on
  the reference workload, so on those numbers invalidation costs roughly
  thirty-six times what every lookup on the node costs put together, and this
  is at 4,096 entries where a 64 MiB budget of point-select answers holds
  twenty-five times more.
  Acceptance: the count falls by an order of magnitude at the same entry
  count, the byte total and the recency order still agree with the entry map
  afterwards, and the improvement is stated as a number against the baseline.
- [x] `M26.2` A hit costs two and a half times a miss. 4,144 instructions
  against 1,605, and the whole argument for a cache is that a hit is the cheap
  path. The difference is the recency bookkeeping: `touch` clones the key into
  the `lru` map, which is six `Arc` increments and two `BTreeMap` traversals,
  on the one path that is supposed to be free.
  Acceptance: a hit costs measurably less than it does now, the LRU order is
  still an order and eviction still takes the least recently used, and the
  number is stated against the baseline.
- [x] `M26.3` A lookup through an `Arc` boxes twice. `pgprox-core` implements
  `QueryCache` for `Arc<T>` so a caller holding one can use the trait, and the
  composition root holds `Option<Arc<dyn QueryCache>>`. Every statement
  therefore boxes once for the forwarding call and once for the real one, and
  the forwarding impl exists to save a deref.
  Found by the allocation budget `M26.2` added, not by reading: a *miss*, which
  touches nothing and returns `None`, allocated two heap blocks.
  Acceptance: a statement costs one block rather than two, the budget test says
  so, and the blanket impl either keeps its callers or loses them with the
  reason written down.
- [x] `M26.4` The recency order allocates on every hit. `lru` is a `BTreeMap`
  keyed by a monotonic sequence, and a hit removes the entry's old sequence and
  inserts a new higher one, so nodes merge at the low end and split at the high
  end for as long as the cache is used. Roughly one block per seven hits on top
  of the trait's two, plus two O(log n) traversals, on the path a cache exists
  to make cheap.
  The fix is the shape every LRU ends up with: an intrusive order the entries
  hold themselves, so a touch is pointer surgery rather than a tree edit. It is
  a real rewrite of this file's bookkeeping and it needs its own commit for
  that reason, not because it is uncertain.
  Acceptance: a hit allocates exactly what a miss does, `cache_hit` falls
  again against the baseline, and eviction still takes the least recently used.
- [x] `M26.5` Close M26, on the terms `M18.4` through `M25.4` closed on.
  Acceptance: the gate passes, the status row says complete, the section
  records what the numbers were before and after, and it says plainly which of
  the store's documented worries the measurement found and which it did not.

## M27: unsafe becomes a governed exception rather than a closed door

- [x] `M27.0` Plan M27, and give it a gate that passes from this commit.
  The workspace sets `unsafe_code = "forbid"`, which cannot be overridden by a
  local `#[allow]` at all, and two standards files give the reason. The
  reasoning is sound for the crates it was written about and it is not a reason
  to close the door everywhere: `forbid` is a decision that no measurement can
  ever reopen, which is the opposite of how every other threshold in this repo
  works.
  This milestone changes the policy and **writes no unsafe code**. What it
  produces is the conditions under which unsafe may be written, and the script
  that refuses it when they are not met. Any actual use is a later task with a
  number attached.
  Acceptance: the roadmap has an M27 section and a status row, this list is
  written, and `scripts/gates/m27-complete.sh` exists, is named in CI, and passes on
  this commit under `M24.0`'s rule.
- [x] `M27.1` The policy, and the script that enforces it.
  Five conditions, because a rule with no script is a rule nobody keeps, which
  is what `M13` is about:
  1. The workspace lint becomes `deny`, so an exception is possible and has to
     be written down where it is taken.
  2. The crates whose argument is about untrusted bytes keep
     `#![forbid(unsafe_code)]` in their own `lib.rs`, where no `#[allow]` can
     reach them. `standards/security.md` says the failure mode of a decoder bug
     must be a wrong answer and never memory corruption, and that sentence is
     about those crates specifically.
  3. Every `#[allow(unsafe_code)]` names the benchmark that justifies it, and
     that benchmark exists in `product/perf/baseline.json`. Unsafe with no
     number is a liability with no evidence of upside.
  4. The hygiene lints are on: `unsafe_op_in_unsafe_fn`,
     `clippy::undocumented_unsafe_blocks`, `clippy::missing_safety_doc`,
     `clippy::multiple_unsafe_ops_per_block`.
  5. A crate containing `unsafe` is named in a Miri job, and the job exists.
  Also corrects `standards/rust-style.md`, which says `unsafe_code` is
  "forbidden at the crate level in every crate". It is forbidden once at the
  workspace root and one crate repeats it, so the sentence describes an
  arrangement that is not there. `M13`'s subject, found while rewriting it.
  Acceptance: `scripts/check-unsafe.sh` enforces all five, is wired into the
  pre-commit hook and CI, and every one of its checks is proven able to fail
  against a planted violation, per `M12`.
- [x] `M27.2` Close M27, on the terms `M18.4` through `M26.5` closed on.

## M28: the build configuration nobody had measured

- [x] `M28.0` Plan M28, and give it a gate that passes from this commit.
  `M27` closed on the observation that `scripts/bench.sh`'s own advice puts
  build configuration before any unsafe, and that this workspace's release
  profile had never been measured. It turned out to be half set already:
  `codegen-units = 1` and `panic = "abort"` are there and `lto` is `"thin"`.
  So there is one lever, not four, and the two that look available are not:
  `panic = "abort"` is already taken, and `-C target-cpu=native` is wrong for a
  binary shipped as a container image.
  Acceptance: the roadmap has an M28 section and a status row, this list is
  written, and `scripts/gates/m28-complete.sh` exists, is named in CI, and passes on
  this commit under `M24.0`'s rule.
- [x] `M28.1` `lto = "thin"` costs the route decision seven to fifteen percent.
  Measured, thin against fat, on the committed baseline:

  | benchmark | thin | fat | |
  | --- | --- | --- | --- |
  | `pgprox-route::route_begin` | 1,536 | 1,294 | -15% |
  | `pgprox-proto::decode_query` | 460 | 390 | -15% |
  | `pgprox-route::route_update` | 7,423 | 6,717 | -9% |
  | `pgprox-route::route_point_select` | 6,982 | 6,444 | -7% |

  The route decision is a declared hot path taken once per statement, and
  `route_point_select` is the largest number in the baseline.
  The cost is link time: a release relink goes from 12.98s to 30.43s, 2.3
  times. That is CI and release builds only, since `[profile.dev]` is
  untouched and `[profile.test]` runs at `opt-level = 1`.
  Acceptance: the profile changes, the baseline is rewritten with the reason,
  and both the win and the link-time cost are stated as numbers.
- [x] `M28.2` A benchmark in the gated baseline moves with a random seed.
  `invalidate_after_one_put` read 5,689, then 6,080, then 5,609 across runs
  that differ in nothing the benchmark is about. That is +6% and -1% around the
  same code, and `scripts/bench.sh` fails at 5%, so this benchmark will
  eventually fail CI on a change that did not touch it. It already reported a
  regression that was not one, during `M28.1`.
  The cause is the one `M26.4` recorded for its predecessor: the work is small
  enough that how many probes a `HashMap` lookup takes, which depends on a
  per-process random seed, is a measurable share of it.
  Acceptance: the benchmark measures enough work that the seed is noise, its
  spread across three runs is inside a percent, and the fix is the shape of the
  measurement rather than a wider tolerance.
- [x] `M28.3` Close M28, on the terms `M18.4` through `M27.2` closed on.

## M29: the first exception the unsafe policy was asked for

- [x] `M29.0` Plan M29. Small enough to be one task and a close, because the
  work is a measurement and its answer is no.
  `M27` produced a policy that lets unsafe in on evidence and deliberately
  shipped no exception. `M28` did the safe half of the same procedure and found
  7 to 15% in one line of `Cargo.toml`. What was left untested is whether the
  unsafe half buys anything here.
  Acceptance: this list, a roadmap section, and `scripts/gates/m29-complete.sh`
  wired into CI.
- [x] `M29.1` Measure `get_unchecked` on the query cache's recency slab.
  The best candidate in the workspace: `Slot` is a private newtype with no
  public constructor, issued only by `claim`, so its in-bounds property is a
  type invariant rather than a runtime fact. A rotating hit touches five.
  **Nothing moved.** 1,801 against 1,812 on `cache_hit_rotating`, 1,462 against
  1,469 on `cache_hit`, 3,753 against 3,745 on `cache_put`. Two of the three
  came out slower unsafe, which is noise rather than a regression. LLVM had
  already elided the checks, which is what the procedure's second step exists
  to catch before anything is written.
  Acceptance: a run document with both arms, the prototype reverted, and no
  unsafe in the tree.
- [x] `M29.2` Close M29.

## M30: the same procedure, applied to every crate

- [x] `M30.0` Plan M30. `M29` ran the unsafe procedure on one candidate in one
  crate and found nothing, and its own closing text said so: four of the five
  patterns were untested and none had a number behind it. This runs the
  procedure across the workspace instead, starting where it is supposed to
  start, which is a measurement rather than a pattern.
  What the measurement says, per iteration, by subtracting a callgrind run at N
  from one at 2N so fixture construction cancels:

  | path | total | where it goes |
  | --- | --- | --- |
  | `route_point_select` | 6,444 | `sql::Lexer::next` 3,404, `matches_any` 1,935, `SessionRouter::route` 985 |
  | `decode_query` | 390 | `str::from_utf8` 262, `memchr` 106 |
  | `acquire_and_release` | 443 | SipHash over `UpstreamId` 174, `release` 117, `HashMap::insert` 81 |

  Three of the four costs are work that does not need doing, and none of the
  four is a bounds check. The fourth is the one place unsafe would pay, and it
  is inside the closed list on purpose.
  Acceptance: this list, a roadmap section, and `scripts/gates/m30-complete.sh` wired
  into CI.
- [x] `M30.1` `begins_read_only_transaction` lexes a whole statement to learn
  its first word. It runs on the route decision's hot path for every statement
  outside a transaction, and its own comment claims one pass and no allocation,
  which is true and is not the point: the answer depends only on the first word
  and, for `SET`, the second. Everything after that cannot change it, and the
  loop reads all of it anyway. On the reference point select that is a complete
  second lex of the statement, next to the one `classify` already does.
  Acceptance: the function returns as soon as the first word rules it out,
  `route_point_select` and `route_update` both fall, `route_begin` does not
  regress, and a test covers the case where the deciding word is the second.
- [x] `M30.2` Every word of every statement is compared against every keyword.
  `matches_any` is a linear scan calling `eq_ignore_ascii_case` per candidate,
  and a read-only statement reaches it twice per word: once for `WRITE_WORDS`,
  fourteen entries, and once for `WRITING_FUNCTIONS`, which is thirty-odd. The
  reference point select has six words, so one route decision runs about 290
  comparisons to find no match at all, at 1,935 instructions, which is 30% of
  the decision.
  The lists stay as they are. Every entry carries a comment naming the
  construct that requires it and those comments are the reason the lists are
  correct.
  Acceptance: a filter computed at compile time from the list itself rejects a
  word that cannot match before any comparison happens, `matches_any` falls by
  more than half, no list entry or comment is edited, and a test shows the
  filter and the scan agree on every entry in every list.
- [x] `M30.3` The pool hashes a proxy-issued integer with a cryptographic hash.
  `UpstreamId` is a `u64` this process hands out. It is the key of the pool's
  `checked_out` map, and SipHash over it is 174 instructions, 39% of
  `acquire_and_release`. The same is true of the cache's `HashSet<Slot>`, whose
  keys are the slab indices `M26.4` introduced.
  This is not a blanket change and must not become one. `CacheKey` holds the
  client's SQL and its database and user names, all peer-chosen, and a map
  keyed on those keeps SipHash because that is what SipHash is for. The rule is
  about who chooses the key, not about how fast the map is.
  Acceptance: a hasher in `pgprox-core` for keys this process issues, used by
  the pool and the cache's slot set, `acquire_and_release` falls, every
  peer-keyed map is still on `RandomState`, and the rule is written down where
  the next person adding a map will read it.
- [x] `M30.4` The held read path zeroes 16 KiB before every read, for a reason
  that is no longer true. `Wire::fill_held` grows its buffer with
  `resize(start + HELD_READ, 0)` and trims after, and the comment says it is
  written that way "because reading into uninitialised capacity needs `unsafe`
  and this workspace forbids it". `M27` made the second half false. The first
  half was never true: `AsyncReadExt::read_buf` reads into uninitialised spare
  capacity through `ReadBuf`, and the crate has had it imported all along.
  So this is not an unsafe candidate at all. It is the procedure's third step
  finding that the safe construct was always there, and a comment that stopped
  anyone looking.
  Acceptance: the memset is gone with no unsafe anywhere, the buffer still
  grows no further than the slab lends, and the measurement is stated for what
  it is, including whether instruction counts can see a memset at all.
- [x] `M30.5` Two thirds of the query decode is a validation the policy will
  not let anyone skip. `str::from_utf8` is 262 of `decode_query`'s 390
  instructions, and the fix is `from_utf8_unchecked` in `pgprox-proto`, which
  is first on `ADR 0026`'s closed list. That is the correct answer and it costs
  something, which is the part worth writing down: the list was justified in
  the abstract and this is the number it was bought with.
  Acceptance: a run document holding the number and the reasoning, and no code
  change.
- [x] `M30.6` A second benchmark in the gated baseline moves with a random
  seed. `serves` read 147, 148, 135 and 154 across four runs that differ in
  nothing it measures. That is a 14% spread against a 5% tolerance, and it
  failed `scripts/bench.sh` during `M30.1` on a change to a different crate.
  This is `M28.2` again, in a benchmark `M28.2` did not look at. The rule it
  wrote down is in the roadmap: a benchmark under about a thousand
  instructions is measuring `scripts/bench.sh` as much as the code. `serves`
  is 141 and was never held against it.
  It asks one question about one tenant, so what it measures is one `HashMap`
  probe, and how many probes a lookup takes depends on a per-process seed.
  Acceptance: the benchmark measures enough work that the seed is noise, its
  spread across four runs is inside a percent, it asks both answers rather
  than only the one that says yes, and the rule that catches this class is
  written where the next benchmark will be read against it.
- [x] `M30.7` Close M30, on the terms `M18.4` through `M29.2` closed on.

## M31: the comments at M30's optimisation sites

- [x] `M31.0` Plan M31. The procedure `M30` followed sets a bar for the comment
  at an optimisation site, and it is the same bar whether or not the
  optimisation is unsafe: a good comment answers which invariant, established
  where, and why it is still true at this line. It names three ways of failing
  that bar, and `M30` left one of each in the tree.
  It also says the `debug_assert!` beside the comment is the executable form of
  the same claim, and to write both. `M30` wrote no `debug_assert!` at any of
  the five sites.
  Acceptance: this list, a roadmap section, and `scripts/gates/m31-complete.sh` wired
  into CI.
- [x] `M31.1` Three comments refer the reader elsewhere, two describe the
  operation instead of justifying it, and none has an executable form.
  The referrals: `matches_any` says "See `WordSet` for why", `Keyed`'s
  connection map says "for the reason given there", and `fill_held` ends on "the
  test below holds that". A reader is at the line, not at the other place, and
  the whole point of the comment is that they should not have to leave.
  The descriptions: `might_hold`'s case fold is explained as what `| 0x20` does
  to a byte. What a reader needs is why folding is required at all, which is
  that the scan behind the filter is `eq_ignore_ascii_case`, so a
  case-sensitive filter would reject `SELECT` while the scan accepts `select`.
  The executable forms, where one exists: the filter never rejects a word the
  scan would accept, and `reserve` leaves at least a full read of spare
  capacity. Both are one line and both run in every test in the workspace that
  touches those paths.
  Where no executable form exists the comment says so rather than leaving the
  reader to wonder whether one was forgotten. `begins_read_only_transaction`
  rests on "no word after the first can change the answer", which is a claim
  about the language and not about any state this function can inspect.
  Acceptance: no comment at an M30 site refers the reader elsewhere for its
  justification, each states the invariant and where it was established, each
  checkable claim has a `debug_assert!` that fails when the claim is broken,
  and the benchmarks are unmoved because none of this is a code change.
- [x] `M31.2` Close M31.

## M32: the comparison against pgbouncer and pgcat

- [x] `M32.0` Plan M32. Every claim this project makes about pooling is against
  its own baseline. `product/perf` holds twenty run documents and not one of
  them has another pooler in it, so "absorbs the ratio" is measured against
  pgprox at a different connection count rather than against the thing an
  operator would otherwise deploy.
  Four arms on one machine, one workload, one Postgres: direct, `pgbouncer`,
  `pgcat`, `pgprox`. What it has to answer is narrow and worth having. Does
  per-connection memory beat a C pooler that has been tuned for it since 2007,
  and what does holding a fleet-wide cap cost in acquire latency next to a
  pooler that does not coordinate at all.
  Acceptance: this list, a roadmap section, and `scripts/gates/m32-complete.sh` wired
  into CI.
- [x] `M32.1` `bin/pgload` cannot authenticate to either of the other two. It
  speaks trust and cleartext only, and says so in a comment that calls MD5 and
  SCRAM "not implemented, and a client that cannot authenticate has to say
  why". `pgbouncer` and `pgcat` both authenticate clients with SCRAM against a
  configured password, so without this there is no comparison to run.
  `pgprox-auth::scram` is already a client-side SCRAM implementation, used by
  the proxy to authenticate to upstream servers. `bin/pgload` is a listed
  composer, so it may depend on it, and it must: a second SCRAM implementation
  in this workspace is the thing `pgprox_core::sql` exists to prevent, one
  category up.
  Acceptance: `pgload` completes a SCRAM handshake against a real Postgres,
  the implementation is `pgprox-auth`'s with no crypto written here, and a
  server offering a mechanism it does not have still fails with a reason.
- [x] `M32.2` The other two arms, configured so the comparison is about
  pooling. Same upstream cap, same pool mode, same database and role, same
  machine. `pgprox` runs with its query cache off and one upstream rather than
  three, because a run that let it answer from cache or spread reads over
  replicas would be measuring features the other two do not have and calling it
  a pooling result.
  What cannot be equalised is stated rather than hidden. `pgprox` resolves a
  grant through a sidecar on every connect and the other two read a static
  password file, so connection establishment is not the same work. The run
  therefore reports the ramp separately from the steady state.
  Acceptance: a compose overlay bringing up all three against one primary, each
  reachable on its own port, each holding the same cap, and a check that the
  three caps are equal read from the files rather than restated.
- [x] `M32.3` `scripts/compare.sh`, the run. One workload replayed against each
  arm in turn with the same seed and the same connection count, sampling the
  proxy's resident memory and the primary's connection count while it runs, and
  reporting a table. Arms run one at a time, because three proxies under load
  on one machine measure the machine.
  Acceptance: the script runs all four arms, refuses to report a number it did
  not get, names which arm each figure came from, and its own failure mode is
  a named arm rather than an exit code.
- [x] `M32.4` The run, recorded, including what it does not say.
  Acceptance: a document in `product/perf` with every arm's figures, the
  configuration each ran under, and the arms it is not fair to compare.
- [x] `M32.6` `pgcat` only offers MD5 to clients, so `M32.1` was half the work.
  Found by running it: `pgcat` answers a startup packet with
  `AuthenticationMD5Password` and its configuration has no client-facing
  alternative. Its own documentation says so in passing, describing `auth_query`
  as fetching "the hash used for md5 authentication". The binary does carry
  `SCRAM-SHA-256`, and that is for its own connections to Postgres.
  This project declines MD5 on purpose and the reason is in
  `pgprox-session`'s dial path: md5 was deprecated in Postgres 14, and adding it
  would put a second hash implementation in the proxy for a server
  configuration nobody should run. That argument is about the proxy. `pgload`
  is a measurement tool that has to speak what the thing it measures asks for,
  and refusing here would mean dropping an arm of the comparison rather than
  making a point.
  Acceptance: `pgload` answers `AuthenticationMD5Password`, verified against
  `pgcat` rather than only against a fake, the digest is a dependency rather
  than a hash written here, and the proxy still refuses MD5.
- [x] `M32.8` The run's first numbers were not reproducible, and its memory
  figure measured the wrong thing.
  Three runs that each tore the stack down and rebuilt it disagreed by a factor
  of two on identical code: pgprox read 17,637 transactions and then 8,427,
  with p99 going from 686ms to 14s. Every arm in the bad run was equally bad,
  which is what says it was the machine. A comparison that rebuilds a Postgres
  and five containers between runs puts a different machine under each one.
  The memory figure was worse, because it looked precise. `peak - idle` is a
  connection cost only while `idle` means idle, and a process does not return
  its heap when its clients leave, so the second round starts at the first
  round's peak. pgbouncer read 7,618 bytes per connection cold and 61 in the
  round after it, on the same code doing the same work.
  Acceptance: the rotation repeats inside one stack, what is reported is the
  median across rounds with the spread beside it, the memory figure is the
  absolute peak with the cold per-connection delta beside it, and a finished
  run can be re-read without being re-run.
- [x] `M32.7` Close M32.

## M33: what pgbouncer and pgcat do differently

- [x] `M33.0` Plan M33. `M32` measured the three poolers against each other and
  found pgbouncer using a third of pgprox's memory. A number is not a reason,
  and the reason is in their source rather than in a table. Both are open, so
  reading them is cheaper than guessing.
  Acceptance: this list, a roadmap section, and `scripts/gates/m33-complete.sh` wired
  into CI.
- [x] `M33.1` The study, and the experiment that refuted its own obvious answer.
  The hypothesis worth testing first is the cheap one: pgbouncer's read buffer
  is 4 KiB by default and pgprox's is 16 KiB, so pgprox should be paying four
  times over. Test it before writing it down.
  Acceptance: a document naming what each of the three actually does with
  memory, read from source rather than remembered, the buffer experiment with
  both numbers, and the question it leaves open stated as a question rather
  than filled in with a guess.
- [x] `M33.2` Close M33.

## M34: the seventeen kilobytes that are not the buffers

- [x] `M34.0` Plan M34. `M33` measured 22,835 bytes per connection, of which
  5,048 is the session future and, by experiment, none is the read and write
  buffers. It named glibc's per-thread allocator arenas as the cheapest
  candidate to rule out and did not run it.
  Two variables, and they are separable without touching the code: `tokio`
  reads `TOKIO_WORKER_THREADS` and glibc reads `MALLOC_ARENA_MAX`. One arm of
  each isolates the arena count from the thread count, which matters because a
  single-threaded runtime changes both at once and would answer neither
  question on its own.
  Acceptance: this list, a roadmap section, and `scripts/gates/m34-complete.sh` wired
  into CI.
- [x] `M34.1` Is it the allocator's memory or the connection's.
  A per-thread arena is a fixed cost per worker and a per-connection cost is
  linear in connections, so the two are told apart by holding one still and
  moving the other. If capping the arenas at one moves the per-connection
  figure, the figure was never per connection.
  Acceptance: three arms of the same binary, differing only in environment,
  each reporting idle and peak resident memory at the same connection count,
  and a document saying which of the two it was or that it was neither.
- [x] `M34.2` Close M34.

## M35: every per-connection memory figure so far was two numbers added together

- [x] `M35.0` Plan M35. `M34` closed saying roughly 12.7 KB per connection was
  unexplained and named the spawned task as the next thing to weigh. Weighing
  it is the wrong next step, because the figure it would be weighed against is
  not a per-connection figure.
  A cost per connection and a fixed cost look identical at one connection
  count. They separate at two. `M32`, `M33` and `M34` each measured at 200 and
  divided by 200, so each reported a slope plus an intercept and called the sum
  a per-connection cost.
  Acceptance: this list, a roadmap section, and `scripts/gates/m35-complete.sh` wired
  into CI.
- [x] `M35.1` The slope, and what it does to every figure this project has
  published about per-connection memory.
  Measured at 100, 200 and 400 connections, the same load and the same stack.
  If the reported figure falls as connections rise, the part that falls was
  never per connection.
  Acceptance: a document with the three points per arm, a slope and an
  intercept for each, what that does to `M32`'s comparison, and the corrections
  it forces on `M33` and `M34` stated as corrections rather than as new
  findings.
- [x] `M35.2` Close M35.

## M36: what an open, quiet connection costs

- [x] `M36.0` Plan M36. `M35` established that per-connection memory under the
  reference workload is a curve rather than a number, because the buffer term
  tracks concurrency and concurrency saturates. It named the one term that does
  not saturate as the thing worth measuring and failed to measure it, by giving
  a twenty-five second window to a workload whose think time starts at thirty
  seconds.
  That term is what decides whether a hundred thousand connections fit on a
  node, and it is the only per-connection figure this project should be
  quoting. `product/perf/workload-idle.yaml` is the workload for it.
  Acceptance: this list, a roadmap section, and `scripts/gates/m36-complete.sh` wired
  into CI.
- [x] `M36.1` The resident cost of a connection that is doing nothing.
  Three connection counts against the idle workload, run long enough that
  transactions happen, so the figure is a line with a slope and an intercept
  rather than one point. Under this workload the buffer term is near zero by
  construction, so what the slope measures is the state a connection holds when
  it is quiet.
  Acceptance: a document with the three points per arm, the slope and intercept
  for each, what the slope says about a hundred thousand connections on one
  node, and whether it is linear this time rather than an assumption that it
  is.
- [x] `M36.2` Close M36.

## M37: what a spawned task costs beyond the future it holds

- [x] `M37.0` Plan M37. `M36` measured an idle connection at roughly 15 KB and
  accounted for 5,048 of it as the session future, leaving about 10 KB that
  three milestones have now failed to name. Every candidate but one is ruled
  out: not the read and write buffers, not the allocator arenas, not the
  prepared statement map.
  The one left is what `tokio::spawn` allocates. `size_of_val` on a future is
  the future, and a spawned task is the future plus a header plus whatever the
  allocator rounds the pair up to. No test in this repo has ever weighed the
  difference, and the test that guards the future's size measures exactly the
  part that is already accounted for.
  Acceptance: this list, a roadmap section, and `scripts/gates/m37-complete.sh` wired
  into CI.
- [x] `M37.1` The cost of a spawned task, against the size of the future in it.
  Measured with `dhat`, which the allocation budgets already use, so the figure
  is bytes requested from the allocator rather than resident memory: it will
  not include what the allocator rounds up beyond what it reports, and the
  document has to say so.
  Several future sizes rather than one, because the answer is a relationship
  and a single point cannot tell a constant header from a proportional one.
  Acceptance: a test that reports what a spawn costs at several future sizes,
  the figure for the size the session future actually is, and a statement of
  how much of `M36`'s ten kilobytes this accounts for.
- [x] `M37.2` Close M37.

## M38: the extrapolation M36 did not need to make

- [x] `M38.0` Plan M38, and carry it out in one commit. The milestone is a
  correction of two paragraphs; splitting it into a plan, a fix and a close
  would produce two commits with nothing in them, and the first non-negotiable
  asks for one change that leaves the tree green rather than for ceremony. Said
  here so the shape is a decision rather than an oversight, on the terms
  `M32.2` carried `M32.3`.
- [x] `M38.1` `M36` extrapolated to a number the repo had already measured.
  It fitted a slope over 200 to 800 connections and reported 1.47 GB at a
  hundred thousand, three times the roadmap's target, with a caveat about the
  extrapolation being 167 times the largest count measured.
  `run-2026-07-28-100k-hold.md` measured that point directly, at a hundred
  thousand connections on this machine: **5,726 bytes each and 546 MB**, 9%
  over the target rather than three times it. `M7` quotes it in the roadmap.
  The failure is `M35`'s own lesson unapplied. `M35` established that the
  figure is fixed cost plus a variable term, and `M36` then extrapolated using
  a slope taken where the fixed cost still dominates and the curve is visibly
  bending.
  Acceptance: the run document and the roadmap say what the measured figure is,
  say that the extrapolation is superseded rather than deleting it, and name
  why it was wrong.
- [x] `M38.2` Close M38.

## M39: documentation for people who are not this repo

- [x] `M39.0` Plan M39. Every document in this repo is written for whoever is
  building it: `product/` holds a mission, a roadmap and a backlog, `standards/`
  holds rules for contributors, and `AGENTS.md` is an index for agents. There is
  no README and no `docs/`.
  A person who finds this on GitHub cannot learn what it is, run it, configure
  it, or read what it has been measured at, without reading a roadmap written
  for somebody else. That is a gap in the product rather than in the process.
  Acceptance: this list, a roadmap section, and `scripts/gates/m39-complete.sh` wired
  into CI.
- [x] `M39.1` A documentation site, and a README that routes to it.
  Diátaxis, one quadrant per page, shallow navigation. A tutorial that gets the
  stack running, a configuration reference, an operations guide, an
  architecture explanation and a performance page carrying the numbers this
  project has actually measured rather than the ones it targets.
  The honesty is a requirement rather than a tone. This has never been
  deployed, its 100k figure is one machine, and every latency number is
  loopback. A doc site that reads like a shipped product would be the same
  defect `M13` found in the standards, on the outside of the repo.
  Acceptance: pages under `docs/`, a README, every number traceable to a run
  document, and a check that the configuration reference still names the fields
  the code actually reads.
- [x] `M39.2` Close M39.

## M40: a control that only worked where nothing else was broken

- [x] `M40.0` Plan M40, and carry it out in one commit, on the terms `M38.0`
  did. The milestone is one helper and four call sites.
- [x] `M40.1` `tests/gates/negative.sh` blamed the wrong component, and three of
  its controls passed for the wrong reason.
  Its four cases for `m1f-complete.sh`'s scope ADRs read the script's exit code.
  That script ends by running the workspace checks, the coverage gate and
  `scripts/conformance.sh`, and the last wants a Postgres in a container.
  Without one the positive case reported "accepts two ADRs that decided: the
  check failed on a good artefact", which names the ADRs and sends a reader to
  the wrong file. It fails that way on every machine without the stack up.
  The three negative cases are worse. `expect_fail` passes on any non-zero
  exit, so on the same machine they passed with the ADR check deleted entirely.
  On a fully provisioned machine they worked, which is why this was invisible
  exactly where it is checked.
  Acceptance: each of the four asserts the message its own check produces
  rather than the script's exit status, deleting the ADR check makes all four
  fail, and the suite passes with no stack running.
- [x] `M40.2` Close M40.

## M41: the docs become a site

- [x] `M41.0` Plan M41. `M39` wrote six pages that render when somebody browses
  the repo and are not a site: no generator, no navigation, no search, nothing
  that deploys. The operator and the agent this project's admin surface was
  designed for both arrive through a browser.
  Astro Starlight, chosen by the person who owns the repo over `mdBook` and
  Jekyll, and the trade is written down rather than glossed: it brings a Node
  toolchain and a lockfile into a repo that had neither, and `cargo-deny` does
  not see any of it. That is a real widening of the supply chain and it was
  accepted deliberately.
  Acceptance: this list, a roadmap section, and `scripts/gates/m41-complete.sh` wired
  into CI.
- [x] `M41.1` A site built from `docs/`, deployed to Pages.
  The Markdown stays where it is. Starlight normally wants
  `src/content/docs/`, and Astro's content layer can point a collection at
  `docs/` instead, which keeps the files readable on GitHub for anyone who
  arrives that way. Two audiences, one source.
  Acceptance: the site builds, every page appears in the navigation, the
  relative links between pages resolve in the built output as well as on
  GitHub, and a workflow publishes it.
- [x] `M41.2` Close M41.

## M42: the site's toolchain leaves the repository root

- [x] `M42.0` Plan M42, and carry it out in one commit, on the terms `M38.0`
  set. The milestone is a directory move and the paths that follow it.
  `M41` put `package.json`, a lockfile, `astro.config.mjs`, `src/` and
  `node_modules` at the root of what is otherwise a Rust workspace. Every one
  of them reads as a top-level concern of the project and none of them is.
  Acceptance: nothing Node remains at the root, the pages stay in `docs/` where
  both readers find them, the site still builds, and the workflow and the gate
  follow the move rather than pointing at where things used to be.

## M43: what it does, and what one request touches

- [x] `M43.0` Plan M43, and carry it out in one commit on `M38.0`'s terms. The
  milestone is two pages and the navigation that reaches them.
  `M39` gave a reader orientation, a tutorial, a reference, an operations guide
  and two explanations, and left two questions it could not answer. What does
  this actually do about caching, replicas and consistency, and what does it
  refuse to do. And what happens inside when a request arrives, which is the
  question anybody about to change the code asks first.
  Acceptance: a features page covering pooling, pinning, replica routing and
  LSN watermarks, the query cache and an explicit unsupported list; a request
  flow page naming the component at each step; both in the navigation; and a
  check that the pin reasons the features page lists are the ones the code has.

## M44: the pages a review asks for

- [x] `M44.0` Plan M44, and carry it out in one commit on `M38.0`'s terms. The
  milestone is six pages, the navigation that reaches them, and a gate that
  reads each list from the code rather than from the page.
  `M39` and `M43` between them cover what the proxy does and what one request
  touches. What no page covers is everything a reader asks before they are
  allowed to run it: what keeps one tenant's data away from another's, what
  several nodes do together and how they are deployed, what the admin surface
  actually offers, how authentication and credential handling work, how to
  build the FIPS variant, and what the performance numbers cost to get.
  The gate is the point as much as the pages are. `M39` documented `SHOW MEM`,
  which the parser has a test rejecting by name, and four milestones of
  documentation work went past it because nothing compared the page against the
  enum. Six lists here have a source of truth in the code and each is read from
  there.
  Acceptance: pages for multitenancy, clustering and deployment, admin and
  management, security, FIPS and optimizations; each in the navigation; and
  `scripts/gates/m44-complete.sh` wired into CI, checking the `SHOW` commands in both
  directions, the admin API paths, the JWT algorithm allowlist, the crates on
  the closed unsafe list, the cache key's component count, the quoted benchmark
  figures against the committed baseline, the cluster defaults, and the two
  things a reader will type or look for on the FIPS path.
- [x] `M44.1` The edit link on every page pointed at a branch called `docs`.
  Starlight builds a page's edit URL with `new URL(path, baseUrl)`. `M41` put
  the collection's source in `docs/` and `M42` moved the toolchain into
  `docsite/`, so every path the collection carries begins `../docs/`, and URL
  resolution spends that `../` on the base rather than on the path. From
  `edit/main/` it consumed `main`, and all fourteen pages linked to
  `edit/docs/<page>.md`.
  Invisible from here: the link points at GitHub either way, nothing local
  follows it, and it renders correctly in every dev server. It is only wrong in
  the built output, which is the only place it is ever clicked.
  Acceptance: the base names the directory the collection's paths are relative
  to, every built page's edit link carries the branch, and the check holds the
  relation between the two settings rather than the string, so a collection that
  moved another level up fails.

## M45: one directory for the pages and the thing that builds them

- [x] `M45.0` Plan M45, and carry it out in one commit on `M38.0`'s terms. The
  milestone is a directory move and the paths that follow it, the same shape as
  `M42` and partly a correction of it.
  `M42` moved the Node toolchain out of the repository root and into `docsite/`,
  which was right about the root and wrong about the split. Two directories a
  level apart, one holding thirteen Markdown files and the other holding five
  files that read them, produced a `../` in every path between them. `M44.1` is
  what that cost: two settings each correct alone, wrong together, and only
  visible in the built output.
  The root stays a Rust project either way, which was `M42`'s actual
  requirement. `docs/` satisfies it as well as `docsite/` did.
  The cost is real and is accepted rather than hidden: browsing `docs/` on
  GitHub now shows `package.json`, a lockfile, `astro.config.mjs` and `src/`
  beside the pages. Four entries of noise against a `../` in every path.
  Acceptance: `docsite/` is gone, the pages have not moved, the collection reads
  its own directory rather than a parent, the site builds the same fifteen
  pages, the workflow and both gates follow the move, and the edit-link check
  resolves a URL rather than pattern-matching a setting, so it survives the
  next move too.

## M46: the licence three files have claimed and none granted

- [x] `M46.0` Plan M46, and carry it out in one commit on `M38.0`'s terms.
  `Cargo.toml` declares `license = "Apache-2.0"` and every crate inherits it.
  The README has a Licence section that says "Apache-2.0." and nothing else.
  Neither is a licence. An SPDX identifier is a label, and Apache-2.0 section
  4(a) requires that anyone the work is distributed to receives a copy of the
  terms; there was no copy to give. GitHub's detector reads the file too, so the
  repository rendered as unlicensed to anybody who arrived at it.
  The text is verbatim from `/usr/share/common-licenses/Apache-2.0`, with the
  appendix's `Copyright [yyyy] [name of copyright owner]` filled in and nothing
  else changed, which is checkable by substituting the line back.
  No `NOTICE` file. Apache-2.0 requires one only where the work already has one,
  and adding it later means every downstream copy has to carry it from then on.
  Acceptance: `LICENSE` holds the full text, the README points at it,
  `docs/package.json` declares the same identifier, and `scripts/check-drift.sh`
  refuses a tree where the manifest names a licence that the file, the README or
  the package manifest does not.

## M47: the links nothing was checking

- [x] `M47.0` A check that every relative Markdown link resolves, and the
  fifteen that did not.
  `check-drift.sh` checks the links out of `AGENTS.md`, because those send a
  reader to a standard. Nothing checked the other hundred and forty. Fifteen
  were broken, all in `product/roadmap.md`, and all in the same way: every link
  it makes to a run document and every link it makes to a page carried one `../`
  too many, as if the file lived a directory deeper than it does. They
  accumulated across several milestones, including three written this week.
  That is the shape worth a check. Not a typo, which somebody notices, but a
  consistent misreading of where a file sits, which produces dozens of wrong
  links that all look right until somebody needs one.
  It resolves the path and not the fragment, and says so: reproducing the site
  generator's heading slugs would be a second implementation of them, and two
  implementations of a slug is two chances to disagree.
  Acceptance: `scripts/check-links.sh` in the pre-commit hook, in CI and in
  `AGENTS.md`'s list; the fifteen fixed; and a file with a link to nothing
  fails it.

## M48: the design record moves under docs/

- [x] `M48.0` Plan M48, and carry it out in one commit on `M38.0`'s terms. The
  milestone is a directory move and the four hundred and sixty references that
  follow it.
  `product/`, `standards/` and `specs/` sat at the repository root beside
  `crates/`, `bin/` and `deploy/`, which reads as though they were part of what
  ships. They are not: they are how this repository is worked in. They move
  under `docs/internal/`, which puts every word written for a reader in one
  place and leaves the root a Rust project.
  Visible rather than hidden, and that was the decision worth making. `.sdd/`
  was the proposal. `rg` and `fd` skip hidden directories by default, so every
  future search of the design record would silently return nothing, and this
  repository's whole arrangement is that an agent is sent to read those files.
  Hidden directories here hold tool state, not content.
  The site is unaffected by construction: `M45` made the collection's glob
  top-level only, so `docs/internal/` is invisible to it without anything
  being excluded.
  Acceptance: the three trees are under `docs/internal/`, every reference
  follows including the `include_str!` paths the workspace compiles against,
  the site still builds fifteen pages, and every check and gate passes.

- [x] `M48.1` A check that was left matching one link in eighteen.
  `check-drift.sh` verified that the paths `AGENTS.md` links to exist, by
  matching `\]\((standards|product|\.agents)/`. After `M48.0` that pattern found
  one link out of eighteen and reported that every path AGENTS.md links to
  exists, having looked at one of them. It did not fail. It narrowed.
  `check-links.sh` from `M47` already resolves every relative link in every
  Markdown file, so the check was redundant as well as broken.
  It is replaced rather than repaired, with the thing `check-links.sh` cannot
  see: every standard in the directory is named by the index. A standard that
  exists and is linked from nowhere is a rule every session must follow and no
  session is pointed at.
  Acceptance: an unindexed standard fails it, and so does an empty standards
  directory, so it cannot pass by describing nothing.

- [x] `M48.2` 240 MB of Node modules in the Docker build context.
  `M45` moved the site's toolchain under `docs/`, and `.dockerignore` names
  `target/`, `target-coverage/`, `reference/` and `.git/`. It did not name
  `docs/node_modules/`, so every image build since has sent 240 MB it does not
  use. Nothing fails; every build is just slower, which is why it went
  unnoticed.
  The pages themselves stay in the context, because the load generator embeds a
  workload file from `docs/internal/product/perf/` at compile time.
  Acceptance: the three generated directories under `docs/` are excluded and
  the pages are not.

## M49: one place for what a run leaves behind

- [x] `M49.0` A scratch directory, and the honest half of what was asked for.
  `reference/` held 30 MB of upstream proxies cloned for protocol comparison,
  gitignored and untracked, sitting at the repository root beside the code. It
  moves to `.tmp/reference/`, and `/.tmp` becomes the one entry that covers
  anything somebody needs to put somewhere and nobody needs to keep.
  **The rest of the scratch cannot follow it, and this is the finding.** The
  intent was to fold eight `.gitignore` patterns into one. Every one of them
  guards a tool that writes to the working directory and gives this repository
  no say in it: `perf record`, `cargo flamegraph`, `cargo mutants`,
  `cargo llvm-cov` and a dhat binary all default to CWD. Folding them in would
  mean the redirect had to be typed by the person running the command, which is
  the one place it will be forgotten.
  Checked rather than assumed, because a script can be told where to write:
  `scripts/bench.sh` puts callgrind output in a mktemp directory,
  `scripts/mutants.sh` writes under `target/`, `scripts/profile.sh` writes to
  `target/profile`, and the dhat budgets build their profiler with `.testing()`,
  which writes no file at all. Not one of the eight is produced by anything in
  this repository. They are all guards against a hand-run, and they stay, with
  the reasoning in the file rather than in this entry alone.
  Acceptance: `reference/` is gone from the root, `.dockerignore` follows it,
  the one document that cites a path inside it still cites a real one, and the
  patterns that stayed say why in `.gitignore`.

## M50: a README in every crate

- [x] `M50.0` A README per crate, for a person rather than an agent.
  Every crate carries an `AGENTS.md` and none carried a `README.md`. Those are
  different documents for different readers: `AGENTS.md` is rules and hazards
  for somebody about to change the crate, and a README is orientation for
  somebody who has just landed in the directory and wants to know what this is
  and how it connects to the rest.
  GitHub renders a README at the foot of a directory listing, which is where a
  person arrives. It rendered nothing for any of the sixteen.
  Each says what the crate owns, what it is built on, what is built on it, and
  the one constraint that shapes it. The last part varies by crate on purpose,
  because a uniform page per crate is a template rather than a document.
  Acceptance: sixteen READMEs; `scripts/check-readmes.sh` in the pre-commit
  hook, in CI and in `AGENTS.md`'s list, checking both directions of the
  dependency claim; and a missing README, an undocumented dependency, an
  invented crate name and an empty tree each failing it.

- [x] `M50.1` Two rows of the crate map that had stopped being true.
  Writing sixteen READMEs from the code meant reading the crate map beside
  them, and two rows disagreed with it.
  `pgprox-cluster` was credited with "SWIM gossip". ADR `0004` was renamed in
  `M18.1` for exactly this reason and says in as many words that no code ever
  matched the SWIM description. The table kept it.
  `pgprox-cache` was "trait stub until M9", and M9 closed twenty-five
  milestones ago.
  The stated exception for `bin/pgload` named two of its four workspace
  dependencies. It composes `pgprox-auth` and `pgprox-tls` as well, because
  measuring a TLS deployment means running a real SCRAM exchange over a real
  client configuration.
  Acceptance: the three corrections, and the map points at the per-crate
  READMEs as the version to read from inside a directory.

## M51: eighty scripts and no index

- [x] `M51.0` An index, and the forty-five files that were burying it.
  `scripts/` held eighty-two files and twelve of them were named anywhere.
  Forty-five are milestone gates, which is more than half the directory and
  none of what a newcomer needs: a gate is one milestone's completion
  condition, satisfied rather than maintained. `ls scripts/` sorted `m1f`
  between `m19` and `m20` and said nothing about which half to read.
  The gates move to `scripts/gates/`, so the directory listing is thirty-seven
  entries of things somebody might run. `lib.sh` stays where it is, so
  `REPO_ROOT` still resolves, and every gate's `source` line grows a `../`.
  `release-check.sh` moves with them: it is `M8`'s completion condition and the
  only gate not following the naming convention, so leaving it out would make
  `scripts/gates/*.sh` mean "most of the gates".
  Nothing was deleted. Every script in the directory is referenced by
  something, which was checked before proposing a cleanup that removed files.
  Acceptance: `scripts/README.md` grouping all thirty-six runnable scripts by
  what they are for and what they need; `check-drift.sh` failing on a script
  the index does not name; `AGENTS.md` pointing at the index rather than
  carrying a second list of eight; every glob over the gates following the
  move; and all forty-five gates plus the negative suite passing from the new
  path.

- [x] `M51.1` The singleflight had a window, and the flake was telling us.
  `concurrent_lookups_of_a_cold_key_make_one_call` failed once in a full-suite
  run, then passed twenty isolated runs and three more full ones. A one-in-a-few
  flake is the shape that gets rerun rather than read.
  It was right. `resolve` reads the cache and then claims the key under two
  separate locks, so a caller descheduled between them finds that the previous
  leader stored and released in the gap, and becomes a second leader for a key
  already cached. The comment above the claim said "two callers cannot both
  decide they are first", which is what the code did not do. This crate's own
  `AGENTS.md` says a reconnect storm must produce one RPC, and
  `docs/request-flow.md` says concurrent resolves become one call.
  The fix is a second look after taking the claim, which costs nothing on the
  hot path: a cache hit returns before the claim lock is ever touched, and the
  extra read lands only where a network call was about to happen anyway.
  Extracted so it can be tested rather than raced: the guard is only reachable
  through the window, so it is a method with two direct tests rather than a
  branch waiting on a coincidence.
  Acceptance: the guard serves the entry, releases the claim and wakes
  subscribers; the cold case keeps the claim; the crate holds its 95%; and
  three full-suite runs are clean.

- [x] `M51.2` Mutation testing that answers before the merge.
  A full run is 3,694 mutants and each is a build plus a test run, which is why
  it is nightly. Nightly means it reports on Tuesday about a test weakened on
  Monday, by which point the change is in.
  `MUTANTS_DIFF` narrows a run to the lines a diff touched **and** to the
  crates that diff reached. The second half matters as much as the first:
  `--in-diff` narrows which mutants are generated, not which crates are
  visited, and a crate costs a baseline build whether or not the diff reached
  it. Sixteen baseline builds to mutate five lines is what would stop anybody
  running it.
  Measured on the previous commit: two crates instead of sixteen, five mutants
  instead of the crate's full set.
  `MUTANTS_SHARD=k/n` runs one slice, and the nightly splits across four
  runners with `fail-fast: false`, because a survivor in one slice is a finding
  and the other three slices' findings are worth having in the same run.
  Acceptance: a per-PR job scoped to the merge-base diff, the nightly sharded
  four ways, and the narrowing documented as a narrowing rather than a
  replacement, since a change can make a mutant survivable in code it did not
  touch and only the full run sees that.

## M52: two failures from the CI replay, and what each turned out to be

- [x] `M52.0` A gate that failed without saying why.
  A full CI replay had `check-coverage.sh` report "test run failed" for
  `pgprox-session` and `pgprox`. It was not reproducible: the same command
  passed clean, the same gate passed clean, the exact CI sequence of negative
  suite then `m-1` then `m0` passed clean, and two concurrent coverage runs
  passed clean. Ephemeral port exhaustion was measured and ruled out: the range
  here is 4,095 ports and `TIME_WAIT` stayed flat at 473 across a full run.
  There was nothing left to look at because the only copy of which test failed
  had gone to `/dev/null`. An intermittent failure is the one kind that most
  needs its evidence kept, and this gate was discarding it for the two crates
  whose tests are slowest and are the only ones binding real sockets.
  This does not fix the flake. It makes the next occurrence diagnosable, which
  is the most that can honestly be claimed without having seen it.
  Acceptance: a failing test is named in the gate's output and the full log
  path is printed; a passing run leaves no temp file behind; verified by
  planting a failing test and reading it back.
- [x] `M52.1` A suite that could not run where the daemon will not pick a port.
  `conformance.sh` started Postgres with `-P`. On Docker Desktop 29.6.2 under
  WSL2 the daemon accepts that and allocates nothing: the container is `Up`,
  `PublishAllPorts` is true, and `NetworkSettings.Ports` is `{"5432/tcp":[]}`.
  `docker port` then prints "no public port '5432/tcp' published", which reads
  like the container failed to start and is not that at all.
  Characterised rather than assumed: every dynamic publish allocates nothing
  and every fixed publish works, on any image and any port, with 53 host ports
  already held and no exhaustion. So it is dynamic allocation specifically.
  The suite now probes for a free port and asks for it by number, which costs
  one socket bind and works on both kinds of daemon. `M1F` never ran its two
  Postgres versions on this machine before this.
  Acceptance: `conformance.sh 17 18` passes including both client-side checks,
  `m1f-complete.sh` passes, no container outlives the run, and a publish that
  still yields nothing says the container is up rather than that it failed.

## M53: the scripts read as stale, and two of them were

- [x] `M53.0` An index for the gates, and the twelve milestones that have none.
  `scripts/gates/` holds forty-four gates for fifty-six milestones. The gap is
  deliberate and nothing said so, which makes a missing filename look like a
  missing gate: there is no `m1-complete.sh` because `M1` is held by
  `conformance.sh`, no `m2` because `M2` is a `cargo nextest` invocation, no
  `m8` because it is `release-check.sh`, and none for `M46` through `M52`
  because their conditions are ordinary `check-*.sh` scripts.
  The listing is also unreadable in a second way: `ls` puts `m1f` between `m19`
  and `m20` and `m3` after `m29`, so nothing about the order says which
  milestone came first.
  Renaming was considered and rejected. Zero-padding to `m00`, `m01f`, `m44`
  would sort correctly and cost roughly two hundred and fifty reference updates
  across CI, globs, the roadmap and the backlog, to fix a listing nobody reads
  in preference to the roadmap. The index answers the actual question, which is
  "which milestone is this and is one missing".
  Acceptance: `scripts/gates/README.md` listing every gate in milestone order
  with the roadmap's own title, and every milestone that deliberately has no
  gate with what holds it instead; `check-drift.sh` failing on a gate the index
  does not name and on a missing index; both verified against a break.
- [x] `M53.1` `cargo fmt` ran twice on every push.
  CI's tier 1 listed `check-fmt.sh` and `check-crate.sh`, and the second runs
  the identical `cargo fmt --all --check` as its first step. Two runs of the
  workspace formatter for one opinion.
  `check-fmt.sh` is not deleted and does not move: it is the pre-commit hook's
  fmt entry, where a separately named hook is what tells a developer which
  check failed, and `m0-complete.sh` calls it directly. That second caller is
  also why removing the CI line costs no coverage even if somebody later takes
  fmt out of `check-crate.sh`, because `m0` runs in the milestone job on the
  same push.
  Acceptance: one fmt run per push, and the reasoning in the workflow rather
  than only here.
- [x] `M53.2` A check whose one-line summary claimed the workspace.
  `check-wired.sh` announced "everything written to be used is used". It reads
  a watchlist of eight symbols and checks those. It cannot find the next
  unwired symbol on its own.
  The script's own body was already honest about this and argues the case
  against a general scanner: nearly every `pub` item in a library legitimately
  has no in-tree caller, so a scan would be mostly false positives, and the
  list is short precisely because every entry is there from a real defect. Only
  the summary oversold it, which is the same shape as the `SHOW MEM` row and
  the regex that matched one link in eighteen.
  So the summary is corrected rather than the check replaced, and the reason a
  scanner would be worse is now in the header where somebody would go looking
  before writing one.
  Acceptance: the header, the run banner and the script index all say what it
  does.

## M54: the repository URL was aspirational

- [x] `M54.0` Point every published URL at the repository that exists.
  `Cargo.toml` claimed `https://github.com/pgprox/pgprox`, the Helm chart named
  it as its home, and the site was built for `https://pgprox.github.io` with a
  `/pgprox` base. No such organisation existed and no such repository existed.
  Nothing failed, because nothing had ever been pushed. The moment it was, four
  kinds of link would have pointed at a 404: every GitHub blob link the site
  generates for a run document or an ADR, every page's edit link, the canonical
  URL on every page, and every entry in the sitemap.
  The `/etc/pgprox/pgprox.yaml` occurrences in `pgprox-config` are a filesystem
  path that happens to match the pattern and are deliberately untouched.
  Acceptance: the site builds with blob links, edit links, canonical URLs and
  sitemap entries all naming the real repository, and the only remaining
  matches are the config path.

## M55: the first push found a dependency CI never installed

- [x] `M55.0` `protoc`, which every developer machine had and no runner did.
  `pgprox-auth`'s build script compiles the sidecar `.proto` and prost-build
  shells out to `protoc`. `deploy/Dockerfile` has installed
  `protobuf-compiler` since the image existed. `ci.yml` never did.
  Nothing caught it because nothing could: the workflow had never run anywhere
  but a machine that already had the tool. The first push to a fresh runner
  failed three jobs at once, and the message is a build-script error four lines
  into cargo's output under a heading that says clippy failed.
  Fixed twice over, because the install is the fix and the message is the
  finding. Every job that compiles the workspace now installs it, and
  `check-crate.sh` and `check-coverage.sh` require it by name, so a machine
  without it is told which tool is missing and how to get it rather than being
  handed a compiler error about a crate it did not ask about.
  `supply-chain` and `secrets` are deliberately left without it: `cargo deny`
  and `gitleaks` compile nothing.
  Acceptance: every compiling job installs protoc, a run with protoc off the
  PATH names it in one line, and CI is green on a fresh runner.

- [x] `M55.1` A gate that failed on a runner for being a runner.
  `m-1-complete.sh` asserts the three pre-commit hooks are installed. A fresh
  clone has none, and cannot meaningfully be given them: nothing on CI commits,
  so a hook installed there would never fire. The milestone job failed three
  times over on the truth.
  Skipped under `CI` rather than dropped, because the guarantee still has to
  hold somewhere. On a developer machine it is the three hooks. On CI it is
  `ci.yml` calling the same scripts, which `check-drift.sh` already enforces by
  failing when a check exists that the workflow does not run.
  Acceptance: the milestone job passes on a runner, and the check still fails
  on a developer machine with the hooks uninstalled.

- [x] `M55.2` `M52.0` kept the evidence in a file CI destroys.
  The instrumentation added three commits ago wrote the failing test output to
  a temp file and printed its path. That works on a developer machine. On CI
  the runner and every file on it are deleted when the job ends, so the first
  real failure after it landed printed a path nobody could ever open, and the
  evidence was gone for the second time.
  Worse, the branch that was supposed to print the tail never ran. `named="$(grep
  ...)"` returns 1 when it matches nothing, `set -e` is on, and the script
  exited having printed the FAIL line and none of the evidence under it. That
  is the second `set -e` trap of this shape in this repository; the first was
  an `&&` list in `scripts/mutants.sh`.
  So the output goes inline: named tests where nextest named some, and the last
  twenty lines of stderr where it did not, which is the case that actually
  happened.
  Acceptance: a planted failing test is named, a planted compile error prints
  the compiler's own message, and neither needs a file that outlives the job.

## M56: what the instrumentation finally showed

- [x] `M56.0` The coverage flake was a five second timeout on a two core runner.
  `M52.0` instrumented the coverage gate and `M55.2` made that instrumentation
  survive CI. The next run named it:
  `run::tests::a_cancel_for_a_peers_connection_is_forwarded_from_a_running_node`,
  failing at 5.085s against a `tokio::time::timeout(Duration::from_secs(5))`.
  It asserted nothing wrong. It ran out of patience.
  Five seconds was chosen on a twenty-core developer machine. A two-core GitHub
  runner under llvm-cov instrumentation is a different machine, and three tests
  in the same file had already been raised to ten seconds one at a time, which
  is the shape of a number nobody owns.
  Fifteen sites across two files now share a named `PATIENCE` constant at
  thirty seconds. This is not a threshold being lowered: the tests assert
  exactly what they did, including the exact forwarded payload, and the only
  thing that changed is how long they wait before calling a hang a hang.
  Virtual time was considered and does not apply. Every one of these drives
  real I/O between spawned nodes, and tokio only auto-advances a paused clock
  when every task is idle.
  Acceptance: one named constant per file with the reasoning and the CI
  evidence beside it, no bare `from_secs(5)` or `from_secs(10)` timeout left in
  either test module, and the crate green.

- [x] `M56.1` `m0` reimplemented a check instead of calling it.
  The milestone job also failed on `cargo deny`, and the reason was that the
  job never installed cargo-deny. The message was "cargo deny (run: cargo deny
  check)", which sends a reader to run a command that is not there without
  saying it is not there.
  `check-deps.sh` already does this check and already reports a missing tool by
  name. `m0-complete.sh` ran `cargo deny check` itself, which is the same check
  written twice, and the copy in the gate was the one without the tool check.
  This repository's own rule is that CI calls the scripts rather than
  reimplementing them, and a gate is not exempt.
  Acceptance: `m0` delegates to `check-deps.sh`, the milestone job installs
  cargo-deny, and a machine without it is told which tool is missing.

## M57: the cancel test discarded the line it was waiting for

- [x] `M57.0` A test that answered before it recorded.
  `a_cancel_for_a_peers_connection_is_forwarded_from_a_running_node` failed on
  every CI run and passed on every local one, including under llvm-cov, on two
  pinned cores, and with the whole crate running.
  The peer listener read a line, replied with a digest, and only then recorded
  the line. `gossip::forward` connects, writes the cancel, flushes and drops the
  stream, so by the time the listener replies the far end is frequently already
  gone: the write fails with a broken pipe, the listener returns, and the line
  it had just read, the one the test is waiting for, is discarded.
  It passed locally because a write to a socket the peer has closed succeeds
  until the RST lands. It failed every time on a GitHub runner, where it does
  not.
  `M56.0` is the wrong turn worth recording. It read the 5.085s failure as a
  five second timeout on a slow machine and raised the wait to thirty. The next
  run failed at 30.078s, and a failure that scales exactly with the timeout is
  not a slow machine, it is something that never happens. That is what ruled
  slowness out and pointed here. The larger constant stays: it is right for
  real-I/O tests on a two-core runner regardless, and it is what made the
  second measurement legible.
  Acceptance: the line is recorded before the reply is attempted; a failed
  reply ends the connection and nothing else; and the diagnosis is proven both
  ways, with the old order failing at 30.039s under a forced broken pipe and
  the new order passing in 0.038s under the same.

## M58: the milestone job kept finding tools it did not have

- [x] `M58.0` cargo-mutants, the second tool the milestone job never installed.
  `m14-complete.sh` runs a real mutation pass over `pgprox-testkit`. The job
  installs cargo-llvm-cov, nextest, protoc and, since `M56.1`, cargo-deny. It
  did not install cargo-mutants, so the gate failed on the tool being absent.
  `mutants.sh` reported it correctly, by name, which is the difference between
  this and `M56.1`: there the gate had reimplemented the check and lost the
  tool test along the way. Here the message was right and nothing had installed
  what it named.
  Acceptance: the milestone job installs cargo-mutants, and every tool any gate
  in that job shells out to is installed by it.

## M59: a benchmark that broke CI on a commit that did not touch it

- [x] `M59.0` `cache_put` put every iteration into one hash bucket.
  It read 3,668, 3,672 and 3,838 across three runs on the same runner: a 4.6%
  spread against a 5% gate, which failed a build on a commit that changed
  nothing in `pgprox-cache`.
  The cause is written in this file already, four constants above the
  benchmark. `HELD` is sixteen rather than one because
  `invalidate_a_tenants_entries` moved 6% between runs of the same code, since
  a `HashMap`'s probe count depends on a per-process random seed and at one
  entry it was a measurable share of the work. `cache_put` was the one
  benchmark still putting a single key.
  The fix cycles `HELD` keys, one put per iteration. The unit does not change,
  so the before-and-after in `docs/optimizations.md` stays a comparison; what
  changes is that a run averages sixteen probe-length draws rather than taking
  one.
  Nine of the sixteen benchmarks are bit-identical across three CI runs and
  match the developer baseline exactly, so the method is sound and this was one
  benchmark rather than the apparatus.
  Acceptance: the spread across CI runs falls below the tolerance it is gated
  against, measured rather than assumed, which needs CI runs of the changed
  benchmark and is why the rebaseline is a separate task.

- [x] `M59.1` Rebaseline on CI.
  The baseline was measured on a twenty-core developer machine. Ten benchmarks
  are identical everywhere; the cache family reads about 4% higher on a GitHub
  runner, which eats most of a 5% budget before any noise.
  Blocked on `M59.0`: rebaselining to numbers measured under the noisy scheme
  would bake in a value that was never stable.
  Six CI runs, `M59.0` through `M64.0`, read out of the archived logs of the
  `instruction counts` job rather than re-run. The ten non-cache benchmarks
  returned the same number in all six and match the committed baseline to the
  instruction, which is what makes the other six a finding rather than noise:
  callgrind is deterministic for a binary, so a benchmark that reads
  differently in two places is running different instructions. All six that
  move are `pgprox-cache`, and all six of that crate's are among them.
  Each new figure is the lower median of six readings, so it is a number a run
  actually produced rather than an average of numbers none did. The cost is
  paid in the other direction: a developer now reads about 4% below the
  baseline for `cache_put` instead of CI reading 4% above it, which is the
  right way round, since CI is where the count gates a build.
  This also settles `M59.0`'s acceptance, which was that the spread across CI
  runs falls below the tolerance, measured rather than assumed: 3,686 to 3,702
  across six runs is 0.43% against a 5% gate, from 4.63% before.
  `docs/optimizations.md`'s table does not subtract any more for the two rows
  that moved, and says so rather than restating the percentages: their before
  and after were measured on one machine and the cuts were real, and the after
  column now carries a CI figure.
  Acceptance: `baseline.json` carries the CI figures for the cache family and
  the developer figures nowhere it disagrees with them, the run is recorded in
  `docs/internal/product/perf/`, and `m44-complete.sh` passes against the
  rewritten page rather than being edited to.

## M60: three gates read history and the runner had one commit

- [x] `M60.0` `actions/checkout` clones shallow, and nothing said so.
  The milestone job failed with "driver-matrix.md names a commit this
  repository does not have". The commit exists, is an ancestor of `main`, and
  resolves locally. What the runner did not have was any history:
  `actions/checkout` defaults to `fetch-depth: 1`, so a gate that asks about
  any commit but the tip is asking about a repository the job does not have.
  Three gates read history. `m21-complete.sh` checks the driver matrix names a
  commit that exists and reports how far behind the proxy it is; `m12` and
  `m22` compare against earlier commits. All three were reading a
  single-commit clone.
  The `secrets` job already sets `fetch-depth: 0` for gitleaks and
  `mutants-diff` sets it for the merge-base diff, so the setting was known and
  the milestone job simply never needed it until the repository had a remote to
  be cloned from.
  Acceptance: the milestone job checks out the full history, and the reason is
  beside the setting rather than only here.

## M61: five gates that ran suites and threw away the result

- [x] `M61.0` A shared runner that keeps the evidence.
  `m5-complete.sh` failed on CI with "suites (run: cargo nextest run -p
  pgprox-pool -p pgprox-route)" and nothing else. The suites pass locally,
  including at twenty times the proptest case count, so the only copy of which
  test failed was on the runner and went with it.
  Five gates had the same shape: `cargo nextest run ... >/dev/null 2>&1`
  followed by a one-line `fail`. That is the third time this lesson has been
  paid for. `M52.0` learned it on the coverage gate, `M55.2` learned that
  printing a path to a log file is worthless for the same reason, and this is
  the same defect in five more places.
  `run_suite` in `lib.sh` replaces the pattern: it runs the command, and on
  failure names the failing tests inline, or prints the tail of the output when
  nothing looks like a failing test, which is the case that keeps happening.
  Every grep in it carries `|| true`, because `set -e` plus a grep that matches
  nothing is the trap that has now bitten this repository twice.
  Acceptance: no gate runs a suite into `/dev/null`; a planted failing test is
  named by the gate that runs it; and the underlying CI failure, whatever it
  is, is legible on the next run.

## M62: the evidence helper could not read coloured output

- [x] `M62.0` `run_suite` printed "no failing test named" above the failing
  test's name.
  `M61.0` added the helper and its first real use on CI reported that nothing
  named a test, directly above a tail that contained
  `FAIL [0.176s] pgprox-route::budgets the_route_decision_stays_inside_its_budget`.
  nextest colours `FAIL`, so the line is escape codes between the whitespace
  and the word, and an anchored pattern misses it. It matched locally because
  output redirected to a file is uncoloured, and missed on CI because nextest
  colours there anyway. Every hand-run command in this session stripped those
  escapes before matching; the helper did not.
  It also only tailed fifteen lines, and nextest puts the assertion far above
  the summary, so the one line that says *why* was cut off both ways.
  Now: colour stripped before anything is matched, the failing tests named, and
  the assertion reported separately, because which test failed and why it
  failed are different questions and the second one was never being answered.
  Acceptance: a planted failing test is named with its assertion, with colour
  forced and with colour absent, and the underlying budget failure is legible
  on the next run.

## M63: a warning that killed the gate printing it

- [x] `M63.0` `git log | head -8` under `pipefail`.
  `m21-complete.sh` reached its last check, warned that the driver matrix is
  seventeen commits behind, printed eight of them, and exited 141. That is
  SIGPIPE: `head` takes its eight lines and closes the pipe, git's next write
  gets EPIPE, `set -o pipefail` reports 141 and `set -e` ends the gate
  mid-run. The gate was killed by its own warning.
  Whether it happens is a race between git finishing its writes and `head`
  exiting, which is why it passed here and failed there. It only became
  reachable at all after `M60.0` gave the runner full history, so there were
  more than eight commits for git to print: the fix for one CI-only failure
  created the conditions for the next.
  `git log -n 8` removes the pipe and the race with it. Reproduced both ways
  before and after: the old form dies before its own trailing `echo`, the new
  form exits 0.
  The other pipelines into `head` in `scripts/` read a line or two from `sed`
  or `docker port` and are left alone; this was the one used as a statement,
  where the status reaches `set -e` directly.
  Acceptance: the gate completes and reports, and the pipeline that killed it
  is gone rather than silenced with `|| true`.

## M64: the allocation budgets counted the whole process

- [x] `M64.0` A budget of zero, measured with a counter that counts other
  threads.
  `the_route_decision_stays_inside_its_budget` failed on CI runs 8 and 10,
  passed run 9, and passed 825 consecutive runs here. Run 10 finally carried
  the number: four allocations across 250,000 routings. Four is not
  per-statement allocation, which would be hundreds of thousands, so the
  question was whether the route decision grows something once that the warm-up
  misses, or whether the measurement was attributing somebody else's
  allocations to it. A count cannot answer that, so the answer needed an
  instrument that names the caller.
  A scratch allocator that captured a backtrace inside the measured window,
  under forty concurrent copies on a twenty-core machine, reproduced it twice in
  1,200 runs and named it every time: `test::run_tests` on the *main* thread,
  inserting the just-spawned test into libtest's `running_tests` map and
  allocating its table. `dhat::HeapStats::total_blocks` is process-wide, so the
  harness's own bookkeeping landed inside a window budgeted at zero. libtest
  spawns the test thread and *then* records it, so whether the two overlap is a
  scheduling race, which is why a loaded runner loses it and an idle machine
  does not.
  `pgprox-route` is the one that failed because its warm-up is four routings,
  so its window opens sooner after the spawn than any other budget's. The other
  five had the same defect and had not yet been unlucky.
  The fix is the instrument, not the budget: `allocation-counter` counts on the
  thread that allocates. Proved rather than assumed, since a fix for a 2-in-1200
  failure cannot be confirmed by not failing. A probe with a thread deliberately
  allocating throughout the window fails 20 out of 20 under the process-wide
  counter, counting 203 allocations it did not make, and 0 out of 20 under the
  thread-local one, with the route body still reading zero.
  No budget moved. The two that assert a number rather than zero read exactly
  what their comments recorded when they were set: the grant-cache hit at 15
  against a budget of 17, gossip encode and decode at 10 and 26.
  `bin/pgprox/tests/spawn.rs` keeps `dhat` and is the exception that makes the
  rule legible: it asks what a multi-threaded runtime holds for a spawned task,
  and those allocations are on worker threads, so process-wide is what it wants.
  Acceptance: the six budget files count per thread; a background thread
  allocating during a measured window does not change any budget; the two
  non-zero budgets read their recorded figures; and `m7-complete.sh` looks for
  the counter that is now used.

## M65: the index page did not say what fleet this is for

- [x] `M65.0` The case, and three pictures of it.
  `docs/index.md` opened with what pgprox is and went straight to a table of
  contents. What it never said is the shape of deployment the whole design
  answers, which is the first thing a reader needs in order to know whether any
  of the rest applies to them.
  The case: about a hundred Postgres clusters, a few thousand databases each at
  one per tenant, and hundreds of application nodes running a thread-per-request
  stack, so a worker thread holds a connection for the length of a request.
  Every part of that is ordinary and together they do not fit. A connection is a
  process on the server, so each cluster's cap is small next to the fleet asking
  for it: five hundred divided by three hundred nodes is under two per node,
  against a node with two hundred worker threads. The nodes cannot lend to each
  other because they share no memory. So the pool cannot exist, and the
  application connects, runs one statement and disconnects seconds later, paying
  TCP, TLS, SCRAM and a backend fork on the request path. Demand grows with
  nodes times clusters; the cap does not move. That is the N by M problem and it
  is what the page now opens with.
  Three diagrams: the problem, the same fleet with a proxy fleet holding the
  cap, and the database-per-tenant fan-out with the three keys that separate
  tenants. Rendered at 3,000 pixels wide so the small type survives a
  high-density display.
  `scripts/diagrams.sh` builds them from HTML and CSS under `docs/img/src/`
  with headless Chrome, so a wording change is a wording change rather than a
  layout change, and the pictures stay something that can be corrected. The
  PNGs are committed because a site that needs a browser installed before it can
  show a picture will one day ship without the picture. They are quantized to a
  256-colour palette: at full colour two of the three sat within twenty
  kilobytes of the large-file hook's 512 KB limit, so an edit could have failed
  a commit for a reason that looked unrelated to it.
  Acceptance: the page states the fleet shape and the arithmetic before it
  offers a table of contents, the three images resolve for both a reader on
  GitHub and the built site, and the diagrams regenerate from committed source
  rather than being unmaintainable binaries.

## M66: the site stopped being published and every check was green

- [x] `M66.0` A ten-minute clock on a deploy that takes longer than ten
  minutes.
  Three docs runs in a row failed at `actions/deploy-pages` with "Timeout
  reached, aborting!", while the build step succeeded in sixteen seconds each
  time and the `ci` workflow went fully green for the first time. The site
  carried on serving a commit that was by then four behind, and nothing about
  the repository said so: the failure was in a workflow whose name reads like
  documentation rather than delivery.
  Not the artefact. It is 500 KB of static HTML and 534,112 bytes on the run
  that failed, against 538,173 on the run before it that succeeded. Not the
  content, not the build, and not a GitHub incident: the status page reported
  all systems operational throughout.
  The action creates a Pages deployment and then polls until Pages reports it
  finished, with a default timeout of ten minutes. That was always marginal for
  this repository and nothing had ever measured it: the one deployment that did
  succeed took 6m39s, two thirds of the budget, for a site that builds in
  sixteen seconds. The three since crossed ten minutes and were killed by the
  action's own clock while still `in_progress`.
  Thirty minutes. This does not hide a stuck deployment, which still fails with
  the same message and takes longer to say so. What it stops is a deployment
  that would have finished being aborted for being slower than a timeout nobody
  chose.
  Acceptance: the timeout is a number somebody picked with the measurement
  beside it, and the site serves the commit at the head of `main`.

- [x] `M66.1` The timeout `M66.0` raised has a ceiling, and the ceiling was
  already in the log.
  `M66.0` set `actions/deploy-pages` to thirty minutes. The next run timed out
  between 13:07:53 and 13:17:55, ten minutes to the second, because ten minutes
  is not the action's default but its maximum:
  `this.timeout = Math.min(timeoutInput, MAX_TIMEOUT)` against a hard-coded
  `MAX_TIMEOUT = 600000`. The action says so when it clamps, and
  `##[warning]Warning: timeout value is greater than the allowed maximum` was in
  the log of the run that proved it. `M66.0` asserted a remedy without checking
  the knob existed, which is the failure mode this repository's third
  non-negotiable is about, one step removed: not claiming a test passed, but
  claiming a fix worked.
  The measurement `M66.0` made still holds and is the part worth keeping: this
  repository's Pages deploys drifted from 3m12s to over ten minutes across
  pushes whose content barely moved, and the artifact is 59 files and 2.4 MB
  with a count that did not change when it broke. What changes is the
  conclusion. A backend slower than ten minutes cannot be waited out from here
  at all, so the setting comes out rather than staying to describe a fix it does
  not deliver.
  Also learned, from a dispatch that failed in nine seconds rather than ten
  minutes: Pages keys a deployment by `pages_build_version`, which is the commit
  SHA. Re-running a publish for a SHA whose deployment was already cancelled
  returns "Deployment cancelled." immediately. Recovery is a new commit, and a
  re-run is not one.
  Acceptance: no setting in the workflow claims to do something the action
  clamps away, the ceiling and its source are written where the next person
  would set it, and the fact that a re-run cannot recover a cancelled SHA is
  recorded beside them.

## M67: every action was on a runtime with a removal date

- [x] `M67.0` Node 20 actions, updated before the runners stop carrying them.
  Every CI and docs run has been printing "the following actions target Node.js
  20 but are being forced to run on Node.js 24", once per job, for as long as
  the workflows have existed. It reads like housekeeping and it is a deadline:
  GitHub flipped the runner default on 2 June 2026, and removes Node 20 from
  hosted runners entirely on 16 September 2026, after which a workflow pinned to
  a Node 20 action stops running rather than warning.
  Six actions moved, and the major numbers are further apart than the warning
  suggests because each Node bump was released as a breaking change:
  `checkout` v4 to v7, `setup-node` v4 to v7, `upload-artifact` v4 to v7,
  `upload-pages-artifact` v3 to v5, `deploy-pages` v4 to v5, and
  `gitleaks-action` v2 to v3.
  Each major between here and there was read rather than assumed, and three
  mattered. `setup-node` v6 limits automatic caching to npm, which this already
  asks for by name. `upload-pages-artifact` v4 stopped including dotfiles, which
  is why `docs/dist` was checked for them and has none. `checkout` v7 blocks
  checking out a fork's pull request under `pull_request_target` and
  `workflow_run`, neither of which this repository uses.
  `Swatinem/rust-cache@v2` and `taiki-e/install-action@v2` are unchanged and
  needed no move: the first is already `node24` at its v2 tag, the second is a
  composite action with no Node runtime of its own.
  `deploy-pages` v5 does not raise the ten-minute ceiling `M66.1` recorded. It
  is still `MAX_TIMEOUT = 600000`, checked in the v5 source rather than assumed
  from the version number, and the comment beside the step now says it holds for
  both.
  Acceptance: no action in either workflow runs on Node 20, the warning is gone
  from the run logs, and every job that was green stays green.

## M68: the docs said what read routing decides, never how

- [x] `M68.0` Read routing, as a page.
  `features.md` stated the two rules a user needs and `architecture.md` restated
  them a shorter way. Neither said how a node learns where a replica has got to,
  what happens when it cannot tell, or which replica gets picked when several
  qualify, so the mechanism existed only in the crate and in ADR 0009.
  `docs/read-routing.md` is that mechanism: the decision end to end, the three
  tests the classifier applies and why the first word is not enough, the
  watermark and the window between classifying a write and learning where it
  landed, the 250 ms poll of `pg_last_wal_replay_lsn()` and
  `pg_is_in_recovery()`, the four states that take a replica out of service, and
  the metrics that say whether any of it is working.
  Two things the page says plainly because a reader would otherwise assume
  otherwise. Selection is the first eligible replica in grant order, with no
  least-lag or round-robin policy at all. And `pg_is_in_recovery()` is not
  decoration: a promoted replica keeps answering and keeps reporting a plausible
  position, and routing reads to it is how a split brain starts serving two
  versions of the truth.
  Drafted with `SHOW REPLICAS` in the observability section. There is no such
  command. `m44-complete.sh` would have caught it, and it was caught before that
  by checking the claim against `show.rs` rather than by running the gate and
  finding out.
  `features.md` and `architecture.md` keep their sections and gain a pointer
  rather than being trimmed: one answers "what does it do" and the other "how is
  this built", and the detail that would have drifted between them now has one
  home.
  Acceptance: the page is in the sidebar and the site builds it, every claim in
  it is checked against the code rather than against the other pages, and no
  page documents a `SHOW` the parser would refuse.

- [x] `M68.1` Pages that opened with the constraint before the capability.
  `read-routing.md` began "pgprox sends a statement to a replica only when it can
  show two things", which is a rule for somebody who already knows the feature
  exists. It never said that the proxy can route reads to replicas at all, and
  never said what replication lag is or why it makes this harder than load
  balancing. `features.md` had the same shape one heading down: "A statement
  reaches a replica only when both halves hold."
  Reviewed every page and every section for it rather than fixing the two that
  were reported. Nine places opened with a rule, a table or a code block where a
  reader needed a sentence saying what the thing was first:
  read routing on three pages, the query cache, protocol support, both admin
  surfaces, draining a node, verifying a FIPS build, the performance page's lead
  and its targets table.
  The rule that came out of it, and the reason this is one task rather than two:
  a heading is not an introduction. A section that opens with a constraint reads
  as an answer to a question the reader has not been given yet, and every one of
  these was written by somebody who already knew the feature and could not see it
  missing.
  Acceptance: no page or section leads with a rule, a table or a code block where
  the capability has not been stated, the site still builds every page, and the
  pages that gained a lead did not lose a fact.

- [x] `M68.2` The watermark's scope was never stated, so readers would infer the
  wrong one.
  Asked whether the route decision stays correct when the writes come from
  another session, another pgprox node, or a client connected straight to
  Postgres. It does, but only against a promise the docs never wrote down: the
  guarantee is read-your-writes for one session, not global freshness and not
  monotonic reads across sessions. The watermark is per-session state in one
  process and there is no fleet-wide watermark anywhere in the codebase.
  Three things the page now says. Another writer's commit does not move your
  watermark, whoever they are, so a read can be older than it: that is what
  asynchronous replication is rather than a routing error. More proxy nodes
  cannot weaken the guarantee, because a session never migrates and its own
  writes therefore always pass through the node that records its floor. And the
  one real gap, which is that the watermark dies with the connection, so write,
  get shed, reconnect, read can land on a replica behind that write, where a
  plain connection to a primary would have kept read-your-writes across the
  reconnect.
  The gap is narrow rather than theoretical: shedding waits for a client idle at
  a transaction boundary past 30 seconds by default, never takes one
  mid-transaction and never takes a pinned one. Narrow is not the same as absent
  and the page says which it is.
  Same omission class as `M68.1`. There the scope of a feature was missing before
  its rules; here the scope of a guarantee was missing under one. Both were
  invisible to somebody who already knew the answer.
  Acceptance: the read routing page states whose writes the watermark covers and
  whose it does not, names the reconnect case, and `features.md` carries the
  one-line version with a link rather than a second copy of the argument.

## M69: a replica set that never changed after the first grant

- [x] `M69.0` The watch was keyed by the primary alone.
  `watch_for` looked up `grant.primary.server`, and on a hit returned the
  existing watch and discarded the new grant's replica list. So the first grant
  to name a primary fixed its replica set for the life of the process. A replica
  added later was never polled and never routed to; one removed left a loop
  querying a host nobody could reach.
  The reordering case is worse than either, and it is a correctness bug rather
  than a missed optimization. `RouteTarget::Replica` is an index: the
  eligibility check reads slot `i` of the watch and `backend_for` resolves `i`
  against the session's own grant. Those agree only while both lists are in the
  same order, and `auth.proto` is explicit that they need not be, describing
  replicas as arriving "in no particular order". Under a reordering the router
  clears a read against one host's replay position and sends it to another,
  which is the stale read the whole watermark design exists to prevent.
  Fixed by putting the ordered list in the key, so a changed list is a different
  watch and the pair a session holds is always one generation of one topology.
  Considered and rejected: keying replica state by `ServerId` instead of by
  position. That is the deeper fix and it is not the first one. `RouteTarget` is
  `Copy` today, a `ServerId` would end that, and the ripple reaches
  `pgprox-core`'s public API, the `Router` trait, its three implementors and the
  contract gate, for a defect this closes inside one module.
  Keying on the list turns a bounded map into an unbounded one, since every
  topology change mints a generation, so eviction comes with it: a watch that no
  session holds and that no grant has asked for in a minute is dropped, and its
  poll loop holds a `Weak` so it stops on its own. Both conditions rather than
  either, because a session keeps its watch for its whole life and may be idle
  much longer than the grace period.
  Acceptance: a reordered list and an added replica each get their own watch, a
  generation in use survives the grace period, an unused one does not, and all
  three assertions were seen to fail against the old keying before they passed
  against the new.

## M70: the document's server entries did not reach the cluster layer

- [x] `M70.0` Three ways a declared cap failed to arrive, and one way an
  undeclared one was invented.
  Found while answering how pgprox learns its topology. All three are the same
  defect class: `servers:` is the operator's statement of what the fleet may
  hold, and three separate paths did not carry it.
  **Pools for a server the document never names.** `apply_quota` iterated
  `config.servers`, so a pool whose server had no entry was never passed to
  `set_limit` at all. It kept the limit `PoolConfig` gave it at startup, derived
  from whichever server happened to be first in the document, and no allowance
  ever applied to it. Replicas are the case that matters and they are also the
  case an operator cannot pre-empt: they arrive from the sidecar at runtime.
  Three nodes each holding that default is a cap nobody declared being exceeded
  by a factor nobody chose.
  **A cap that changed.** `set_cap` was called once, during `App::build`. A
  reload that raised or lowered `max_connections` never reached the cluster, so
  the fleet went on dividing the number it started with while the admin surface
  reported the new one.
  **`servers[].guaranteed_fraction`.** Parsed, validated against `0.0..=1.0`,
  defaulted, documented, and read by nothing. The split used
  `CoordinatorConfig::guaranteed_fraction`, a fleet-wide default `wiring.rs`
  never overrode, so setting the documented per-server field changed nothing at
  all.
  The loop now walks the servers that actually have pools, resolves each one's
  quota from the document directly or from the primary it replicates, and
  re-registers it every tick. `ServerQuota` carries the cap and the fraction
  together, because holding them apart is how the fraction came to be orphaned.
  A server nothing declares a cap for is held at zero rather than defaulted,
  with a log line naming it and both fixes. Failing closed is the right
  direction for the one property the mission gives no graceful degradation, and
  the inheritance rule is what stops that from making read routing
  configuration-impossible.
  Also removed: `App::replicas`, an `Arc<ReplicaWatch>` of length zero written
  at construction and read nowhere. The registry that matters lives on the
  connection context, and the quota loop now takes it.
  Acceptance: an undeclared server's pools read zero, a replica of a declared
  primary reads more than zero and within its allowance, a reloaded cap moves
  the allowance, and all three were seen to fail against the old loop.

## M71: a demoted primary could be handed to a new client for five minutes

- [x] `M71.0` Local, fast detection that a primary stopped being one, and
  invalidation of what it can reach: the grant cache.
  `features.md` decides against automatic failover, correctly, and said
  nothing about the gap beside it: the grant cache had no way to learn a
  primary demoted, so it kept serving a stale grant to every *new* client for
  up to `grant_ttl_cap`, 300 seconds by default. A session already connected
  learns the ordinary way, from its next write failing; this is about the
  clients who have not connected yet.
  `pg_is_in_recovery()` answers the question directly, and the replica poller
  already asks it of every replica every 250 ms. `bin/pgprox/src/primary_watch.rs`
  asks the same question of every session's primary, on the same cadence,
  using the same `SqlReplicaProbe` the replica poller uses, built with one
  backend instead of a list.
  On the transition into recovery, every cached grant naming that primary is
  dropped, through a new `pgprox_core::auth::GrantInvalidation` trait
  implemented by `pgprox-auth`'s `CachingResolver`. A new trait rather than a
  new method on `CredentialResolver`: eviction is a property of the cache
  wrapping a resolver, not of resolving itself, and a raw resolver given a
  method it can only no-op is an API that lies about what it does. Adding it
  required an ADR and every implementor in the same commit, per this
  repository's rule for a `pgprox-core` trait change, satisfied by
  `scripts/check-core-contract.sh` even though the trait is new rather than
  changed: the gate compares method sets between HEAD and the index and does
  not distinguish the two.
  Edge-triggered, once: an `AtomicBool::swap` makes "was this already known"
  and "mark it known" one step, so a primary demoted for an hour is
  invalidated once rather than fourteen thousand times. A failed probe is
  inconclusive rather than a demotion, for the same reason the replica poller
  does not treat a miss as ineligible outright: a network blip must not turn a
  poll interval into a resolve storm on the sidecar for a primary that never
  changed.
  Proved end to end rather than only at the unit level:
  `a_demoted_primary_is_detected_and_invalidated_within_two_seconds` resolves a
  grant into a real `CachingResolver`, points a real `PrimaryWatches` at a real
  socket, and asserts the entry is gone against a real two-second deadline.
  Caught before it reached a test assertion: `fakepg::fake_postgres()`
  answers `pg_is_in_recovery()` with `t` unconditionally, documented as
  modelling a replica, and every one of this module's other tests uses it as
  a *primary*. That is the fixture already doing what this milestone is
  about, harmlessly in tests that assert nothing about it and exactly on
  purpose in the one that does.
  Considered and rejected: discovering the *new* primary from
  `pg_stat_replication` rather than only detecting the old one's demotion. The
  view has no port, database, role or password, and `client_addr` is a
  replication-connection address that is not necessarily where a client should
  connect. The control plane is the one place that correctly knows what a
  proxy should connect to; this stops serving the wrong answer rather than
  guessing a right one.
  Acceptance: a probe answering `pg_is_in_recovery(): true` invalidates
  exactly the cache entries naming that primary within one poll interval, a
  second such reading invalidates nothing further, a failed probe invalidates
  nothing, and the whole path was proved against a real socket and a real
  two-second clock rather than only at the unit level.

## M72: an established session had no way to learn a corrected primary

- [x] `M72.0` A second, token-free RPC, and a session's next acquire finds the
  correction.
  `M71.0` gave new clients fast relief from a demoted primary by invalidating
  the grant cache. It could do nothing for a session already connected,
  because that session holds a `Grant` rather than a token from the moment it
  authenticated, and `Resolve` needs a token. Its only recourse was to keep
  failing writes until it reconnected.
  `RefreshTopology`, additive to the frozen `pgprox.auth.v1` contract: keyed
  by the primary's host and port rather than by tenant, since one primary can
  host thousands of tenant databases and a failover is one fact about all of
  them. The response carries no `ttl_seconds`, no pool hints, no claims,
  deliberately: it answers where the database is, not who may use it or for
  how long, and reusing `ResolveResponse` with those fields left at their zero
  values would have let a caller read a default TTL of zero as "expires now"
  rather than "not sent". ADR 0028.
  A new `pgprox_core::auth::TopologyRefresh` trait rather than a method on
  `CredentialResolver`, with its own `FakeTopologyRefresh` behind
  `test-fakes` per this crate's standing rule. Two different questions with
  two different inputs, and a caller holding only a `Grant` should not be
  offered a method it cannot call meaningfully.
  On the same edge in `PrimaryWatches` that triggers `M71.0`'s invalidation,
  a successful refresh is stored keyed by the *original* primary's
  `ServerId`, in a table `backend_for` — the one place a `RouteTarget`
  becomes a `Backend` to connect to — checks before falling back to the
  grant's own value. A session's next connection acquire therefore resolves
  the corrected primary from its own unmodified grant, at the next
  transaction boundary, with no new grant and no reconnect. `backend_for`
  changed from returning `&Backend` to an owned `Backend` to make this
  possible, which costs the same Arc-clone-only allocation `PoolKey`
  construction already pays on every acquire and is not part of any measured
  budget.
  Best-effort by construction: if the refresh RPC fails, invalidation still
  ran, so a new client is exactly as well served as under `M71.0` alone, and
  an already-connected session gets no worse an outcome than it had before
  this existed.
  Considered and rejected: pushing the refresh from the sidecar instead of
  pulling it, which would need a standing connection per pod and `auth.v2`;
  answering with the replica list only, leaving the primary implicit, which
  is the shape of bug the frozen contract's own versioning rules exist to
  prevent; and mutating `grant.primary` in place, which would make every
  reader of a session's grant a place that has to reconsider whether it can
  change underneath them.
  Proved end to end, not only at the unit level: one new test resolves a
  grant naming a real socket, waits on a real two-second clock, and asserts
  `backend_for`, called against that same unmodified grant, now resolves the
  corrected primary a `FakeTopologyRefresh` was taught — the exact call a
  live session's connection acquire makes.
  Acceptance: the mock sidecar implements the new RPC and the real client
  round-trips it over a live socket; a successful refresh is visible to
  `backend_for` without a new grant; a failed refresh changes nothing; and
  every one of the four crates touched — `pgprox-core`, `pgprox-auth`,
  `pgprox`, and the frozen proto itself — passes its own gates with the
  change included.

## M73: a failed dial had exactly one chance, unconditionally

- [x] `M73.0` Safe-only retry, scoped to a connection that sent nothing.
  The request was broader than this: retry any transient failure, reading or
  writing, configurably. Granted only where it can be granted without knowing
  anything about what a statement did, because the wider case needs to know
  whether a server already acted on bytes this process sent it, which this
  change does not attempt to track. ADR 0029 names the distinction and the
  follow-up left undone.
  A failed dial has sent nothing to anyone, on every attempt, because opening
  a connection is the whole of what happened: there is no partial state to
  reason about, which is what makes `LivePool::open` the one place a retry
  policy applies unconditionally rather than behind a runtime check for
  safety.
  `pgprox_core::retry::RetryConfig`: attempts, base, max. Off by default.
  `pgprox_core::retry::backoff` is a pure function taking the random draw as a
  parameter rather than drawing it, full jitter, tested exhaustively without a
  socket or a clock including the overflow case a large configured attempt
  count would otherwise hit.
  A new `pgprox_pool::jitter::Jitter` trait draws the roll, implemented in
  `bin/pgprox` by `SystemJitter` over the same `aws-lc-rs` provider the
  cancel-key entropy source uses, so a FIPS build carries one validated
  randomness source. Not `pgprox_session::cancel::Entropy` reused: that
  trait's contract is a cancel key's unguessability and refuses outright
  rather than fall back to anything predictable, which is the wrong shape for
  a delay defending nothing; reusing it would also reach the wrong way across
  the dependency graph, since `pgprox-session` depends on `pgprox-pool`.
  Read from a `retry:` section in the configuration document, parsed the same
  way `drain_grace` is. Does not hot-reload: `max_client_conns` and each
  server's cap reload because `M70.0` wired them through the tick loop,
  `retry` was not, and the document says so rather than leaving an operator to
  discover it by watching a change not take effect.
  Proved with a fake connector scripted to fail a set number of times: a dial
  that fails once and then succeeds is retried and returns the connection; a
  dial that never succeeds is reported once the policy is exhausted and opens
  nothing, ever; the default policy retries nothing, unchanged from before
  this existed. All at microsecond-scale delays, so the suite pays nothing for
  it.
  Considered and rejected: a `pgprox_pool_retry_total` metric, reasonable but
  a shape nothing yet uses; hot-reloading the policy, which would need
  threading `retry` through the same tick-loop path `M70.0` built for caps.
  Acceptance: a scripted dial failure is retried up to the configured count
  and no further; the default is unchanged behaviour; the pure backoff
  function is tested including the shift-overflow case; and the whole
  workspace, 1,957 tests across every crate, passes with the change included.

## M74: an authenticated client that went quiet had no way to be closed

- [x] `M74.0` A configurable client idle timeout, and three rejected designs
  that measured what a per-connection future can't afford.
  `client_idle_timeout:` in the document, off by default: how long an
  authenticated client may sit idle, between transactions, before pgprox
  closes it. Closed with `57P05`/`idle_session_timeout`, the code Postgres's
  own setting of the same name uses, so a driver that already handles that
  GUC needs nothing new to handle this.
  The obvious implementation, a `tokio::time::Sleep` or a second per-session
  `Shutdown` watched in the relay loop's own `select!`, was tried both ways
  and both failed the session-future size budget `M9.23` set:
  `one_session_costs_less_than_the_slab_buffer_it_no_longer_holds` asserts
  under 5 KiB, was 5,048, and had 72 bytes of headroom before this. A second
  `Shutdown` measured 5,288; a raw timer measured 5,224; both fail. A future
  is the union of everything alive across its awaits, and every branch a
  `select!` can take is paid by every connection whether or not it uses that
  branch.
  The design that fits reuses the `Shutdown` `Sessions::shed` already fires,
  for both reasons, and answers "which reason" with
  `Sessions::was_idle_timeout(conn)` — a registry lookup made once, at the
  moment the signal wakes, through state (`context`, `conn`) the relay loop
  already carries, rather than a flag threaded in and held across every
  subsequent await. That version measured 5,112: 64 bytes over baseline, with
  margin, against a flag-as-parameter version that measured 5,136 and had
  almost none. ADR 0030 records all four numbers and the order they were
  found in.
  The timer itself is not in the relay loop. `idle_timeout_pass` is a plain
  function the existing tick loop calls beside `shed_pass`, walking the same
  `Sessions::views` shed already walks and closing whichever clients have
  been idle long enough: one `Instant` comparison per idle client per second,
  in a walk that already runs, rather than a timer every connection pays for
  whether or not it is configured.
  Never mid-transaction, the same guard `shed_pass` uses and for the same
  reason: a session holding a connection is doing something, whatever the
  client on the other end of it is doing.
  Proved end to end: a real session over a real socket, closed by
  `Sessions::close_idle` the way the tick loop would close it, told 57P05 and
  the idle-session-timeout message rather than 57P01 and the shed one. Also
  proved that the pass reads each client's own age rather than a fleet clock,
  and that a session holding a connection is left alone regardless of how
  long it has held it.
  Acceptance: the session future stays under the 5 KiB ceiling with the
  feature built in, closing an idle client and shedding one are
  distinguishable to the client and to `SHOW`-level counters, and the whole
  workspace, 1,966 tests, passes with the change included.

## M75: upstream TLS sent a ClientHello where Postgres expects a request

- [x] `M75.0` Negotiate TLS the way Postgres negotiates it, and refuse the
  answer that means no.
  `TlsMode::Verified` handed the freshly opened socket straight to
  `TlsConnector::connect`. Postgres does not speak TLS from the first byte: a
  client sends an `SSLRequest`, a bare length and a magic code with no message
  tag, and the server answers one byte, `S` to proceed or `N` to say it has
  none. A real server therefore read `0x16 0x03 0x01 ...` as a startup packet
  declaring a 369 MB message and hung up, so the default upstream TLS mode
  could not connect to any Postgres at all.
  `request_tls` sends the request through `pgprox_proto::encode_frontend::
  ssl_request`, the same encoder `bin/pgload` already used for the same three
  bytes on the wire, and reads the answer. `S` proceeds. Everything else is a
  refusal, `N` included: carrying on would put this tenant's backend password
  on the plaintext socket just established, after asking for encryption and
  being told there is none, which is the downgrade this module's own header
  says must not happen. A socket that closes before answering is a refusal
  too, rather than a read of whatever was in the buffer.
  Not direct TLS. `sslnegotiation=direct` exists in a modern libpq and is not
  a smaller change: the server requires the `postgresql` ALPN protocol for it,
  which is exactly how it tells a stray `ClientHello` from a client that meant
  one, and this `ClientConfig` sets no ALPN.
  Why nothing caught it, which is the part worth keeping. `TlsMode::default()`
  is `Verified` and an unspecified proto value maps to it, so this is what a
  correct sidecar returns. Every fixture in the repository sets
  `PGPROX_MOCK_TLS=disabled`: `docker-compose.yml`, `.fips.yml`, `.compare.yml`,
  `kind/values.yaml` and `localstack.sh`. And
  `a_verified_backend_is_dialled_over_tls` connected to a bare `TlsAcceptor`.
  `M17.4` wrote that test precisely because every other `Verified` case
  asserted a failure, and the fixture it chose still could not tell a
  handshake that runs from one that reaches a Postgres. The fixture now
  negotiates, so it can.
  Proved against a real server, not only against the fixture: PostgreSQL 17
  with `ssl=on` and a leaf certificate under a test CA, dialled with
  `TlsMode::Verified`, a startup packet written through the negotiated channel
  and an `Authentication` message read back. The same test against the same
  container fails on the code before this change and passes after it, which is
  what makes it evidence rather than a demonstration.
  Not done, and named rather than left: no committed fixture exercises this.
  The verification above was a container reachable only from inside its own
  network namespace on this machine, which is not something `e2e.sh` can be
  changed into without a run nobody here could execute. A compose arm with a
  TLS-enabled primary and a mock sidecar left on its default mode is the shape
  it wants, and until one exists the default path is covered by unit tests and
  one manual run.
  Acceptance: the first bytes upstream under `Verified` are the eight-byte
  `SSLRequest`; a server answering `N` is refused with a reason naming TLS,
  and one that vanishes mid-negotiation is refused too; the fixture that
  proves the happy path negotiates before accepting; and the whole workspace
  passes with the change included.

## M76: the allowance was divided into the pools, with a floor under the division

- [x] `M76.0` Spread an allowance across pools instead of dividing it, so the
  sum is the allowance.
  `share_per_key` was `(guaranteed + leased) / keys` with `.max(1)` under it,
  and the floor is where the cap went. A pool is one `(server, database, user)`
  triple, so a node holding more pools than its allowance gave every one of
  them a limit of one: allowed a hundred, opens three hundred. The test beside
  it called this "overshooting a division by one". It overshoots by
  `keys - total`, which is unbounded, and on the fleet described in
  `mission.md`, thousands of tenant databases per server, it is routinely a
  multiple rather than an edge.
  This was the only arithmetic in the binary that could breach the cap while
  every layer under it reported itself correct. `pgprox-cluster` holds
  `guaranteed x nodes + leases <= cap` and hands out an entitlement; the pool
  holds whatever limit it is handed. Neither was wrong. What sat between them
  was.
  `shares_across_pools` returns one limit per pool and its sum is never more
  than the allowance. Each pool takes the floor and the remainder goes one
  connection at a time to the pools with the most demand behind them, ties
  broken by position so two reads of the same state decide the same way. The
  remainder used to be stranded as well, so nine across two pools was four each
  and the ninth was allowed by the cluster and offered to nobody.
  A pool given zero waits rather than opening, which is what already happens at
  a cap and is reported the same way. It is recomputed every tick and waiting
  clients are demand, so a pool that gets nothing this second is first in line
  the next. That is a real behaviour change for a node with more pools than
  allowance: it now refuses some clients rather than serving all of them past
  the cap. The mission gives the cap no graceful degradation and gives
  everything else some, which is the order this resolves them in.
  `pools_for` now carries each pool's own count as well as the total, because
  pointing an allowance at the demand needs the counts apart.
  Proved exhaustively over every allowance up to forty against every pool count
  up to twelve, the way `pgprox-cluster`'s own split test is: the sum never
  exceeds the allowance. Plus the two cases that name the behaviour, an
  allowance smaller than the pool count landing where the clients are, and a
  remainder going to the busier pool rather than nowhere.
  Removed `an_allowance_divides_across_pools_and_never_reaches_zero`, whose
  assertions were the bug: it required a limit of one where zero is the only
  answer that holds the cap.
  Not done: a node still does not close connections it holds above a lowered
  limit, so an allowance that shrinks takes effect on opens rather than on what
  is already open. That is `M76.1`, and it is the other half of the same
  finding.
  Acceptance: the sum of the shares never exceeds the allowance for any
  combination in the swept range; an allowance of one across eight pools opens
  one connection rather than eight; and the whole workspace passes with the
  change included.

- [x] `M76.1` A lowered limit reaches the connections that already exist.
  The other half of `M76.0`'s finding. `Pool::release` pushed a cleanly
  released connection back into `idle` whatever the limit said, and `acquire`
  reuses from `idle` without consulting it, so a lowered allowance only ever
  took effect on opens. Under steady traffic nothing is idle long enough for
  the reaper's thirty-second timeout to reach it, so a node whose lease was
  refused held its old count until `max_lifetime`, an hour by default, retired
  connections one at a time.
  That is a fleet-level cap breach rather than a local untidiness, and it is
  the case the ledger cannot see. `LeaseLedger` frees a grant's capacity the
  moment it expires, and `lease.rs` is explicit that waiting one TTL after
  taking office proves every lease the old leader issued "has either been
  renewed, and so is known, or has expired, and so is gone". Gone from the
  ledger. The sockets were never part of that argument, so a new leader could
  hand the free pool to a peer while the old holder still had it open.
  A clean release above the limit is now closed instead of kept. Measured
  against what the pool would be *keeping*, `idle + opening`, rather than
  against its total: connections still checked out are in use and are not this
  decision's to make, and counting them would discard a healthy connection
  because two others happened to be mid transaction. A pool told to hold one
  while three were busy drained to nothing under the first version of this,
  which `lowering_the_limit_does_not_close_connections_in_use` caught.
  Bounding the idle set bounds the pool, because nothing new opens above the
  limit and every connection in use comes back through here eventually.
  Nothing is closed underneath a running transaction, which is the rule this
  had to be written around rather than through: `set_limit` still closes
  nothing at the moment it is called, and the drain happens at transaction
  boundaries, so it goes as fast as the clients finish and no faster.
  Proved three ways: five clean releases against a limit of two leave two; the
  same pool under fifty rounds of acquire-and-release, which is the traffic
  pattern that defeated the reaper, converges on the limit rather than staying
  above it; and a pool inside its limit still keeps what it is handed, since
  discarding a healthy connection costs the reconnect a pool exists to avoid.
  Acceptance: a busy pool converges on a lowered limit, an unbusy one is
  unchanged, connections in use are never closed, and the whole workspace
  passes with the change included.

## M77: the chart put an unauthenticated write API on the tenants' address

- [x] `M77.0` The admin port comes off the client service and gets its own,
  off by default.
  `admin.md` has said since `M43` that the HTTP API authenticates nobody, that
  this is a deployment decision, and "do not put the admin port on a network a
  tenant can reach". The chart then put it on the client service, as a second
  port beside 6432, so the one address every tenant application is given also
  answered `POST /v1/drain`, `/v1/undrain` and `/v1/pools/{...}/reset`. With
  `service.type: LoadBalancer`, which is an ordinary choice for a client-facing
  proxy, that address is external.
  The client service now carries the client port alone. An `adminService`
  block, disabled by default, creates a service carrying only the admin port
  for an operator, a dashboard or a Prometheus that wants one address for it,
  which can then take its own type, annotations and policy without any of that
  reaching the port tenants use.
  What this is not: a service is not an access control. Pod IPs are routable
  whatever services exist, and the headless service still resolves every pod so
  that gossip works, so anything that can open a socket in the cluster can
  still reach the port. Restricting it is a NetworkPolicy, and the chart does
  not ship one, because a default-deny policy on that port breaks the readiness
  probe on any cluster whose CNI does not exempt the kubelet, and shipping a
  security control that silently takes the fleet out of service is worse than
  shipping none and saying so. `NOTES.txt` says so at install time, where an
  operator is looking.
  What the chart can do, and now does, is refuse to create an external address
  for the port and refuse to put it on the name tenants are told to use.
  Also considered and not done: serving the write half on a separate bind
  address. `pgprox-admin` already splits `read_routes` from `write_routes`
  precisely "so a deployment can expose it on a surface with different access",
  and `bin/pgprox` merges them onto one router, so the split currently buys
  nothing. Acting on it means a second listener, a flag, and its own tests,
  which is a milestone rather than a line.
  Proved by rendering: the client service carries one port, the admin service
  appears only when enabled and carries one port, and the drain command in
  `NOTES.txt` is unaffected because it goes through `kubectl exec` to
  localhost.
  Acceptance: `helm template` with default values yields a client service with
  no admin port and no admin service; with `adminService.enabled=true` it
  yields one; and the documentation says what the change does and does not
  achieve.

## M78: the cache key named which rows, not what the bytes said

- [x] `M78.0` The key names how an answer was rendered, in both of the two ways
  it was not.
  ADR 0024 carried one observation two fields further than ADR 0021 had, and
  its closing table has a row for "two sessions differing in nothing" answering
  "the same". That row was wrong, because what the store holds is not rows: it
  is the server's bytes, verbatim, for the same reason the relay never parses a
  `DataRow`. Two sessions can agree on every row of an answer and disagree
  entirely on what the answer looks like. ADR 0031 records the distinction and
  the two things it found.
  **The result format.** A `Bind` carries a result format code per column and
  the server encodes what it returns accordingly, so `SELECT id` is two ASCII
  bytes in text and four big-endian ones in binary. `bind_parameters` read the
  values and skipped the format codes, so the two keyed identically and a
  client that asked for binary could be served the text entry, with a
  `RowDescription` telling it text while it expected binary. Reachable through
  every driver that binds for binary results, which is most of them.
  Normalized so that every spelling of all-text is empty: no codes, one code of
  zero for every column, and a list of zeroes are one request on the wire and
  have to be one key. Empty is also what a simple query has, so a text `Bind`
  still shares an entry with the simple query of the same SQL, which is
  `M9.22`'s property and had to be left standing.
  **The session settings that render a value.** `TimeZone`, `DateStyle`,
  `IntervalStyle`, `extra_float_digits`, `bytea_output` and `client_encoding`
  are all on `pgprox-pool`'s replay allowlist, which is exactly what makes this
  reachable: a session sets one, is not pinned, keeps it across a connection
  change, and shares entries with every other session of that tenant, database
  and role. `client_encoding` is the worst of them, since it decides the
  encoding of most of the bytes in most answers. `search_path` was in the key
  already and was there for the narrower reason, which is why the other six
  were not noticed beside it. `standard_conforming_strings` is a seventh of a
  different kind: it changes what the SQL text means rather than how the answer
  prints.
  `role` and `session_authorization` would be the most serious of the lot and
  are safe today, because they are absent from the replay allowlist, so a
  session that sets either is pinned and a pinned session is refused a key.
  That is two lists happening to agree rather than a decision, so it is
  written down in `settings.rs` where somebody adding `role` to the replay list
  would read it.
  The list lives in `pgprox-cache`, beside the rule about which statements may
  be cached, rather than in the composition root that builds the key: there is
  one rule about what reaches an answer and it should have one home.
  The fingerprint is length-prefixed rather than delimiter-separated, and that
  came from the test rather than from the design. Joined by newlines, a session
  setting `TimeZone` to `UTC\ndatestyle=ISO` produces the same string as one
  that set both, so a value can forge another session's fingerprint. The values
  are a tenant's own text and every delimiter the format could use is one a
  tenant can write, so the length is what makes the encoding injective.
  `a_value_cannot_forge_another_sessions_fingerprint` failed against the first
  implementation, which is the only reason this paragraph exists.
  `CacheKey` changed, which is a contract change: the struct, every
  construction site across three crates and their tests, benches and budgets,
  the crate's `AGENTS.md`, and ADR 0031, in one commit.
  What this costs: a tenant whose sessions disagree about their settings now
  holds more entries, which is the correct number rather than a regression,
  since those sessions were never asking the same question. A fleet where every
  session sets the same `TimeZone` at connect time is unaffected. `application_name`
  is excluded despite being replayable, because it reaches no answer and half
  the drivers set it per process, which would give every application instance a
  private copy of every entry.
  Acceptance: two sessions differing only in `TimeZone` build different keys; a
  binary-format bind and a text one build different keys; a text bind still
  keys as the simple query of the same SQL; a session that set nothing still
  fingerprints to empty; every setting on the list reaches the fingerprint; and
  the whole workspace, 2,003 tests, passes with the change included.

- [x] `M78.1` A comment that described a feature the next task had already
  built.
  `cache_key`'s documentation said "Only the simple protocol, for now", and
  explained that the codec could not read a `Bind`'s parameters so a bound
  statement was deliberately a miss rather than a wrong key. That was true when
  `M9.9` wrote it and stopped being true at `M9.12`, which taught the codec to
  read them and added `serve_held` as the extended protocol's half of the same
  path. The sentence outlived its reason, so a function's scope read as the
  whole feature's, and a reader working out whether the `M78.0` bug could reach
  bound statements would have concluded from this comment that it could not.
  It now says which half it is and names the other, and records what it used to
  claim, because "this was true once" is the part that stops somebody
  re-deriving the old answer from an old commit.
  Acceptance: the comment names both halves and the crate's tests are
  unchanged, since nothing here is behaviour.

## M79: the fixture gap M75.0 named

- [x] `M79.0` An e2e arm with a TLS-enabled primary, so the default upstream
  mode meets a Postgres.
  `M75.0` fixed a proxy that could not open a TLS connection to any Postgres
  and said plainly what it could not do: prove it in a fixture anybody else
  could run. Every service in `deploy/docker-compose.yml` set
  `PGPROX_MOCK_TLS: disabled`, so the mode a real sidecar returns by default
  had never been pointed at a database, and the one unit test of that path
  connected to a bare `TlsAcceptor`, which cannot tell a handshake that runs
  from one that reaches a Postgres. This is that fixture.
  An `upstream-tls` service makes a CA and one leaf naming `primary`,
  `replica-1` and `replica-2`, then exits. A CA rather than the self-signed
  certificates the proxy nodes make for themselves, because the two sides are
  not symmetrical: a client is told `PGSSLMODE=require`, which encrypts without
  verifying, and the proxy's upstream side has no such mode. `TlsMode::Verified`
  is the only mode it has, so a self-signed server certificate is refused as a
  CA used as a leaf and would have proven only that the failure path works.
  It runs on the proxy image because the postgres image has no `openssl` and
  the proxy image has one for its own entrypoint, which is cheaper than a third
  image.
  The primary copies the leaf into `PGDATA` rather than opening it where it
  lies. Postgres refuses a key it considers loosely held, and the generator
  runs as root in another container and cannot know this image's postgres uid;
  the init script runs *as* postgres, so its copy is owned correctly by
  construction rather than by a uid somebody has to keep true. The paths in
  `postgresql.conf` are relative because the replicas are `pg_basebackup`
  clones and inherit that file: an absolute path would name the primary's
  `PGDATA`, which is not where a replica keeps its copy.
  One node of the three, `pgprox-3`, dials over TLS. The stack keeps a node on
  each side of the choice, a failure names the mode instead of taking the fleet
  with it, and it is the node the watermark assertion already drives, so the
  TLS path carries real traffic rather than one probe.
  The assertion asks the *database*, not the proxy: `SELECT ssl FROM
  pg_stat_ssl WHERE pid = pg_backend_pid()` is Postgres's own record of how the
  backend carrying this very statement is connected. Nothing the proxy reports
  about itself could say that.
  Proved three ways rather than asserted once. The full run is green with the
  new check among the others. The `prove` path runs the same query through
  `pgprox-1`, whose upstream mode is `disabled`, and requires `f`: same
  statement, same database, same view, one difference, so the predicate is
  shown able to tell them apart at no setup cost, because the two plaintext
  nodes were already there. And the check was run against a build with
  `M75.0` reverted, where it fails along with two others, which is the claim
  that this arm would have caught the original bug rather than merely passing
  beside it.
  What the reverted run also showed, and what is not fixed here: a client whose
  upstream dial fails is told "too many connections, please retry".
  `pgprox_core::pool` maps every `PoolError::ConnectFailed` to
  `ClientError::UpstreamAtCap` with a cap of zero, on the argument that from
  the client's side an unreachable upstream is the same as a full one. That is
  true of what a client should do and false of what an operator should look at,
  and it is the same distinction `Pool::give_up` is careful to draw between
  being at a cap and having timed out. A separate finding, named rather than
  widened into this task.
  Acceptance: `scripts/e2e.sh` passes with the new assertion; `scripts/e2e.sh
  prove` shows the assertion distinguishes an encrypted upstream from a
  plaintext one; and the assertion fails against a build without `M75.0`.

## M80: a dial that failed said the server was full

- [x] `M80.0` An unreachable upstream is its own condition, and its reason
  reaches the log.
  `M79.0` reverted `M75.0` to prove the new e2e arm could fail, and what the
  reverted stack reported was not a proxy that could not reach its database. It
  was `ERROR: too many connections, please retry`. `pgprox_core::pool` mapped
  every `PoolError::ConnectFailed` to `ClientError::UpstreamAtCap` with a cap
  of zero, on the argument that from the client's side an unreachable upstream
  is the same as a full one.
  That argument is right about what a client should do, retry either way, and
  wrong about everything else. The SQLSTATE was `53300`, which says capacity to
  anything reading it. `Display`, which is what the log line carries, read
  "upstream primary:5432 is at its connection cap of 0" about a server nobody
  had counted. And `PoolError::ConnectFailed`'s `reason`, the one field naming
  the certificate or the hostname or the route that was actually wrong, was
  dropped by the conversion and logged nowhere. So an operator whose fleet
  could not reach its database was told to go and look at capacity, and given
  nothing anywhere that pointed at the truth.
  It is the same distinction `Pool::give_up` already draws, in a comment, when
  it decides between `AtCap` and `Timeout`: "one says the server is full, the
  other says this node is. Reporting the cap when the pool has headroom would
  send them to the wrong place." That care had not been applied one layer up.
  `ClientError::UpstreamUnreachable { server }`, `08006 connection_failure`,
  retryable, and a client message that says the database could not be reached
  and says nothing about which of the several ways a dial can fail this was.
  No `reason` field, deliberately. A `String` is 24 bytes on every
  `ClientError`, and that type is live across an await in the session future,
  which `one_session_costs_less_than_the_slab_buffer_it_no_longer_holds` holds
  under 5 KiB with eight bytes to spare after `M74.0`. Carrying it would have
  cost sixteen and failed that test. Measured rather than assumed:
  `ClientError` is 32 bytes before this change and 32 after.
  So the reason is logged instead, at `why_upstream_failed`, the one boundary
  where a `PoolError` becomes a `ClientError` on the session path. That is also
  where it belongs, since it was never going to reach a client. Only the dial
  case logs: a cap and a timeout are fully described by the errors they become,
  and a line per refusal on a fleet that is genuinely full is a log nobody can
  read.
  Proved at both levels, and each shown able to fail. The conversion test
  requires `08006`, requires the client message not to mention connections, and
  requires the operator-facing `Display` not to mention a cap; it was written
  first and failed with `left: "53300"`. The wire-level test runs a real
  `ConnectFailed` through the production path into an `ErrorResponse` and reads
  the bytes; against the old mapping it fails with
  `C53300 Mtoo many connections, please retry`. It also asserts the reason and
  the hostname do not reach the client, since the new variant carries a
  `ServerId` and the whole point of `client_message` is that it does not travel.
  Added to `one_of_each`, so the test that proves no hostname reaches a client
  message covers it, and to the documented SQLSTATE table and the test that
  holds the table and the code together.
  Acceptance: a failed dial reports `08006` and not `53300`; neither the reason
  nor the upstream hostname reaches the client; the reason appears in the log
  with the server that produced it; the session future is unchanged in size;
  and the whole workspace, 2,004 tests, passes with the change included.

## M81: a test that counted everything the fake heard

- [x] `M81.0` The cache tests count what a session sent, not what the server
  was told.
  Six assertions in `serve.rs` captured `statements_seen(addr).len()` before a
  statement and compared it after. The fake records every frame body it is
  sent, and the fake in these tests is the primary *and* the replica, so the
  replica poller's `REPLICA_QUERY` lands on it too, on the poller's schedule
  rather than the test's. The number being compared was one a background task
  could change between the two reads.
  Found by running the suite thirty times at a hundred and twenty-eight test
  threads: six failures, and the one that named itself said
  `a differently spelled statement missed: ["SELECT 1", "SELECT
  pg_last_wal_replay_lsn(), pg_is_in_recovery()"]`, left 2, right 1. The cache
  had hit. The poller had spoken.
  Two of the six failed that way, about once in fifteen suite runs at that
  parallelism and about twice in twenty-five at the default. The other four are
  worse and had never failed: they assert `len() > before`, so a poller probe
  arriving in the window satisfies them whether or not the statement they are
  about ever went upstream. A test that passes for the wrong reason does not
  announce itself, and four of these were one background probe away from
  holding while the behaviour they name was broken.
  `client_traffic` filters `pgprox_session::probe::REPLICA_QUERY` out and every
  site reads it instead. The constant rather than a literal, so the filter
  cannot drift from the query. All six now print what the fake heard when they
  fail, which is what made the cause obvious in a hundredth of the time it took
  to find it.
  The poller is not the problem and nothing about it changed. What it exposed
  is that "what the server heard" and "what this session sent" were one number
  in tests that only ever meant the second.
  Proved by the same stress that found it: thirty runs at a hundred and
  twenty-eight threads, six failures before and zero after, plus forty runs at
  the default parallelism, also zero.
  Acceptance: the two flaking assertions survive the stress that broke them,
  the four that could pass wrongly now cannot, and the whole workspace passes.

- [x] `M81.1` Three tests wait for the event instead of sleeping past it.
  `a_broken_document_leaves_the_previous_one_serving` slept 2,500ms,
  `a_drain_through_the_admin_api_takes_the_node_out_of_service` and
  `two_running_nodes_learn_about_each_other` slept 1,500ms each, and then
  asserted. The second one's own comment admitted the shape: the margin was
  "for a loaded machine rather than for the protocol", which is a number that
  is too long on a fast machine and unproven on the machine it was chosen for.
  `M56.0` is this repository's record of exactly that going wrong, a five
  second timeout that was not enough on a two core runner.
  An `until` helper waits for the condition with `PATIENCE`, thirty seconds, so
  a slow machine waits rather than fails and a fast one stops as soon as the
  work is done. 1.585s becomes 0.148s, 2.608s becomes 1.065s, and 1.5s becomes
  1.096s.
  The broken-document test needed the event choosing carefully. It asserts a
  negative, that a good configuration survived a bad one, which no amount of
  polling can prove; what it now waits for is `is_healthy` going false, the
  node having read the bad document and refused it, and it makes the negative
  assertion at exactly that moment. Sleeping a guess asserted it at a moment
  that might have been before the bad document was read at all.
  The drain test caught its own conversion. Waiting only for `/readyz` to fail
  turned it green in 0.09s and then red at the next line with `left: Active`:
  readiness flips when the API sets the state and the fleet learns on the next
  gossip round, so one wait saw the first and was too early for the second. It
  waits for both, in the order the test says it checks them, which is the
  ordering claim it was making all along and had been getting for free from a
  sleep long enough to cover both.
  Not converted, and named rather than left quiet: four sleeps in this file
  assert that the tick *ran*, `ran >= 2` and the like. A rate needs elapsed
  time, so the only way to make those fast is an injectable `TICK`, which is a
  change to production code bought entirely with test speed. The suite is 5.3
  seconds; it is not worth it today.
  Acceptance: the three tests assert the same properties, none sleeps a
  duration chosen for a machine, and the whole workspace passes.

- [x] `M81.2` A flake found, characterised, and left in the tree on purpose.
  `a_connection_that_died_while_idle_is_not_handed_to_a_client` fails roughly
  one suite run in twelve. It is not one of the two `M81.0` fixed and it is not
  caused by anything in this milestone: it failed twice in the first hunt,
  before any change here.
  Characterised rather than guessed at. The panic is an `UnexpectedEof` in the
  test's `expect` helper, and a backtrace puts the call at the read of the
  answer to `SELECT 2`: the second statement, the one that has to be served
  from a fresh connection after the pooled one died. So the session is ending
  without answering and closing the client rather than writing an error to it.
  Two candidates, neither confirmed. `fit_connection` can return
  `ShellError::Disconnected` when `take_connection` finds no payload for a
  guard it just acquired, which ends a session silently and is the only path
  here that produces no message. Or the session task panics, which a
  `tokio::spawn` swallows and which reaches the test as the same EOF. Its
  exhaustion path is not the answer: that returns `UpstreamClosed`, which the
  client would see as an `ErrorResponse` and the test would report differently.
  Not fixed, and the attempt is recorded because it is the useful part. The
  fake was given a `Notify` fired when it drops the socket, so the test could
  wait for the close rather than sleep 20ms hoping to cover it. That is a
  better test and it fixed nothing: the unmodified version survived thirty runs
  pinned to one contended core, and the signalling version *raised* the failure
  rate from about two in twenty-five suite runs to eight in forty, because
  waiting for the close makes the dead-connection path deterministic instead of
  sometimes skipped. A change that does not fix the fault and makes the suite
  fail more often is not an improvement to commit, so it was reverted whole.
  Instrumenting it to print the session's own outcome hid it: thirty stressed
  runs of the diagnostic build produced nothing.
  What is left in the tree is the original 20ms sleep with a comment saying it
  is not the cause and pointing here, so the next person does not spend the
  same afternoon proving it again.
  Acceptance: the failure has a location and a mechanism written down, the two
  candidate causes are named, and nothing claims to have fixed it.

## M82: the flake that will explain itself next time

- [x] `M82.0` Three candidates ruled out, and a failure that names its own
  cause when it next fires.
  `M81.2` left `a_connection_that_died_while_idle_is_not_handed_to_a_client`
  characterised and unfixed: the client sees its half of the duplex close while
  waiting for the answer to `SELECT 2`, so the session ends without answering
  and without writing an error. This is the second attempt at the cause.
  **What is now ruled out.** `M81.2` named two candidates and both are wrong.
  Each `ShellError::Disconnected` on the statement path was temporarily made a
  distinguishable `Internal` error, which writes to the client rather than
  closing silently, so either firing would have arrived as an `ErrorResponse`
  naming itself. The failure reproduced twice under that build and neither
  marker appeared: not `fit_connection`'s `take_connection` returning `None`,
  and not the release path's `held.take()` returning `None`. A third path was
  ruled out by reading rather than by running: the arm that catches a dead
  upstream mid-answer answers `wire.refuse(UpstreamClosed)`, which writes, so
  it cannot produce silence either. What is left is a panic inside the session
  task, which `tokio::spawn` swallows, or a silent exit from a path not yet
  enumerated.
  **Why it was not caught.** It needs whole-suite load and does not survive
  being watched. In isolation it never fires: three hundred consecutive
  executions of the same scenario in one process pass in 8.6 seconds. Under the
  full suite at a hundred and twenty-eight test threads it fires in single
  figures per hundred runs, and every build carrying instrumentation on the
  read path went hundreds of runs without it. A loop harness built to force the
  odds hit `AddrInUse` on its own listeners first, which is the harness and not
  the proxy, and after being trimmed it went six hundred loaded executions
  without a failure. Roughly four hundred stressed suite runs across two
  sessions produced four usable failures, none of them under instrumentation
  that could name the cause.
  **What is committed instead.** The read of the answer to `SELECT 2` no longer
  goes through `expect`, which unwraps and reports "early eof" at a line in a
  shared helper. It reads fallibly and, on the close, joins the session task and
  reports what it returned, whether it panicked rather than returned, what the
  client had been sent, and the pool's state. The branch runs only when the test
  is already failing, so the passing path is untouched.
  Verified by forcing it: aborting the session task produces the full message,
  including `session panicked: false` and the join error, which is exactly the
  discrimination two sessions of debugging did not have.
  This is diagnosis deferred, not a fix, and the entry says so. What it buys is
  that the next occurrence, in CI or on anyone's machine, answers the question
  instead of restarting it.
  Acceptance: the failure path reports the session's outcome and whether it
  panicked, the message was proven to render by forcing the condition, the
  passing path is unchanged, and nothing claims the flake is fixed.

## M83: the pages still described a six-part cache key

- [x] `M83.0` Two pages catch up with the key `M78.0` widened.
  `M78.0` added `result_formats` to `CacheKey` and renamed `search_path` to
  `settings`, taking it from six components to seven. `docs/features.md`
  enumerated the six and said "All six", and `docs/multitenancy.md` said "All
  six components are load-bearing". Both were then wrong about the one thing
  those paragraphs exist to state.
  Caught by `scripts/gates/m44-complete.sh`, which counts the `pub` fields of
  `CacheKey` and requires both pages to agree in words. It failed with "the
  cache key has seven components and the pages claim otherwise", which is the
  gate doing exactly what its own comment says it is for: "a seventh added
  without a word on either page leaves a reader with a wrong picture of what
  separates one tenant's answers from another's".
  Worth recording how it was missed rather than only that it was. The gates are
  not on the pre-commit path, by design, because they are slow; CI runs all
  forty-five on every commit. So `M78.0` was green through every hook and would
  have gone red on the first push. Running the full gate sweep locally is what
  found it, and it is the only one of the forty-five that was failing.
  The pages now say seven and say what the two new components are for, which is
  the part a reader needs: they are about the bytes rather than the rows, so a
  client that asked for binary results is not handed the text ones, and two
  sessions disagreeing about `TimeZone` or `client_encoding` are not asking the
  same question even when every row is identical.
  Acceptance: `scripts/gates/m44-complete.sh` passes, both pages name seven
  components, and the two added since `M24.4` are explained rather than
  counted.

## M84: the suites that need Docker, run against this session's changes

- [x] `M84.0` Forty-five gates, the conformance suite and the driver matrix,
  against the tree `M75` through `M83` left.
  Everything committed since `M74` had been verified by the pre-commit hooks
  and the unit suite. That is sixteen fast checks and 2,004 tests, and it is
  not what CI runs: the forty-five milestone gates, the conformance suite
  against real Postgres and the driver matrix against a real proxy had none of
  them seen this work. `M75.0` changed how every upstream connection is
  established and `M78.0` changed a `pgprox-core` DTO, which are exactly the
  changes those suites exist to catch.
  **The gates found one.** `m44-complete.sh` counts the `pub` fields of
  `CacheKey` and requires two pages to agree in words; `M78.0` made it seven
  and left both saying six. Fixed in `M83.0`. Forty-four of the forty-five
  passed, and the one that failed failed for the right reason.
  **Conformance passes** against Postgres 17 and 18, both directions: the codec
  driving a real server, and psql, pgx, asyncpg, JDBC and npgsql driving a
  harness built on the codec. No container outlived the run.
  **The driver matrix passes** against a running three-node stack, all five
  drivers. The regenerated report differs from the committed one only in its
  date and the commit it describes, so the result is unchanged by everything
  this session did to the dial path, the pool, the cache key and the error
  taxonomy.
  Not run, and worth naming rather than implying: `mutants.sh`, which is hours
  and which CI shards nightly; `scale.sh`, `compare.sh`, `pinning.sh` and the
  other measurement scripts, which produce numbers rather than verdicts; and
  `fips-check.sh`, which needs a toolchain this machine does not have, since
  `aws-lc-fips-sys` will not compile here against gcc.
  Acceptance: the gate sweep is green after `M83.0`, conformance passes on both
  major versions, the driver matrix passes and its report is refreshed, and
  what was not run is listed.

## M85: eighty-seven milestones and no way to jump to one

- [x] `M85.0` A table of contents at the top of this file, one line per
  milestone heading, and a `check-drift.sh` rule that fails if a heading is
  added here without a matching line. Nothing else moved: every task stays at
  the line its commit put it on, and every existing gate that greps this file
  by exact path still reads the same content it always has.
  Read alone, `git log` names 84 milestones; this file holds two more headings
  that are not milestones at all, "Found after M7 closed" and "Found after M8
  closed", so the index links milestones only and leaves those to be found by
  reading the section around them.
  The check reuses the shape `M51`'s script index and `M12`'s gate index
  already use: an index is only worth its completeness, so the thing enforced
  is not "a table of contents exists" but "every heading is in it". Planted in
  `tests/gates/negative.sh` with a heading added and no matching line, same as
  every other check-drift.sh rule that reads a file by path.
  Acceptance: every `## M...` heading in this file has a link in the table of
  contents, `scripts/check-drift.sh` fails on a planted heading with no
  matching line and passes once one is added, and no existing task's text
  changed.

## M86: the status table nobody kept adding rows to

- [x] `M86.0` `roadmap.md`'s status table stopped at `M29`. Rows added for
  `M30` through `M53`, whose sections had been sitting there complete and
  unlisted, and for `M85`. `M54` through `M84` are noted rather than rowed:
  they have tasks and commits in this file and no roadmap section yet, and a
  row naming a section that does not exist is the defect this milestone is
  about, pointed the other way.
  `check-drift.sh`'s `M18.3` rule, that every milestone in the table names a
  real completion condition, only ever reads the table. Fifty-six milestones
  outside it were never checked by a rule written to guarantee exactly that.
  Backfilling the rows is what found it: `M35` and `M42` each had a section
  whose own closing sentence named a real script, and no fenced command block
  for the rule to read. Both scripts existed; the milestones had simply never
  been looked at. Fixed by adding the block each section already pointed to.
  Also moved: a sentence about `M-1` and `M0` being hard barriers that had
  landed between two table rows, breaking the table there, to prose below it.
  Acceptance: every milestone `M-1` through `M53` and `M85` has a row in the
  status table, every row's section names a runnable command,
  `scripts/check-drift.sh` passes, and `M54` through `M84`'s absence from the
  table is stated rather than silent.

## M87: the mutants nobody has swept since M22

- [x] `M87.0` A mutation sweep of `pgprox-core` found a mutant that does not
  fail a test. It takes down the machine running it.
  `cargo mutants` replacing `Lexer::advance`'s body with `()` turned
  `Lexer::next` into a function that returns `Some(token)` for the same
  unconsumed character forever, because eleven call sites across `next` and
  `skip_trivia` depend on `advance` to shrink `self.rest` and none of them
  re-check that it did. A caller that collects the iterator, which is what a
  lexer is for, grows its `Vec` at whatever rate the CPU can loop: thirty
  gigabytes free to swapping in under ten seconds, reproduced twice under a
  supervised, memory-monitored single-mutant run before being trusted as the
  cause rather than a coincidence of machine load.
  Nothing here is a defect in the shipped code. `advance` is correct, and
  `cargo nextest run -p pgprox-core` passes its real 241 tests in 0.14s with
  memory flat. The gap is that nothing stated the invariant a mutant broke:
  every arm of `next` assumes `advance` consumes input and nothing checked
  that it did, the same shape `M22.5` already fixed once for `word_end` and
  `is_word_char` disagreeing, in the same function, guarding a different pair
  of primitives.
  Fixed with one `debug_assert!` after `next`'s match, comparing `self.rest`'s
  length before and after, which is upstream of all eleven call sites rather
  than one more assert per arm. `skip_trivia`'s two comment-skipping branches
  got the same check for the same reason: `advance -> ()` breaks them too, by
  the same mechanism, one function up.
  This is why the milestone runs supervised. `cargo mutants` gives a suite a
  timeout for a hung *test*; nothing bounds a test that keeps producing output
  a caller keeps allocating, and that is a different failure mode than the
  ones `M10.13`'s per-test cap was built for. The fix stands on its own
  regardless of what caused the crashes reported against this session, and it
  was reproduced under monitoring specifically so a real fix would not be
  confused with a flaky environment.
  Acceptance: `cargo mutants -p pgprox-core -f crates/pgprox-core/src/sql.rs
  -F advance` reports the mutant caught rather than hanging, `cargo nextest
  run -p pgprox-core` passes all 241 tests unchanged, and the invariant is
  stated once per function rather than once per call site.

- [x] `M87.1` `nextest`'s `[profile.mutants]` capped at `test-threads = 4`.
  Added before `M87.0` found the actual cause of this session's crashes, on
  the working theory that `nextest` running one OS process per test, in
  parallel up to one per logical CPU by default, was compounding whatever
  `MUTANTS_JOBS` already had building or testing. That theory turned out to
  be incomplete rather than wrong: `M87.0`'s mutant crashed the machine in a
  single-mutant, `MUTANTS_JOBS=1` run where `test-threads` was never the
  limiting factor, so capping it did not and could not have fixed that class
  of defect on its own.
  Kept anyway, as a second, independent bound. `MUTANTS_JOBS` governs how
  many *cargo-mutants* workers build and test at once; `test-threads`
  governs how wide one worker's own test run fans out, and the two are
  orthogonal. A large suite (`pgprox-core`, `pgprox-session`) fanning out to
  twenty concurrent processes for every mutant, on top of `MUTANTS_JOBS`'s
  own parallelism, is unnecessary resource pressure with or without a
  memory-growing mutant in the mix.
  Acceptance: `[profile.mutants]` in `.config/nextest.toml` states
  `test-threads = 4` with the reasoning above; the default profile used by
  ordinary `cargo nextest run` is untouched.

- [x] `M87.2` Resuming the swept shard past `M87.0` found a second mutant of
  the same shape, in the branch `M87.0` did not guard. `trim_leading_space`
  replaced with the fixed literal `"xyzzy"` does not merely fail to shrink
  `self.rest`: called on an empty `rest`, it **grows** it back to `"xyzzy"`.
  `skip_trivia`'s first branch only checked `trimmed.len() != self.rest.len()`,
  which is true either way, so it reassigned `self.rest = trimmed` without
  asking whether that shrank or grew. Every time a caller's loop emptied
  `rest`, this branch refilled it, `next()` tokenised the same five bytes as a
  fresh `Word` again, and the cycle produced a `Some` forever. Reproduced
  isolated and monitored the same way as `M87.0`: 30 GB free to 16 MB in
  twelve seconds.
  `M87.0` guarded the two branches that call `advance`, on the theory that
  `advance` not shrinking `rest` was the whole shape of the risk. It was one
  instance of a broader one: any branch that can replace `rest` needs the
  same check, whether or not `advance` is involved, because a well-formed
  trim can only return a suffix of its input and nothing before this
  enforced that a mutant obeys the same rule.
  Fixed with the same pattern one branch to the left: a `debug_assert!` that
  `trimmed.len() < self.rest.len()` before the assignment. `next()`'s
  after-the-match invariant from `M87.0` did not catch this one because each
  individual `next()` call still nets a shrink — `rest` goes from 5 bytes
  ("xyzzy") down to 0 after consuming the word — the growth happens between
  calls, when `skip_trivia` refills what the previous call emptied.
  Acceptance: `cargo mutants -p pgprox-core -f crates/pgprox-core/src/sql.rs
  -F 'trim_leading_space -> &str with "xyzzy"'` reports the mutant caught,
  `cargo nextest run -p pgprox-core` passes all 241 tests unchanged.

- [x] `M87.3` `Sweeps:` markers updated for the seven crates now freshly and
  fully swept: `pgprox-proto`, `pgprox-route`, `pgprox-cache`,
  `pgprox-session`, `pgprox-cluster`, `pgprox-pool` and `pgprox-core` (the
  last after `M87.0` through `M87.2`). Every survivor in each is either
  already accepted in `docs/internal/product/mutants-baseline.txt` or was
  fixed. `pgprox-core`'s four pre-existing accepted entries were briefly
  reported as "now caught" mid-sweep; that was the sharded comparison
  reading absence-from-one-shard's-survivors as caught, not a real change,
  and resolved once all eight shards had run — none are marked equivalent
  by a test that could not exist, so none moved.
  Nine crates remain unswept: `pgprox-admin`, `pgprox-auth`, `pgprox-config`,
  `pgprox-observe`, `pgprox-load`, `pgprox-tls`, `pgprox-testkit`, `pgprox`
  and `pgload`.
  Acceptance: `scripts/gates/m22-complete.sh` reports these seven crates at
  zero commits past their sweep.
  Also folded in: `pgprox-testkit` and `pgprox-config`, swept clean earlier
  in the same session while diagnosing the crash before `M87.0` had a name.

- [x] `M87.4` `pgprox-tls` swept clean and found one real gap, of a different
  shape than `M87.0`–`M87.2`: not a hang, a survivor. `cargo mutants`
  replacing `CertReloader::resolve`'s body with `None` refuses every TLS
  handshake, and no test caught it.
  `resolve` is `ResolvesServerCert::resolve`, called by rustls during a
  handshake and by nothing else; `ClientHello` has no public constructor a
  test could use to call it directly. Every existing test in this crate
  reads `serving()` instead, which is a different accessor on the same
  struct that never goes through `resolve` at all — correct on its own
  terms, and it is why a mutant this severe survived twenty-one tests.
  Fixed by adding the handshake the crate's tests never ran: a real
  `rustls::ServerConnection` and `rustls::ClientConnection`, pumping
  handshake bytes between them by hand, with the client's root store built
  from the same self-signed test certificate. No new dependency —
  `rustls`'s own synchronous connection API needs neither a socket nor a
  runtime.
  Acceptance: `cargo mutants -p pgprox-tls -F 'resolve -> ... with None'`
  reports the mutant caught, `cargo nextest run -p pgprox-tls` passes all 22
  tests, `scripts/check-coverage.sh pgprox-tls` holds at 100%.

- [x] `M87.5` `pgprox-auth` swept and found a gap in `scram.rs` of the same
  shape as `M87.4`: every existing test in the module drives the free
  functions (`client_first_bare`, `parse_server_first`, `client_proof`,
  `verify_server_final`, ...) directly, and none drives `ClientExchange`
  itself through a real exchange. `cargo mutants` had eleven live mutants
  across `ClientExchange::client_first`, `client_final` and `verify` — wrong
  message prefixes, a skipped proof check, a signature check that always
  passes — none caught, because the struct that holds the exchange's own
  state was never the thing under test.
  Fixed by adding one end-to-end test that plays both sides: derives real
  `ScramKeys`, drives `ClientExchange::default()` through `client_first` and
  `client_final` against a hand-built server-first message, independently
  reconstructs the server's own auth message and calls `verify_client_proof`
  on the client's output, then calls `.verify()` against both a forged
  signature (built from the wrong password) and the real one. This is the
  same argument as `M87.4`: a crate can have a correct free-function library
  and an untested stateful wrapper around it, and only a test that goes
  through the wrapper's own entry points finds the gap.
  Acceptance: `cargo mutants -p pgprox-auth -f crates/pgprox-auth/src/scram.rs
  -F 'client_first|client_final|ClientExchange::verify'` reports all eleven
  mutants caught, `cargo nextest run -p pgprox-auth` passes all 82 tests.

- [x] `M87.6` The same sweep found `Entries::sweep` in `cache.rs` had never
  been exercised past its capacity trigger: existing tests filled the cache
  and checked what stayed, but none advanced a clock far enough past an
  entry's TTL and *then* forced a sweep to watch it actually get dropped.
  `cargo mutants` found two live mutants at the guard `entry.expires_at >
  now`: replacing the whole function with `()` (nothing ever swept) and
  replacing `>` with `==` (nothing swept unless the clock reads the exact
  expiry instant) both survived.
  Fixed by adding a test that inserts a short-TTL and a long-TTL entry at
  capacity, advances a `FakeClock` two seconds past the short entry's TTL and
  the sweep interval, then inserts a third entry to force `sweep()` to run at
  capacity, and asserts the short entry is gone, the long entry survived
  without a new resolve, and the new entry was admitted. That kills `()` and
  `==` outright.
  A third mutant at the same guard, `>` replaced by `>=`, survives this test
  and every test that could be written against a real clock: the two
  programs disagree only when `now` reads exactly `expires_at`, which
  `Clock::now()` cannot be made to do from outside the module without
  injecting the exact instant being compared against, the same shape as the
  `Drain<'_>::settled` and `pgload::one_connection` entries already accepted
  below. Accepted in `mutants-baseline.txt` rather than chased, with the
  reason written out there.
  Acceptance: `cargo mutants -p pgprox-auth -f crates/pgprox-auth/src/cache.rs
  -F 'Entries::sweep'` reports one surviving mutant, matching the accepted
  entry; `cargo nextest run -p pgprox-auth` passes all 82 tests;
  `scripts/check-coverage.sh pgprox-auth` holds at 98.45%.

- [x] `M87.7` `Sweeps:` marker updated for `pgprox-auth`, now fully swept
  after `M87.5` and `M87.6`. Five crates and binaries remain unswept:
  `pgprox-admin`, `pgprox-observe`, `pgprox-load`, `pgprox` and `pgload`.
  Acceptance: `scripts/gates/m22-complete.sh` no longer lists `pgprox-auth`
  as behind its sweep.

- [x] `M87.8` `pgprox-observe` swept clean: 62 mutants, 55 caught, 7
  unviable (all `Default`/leaked-`Vec` bodies cargo-mutants cannot even
  compile against this crate's stricter return types), 0 missed, 0
  timeout. No fix needed.
  Acceptance: `cargo nextest run -p pgprox-observe` passes all 54 tests,
  `Sweeps:` marker updated.

- [x] `M87.9` `pgprox-admin` swept clean: 152 mutants, 74 caught, 78
  unviable, 0 missed, 0 timeout. No fix needed.
  Acceptance: `cargo nextest run -p pgprox-admin` passes all 81 tests,
  `Sweeps:` marker updated.

- [x] `M87.10` `pgprox-load` swept: 218 mutants, 203 caught, 7 unviable, 8
  surviving, 0 timeout. All eight were already carried in
  `mutants-baseline.txt` from `M14.43`, six distinct keys (`bucket|<=`,
  `bucket|+`, `bucket|/`, `weighted|+`, `weighted|/`,
  `validate_tenants|>=`) covering boundary and unreachable-fallback
  equivalences argued out at the time. No new finding, no fix needed.
  Acceptance: `cargo nextest run -p pgprox-load` passes all 70 tests,
  `Sweeps:` marker updated.

- [x] `M87.11` `pgload` swept: 135 mutants, 90 caught, 40 unviable, 5
  surviving, 0 timeout. All five match keys already accepted in
  `mutants-baseline.txt`. `cargo mutants` also flagged two more accepted
  keys, `one_connection|` and `one_connection|<`, as "now caught" — both
  are timing-dependent by their own written reason, and the sibling entry
  `run|` already documents this class flipping between missed and caught
  across runs of an unchanged tree. Left as accepted rather than removed:
  one run catching a mutant that depends on which of two concurrent
  connections a fake server answers first is not evidence the mutant
  became reliably observable. No new finding, no fix needed.
  Acceptance: `cargo nextest run -p pgload` passes all 51 tests, `Sweeps:`
  marker updated.

- [x] `M87.12` `pgprox`'s sweep is running sharded, 1/8 through 8/8, since a
  full run is 685 mutants. Shard 2/8 found a real gap in
  `bin/pgprox/src/primary_watch.rs`: `PrimaryWatches::is_empty` was never
  asserted `true` on a freshly built registry, only `false` after
  `ensure_watched`, and its `Debug` impl was never read by anything,
  so `fmt` replaced with a no-op `Ok(Default::default())` and `is_empty`
  replaced with a constant `false` both survived.
  Fixed with one test that checks a fresh `PrimaryWatches` reports
  `len() == 0`, `is_empty() == true`, and a `Debug` string containing
  `"watched: 0"`, then does the same past `ensure_watched` for the
  non-empty and `"watched: 1"` cases.
  Acceptance: `cargo mutants -p pgprox -f
  bin/pgprox/src/primary_watch.rs -F 'fmt|is_empty'` reports all three
  mutants caught, `cargo nextest run -p pgprox -E 'test(primary_watch)'`
  passes all 15 tests.

- [x] `M87.13` Shards 3/8 and 4/8 found four more real gaps, all the shape
  `M17.4` and `standards/observability.md` already named: a comparison
  feeding only a log line, untested because nothing reads logs.
  `primary_watch.rs::refresh` computes `moved` from `answer.primary.server
  != *server` and only ever logs it; `!=` survived as `==`. Fixed with a
  `logged()` helper (the same pattern `run.rs` already has for exactly this
  problem) and a second test giving `refresh` a same-primary answer, so
  both `moved=true` and `moved=false` are asserted from a captured log.
  `replicas.rs::primary_of` was entirely untested, including the one
  branch its own doc comment argues for: `!=` survived as `==`, which
  would call two *agreeing* sightings of a replica's primary a conflict.
  Fixed with four direct tests, including one naming the same replica
  under two generations of the same primary.
  `replicas.rs::evict_unused`'s `<` against `WATCH_GRACE` had two
  existing tests, at half and double the grace period, neither of which
  can separate `<` from `<=` or `==`. Fixed with a third test at exactly
  `WATCH_GRACE`, reachable and exact because `FakeClock`'s offset is
  arithmetic rather than a real clock reading.
  `run.rs::hold_at_nothing`'s `was_open` filter (`&&`, `==`, `>`, four live
  mutants) and a `closed > 0` log guard in `ticker` were both untested,
  the second the same shape `something_happened` and
  `peers_went_unanswered` already exist to avoid. Fixed by extracting both
  into named predicates, `was_previously_open` and `some_were_closed`,
  each with direct unit tests in the same style as their siblings.
  One further survivor, `run.rs`'s `TICKS_PER_RELOAD / with *`, is
  equivalent given today's constants: `TICK` is one second, and dividing
  or multiplying by one is the same operation. Accepted in
  `mutants-baseline.txt` with the arithmetic written out.
  Acceptance: `cargo mutants -p pgprox -f bin/pgprox/src/primary_watch.rs
  -f bin/pgprox/src/replicas.rs -F 'refresh|primary_of|evict_unused'`
  reports all fourteen mutants caught or unviable; `cargo mutants -p
  pgprox -f bin/pgprox/src/run.rs -F 'was_previously_open|some_were_closed'`
  reports all thirteen caught; `scripts/check-crate.sh pgprox` passes.

- [x] `M87.14` Shards 5/8 through 7/8 swept clean but for one, in
  `bin/pgprox/src/wiring.rs::App::build`: deleting `retry: config.retry,`
  from the `PoolConfig` struct literal survived, silently falling back to
  `PoolConfig::default()`'s `retry`, which is "off" by ADR 0029. A document
  asking for retries would get none and nothing would say so.
  Untestable from `bin/pgprox` alone: `LivePool`'s `config` field is
  private, so nothing outside `pgprox-pool` could see what a pool was
  actually built with, only what was passed to build it, which is the
  side of this bug that already worked. Fixed by adding
  `LivePool::config`, a `#[cfg(any(test, feature = "test-fakes"))]`
  accessor in `pgprox-pool`, mirroring `pgprox-tls`'s `serving()` and
  `pgprox-core`'s `FakeClock`: test-only introspection added specifically
  because a wiring bug needs a seam a production caller has no reason to
  want. One new test in `wiring.rs` builds an `App` with a non-default
  `RetryConfig` and asserts `app.pool.config().retry` equals it.
  Acceptance: `cargo mutants -p pgprox -f bin/pgprox/src/wiring.rs -F
  'retry'` reports all three mutants caught; `scripts/check-crate.sh` and
  `scripts/check-coverage.sh` pass for both `pgprox` and `pgprox-pool`.

- [x] `M87.15` Shard 0/8 was missed entirely by the first pass through this
  milestone: `cargo mutants`' `--shard` is zero-indexed (`k` must be less
  than `n`), so `MUTANTS_SHARD=1/8` through `8/8` covered the second
  through eighth eighths and `8/8` itself was rejected outright, caught
  only when it errored rather than reported a survivor. Re-run as `0/8`.
  Nine of its ten survivors already matched accepted or fixed baseline
  keys. The tenth was real: `bin/pgprox/src/entropy.rs`'s `SystemJitter`,
  drawing retry backoff jitter through the same system RNG
  `SystemEntropy` already does, had no tests at all, where its sibling in
  the same file has three. Seven mutants survived: three constant returns
  (`0.0`, `1.0`, `-1.0`) and four that swap the shift direction or `/` for
  `%` or `*` in scaling a 53-bit draw into `[0, 1)`.
  Fixed with two tests in the same style as `SystemEntropy`'s: a range
  check (`0.0..1.0`, which alone catches every mutant but the constant
  `0.0`, since the arithmetic ones land many orders of magnitude outside
  it) and a distinctness check over 64 draws (which catches `0.0`, the one
  the range check cannot).
  Acceptance: `cargo mutants -p pgprox -f bin/pgprox/src/entropy.rs -F
  'roll'` reports all seven mutants caught; `scripts/check-crate.sh
  pgprox` and `scripts/check-coverage.sh pgprox` pass.
  Also re-swept `pgprox-pool`, stale by the one line `M87.14` added to it:
  `LivePool::config` itself had no test in the crate that owns it, only an
  indirect one through `bin/pgprox`'s wiring test, and `cargo mutants -p
  pgprox-pool` found the accessor's body replaceable with
  `PoolConfig::default()` undetected. Fixed with a direct test using the
  crate's own `pool_with_retry` fixture. `pgprox-pool` is otherwise
  unchanged since its `M22`-era sweep: 218 mutants, 171 caught, 47
  unviable, 0 surviving.

## M88: a second reading of every crate, and the eighteen things it found

- [x] `M88.0` Plan M88, and give it a gate that passes from this commit.
  A second read of every crate against correctness, completeness, design,
  performance and test quality, the same five questions `M24` asked over the
  workspace as it stood sixty-four milestones ago. Eighteen findings, filed
  below in the order of what they cost rather than the order they were found.
  The costliest is a resource leak on future cancellation in `pgprox-auth`'s
  singleflight grant resolution, reachable on every client disconnect during
  an in-flight sidecar call. The cheapest are documentation and test-quality
  gaps that cost nothing at runtime but leave the suite unable to catch a
  regression a reader would.
  Acceptance: the roadmap has an M88 section and a status row, this list is
  written, and `scripts/gates/m88-complete.sh` exists, is named in CI, and
  passes on this commit by checking what has landed rather than what is
  planned.
- [x] `M88.1` A resolver whose leader is cancelled leaks its own bookkeeping.
  `CachingResolver::resolve` and `resolve_and_store` coordinate concurrent
  callers asking for the same grant through a singleflight `inflight` map: the
  first caller becomes the leader and does the sidecar RPC, the rest await its
  result. If the leader's future is dropped before it finishes — a client that
  disconnects mid-lookup, cancelled by the connection task that was awaiting
  it — nothing removes its entry from `inflight`. Every follower still parked
  on it awaits forever, and every subsequent request for that grant joins a
  singleflight that will never resolve, until the process restarts.
  Acceptance: a test that drops the leader's future mid-RPC and shows a new
  caller for the same key still gets an answer rather than joining the dead
  entry, failing before the fix; the cleanup runs through a drop guard rather
  than a line that cancellation can skip over.
- [x] `M88.2` The free-pool ceiling `LeaseLedger` hands out does not move when
  a live cap does. `coordinator.rs::observe` computes each node's share via
  `split_for`, but an existing `LeaseLedger` only reads that value when it is
  first created; a cap change afterward recomputes `split_for`'s answer and
  never writes it back into the ledger already handing out leases against the
  old one. A node whose cap grows stays capped at the stale ceiling, and a node
  whose cap shrinks keeps leasing above the new one.
  Acceptance: a test that changes a live cap and shows an existing ledger's
  ceiling moves with it, failing before the fix.
- [x] `M88.3` `pgprox-route` reads SQL with `split_whitespace()` instead of the
  shared lexer. `parse_route_assignment` and `begins_transaction` tokenize on
  raw whitespace, which is exactly the second-scanner mistake `M24` found and
  fixed twice elsewhere and both `pgprox-pool` and `pgprox-route` carry a
  written rule against: a comment or a quoted string containing route-looking
  or transaction-looking words is read as SQL rather than as data.
  Acceptance: a test with a `-- BEGIN` comment or a quoted literal that would
  misparse under `split_whitespace()` and does not under `pgprox_core::sql`,
  failing before the fix.
- [x] `M88.4` `pgprox-pool`'s `ParsedSet::parse` is not comment-aware. A `SET`
  statement preceded or followed by a SQL comment containing text that looks
  like another `SET` or a semicolon confuses the parser, the same shape `M24.1`
  fixed for statement splitting, not yet closed for `ParsedSet` itself; the
  crate's own `deallocates_everything` matcher has the same gap.
  Acceptance: a test with a comment adjacent to a `SET` or `DEALLOCATE ALL`
  that a comment-blind scanner reads wrong, using the shared lexer, failing
  before the fix.
- [x] `M88.5` `SHOW CLIENTS`, `SHOW SERVERS` and `SHOW STATS` report wrong data.
  `SHOW CLIENTS` printed `client.tenant` into both the `user` and `database`
  columns, which are not the tenant ID and not each other; `SHOW SERVERS`
  emitted one row per pool rather than one per upstream connection, hiding
  how many server connections a pool actually held; `SHOW STATS`'s
  `total_query_count` was `stats.transactions.to_string()`, the same value
  already in `total_xact_count` two columns earlier.
  Landed narrower than filed: `total_query_count` is blanked rather than
  computed, because nothing in the workspace counts queries, only
  transactions, and building a real counter is a new instrumentation feature
  rather than a display fix. Blanking follows this module's own stated
  policy for a column pgprox does not have real data for: an invented value
  that looks like the others is worse than an empty one, which is why
  `SHOW CLIENTS`'s `user`/`database` are blanked the same way rather than
  filled with the tenant. `SHOW SERVERS` is fixed in full: `PoolStats`
  already carries `active`/`idle` counts, so the row count now matches the
  connections a pool holds without needing a new field anywhere.
  Acceptance: a test per command showing the correct field or row count against
  a constructed pool/session state, each failing before its fix.
- [x] `M88.6` `bin/pgprox` doubles one metric and drops a label from another.
  The actual mechanism was narrower than filed and in the exporter rather than
  the counters: `pgprox_client_conns` was not double-incremented, it was
  emitted as two separate marginal breakdowns under one metric name — a
  state-only set of samples and a tenant-only set, both covering the full
  client count — so a bare `sum(pgprox_client_conns)` with no label filter
  counted every client twice. Fixed by merging into one joint
  `(state, tenant)` breakdown, one sample per pair, with `tenant` added to
  `pgprox-observe`'s registry as a properly bounded label (the allowlist plus
  `other`, cardinality 17) rather than a second ungoverned dimension.
  `pgprox_upstream_conns` was emitted per pool with no `state` label at all,
  collapsing active and idle into one number, and — a second bug the fix for
  the first one exposed — two pools sharing a server produced two samples
  with an identical label set, which is not valid Prometheus exposition.
  Fixed by summing `active`/`idle` per server from `PoolStats` (already on
  the `Observatory` contract, no contract change needed) and emitting one
  `state="active"` and one `state="idle"` sample per server.
  Acceptance: a test asserting a bare sum over every `pgprox_client_conns`
  line equals the true client count, not double it, and a test asserting
  `pgprox_upstream_conns` carries a `state` label with the right per-state
  counts, both failing before the fix.
- [x] `M88.7` TLS-required-with-JWT is operator discipline, not code. Running
  JWT authentication without `--require-tls` sends a bearer token over a
  plaintext connection, and nothing in `pgprox-tls` or its wiring refuses that
  combination; the safety depends entirely on whoever writes the deployment
  config remembering the flag.
  Landed with an escape hatch rather than an unconditional refusal:
  `start_with` (the one function both `start` and every alternate caller
  reach) now refuses `require_tls: false` unless a new
  `--insecure-plaintext-auth` names that as deliberate. An unconditional
  refusal was considered and rejected — `scripts/scale.sh` and
  `scripts/bench.sh`'s compare stack run pgprox with `--require-tls` off on
  purpose, because `bin/pgload` cannot speak TLS at all, and a fix that broke
  them would trade one gap for another. The new flag is what
  `docker-compose.scale.yml` and `docker-compose.compare.yml` now pass
  explicitly; every other caller, including the Helm chart's default with no
  TLS secret named, gets the refusal.
  Acceptance: a test that constructing the config with JWT auth enabled and TLS
  not required is refused at startup rather than accepted, failing before the
  fix.
- [x] `M88.8` `pgprox-config`'s `FileSource::read` blocks the shared async
  runtime. It performs synchronous file I/O directly on a Tokio task rather
  than through `tokio::task::spawn_blocking`, which under the async-concurrency
  standard's rule stalls every other task on that worker thread for the
  duration of the read, on a proxy meant to hold tens of thousands of
  concurrent connections.
  Acceptance: a test (or a `debug_assert!`/structural check, whichever proves
  it) that the read path yields to the runtime rather than blocking it, failing
  before the fix.
  Landed narrower than filed: only `FileSource::poll` (and the `run` loop that
  calls it once a second for the life of the process) moved to
  `spawn_blocking`. `FileSource::new` and `ConfigSource::load` read inline
  deliberately left alone — the standard's rule is specifically about "a task
  that also serves connections", and both run exactly once, during node
  construction in `wiring.rs`'s `App::build`, before any connection exists for
  that task to be serving. `Options::tls()`'s certificate loading is the same
  shape and was already accepted.
- [x] `M88.9` `pgprox-session`'s `ParameterCache::ensure` drops its probe
  connection without saying goodbye. It opens a connection to discover a
  backend's server parameters and drops it when done, skipping the `.goodbye()`
  call every other exit path uses, which under load leaves the backend holding
  a connection slot until its own TCP timeout notices the client vanished.
  Acceptance: a test that `ensure`'s probe connection sends the goodbye message
  before it is dropped, failing before the fix.
  Fixing it moved a downstream assumption: `bin/pgprox`'s
  `a_reaped_connection_says_goodbye_rather_than_vanishing` treated any
  `Terminate` reaching its fake server before the reap call as proof
  something closed early, and a login's own probe now legitimately sends one
  earlier than that. Rewritten to count occurrences and compare before and
  after reaping rather than asserting none at all before it.
- [x] `M88.10` `pgload`'s `NoConnection` error swallows the reason. It reports
  that no connection could be obtained without carrying the per-attempt failure
  that caused it, so a load test failing to connect gives no signal about
  whether the backend refused, timed out, or the pool was exhausted.
  Acceptance: a test that `NoConnection`'s message or fields carry the real
  underlying error from the last attempt, failing before the fix.
  What was actually found differed from how this was filed: `NoConnection` and
  `Report::first_error` already carried a real, specific underlying error —
  what they carried was the *first* one a retrying connection ever saw, kept
  for the life of the run by `get_or_insert_with`, rather than its most recent
  one. A target that came up refusing for one reason and stayed up refusing for
  a different one reported the reason that stopped being true first. Fixed by
  overwriting rather than inserting-once, both where a connection attempt fails
  and where a transaction does; `Report::first_error`'s field name and JSON key
  are unchanged (`scripts/scale.sh` reads it by name), its doc comment is not.
- [x] `M88.11` `pgprox-tls`'s `CertReloader` never checks a certificate's
  validity window. `read`/`reload` parse and swap in a new certificate without
  checking `notBefore`/`notAfter`, so a certificate that has already expired,
  or one dated in the future, is served without complaint until a TLS peer
  rejects it, one connection at a time.
  Acceptance: a test that a certificate outside its validity window is refused
  by `reload` rather than swapped in, with the previous certificate left
  serving, failing before the fix.
  `read` (the function both `new` and `reload` share) now checks the leaf's
  `notBefore`/`notAfter` via `x509-parser`, a new real (not dev-only)
  dependency of this crate — already present in the lockfile through `rcgen`'s
  own graph, and hand-rolling ASN.1 parsing for two fields was the worse
  trade. `now` is threaded in as a parameter rather than read inside this
  crate, per the workspace's sans-I/O rule; `bin/pgprox` supplies
  `app.deps.clock.wall()` on the reload path and `SystemTime::now()` once at
  startup, before `Deps` exists to hold a `Clock`. Three tests, covering
  refusal at construction, refusal by `reload` with the previous certificate
  left serving, and the not-yet-valid direction the acceptance text does not
  name but the finding's own summary does.
- [x] `M88.12` The query cache denylist misses a quoted built-in function name.
  `cacheable()` and `statement_words` fail to catch a denylisted function name
  when it is written quoted (`"pg_advisory_lock"(1)` or similarly), the same
  quoting gap `M24.2` found and fixed for `SET`'s parameter name, not yet
  closed for the cache's own denylist check.
  Acceptance: a test with a quoted denylisted function name that a
  quote-blind check would cache and a quote-aware one refuses, failing before
  the fix.
  Same trade `M24.2` took, not the same fix: `Token::Quoted` carries no text
  by design, so a quoted name can never be compared against the denylist by
  what it says. What can be told is whether a quoted token is immediately
  called — `has_quoted_call` reads tokens rather than words for exactly that —
  and any quoted call is refused conservatively, whether or not the name
  behind the quotes is actually denylisted. `NotCacheable::QuotedFunctionCall`
  carries no name, same reasoning as `PinReason::UnreplayableSet`.
- [x] `M88.13` `PoolConfig.min_size` is a dead field. It is read from config,
  stored, and never consulted by anything that opens or reaps a connection —
  `min_pool` is documented elsewhere in this crate as always 0, which is a
  design decision this field contradicts by existing and accepting a different
  value silently.
  Acceptance: either the field is removed (with every construction site and
  the ADR touched together, per non-negotiable 6 if it is a `pgprox-core`
  type, or a plain removal otherwise) or it is wired to something a test
  observes; whichever, a test proves the field's value now has an effect or
  the field no longer exists to have none.
  Removed, a plain removal: `PoolConfig` is a `pgprox-pool` type, not
  `pgprox-core`, so non-negotiable 6 does not apply, and it was never set from
  the operator-facing config document in the first place — `wiring.rs` builds
  it with `..PoolConfig::default()`. `ReapConfig::keep_warm` turned out to be
  the field this crate actually uses for a floor, already wired and always
  zero by design; `min_size` was a second, unwired name for the same idea.
  The test proving the field is gone is an exhaustive `PoolConfig` literal
  that does not compile if `min_size` — or anything else unnamed — exists to
  be missing from it.
- [x] `M88.14` `Lsn`'s `Display` impl does not zero-pad its low half. The type's
  own doc comment says the textual form is the standard `XXXXXXXX/XXXXXXXX`
  Postgres LSN format, which zero-pads each half to eight hex digits; the
  actual impl formats the low half without padding, so an LSN like
  `0/A` prints as `0/A` instead of `0/0000000A`, disagreeing with every real
  Postgres tool that reads or greps for this format.
  Acceptance: a test that a low half needing padding prints as
  `XXXXXXXX/0000000A` rather than `XXXXXXXX/A`, failing before the fix.
  Landed as filed: `write!(f, "{:X}/{:X}", ...)` became
  `write!(f, "{:X}/{:08X}", ...)`, padding only the low half — the high half
  stays unpadded, matching real `pg_current_wal_lsn()` output like
  `16/B374D848`. The existing `lsn_zero_is_the_default` test was itself
  asserting the buggy output (`"0/0"`) and had to be corrected to
  `"0/00000000"` alongside the two new tests.
- [x] `M88.15` `pgprox-config`'s `AGENTS.md` and ADR 0006 claim three config
  providers; the crate implements one. Whichever is true, the other is a
  reader-facing claim that does not match the workspace, the same shape
  `M24.9` found for certificate hot reload and `M13` found for the
  non-negotiables list: a document asserting a capability with no code behind
  it, or code with no document catching up to it.
  Acceptance: `AGENTS.md` and the ADR describe what the crate actually does,
  or the crate is extended to match and a test proves the added provider
  works; whichever, checked by `scripts/check-drift.sh` finding no remaining
  mismatch it can detect and by a reading of both documents against the code.
  Landed as the documentation side, not the extension side: `docs/internal/product/plan.md`
  also overclaimed the same "three providers" and was corrected alongside
  `AGENTS.md` and the ADR. The ADR's `Status` line now reads "accepted, one of
  three providers implemented" with a new `## Outstanding` section naming
  what was never built and why building it now is not warranted — nothing in
  the roadmap has asked for a non-k8s deployment, the only reason either
  missing provider would exist. Unlike `M24.9`'s cert hot reload, which was a
  small, self-contained, security-relevant gap worth building outright, an
  etcd-watch and an HTTP-poll provider are two new network-facing subsystems
  each needing their own dependency, feature gate, and test suite — not a
  single-commit fix. Checked mechanically: two new tests in
  `pgprox-config`'s `lib.rs`, `include_str!`-ing `AGENTS.md` and the ADR at
  compile time and asserting neither reads as claiming three built
  providers, each confirmed to fail against the pre-fix wording and pass
  against the fix.
- [x] `M88.16` `pgprox-core`'s `lib.rs` contracts table is missing four traits.
  The crate-level doc comment tables the traits non-negotiable 6 governs, and
  four defined in the crate are not rows in it, which means a future
  `check-core-contract.sh` reader or a human skimming the doc comment cannot
  tell those four traits are covered by the same rule the table exists to
  advertise.
  Acceptance: the table lists all traits `check-core-contract.sh` actually
  governs, checked by a script or a manual cross-check recorded in the commit,
  and a test or doctest that the table's row count matches the trait count if
  one can be written cheaply.
  Landed as filed, with the count precise rather than assumed: the crate
  defines twelve `pub trait`s, not eight — `check-core-contract.sh` governs
  any `pub trait` block under `src/`, with no list of its own to fall out of
  date. The four missing were `GrantInvalidation`, `TopologyRefresh`,
  `Observatory`, and `PeerSource`, each with a genuine public fake and a real
  cross-crate implementor. The twelfth, `ConnectionRelease`, stays out of the
  table on purpose: unlike the other eleven, its fake (`pool`'s `FakeRelease`)
  is private, used only inside `FakeUpstreamPool` — it is `UpstreamGuard`'s
  release plumbing, not a seam a downstream crate swaps, so it never had the
  "implemented by, faked by" shape the table records. Two new tests in
  `lib.rs` (`include_str!`-ing the crate's own source at compile time): one
  asserting every governed trait has a row, one counting `pub trait` blocks
  across every source file and checking that count against the table plus the
  one deliberate exclusion, so a *future* trait added with no table update
  fails the same way this finding did.
- [x] `M88.17` `pgprox-proto`'s `FrameRelay::push` has no fuzz target. It is the
  function that decides how a partially-read frame from an untrusted peer is
  buffered and re-assembled, exactly the kind of function `M15` fuzzed
  elsewhere in this crate for the same reason, and it has none.
  Acceptance: a fuzz target exercising `FrameRelay::push` exists under the
  crate's fuzz directory, and `scripts/fuzz.sh` (or the equivalent invocation
  named in `pgprox-proto`'s `AGENTS.md`) runs it for a bounded duration without
  finding a crash.
  Landed as filed: `fuzz/fuzz_targets/frame_relay.rs`, registered as a
  `[[bin]]` in `fuzz/Cargo.toml` and added to `scripts/fuzz.sh`'s target list.
  Unlike `frame_decode` and `message_decode`, which hand a whole message to a
  decoder in one call, this target drives `FrameRelay::push` in chunks whose
  size comes from the fuzz input itself, so the same corpus discovers both a
  single large read and a byte-at-a-time one — the reassembly path a real
  socket read actually exercises and the one `relaying_never_panics_on_arbitrary_input`
  (a pre-existing fixed-seed PRNG loop, not corpus-driven or crash-minimizing)
  did not reach the same way. Actually run, not just wired: `cargo +nightly
  fuzz run frame_relay -- -max_total_time=45` completed ~14 million
  executions with no crash, and `scripts/fuzz.sh 20` passed end to end with
  the new target included. Checked mechanically by a new test,
  `push_has_a_real_fuzz_target`, whose `include_str!` on the target file
  itself makes the target's absence a compile failure, confirmed by
  temporarily moving the file aside and back.
- [x] `M88.18` Five smaller test-quality gaps, grouped because none is worth
  its own task on its own: `pgprox-cache`'s `result_formats` field has no test
  touching it; `pgprox-testkit`'s truncated-body test is weak enough to pass
  for reasons unrelated to truncation; the agreement between the `role` and
  `session_authorization` replayable-parameter lists has no cross-crate
  regression guard, so the two could drift apart silently; `routes.rs` and
  `metrics.rs` each match a non-exhaustive enum with a wildcard arm that would
  silently swallow a new variant instead of failing to compile; `fakepg.rs`
  does not distinguish `CopyFail` from `CopyDone` in its test fixtures.
  Acceptance: one test (or one compile-time check, for the wildcard arms)
  addressing each of the five, five failing before their fix or five
  `#[deny]`-equivalent compile failures demonstrated removed.
  All five landed, none quite as filed:
  `different_result_formats_are_different_entries` mirrors the existing
  `different_parameters_are_different_entries` - `CacheKey` already derives
  `Eq`/`Hash` over every field including `result_formats`, so this is a
  regression guard against a future refactor rather than a fix, and there was
  nothing to make fail first. `pgprox-testkit`'s test kept its name and grew a
  real truncated prefix of an actual error body plus one assertion that a cut
  right after a complete field still finds it, replacing bodies of repeated
  `C` bytes that had no field structure to truncate. The cross-crate guard
  lives in `bin/pgprox`, not either crate the finding named: `pgprox-cache`
  depends on `pgprox-core` and nothing else in the workspace, the same rule a
  test importing `pgprox-pool` from it would have broken, and `bin/pgprox` is
  the one place both are already dependencies. `routes.rs`'s wildcard cannot
  become a compile failure - `RouteTarget` is `#[non_exhaustive]`, which is
  what forces the wildcard in the first place - so the fix names `Primary`
  on its own arm and gives the trailing wildcard a `debug_assert!`, checked by
  a test that reads the source for `Primary`'s own arm rather than by calling
  anything nothing can construct. `metrics.rs`'s half turned out to already be
  covered: `every_metric_has_samples_or_is_named_as_having_no_source`, which
  predates this finding, fails for any metric in the registry that is neither
  sampled nor excused, which is exactly what a silently-swallowed new metric
  would be - confirmed by reading it rather than adding a redundant test.
  `fakepg.rs` now answers `CopyFail` with an `ErrorResponse` (`57014`, the code
  real Postgres uses) instead of the same `CommandComplete` `CopyDone` gets.
- [x] `M88.19` Close M88. Filed as its own task for the reason `M24.10` was:
  closing a milestone is a claim about the whole of it, and bundling that
  claim into the last piece of work makes it look like a side effect of that
  piece rather than a judgement about all of them.
  Acceptance: the gate passes, the status row says complete, and the section
  records which findings shared a cause with `M24`'s (the quoting and
  raw-scanner shape recurring in `M88.3`, `M88.4` and `M88.12`) and which were
  new shapes `M24` did not have a category for.
  `scripts/gates/m88-complete.sh` passes; the status table's `M88` row now
  reads complete. The section title gained its own `(complete)`, matching
  `M24` and `M89`'s. Two shapes recurred, not one: the quoting/raw-scanner
  shape in `M88.3`, `M88.4` and `M88.12` as named above, and `M24.9`'s
  document-asserting-more-than-the-code-delivers shape in `M88.15`, plus its
  mirror image in `M88.16` — a document asserting *less* than the code
  actually does. The other thirteen findings were shapes `M24` had no
  category for, each named in the roadmap's closing paragraph rather than
  repeated here.

## M89: the review from outside this repo, and the four gaps it found

`M39` wrote documentation for people who are not this repo; `M88` read the code
a second time for correctness, completeness, design, performance and test
quality. Neither asked whether someone who is not already convinced could
actually adopt this: point it at a real database, pin a version, migrate off
what they run today, and know what to check before they let a tenant's token
near it. This milestone is that question, asked once, from outside.

- [x] `M89.0` Plan M89, and give it a status row and a section before any of it
  lands. Four gaps, each a doc or a release artefact rather than a code change,
  found by reviewing the project the way a prospective adopter would rather
  than the way a contributor grading its internals would: no path from the
  bundled mock stack to a real Postgres and a real sidecar; no tagged release
  or changelog to pin against; the `SHOW`-command compatibility with pgbouncer
  built and undocumented; and a security posture spread across three pages
  with no single list to work through before launch.
  Acceptance: the roadmap has an M89 section and a status row, and this list
  is written, on this commit.
- [x] `M89.1` A getting-started path for a real Postgres and a real sidecar.
  `docs/getting-started.md` is the only quickstart, and it runs entirely
  against the bundled mock token service and a Postgres compose brings up
  itself; nothing shows what changes to point at an existing database and a
  sidecar built against `proto/pgprox/auth/v1/auth.proto` rather than the
  mock. The gap is invisible until someone has already committed to the
  architecture and gone looking.
  Acceptance: a new doc, linked from `docs/index.md`, that names every
  argument and document field that changes between the mock stack and a real
  one, walks through implementing the two RPCs the contract requires, and
  says what a sidecar built against the frozen proto owes a client versus what
  pgprox owes it. `scripts/check-links.sh` passes.
- [x] `M89.2` A first tagged release and a changelog. Every artefact —
  `Cargo.toml`, the Helm chart's `Chart.yaml`/`appVersion` — has said `0.1.0`
  since before this milestone existed, and nothing has ever tagged that
  version or written down what it contains. An operator deciding whether to
  adopt this has no artefact to pin against and no record of what changed
  between one commit and the next.
  Acceptance: a `CHANGELOG.md` at the repository root describing what `0.1.0`
  contains and what it explicitly does not (linking `docs/features.md` for the
  latter rather than duplicating it), a `v0.1.0` git tag on this commit, and
  `README.md` pointing at the changelog.
- [x] `M89.3` A migration guide from PgBouncer. `docs/admin.md` already keeps
  the five overlapping `SHOW` commands' column names and order identical to
  pgbouncer's "so an existing dashboard reads them unchanged" — real
  migration-friendliness that is invisible unless someone already suspects it
  exists and goes reading the admin reference for it.
  Acceptance: a new doc, linked from `docs/index.md`, mapping `pgbouncer.ini`'s
  pooling-relevant directives to their `config.yaml` or command-line
  equivalent, naming what has no equivalent (the userlist/auth_query
  credential model, since pgprox's is a sidecar and a JWT) and what changed
  behaviour on both sides has to be checked for before a cutover.
  `scripts/check-links.sh` passes.
- [x] `M89.4` A pre-launch security checklist. `docs/security.md` and
  `docs/admin.md` between them state every decision an operator needs to have
  made before exposing this to a tenant — TLS posture, the admin port's
  network boundary, the static-admin credential path — as prose spread across
  two pages a launch review has to read in full and extract from by hand.
  The admin-port-on-a-public-load-balancer mistake `docs/admin.md` already
  records happened once in this project's own history because nothing forced
  that extraction.
  Acceptance: a new doc, linked from `docs/index.md`, structured as a checklist
  rather than prose, with every item pointing back at the page that explains
  it rather than restating the explanation. `scripts/check-links.sh` passes.
- [x] `M89.5` Close M89. Filed as its own task for the reason `M24.10` and
  `M88.19` were: closing a milestone is a claim about the whole of it.
  Acceptance: the status row says complete and the section names what, if
  anything, a prospective adopter would still hit that this milestone left
  open.
  What is left open: `M88.11` and `M88.12`, the two still-open findings from
  the second reading with the clearest adoption-time consequence (certificate
  expiry never checked before serving, and a quoted builtin name bypassing
  the query cache's denylist), are tracked there, not here — this milestone
  is documentation and a release artefact, not a code fix, and did not
  duplicate that tracking. `M16`'s multi-machine 100k-connections-serving run
  stays the roadmap's own open item. Nothing this milestone added changes
  what pgprox does; a reader who works through all four new pages still
  cannot verify the scale claims `docs/performance.md` already marks as
  unmeasured, because verifying them needs hardware this repository's own CI
  does not have, not more documentation.

## M90: a third reading, from several angles at once, and what each one found

`M24` read every crate once; `M88` read every crate a second time. Both were
one reviewer reading sequentially. This milestone instead points several
readers at the same tree at once, each with a different question —
concurrency and cancellation, security-sensitive data flow, error handling
and panics, non-exhaustive-enum wire encoding, documentation drift — on the
theory that a single reader's blind spots are consistent across a pass and a
second pass by the same method finds mostly what the first pass already
would have. Findings are filed below as they are confirmed by reading the
actual code, not by trusting an angle's report unread, and each lands as its
own commit with a test that fails before the fix. Excluded by the standing
direction that opened this milestone: anything whose verification needs a
scalable real production machine, which stays `M16`'s open item.

- [x] `M90.0` Plan M90, and give it a gate that passes from this commit.
  Acceptance: the roadmap has an M90 section and a status row, this list is
  written, and `scripts/gates/m90-complete.sh` exists, is named in CI, and
  passes on this commit by checking what has landed rather than what is
  planned.
- [x] `M90.1` `SessionRouter::route` stops tracking whether a transaction
  wrote anything once its routing target is fixed. The target a transaction's
  first statement chose is correctly held for the rest of the transaction,
  but the code held `wrote` to that same first answer instead of continuing
  to classify each later statement — the ordinary `BEGIN; UPDATE ...; COMMIT`
  shape has its write as the *second* statement, never the one that fixed the
  target, so `wrote` stayed `false` and the caller never fetched the commit
  LSN. A read right after such a commit could land on a replica that had not
  replayed it, silently violating the read-your-writes guarantee ADR 0009
  exists to provide.
  Acceptance: a test with `BEGIN; UPDATE ...; COMMIT` showing `wrote` true at
  commit, failing before the fix.
- [x] `M90.2` `NodeMode`'s two wire-conversion sites in `bin/pgprox/src/gossip.rs`
  (`ClientWire::from`, `DigestWire::from`) and its numeric hash in
  `pgprox-cluster`'s `digest.rs::view_hash` each hand-rolled their own match
  against a `#[non_exhaustive]` enum, with a wildcard arm that silently
  downgraded any variant it did not recognise — `ClientWire::from` folded
  anything but `Active`/`Waiting` into `"idle"`, `DigestWire::from` folded
  anything but `Draining` into `"active"`, and `view_hash` could hash an
  unrecognised mode onto the same number a known one already used. The
  `DigestWire` field is exactly what `coordinator.rs` reads to decide whether
  to keep routing tenants onto a peer.
  Acceptance: `pgprox-core::cluster::NodeMode` grows an exhaustive `as_str()`
  method, the two `gossip.rs` sites and `digest.rs`'s hash use it or assert
  loudly on an unrecognised variant, and a test per site shows the fix,
  failing before it.
- [x] `M90.3` `CachingResolver`'s grant cache key hashes only the auth token
  and `startup_database`, omitting `startup_user`. The sidecar's own proto
  sends `startup_user` as a first-class resolution input "for policy", and
  the bundled `mock_sidecar.rs` demonstrably varies the resolved backend
  `user` by it; two different users on the same token and database can
  collide on the same cache entry and one gets served the other's grant.
  Acceptance: a test with two `AuthRequest`s differing only in
  `startup_user` showing they resolve to distinct cache entries, failing
  before the fix.
- [x] `M90.4` A rejected `SET pgprox.route` hint is never actually reported
  to the client. `router.rs`'s own doc comment for `Routed::HintRejected`
  says the caller reports this "so a typo does not leave them believing
  their reads are on replicas", but `bin/pgprox/src/serve.rs` is the only
  place that turns a `ClientAction::Answer` into wire bytes and it matches
  on `Answer(_)`, sending the same bare `ReadyForQuery` whether the hint was
  accepted or rejected. A client that mistypes `pgprox.route` gets no signal
  that its session hint did not change.
  Acceptance: a test showing a rejected hint's `ReadyForQuery` differs
  observably from an accepted one's, failing before the fix.
- [x] `M90.5` `pgprox-session::cancel::Registry` leaked one entry per session
  that ended while holding an upstream connection outside the clean
  transaction boundary — a mid-transaction client disconnect, chiefly.
  `Registry::release` had exactly one call site, the clean-boundary
  `release()` in `serve.rs`; `context.sessions`'s entry gets an RAII guard
  (`Registration`) that runs on every exit, clean or not, and the cancel
  registry's entry, inserted on every acquire, got no such guard. The entry
  then outlived the session that made it, forever, since a cancel key is
  random and never reused.
  Also investigated: `PgConnector::backends` and `ParameterCache::entries`.
  Not a real gap — both are keyed by `PoolKey`/`(ServerId, database)`, whose
  cardinality is the deployment's own server/database/role topology as
  resolved by the trusted sidecar, not by client-controlled input or by
  connection count; the same bounded-by-topology shape `pgprox-cluster`'s
  `ClusterDigest::tenant_usage` already documents as the reason it needs no
  eviction.
  Acceptance: `Sessions` gets a `wire_cancels` method reusing `Registration`'s
  existing drop-on-any-exit guarantee for the cancel registry too, at zero
  cost to the per-connection future — `Sessions` is a shared, once-per-node
  `Arc`, not local state a session's future carries — rather than a new
  per-session guard, which was tried first and cost 16 bytes against a
  budget that had 8 to spare. A test that disconnects mid-transaction and
  shows the cancel registry empties anyway, failing before the fix.
- [x] `M90.6` Three documentation-drift findings, one narrower than filed.
  `plan.md` said "with two exceptions" for the workspace's dependency rule;
  `architecture.md`, the section's own kept-in-sync source, has said three
  since `bin/pgload` was added, and `plan.md` never mentioned it at all.
  ADR 0009 and `plan.md` both described `replica_poll_interval` as a
  configured setting with a stated default; it is `POLL_INTERVAL`, a
  compile-time constant. Both also described a `max_replica_lag`
  bounded-staleness opt-in for replica routing that does not exist anywhere
  in `pgprox-route` — every replica-eligible session routes under the strict
  watermark rule, with no time-based alternative to opt into.
  Narrower than filed: `POLL_INTERVAL`'s appearance in both
  `primary_watch.rs` and `replicas.rs` looked like undocumented duplication
  from the outside, but `primary_watch.rs`'s own doc comment already names
  `replicas.rs`'s constant and explains why the two are kept equal
  deliberately. Not a finding — reading the code before writing up what
  looked like one is what caught it before it became a bogus fix.
  Acceptance: `plan.md` points to `architecture.md` for the current exception
  count rather than repeating a number that can drift again; ADR 0009 gets
  an `## Outstanding` section, `M88.15`'s precedent, naming both real gaps;
  `plan.md`'s own copy of the same two claims is corrected to match. Two new
  tests in `bin/pgprox/src/replicas.rs` `include_str!` both documents and
  check the overclaim cannot come back silently, failing before the fix.
- [x] `M90.7` `run_with_peers` read the peer table once, before its tick loop
  started, and handed that frozen snapshot to both the periodic gossip round
  and the drain announcement — despite an adjacent comment claiming both
  read "the current one... per tick", left over from `M19.3`, which fixed
  exactly `GossipTransport`, `NodeObservatory` and `Context`'s cancel
  forwarding to read live and did not touch these two. A peer that joined
  after the tick loop started was never gossiped with, and a node draining
  after that point never announced itself to that peer either, for the
  loop's entire remaining lifetime.
  Acceptance: `Drainer` and `ticker` take the `PeerSource` itself rather than
  a `Vec<String>` snapshot, and read `.peers()` fresh on each use. A test
  publishes a peer to a `FakePeerSource` after the tick loop has already
  started and shows the next gossip round reaches it, failing before the
  fix.
- [x] `M90.8` `bin/pgprox/src/metrics.rs`'s Prometheus exporter wrote the
  `tenant` and `server` label values with no escaping. `TenantId::new` and
  `ServerId::new` accept any string, so `TenantAllowlist::label_for`'s output
  for an allowlisted tenant reaches the `tenant` label verbatim; Prometheus's
  text exposition format requires a label value to escape backslash, double
  quote and newline, and an unescaped one does not corrupt only its own
  sample — it breaks the parse of every line the scraper reads after it, so
  one tenant with a quote in its name could blind monitoring for the whole
  node. `server` is lower risk, since an operator configures it rather than
  a tenant, but was exported the same unescaped way.
  Acceptance: a new `escape_label_value` helper applied to both labels, unit
  tests for the three characters the format requires escaped, and two
  scrape-level tests — one tenant name and one server name, each containing
  a character that would otherwise break the format — showing the emitted
  line is escaped and the raw value never appears, failing before the fix.
- [x] `M90.9` `pgload`'s `NoConnection` error said "nothing was attempted"
  for a run that was, in fact, attempting continuously — a target that
  refuses every connection with the draining code (`57P01`) the whole run
  through produces zero transactions, but every attempt is a relocation, not
  an error, so `last_failure` (set only on the error branch, since it also
  backs `Report::first_error`'s documented "most recent failure" meaning)
  was never set. `summarise` fell back to a hardcoded string instead of the
  `Outcomes` map, which already records every relocation's code and message.
  A run against a permanently draining target — the most operationally
  realistic reason a load run sees zero transactions — got the least
  informative message the tool can produce.
  Acceptance: a `describe` helper reads `Outcomes` when `last_failure` is
  unset, and a test against the existing `Fake::Refusing` fixture (already
  used by a prior test that checked only the error variant, not its detail)
  shows the resulting message names the code actually seen, failing before
  the fix.
- [x] `M90.10` `parse_route_assignment` matched a `SET`/`RESET pgprox.route`
  regardless of what followed it in the same wire message. The simple query
  protocol allows several `;`-separated statements in one message, and
  nothing here checked that the hint was the *only* statement: `RESET
  pgprox.route; DELETE FROM t` matched the reset, was consumed as the hint,
  and the `DELETE` was never classified, forwarded, or run — no error
  either, the client got a bare `ReadyForQuery` and every reason to believe
  it had succeeded. `SET pgprox.route = 'primary'; DELETE FROM t` took the
  whole remaining text as the value's raw capture, so it became
  `RouteAssignment::Invalid` — a generic "invalid route hint" error for a
  value that was never invalid — and discarded the `DELETE` the same way.
  Acceptance: `route_parameter` matches are followed by a check that only
  trivia and at most one trailing `;` remain (`statement_is_exhausted`); the
  `SET` value is now read by walking tokens to the next statement-separating
  `;` rather than taking the lexer's raw remainder wholesale
  (`first_statement`), so a `;` inside a quoted value is never mistaken for
  the boundary and real content after it disqualifies the match, falling
  through to ordinary forwarding — where the server runs both statements
  itself, matching `classify`'s own already-tested handling of a write
  anywhere in a multi-statement message. Tests at both `hints.rs` and
  `router.rs` for the reset case, the set case, a lone trailing `;` (still
  matched), and a `;` inside a quoted value (not mistaken for the
  boundary), each failing before the fix.
- [x] `M90.11` `pgprox-cache`'s `normalize` folded unquoted words with
  `char::to_lowercase`, which implements Unicode's full case-folding table
  rather than the codepoint-for-codepoint transform this module's own doc
  comment claims ("the rule, which is Postgres's own"). Unicode's
  unconditional special-casing table has exactly one lower-casing
  expansion: `İ` (U+0130, one codepoint) folds to `i` + COMBINING DOT ABOVE
  (U+0307, two codepoints). `pgprox_core::sql::is_word_char` treats every
  non-ASCII character, combining marks included, as a word character by
  design, so `İ` and a source that already spells `i` followed by U+0307
  lex as two different, independently typeable single-token identifiers
  that folded to the identical output — one cache key for two different
  questions, which is exactly the failure mode this module's own comments
  elsewhere name as the unsafe direction.
  Acceptance: word-folding is ASCII-only, matching the convention
  `pgprox_core::sql::statement_words` already uses, which guarantees the
  transform can never change a word's codepoint count and so can never
  conflate two distinct source identifiers. A test folds `İ` and `i` +
  U+0307 and asserts they produce different keys, failing before the fix.
- [x] `M90.12` `ReapConfig::max_lifetime` and `pgprox_pool::reap::is_expired`
  had no caller anywhere in the workspace outside `is_expired`'s own unit
  tests. `Connection` carried no field recording when it was opened,
  `Pool::release` never consulted age, and `reap()` filtered purely on
  `idle_since`/`idle_timeout`. A connection released and immediately
  reused, over and over — the ordinary shape transaction pooling makes —
  never sits idle long enough for the reaper to reach it, so exactly the
  traffic pattern `max_lifetime` exists for (bounding "a connection that
  has accumulated state nobody noticed", and giving a rolling restart of
  the database a way to actually finish) was the one where the documented
  default of one hour silently did nothing. The same shape `M88.13` found
  and removed for `min_size`, except `is_expired` had direct unit test
  coverage as a pure function, which is what let it satisfy the coverage
  gate while never being called from acquire, release or reap.
  Acceptance: `Connection` gains `opened_at`; `reap()` closes an idle
  connection past `max_lifetime` even before `idle_timeout`, and
  `keep_warm` does not exempt it (a kept-warm connection this old is
  exactly the case `max_lifetime` bounds); `Pool` gains `expire_in_use`,
  called from `LivePool::reap_idle`'s existing periodic cadence, which
  marks a checked-out connection past its lifetime so `Pool::release`
  discards it at its next clean release rather than closing a socket out
  from under a running transaction. Eight new tests across `pool.rs`,
  `reap.rs` and `live.rs` — marking and release, the zero-means-no-limit
  case, an idle connection reaped before its idle timeout, `keep_warm` not
  exempting an expired connection, and the full `LivePool` pipeline for
  both a continuously-busy connection and a young one — each verified
  against a reverted piece of the fix before landing.
- [x] `M90.13` The drain sequence's last resort — `Context::closing`, fired
  once `drain_grace` expires with a client still connected, or directly on
  ordinary process shutdown — closed the socket with no `ErrorResponse`
  queued, unlike the two sibling `select!` arms for `draining` and `shed`,
  which both call `wire.refuse` before returning. A client force-closed
  this way saw a bare disconnect indistinguishable from a crash rather than
  a decodable reason. `run.rs`'s own comment on the signal that fires this
  says clients "are told rather than cut", and `bin/pgload`'s model of a
  drain's force-close (`Failed::work_lost`) assumes the very same `57P01`
  a graceful drain sends still arrives here, just after a statement had
  already run — an assumption this arm did not meet. The existing test for
  this path asserted only that the session ended, never that a message
  reached the client first.
  Acceptance: the `closing` arm now calls `wire.refuse(ClientError::
  Draining)` before returning, matching its siblings. The existing
  `a_client_holding_a_connection_is_not_closed_by_the_drain_alone` test now
  also reads the client's socket and asserts a `57P01` `ErrorResponse`
  arrives before the session ends, failing before the fix with an
  unexpected-EOF rather than a decoded frame.
- [x] `M90.14` A failed post-commit LSN probe left a session free to read its
  own unconfirmed write from a replica. `serve.rs`'s `release()` and
  `probe.rs` both document the safety claim in prose — "a failure leaves the
  watermark where it was, so the session keeps reading from the primary" —
  but nothing in `SessionRouter`/`decide()` enforced it. `wrote()` tracked
  exactly the right fact (a write classified but not yet confirmed) and was
  read by the cache-eligibility gate, but `route()` never consulted it: the
  target came from `watermark` alone, and `RouteCtx` had no field for
  "unconfirmed" at all. For a session's first-ever write, a failed probe
  left `watermark` at `None` — indistinguishable from a session that never
  wrote anything — so `ReplicaState::can_serve`'s `watermark.is_none_or(...)`
  waved any healthy replica through. Reachable without a scalable production
  setup: the probe runs on the same connection immediately after commit's
  `ReadyForQuery`, so a primary failover, `pg_terminate_backend`, or a reset
  landing in that window (the exact scenario ADR 0027/0028 exist for) is
  enough, and no test anywhere exercised "probe fails after a write, then a
  read happens."
  Acceptance: `pgprox_core::route::RouteCtx` gains a `wrote` field, additive
  like every other field here (`..RouteCtx::default()` at every call site);
  `decide()` forces `RouteTarget::Primary` when it is set, the same as
  `pinned` and `in_transaction`; `SessionRouter::route` passes `self.wrote`
  at the point a new transaction's target is decided. New tests:
  `pgprox-core`'s `an_unconfirmed_write_stays_on_the_primary_even_with_no_
  watermark` and `pgprox-route`'s
  `a_write_whose_position_probe_failed_keeps_the_next_read_on_the_primary`,
  both verified against a revert of the fix before landing.
- [x] `M90.15` ADR 0009's Decision section named the wrong function for the
  write watermark probe: "appending `SELECT pg_current_wal_lsn()` to the
  commit round trip." The code (`probe.rs`) deliberately queries
  `pg_current_wal_insert_lsn()` instead, with its own comment explaining why
  the more conservative function was chosen over the one the ADR names. This
  survived `M90.6`'s pass over the same document, which fixed two other
  overclaims but not this sentence.
  Acceptance: the ADR names `pg_current_wal_insert_lsn()`, carrying the same
  conservatism rationale the code comment already gives. New test
  `adr_0009_names_the_watermark_query_the_code_actually_runs` asserts the
  ADR's text contains `probe::PRIMARY_LSN_QUERY` itself rather than a second
  copy of the string, so the two cannot drift apart silently again; verified
  against a revert of the fix before landing.
- [x] `M90.16` `TenantAllowlist::add` had no check preventing a tenant named
  exactly `OTHER` (`"other"`) from being allowlisted. `label_for` returns the
  tenant's own name if allowlisted and [`OTHER`] otherwise — for a tenant
  actually named `"other"`, both branches produce the identical string, so
  its series becomes indistinguishable from the aggregate bucket every
  unlisted tenant already shares, breaking the module's own documented
  promise ("Everything else is aggregated, not dropped... The totals stay
  correct"). Not reachable in the shipped proxy today — `wiring.rs`
  constructs `TenantAllowlist::new()` empty and nothing populates it from
  config yet — but latent in the crate's own public contract, which
  `bin/pgprox` will trust the moment that wiring lands.
  Acceptance: `add` refuses a tenant named `OTHER` with a new
  `AllowlistError::ReservedName` variant (the enum is already
  `#[non_exhaustive]`, so this is additive), reachable through
  `from_configured` as well since it calls `add` internally. New test
  `a_tenant_named_other_is_refused_rather_than_swallowing_the_aggregate`,
  verified against a revert of the fix before landing.
- [x] `M90.17` `GossipCoordinator`'s outgoing digest version — the `u64` a
  peer's `DigestStore` uses to tell a newer report from a reordered old one,
  with no shared clock — was an `AtomicU64` seeded at 0 on every
  construction. A node killed and restarted inside `dead_after` (10s by
  default) is not reaped from a peer's store first, per `observe()`'s own
  comment on why `DigestStore` is not liveness-filtered: nothing tells a
  peer the node is gone except an explicit leave announcement, which a kill
  never sends. So the restarted node's first digest is compared against
  whatever version its previous incarnation reached, and a counter that
  always starts at zero loses that comparison on exactly the schedule a
  crash loop produces — `merge` treats a lower version as permanently stale,
  with no notion that a lower version could mean a fresher process rather
  than a reordered message. The peer keeps whatever it held before the
  restart (stale counts, and via `heard_without_mode`'s deliberate
  mode-preservation on a stale merge — a real fix for a different scenario,
  `M14.16`, message reordering while still running — a `Draining` mode that
  never clears). `NodeCoordinator::self_version`, the counter for a node's
  own entry in its own store, is a different thing entirely and was not at
  risk: that store is always empty at construction, so there is nothing
  stale to lose a comparison against.
  Acceptance: the counter is now seeded from `Clock::wall()` — already
  injected, so no call site changes anywhere in the workspace — as
  milliseconds since the Unix epoch rather than 0. Real time only moves
  forward across a restart, and even a node gossiping once a second for a
  full year only reaches the high tens of millions, four orders of magnitude
  below where the next boot's floor begins, so no coordination between
  incarnations is needed for the new one to win the comparison. New tests
  `version_floor_reads_milliseconds_since_the_epoch`,
  `version_floor_does_not_panic_before_the_epoch`, and
  `a_process_that_restarts_inside_dead_after_is_not_rejected_as_stale` (an
  end-to-end reproduction through the real `gossip`/merge path, using a
  `Clock` fake with an explicit wall time rather than the real one so the
  test does not depend on how fast it happens to run); the workspace-wide
  `guaranteed_plus_leased_never_exceeds_the_cap` property test still holds,
  since this changes nothing about quota accounting. Verified against a
  revert of the fix before landing. ADR 0004 records the reasoning
  alongside the version field it already documents.
- [x] `M90.18` `SqlReplicaProbe::ask`'s failure branch dropped its held
  connection outright with no `Terminate`, the same shape `M88.9` fixed for
  `ParameterCache::ensure`. This connection never enters COPY, so
  `Upstreamed::goodbye`'s "only on a clean close" restriction does not
  apply — the failure state left after an `ErrorResponse` to a plain query
  is exactly the state `goodbye` is safe in. A replica refusing every poll
  (the "database system is starting up" case `probe.rs`'s own test already
  scripts, ordinary during a failover) abandoned one un-terminated backend
  connection per quarter-second poll, each reclaimed only by the replica's
  own timeout rather than promptly.
  Acceptance: the error branch calls `connection.goodbye()` before dropping
  it. New test `a_refused_probe_says_goodbye_before_dropping_the_connection`,
  recording bytes the fake replica reads after close the same way
  `ensure_says_goodbye_to_its_probe_connection` already does for the sibling
  bug; verified against a revert of the fix before landing.
- [x] `M90.19` `Wire::read_header`'s doc claimed it was unsafe to call from
  inside a `select!` a drain branch could win, naming the server-to-client
  pump as the one place that could use the split read/body pattern safely
  because "the relay loop" instead calls `read_tagged` to stay atomic. Both
  claims were wrong: the client-read loop (`bin/pgprox/src/serve.rs`'s
  `relay()`) has called `read_header` from inside exactly such a `select!`
  since `M16.12`, correctly — `read_header` alone never partially consumes a
  message when cancelled, and `relay()` keeps the body read that must follow
  it on a plain unraced `.await` right after, which is what actually makes
  the pair safe there. The doc was internally contradicted by the very code
  it was describing, which is the shape of doc drift most likely to mislead
  the next change: a future `select!` branch added around `relay()`'s body
  read would look like it was following documented practice.
  Acceptance: `read_header`'s and `read_body_into`'s doc comments in
  `crates/pgprox-session/src/shell.rs`, and the matching comment in
  `bin/pgprox/src/serve.rs`'s server-to-client pump, now state the actual
  safety split — the header read is fine to race, the body read after it
  never is — rather than a blanket "not cancellation-safe" that the code
  does not follow. No behavior change; verified by the full existing test
  suite for both crates still passing and `cargo doc` finding no broken
  intra-doc link.
- [x] `M90.20` A pipelined statement that changed routing target — most
  notably one that pinned — could be sent to the connection a *previous*
  pipelined statement was already holding, silently ignoring the new
  decision. `Relay::on_client` decided whether to acquire from `!self.
  holding` alone: whether *anything* was held, never whether it was held
  *for the target this statement needs*. Outside an explicit SQL transaction
  `pgprox-route` decides fresh per statement (its own rule: one decision per
  transaction, and an autocommit statement is its own), so two statements
  pipelined before either's `Sync` — an ordinary shape for a driver using
  libpq/asyncpg pipeline mode, not an edge case — can legitimately route
  differently: a plain read to a replica, then `LISTEN` pinning the session
  to the primary. `awaits_more` is exactly why the shell never gets a chance
  to release between them. With only "is anything held" to go on, the second
  statement's own routing decision (primary, because it just pinned) was
  computed correctly and then ignored: `acquire` stayed false, the stale
  replica connection from the first statement was reused, and the pin this
  session's own state now records was never reflected in what connection
  bytes actually reached. Every write after that point in the session — the
  ordinary continuation once something has pinned — would hit a read-only
  replica.
  Acceptance: `Relay` now tracks `held: Option<RouteTarget>`, what the held
  connection is *for*, not only whether one is held; `acquired()` takes the
  target it was given; `on_client`'s `acquire` compares against it
  (`self.held != Some(target)`) instead of testing only `is_none()`. Both
  shell call sites (`relay()`'s main loop and `replay_held`'s query-cache
  replay, which share `Relay::on_client` through `decide()`) are fixed by
  the one change, since both already thread `acquire`/`target` through
  unmodified. New test
  `a_pipelined_statement_that_pins_reacquires_off_the_held_replica`,
  verified against a revert of the fix before landing.
  `Option<RouteTarget>` costs 16 bytes fully niched (measured directly, an
  `Option` of it costs no more than the type itself) against the 1 byte
  `holding: bool` it replaced, which pushed
  `one_session_costs_less_than_the_slab_buffer_it_no_longer_holds`'s future
  past its 5 KiB ceiling; the ceiling moves to 5.25 KiB to hold the 5,128
  bytes this leaves it, the same kind of deliberate, documented bump `M74.0`
  made for the idle timeout, preferred here to a smaller hand-rolled
  encoding for a type this crate does not own.
- [x] `M90.21` `invalidate_on_write` only recognized `Query` and `Parse`, so
  a write sent as `Bind`/`Execute` of an already-`Parse`d statement — the
  ordinary "prepare once, execute many" pattern most drivers with their own
  statement cache use for every repeat execution — never invalidated the
  tenant's cached entries. `facts_for` already resolves a `Bind`'s SQL via
  `session.statements.get(statement)` for the *serving* side of the same
  feature; invalidation had no equivalent resolution, so a session using
  this pattern invalidated correctly on its write's first execution (which
  carries the `Parse`) and never again, serving reads that predated later
  writes for up to the tenant's whole configured TTL.
  Acceptance: `invalidate_on_write` gains the same `Bind`-resolves-through-
  `session.statements` logic `facts_for` already has, threaded through
  `cache_before_sending`'s new `session` parameter. New test
  `a_write_sent_only_as_bind_still_invalidates`, which prepares a write once
  (invalidating correctly, before this fix, on that round trip) then seeds a
  fresh entry and runs the same statement again via `Bind`/`Execute`/`Sync`
  alone — the round trip this fix is actually about. Verified against a
  revert of the fix before landing.
- [x] `M90.22` `TenantView.upstream_conns`, gossiped in `report_tenants` and
  read as a tenant's live upstream usage by every shed and quota decision on
  the cluster, was neither of those things on two counts. `Sessions::
  per_tenant` counted every registered client regardless of `ClientState`, so
  a tenant with connections `Idle` or `Waiting` — holding no upstream
  connection at all — inflated the number a shed decision weighs against the
  tenant's grant. Separately, `run.rs`'s `ticker()` reported every tenant
  with a client on this node, not only the tenants
  `MembershipView::is_home_for` says this node homes; `ClusterDigest.
  tenant_usage`'s own doc requires the latter, to bound message size at
  roughly `tenants/nodes` rather than every tenant touching every node.
  Acceptance: new `Sessions::per_tenant_upstream()` counts only
  `ClientState::Active` entries per tenant, fixing the metric itself; new
  `tenants_to_report()` in `run.rs`, a pure function filtering a tenant-usage
  list to `membership.is_home_for(tenant)`, fixing the scope, wired into
  `ticker()`'s call to `report_tenants`. New tests
  `only_connection_holding_clients_count_toward_upstream_usage` and
  `only_the_tenants_this_node_homes_are_reported`, each verified against a
  revert of its half of the fix before landing.
- [x] `M90.23` `SHOW CONFIG`/`GET /v1/config` marked `drain_grace` and
  `grant_ttl_cap` `changeable: yes`, and both read from the live document
  watch for the `value` column shown beside that claim. Neither is actually
  live: `Drainer.grace` is copied from `App.config` — a snapshot the type's
  own doc calls out as "Not the live one" — once when `run()` builds it, and
  `grant_ttl_cap` is baked into the `CachingResolver`'s `CacheConfig::max_ttl`
  once when `entry.rs` builds the resolver. An operator raising either during
  an incident, expecting the next reload to pick it up the way `max_client_
  conns` and `servers.*.max_connections` genuinely do, would see the new
  value reflected immediately in `SHOW CONFIG` while enforcement kept the
  old one — the worst version of this shape, since the surface actively
  claims a reload took effect. `retry` and `client_idle_timeout` have the
  identical startup-only shape and are already correctly marked `"no"` and
  documented as such; these two were the ones left inconsistent with that
  precedent, most likely simply missed rather than a considered choice, since
  nothing in either field's own code or comments promises live reload.
  Decided against wiring live reload for either (a materially larger change
  touching `Drainer`'s six test call sites and `pgprox-auth`'s cache
  internals, closer to a new feature than a fix, and not what either field's
  existing code commits to) in favor of making the claim match the code, the
  same direction already taken for `retry`/`client_idle_timeout`.
  Acceptance: both rows now report `changeable: "no"`; `docs/configuration.md`
  documents both as read once at startup, alongside the existing `retry`/
  `client_idle_timeout` explanation. New test
  `drain_grace_and_grant_ttl_cap_are_reported_not_changeable`, verified
  against a revert of the fix before landing.
- [x] `M90.24` `pgload`'s `summarise` picked the report's `first_error` (in
  fact documented as "the most recent failure") from whichever `Tally` this
  loop's iteration order reached last — the order each connection's task
  finished and got pushed into the shared `Vec` behind a `Mutex`, which is
  scheduling order, not the wall-clock order the failures themselves
  happened in. A connection whose last failure was seconds ago but whose
  task happened to finish last could overwrite a connection whose failure
  was genuinely the newest, describing a moment already gone by the time the
  report is read — exactly the failure mode the field's own doc explains
  `first_error` exists to avoid, just reintroduced across connections
  instead of within one.
  Acceptance: `Tally` gains `last_failure_at: Option<Instant>`, stamped
  alongside `last_failure` at both failure sites; `summarise` keeps the
  failure with the later `last_failure_at` instead of the one this loop
  reaches last. New test
  `the_report_keeps_the_actually_most_recent_failure_across_connections`,
  which gives the genuinely newer failure an earlier slot in the slice and
  the genuinely older one a later slot so Vec order and wall-clock order
  disagree, verified against a revert of the fix before landing.
- [x] `M90.25` Cycle 7 — a deep dive on `pgprox-cache` (byte-budget
  accounting, TTL/expiry arithmetic, cache key construction, invalidation
  completeness, lock ordering) and a pass on `pgprox-proto`'s real hot-path
  parsing (`pgprox_session::shell::Wire`'s length-prefix framing, extended-
  query decoding, startup/SSL negotiation) plus all of `pgprox-tls` (cert
  reload race, upstream verification, secret-in-log) — came back with no
  confirmed findings, the first clean cycle of the seven this milestone ran.
  `pgprox-cache`'s agent flagged three candidates, each independently traced
  and judged not a fresh finding: `ttl_cap` lowered by a reconfigure is not
  retroactive on already-cached entries (the ordinary per-entry TTL
  contract, distinct from the byte budget's shared-resource eviction, which
  is eager); a `FunctionCall` fast-path write is invisible to invalidation
  because its payload is deliberately never parsed, already documented as an
  accepted limit by ADR 0013; and a cache holding `u32::MAX` live entries
  would alias two slot indices, unreachable under any realistic byte
  budget. Closes M90: status row marked complete, `docs/internal/product/
  roadmap.md`'s "Where it stands" section names all twenty-four findings by
  theme and what the milestone leaves open.
