# Backlog

One task equals one commit equals one change that leaves the tree green. If a
task cannot be finished in one green commit, split it before writing code.

Task IDs are stable. Completed tasks stay here with their commit reference so
the history of why something was done survives.

Decomposition rule: only the current milestone is decomposed in detail. Future
milestones stay as roadmap entries until their turn, because decomposing them
early produces tasks that are wrong by the time they are reached.

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
- [x] `M-1.16` `scripts/m-1-complete.sh`, the milestone completion condition.
  Acceptance: exits zero on a complete M-1 and non-zero with a useful message on
  each individual failure.
- [~] `M-1.17` Portability check on a second tool. Run a small throwaway task
  under Codex CLI or Cursor and record the result as an ADR. Acceptance: the ADR
  states what worked, what did not, and what was changed as a result.

## M0: contracts and quality gates

`pgprox-core` holds the traits and types every other crate depends on, plus a
working fake for each. It is what lets five tracks run in parallel from M1.

Sizing note: the coverage gate is 95% per crate, so a task that adds a trait
without its fake and tests leaves the tree red and is half a task. Every entry
below is types plus tests plus fake where one applies.

- [x] `M0.1` Define M0: this decomposition, and `scripts/m0-complete.sh`.
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
  Acceptance: `scripts/m0-complete.sh` exits zero.

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
  down to 3.0; a `CancelRequest` yields the encoded node and counter.
- [x] `M1.7` Session state machine: transaction status tracking and
  extended-query sequence tracking.
  Acceptance: release is permitted only at `ReadyForQuery('I')` with no sequence
  outstanding; a `Sync` missing mid-sequence keeps the session held.
- [x] `M1.8` COPY mode, both directions.
  Acceptance: a session in COPY is never released until the stream ends.
- [ ] `M1.9` Fuzz targets for the decoder, with a committed corpus.
  Acceptance: `cargo fuzz` runs both targets; any crash found becomes a unit
  test.
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
- [ ] `M1.14` Close M1. Acceptance: `scripts/conformance.sh 17 18` exits zero.

## M2 and later

Not yet decomposed. See [roadmap.md](roadmap.md). The `next-task` skill
decomposes the next milestone when the current one closes.
