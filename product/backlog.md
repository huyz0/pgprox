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
  down to 3.0; a `CancelRequest` yields the encoded node and secret.
- [x] `M1.7` Session state machine: transaction status tracking and
  extended-query sequence tracking.
  Acceptance: release is permitted only at `ReadyForQuery('I')` with no sequence
  outstanding; a `Sync` missing mid-sequence keeps the session held.
- [x] `M1.8` COPY mode, both directions.
  Acceptance: a session in COPY is never released until the stream ends.
- [~] `M1.9` Fuzz targets for the decoder, with a committed corpus.
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

- [x] `M1R.1` Define M1R: this decomposition and `scripts/m1r-complete.sh`.
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
- [!] `M1F.12` **Blocked on M6.** Wire SCRAM into the auth path as the non-JWT
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
- [ ] `M1F.21` The replayable session-parameter allowlist as a real type, with
  `SET`, `SET LOCAL`, `RESET`, and `RESET ALL` handling. ADR 0001 named it; it
  does not exist yet.
- [ ] `M1F.22` `GSSENCRequest` beyond refusal: confirm the refusal path against
  a GSSAPI-capable client rather than assuming it.

### Group F: conformance depth

- [x] `M1F.23` A message-coverage report. Instrument the conformance run to
  record which tags were actually seen in each direction, and fail if a tag with
  a decoder was never exercised. This is what turns "we handle it" into "we
  tested it".
- [ ] `M1F.24` Driver matrix against real Postgres, not only the harness. The
  drivers currently only meet our own server; run each against real Postgres
  through the relay to catch anything the harness gets wrong in the same
  direction we do.
- [ ] `M1F.25` Corpus seeding from the references: extract their protocol test
  fixtures into the fuzz corpus, so their accumulated edge cases become ours.

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

- [x] `M1F.26` Close M1F. Acceptance: `scripts/m1f-complete.sh` exits zero.

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

- [x] `M3.1` Define M3: this decomposition and `scripts/m3-complete.sh`.
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

- [x] `M5.1` Define M5: this decomposition, `scripts/m5-complete.sh`, and the
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

- [x] `M4.1` Define M4: this decomposition, `scripts/m4-complete.sh`, and the
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

- [x] `M6.1` Define M6: this decomposition and `scripts/m6-complete.sh`.
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
- [ ] `M6.18` The live `Observatory`, reading the real components. Acceptance:
  the surfaces-agree suite from `M4.18` passes against the live implementation
  unchanged, since it was written against the contract rather than the fake.
- [ ] `M6.19` The accept loop and listener, with TLS and the client connection
  ceiling. Acceptance: a node at its ceiling refuses with a message naming the
  limit, and refusal never takes down connections already established.
- [ ] `M6.20` The drain sequence, end to end. Acceptance: `/readyz` fails
  first, gossip announces before any client is closed, in-flight transactions
  finish, and the grace timer force-closes the remainder.
- [ ] `M6.21` `deploy/` and `scripts/e2e.sh`: three proxy nodes, a primary, two
  replicas, the mock sidecar. Acceptance: the script brings the stack up and
  reports which component failed when it does not, rather than a compose exit
  code.
- [ ] `M6.22` The e2e assertions the milestone is judged on: pgbench clean,
  drain with zero failed transactions, no replica read behind the session
  watermark. Acceptance: each assertion fails when its property is broken on
  purpose, verified once per assertion.
- [ ] `M6.23` Close M6.

## M7 and later

Not yet decomposed. See [roadmap.md](roadmap.md). The `next-task` skill
decomposes the next milestone when the current one closes.
