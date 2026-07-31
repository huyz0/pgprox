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

- [x] `M7.1` Define M7: this decomposition and `scripts/m7-complete.sh`.
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
  which `scripts/m3-complete.sh` exists to protect. And at 500 connections
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

- [x] `M8.1` Define M8: this decomposition and `scripts/release-check.sh`.
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
- [x] `M8.10` Close M8. `scripts/release-check.sh` exits zero, and so do the
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

- [x] `M9.1` Define M9: this decomposition and `scripts/m9-complete.sh`. The
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
  Acceptance: `scripts/m9-complete.sh` exits zero.
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
  and `scripts/release-check.sh` is the same story for M8.
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
- [x] `M10.17` Write `scripts/m10-complete.sh`, which the roadmap has named as
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
  `M11.5` writes `scripts/m11-complete.sh` rather than leaving it to be
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
- [x] `M11.5` Write `scripts/m11-complete.sh` before the milestone needs
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
- [ ] `M11.7` The pinning curve itself, once `M11.4` has given the load
  generator a statement kind that pins. At least three values of the pinned
  share, each a matched run, reading `pgprox_pin_total` by reason and the
  upstream connection count against the median.
  The question is where multiplexing stops paying for itself: ADR 0001 says a
  fleet whose tenants all use `LISTEN`/`NOTIFY` collapses back to session
  pooling, and the curve is what says at what share that starts to bite.
  Acceptance: three or more values, matched runs, and a stated crossing point
  or a statement that there is none inside the range measured.
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
  `scripts/m3-complete.sh` still passes, which is the check that matters most
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
