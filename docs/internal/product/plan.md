# pgprox: multitenant Postgres proxy in Rust

## Context

We need a Postgres proxy that sits between a very large fleet of client
connections and a small number of upstream Postgres servers, each hosting up to
5,000 tenant databases with distinct roles and passwords. Clients authenticate
with a JWT rather than a database password; an external sidecar validates the
token and returns the real backend coordinates.

The core tension: downstream connections are cheap and numerous (target 50k to
100k per proxy node), upstream connections are scarce and capped (~5k per
Postgres host). The proxy must absorb that ratio through transaction-level
multiplexing while never breaching the upstream cap, including when the proxy
itself is running as 3 to 5 pods that each hold their own pools.

Secondary goals that shape the design from day one rather than being bolted on
later: read routing to replicas with read-your-writes consistency, a FIPS build
variant, zero-downtime node drain driven by pulled config, and an operational
surface that a human or an agent can drive without guessing which pod to ask.

The repo is empty. This is a greenfield build.

## Decisions already made

| Area | Decision |
| --- | --- |
| Pooling | Transaction-level multiplexing, automatic pinning on session-scoped features |
| JWT transport | Password field over required TLS, plus SCRAM passthrough for non-JWT admin clients |
| Sidecar | gRPC over a Unix domain socket; sidecar owns signature and claim validation |
| Cluster | SWIM gossip for membership, lowest-node-ID leader granting TTL'd quota leases |
| Affinity | Rendezvous-hashed home node per tenant, decaying quota reservations, opportunistic idle shedding |
| Config | Pluggable `ConfigSource` trait, ConfigMap file watch as default, drain expressed as desired state |
| Observability | Prometheus + OTel + JSON logs, cluster-scoped HTTP/JSON admin API, PgBouncer-style `SHOW` |
| Scale | 50k to 100k client conns per node, ~5k upstream per host, buffers reclaimed on idle |
| Replicas | LSN-watermark read routing in MVP |
| FIPS | Build variant behind a feature flag, non-FIPS builds are the default |

## The AI development system (built first, before any Rust)

Five parallel tracks written largely by agents only works if every session starts
with the same context and the same non-negotiables. Otherwise each track drifts
into its own idea of what an error type looks like, and integration becomes a
rewrite. So this gets built before `pgprox-core`, as M-1.

Three layers, distinguished by *when* they apply. Getting this split right is
what keeps the system from becoming a pile of documents nobody reads.

| Layer | Standard | Applies | Portable? |
| --- | --- | --- | --- |
| Context | `AGENTS.md` | Always, no invocation | Natively, 30+ tools |
| Workflow | `SKILL.md` (Agent Skills) | On demand, when the task matches | Natively, ~40 tools |
| Enforcement | none exists | Mechanically, on every change | No, so it goes in git and CI |

Documents alone do not work. An agent that has read `testing.md` still skips the
test when the change looks trivial. The enforcement layer is what makes standards
binding, and it is the part most spec systems leave out.

### Portability: two open standards and one gap

The tooling converged during 2025, which makes this much easier than it would
have been a year ago. Nothing here needs a Claude-specific format.

**Context is `AGENTS.md`.** Released by OpenAI in August 2025, transferred to the
Linux Foundation's Agentic AI Foundation, and read natively by Codex CLI, Cursor,
Copilot, Gemini CLI, Aider, Windsurf, Zed, Jules, Factory, and Devin. Claude Code
is the one holdout that reads `CLAUDE.md` instead, and it is handled with a
one-line file:

```
CLAUDE.md      contains exactly: @AGENTS.md
```

**Skills are `SKILL.md`.** Anthropic published Agent Skills as an independent
open standard in December 2025 (maintained at agentskills.io); Microsoft and
OpenAI adopted it within days, and by mid-2026 roughly 40 products read the
format unchanged, Codex CLI, Gemini CLI, Cursor, Copilot, VS Code, Goose, and
OpenCode among them. A skill directory with YAML frontmatter and a Markdown body
runs everywhere without translation. Only the *discovery path* still varies by
tool, so keep one canonical directory and symlink the rest:

```
.agents/skills/<name>/SKILL.md    canonical
.claude/skills                    symlink -> ../.agents/skills
```

**Enforcement has no standard**, and that is the real portability constraint. So
the canonical enforcement lives in git hooks and CI, which every tool and every
human passes through regardless of editor. Vendor hooks are then a thin
accelerator that calls the same scripts, never a second implementation.

A CI drift check regenerates the derived files and diffs them, so nobody
hand-edits a vendor path and quietly forks the standards.

### Layer 1: context, always loaded

```
AGENTS.md                        root: mission, invariants, links to standards
CLAUDE.md                        one line: @AGENTS.md
standards/
  rust-style.md                  naming, module layout, what goes in lib vs bin
  error-handling.md              error taxonomy, no unwrap outside tests, context rules
  async-concurrency.md           sans-I/O rule, no blocking in async, cancellation safety
  testing.md                     the three tiers, coverage floor, fakes over mocks
  observability.md               metric naming, span conventions, what may never be logged
  security.md                    SecretString discipline, untrusted input boundaries
  contracts.md                   how to change a pgprox-core trait without breaking tracks
  behavior.md                    agent working agreement (see below)
product/
  mission.md                     what pgprox is, who uses it, what it must never do
  architecture.md                the crate map and dependency rules, kept in sync
  decisions/0001-....md          ADRs, one per decision in the table above
crates/<name>/AGENTS.md          per-crate context, auto-loads when working in that crate
crates/<name>/CLAUDE.md          one line: @AGENTS.md
```

The per-crate `AGENTS.md` is the highest-leverage piece and the one most projects
miss. Agents load the nearest instruction file when working in a subtree, so an
agent editing `crates/pgprox-cluster/` automatically gets the quota invariant, the
gossip message format, and the simulation harness conventions, without anyone
remembering to paste them. That is the real answer to "inject specs into any AI
coding session": for the always-true things, do not inject, arrange for them to
already be there.

Keep the root `AGENTS.md` short and make it link out. It is loaded into every
session by every tool, and tools differ in how much they will pull in, so the
root file should be the index and the standards files should carry the detail.

`behavior.md` is the working agreement: never claim a test passes without running
it, never edit generated code, never change a `pgprox-core` trait without
updating every fake and impl in the same change, never lower a coverage threshold
to make a commit pass, always state explicitly what was left undone.

### Layer 2: skills, invoked on demand

Each skill is a workflow, not a document. The test of whether something should be
a skill rather than a standard: does it describe a *procedure with steps*, or a
*rule that is always true*? Rules go in standards, procedures become skills.

| Skill | Does |
| --- | --- |
| `spec` | Turn a feature request into `specs/<date>-<slug>/` with spec, contracts, test plan, and an ordered task list. Nothing gets built without one. |
| `tdd` | Enforce red/green/refactor: write the failing test, run it, show the failure, then implement. Non-optional given the 95% floor. |
| `contract-change` | Changing a `pgprox-core` trait: update the trait, every fake, every impl, the ADR, and the dependent tracks' specs, as one atomic change. |
| `crate-review` | The project review checklist: sans-I/O held, no `unwrap`, no `unsafe`, secrets redacted, coverage clears, public items documented. |
| `hot-path` | The performance discipline below: baseline, allocation budget, benchmark, compare, record. |
| `adr` | Write an architecture decision record, including the alternatives rejected and why. |
| `wire-debug` | Capture and decode a Postgres wire trace when something misbehaves at the protocol level. |
| `next-task` | Decompose the next milestone into commit-sized tasks with acceptance criteria, and pick the top unblocked one. Drives the autonomous loop. |
| `skill-forge` | The metaskill: scaffold, review, and validate new skills for this repo. |

Every skill is written to the Agent Skills spec and stays vendor-neutral in its
body: no `.claude/` paths, no tool-specific tool names, no assumption about which
model is reading it. Where a skill needs to run something, it names a script in
`scripts/` rather than a built-in, since script invocation is the one capability
every coding agent has.

`skill-forge` is the metaskill. It scaffolds a skill to the spec, then validates
it: frontmatter present and well-formed, description written to earn retrieval
(the description is the only thing loaded until the skill fires, so it is the
entire retrieval surface), body free of vendor-specific references, and a trigger
check confirming the skill actually fires on realistic prompts. It also carries
the repo conventions a generated skill must follow, so new skills inherit the
system rather than reinventing it.

Prose skills (`human-tone` and similar) may already exist in a developer's
personal setup. Keep the repo's copies limited to what is project-specific so the
two do not fight, and note the overlap in `skill-forge` so generated skills do
not duplicate them.

### Layer 3: enforcement, portable by construction

No standard exists for agent hooks, so enforcement cannot live there. The
canonical implementation is one set of scripts, invoked from three places.

```
scripts/check-fmt.sh
scripts/check-crate.sh <crate>       fmt + clippy for one crate
scripts/check-coverage.sh <crate>    the 95% gate
scripts/check-drift.sh               derived vendor files match canonical source
```

- **Git hooks** via `pre-commit` (the widest-known framework, large ecosystem of ready hooks, works
  identically on every machine and in every editor). This is the binding layer:
  whatever wrote the code, agent or human, it passes through here.
- **CI** runs the same scripts, so a bypassed hook cannot ship.
- **Agent hooks, where the tool has them**, call the same scripts for faster
  feedback. Claude Code gets `PostToolUse` on `Edit`/`Write` for `*.rs` running
  `check-crate.sh` on the touched crate, `PreToolUse` blocking writes into
  generated directories, and `Stop` checking coverage on touched crates. Cursor
  and others get the equivalent where supported. All of it is optional
  acceleration: the same checks still run at commit and in CI, so a developer on
  a tool with no hook support is never working under weaker rules, just with
  slower feedback.

The distinction matters practically. In-session feedback lets an agent fix a
clippy failure in the same turn while the context is live; catching it at commit
time means re-deriving what changed. But the guarantee comes from git and CI, so
the system degrades in speed rather than in safety when a tool lacks hooks.

### Execution: driving the build with /goal

Claude Code's built-in `/goal` runs an autonomous loop toward a stated completion
condition, with a separate checker model verifying after each turn whether the
condition is met. That is the delivery mechanism for this roadmap. It is a
command the human types, not something the agent invokes, so the repository's job
is to make the loop safe and to make "done" mechanically checkable.

**One goal per milestone, never one goal for the whole project.** `/goal` is only
as good as the checker's ability to verify the end state. "Deliver pgprox" gives
the checker nothing to test; "M0 is complete" does, if M0's completion condition
is a command that exits zero. So every milestone in the roadmap carries an
executable completion condition:

```
M-1  scripts/check-drift.sh passes, pre-commit hooks installed, every skill validated
     by skill-forge, and the second-tool portability check recorded
M0   cargo llvm-cov nextest --fail-under-lines 95 passes on every crate, every
     pgprox-core trait has a fake with its own tests, cargo deny check is clean
M1   the protocol conformance suite passes against Postgres 17 and 18
...
```

That way the checker runs a command rather than forming an opinion.

**Task granularity is the other half.** One task equals one commit equals one
coherent change that leaves the tree green: fmt, clippy, tests, and the coverage
gate all pass. If a task cannot be finished in one commit with the gate green, it
is too big and gets split before any code is written. This is what makes an
unattended loop safe, because every commit is a known-good state and a bad turn
costs one revert instead of a bisect through half a milestone. The rule lives in
`behavior.md` so it applies to every autonomous turn without being restated.

**The per-task cycle**, encoded in `behavior.md` and the `next-task` skill:

1. Read `product/roadmap.md` and `product/backlog.md`. If the next milestone is
   not yet decomposed, break it into commit-sized tasks with acceptance criteria
   and dependencies, and write them down before writing code.
2. Take the top unblocked task.
3. Implement under the `tdd` skill: failing test, observe the failure, implement,
   observe the pass.
4. Review before committing, not after: `crate-review` plus the enforcement
   scripts. Anything red means keep working. Never commit a broken tree, never
   lower a threshold to get green.
5. Commit on a branch, referencing the task ID. Never commit to the default
   branch, never push unless asked.
6. Mark the task done. If the work invalidated part of the roadmap, amend it and
   say so rather than silently diverging.

**Escalation conditions**, where the loop should stop and report instead of
improvising: a task needs a decision from the open items list, a `pgprox-core`
contract change has cross-track blast radius, the gate cannot be made green after
a bounded number of attempts, or a roadmap assumption turns out to be wrong.
These belong in `behavior.md` too, since `/goal` will otherwise keep trying.

Before any of this runs the repository needs `git init`, since it is not
currently a git repository and the entire safety story rests on commits.

### Specs and parallel tracks

Each of the five tracks gets a spec directory before any code. A spec carries the
exact type signatures it owns, its acceptance criteria as observable behaviours
(given/when/then, so they translate directly into tests), and an ordered task
list. The value is that two agents working different tracks share `contracts.md`
and cannot silently disagree about an interface: the contract is the artifact,
the code is derived from it.

Specs are versioned in git alongside the code, and a contract change is a spec
change first. When something in `pgprox-core` moves, the `contract-change` skill
makes the blast radius explicit rather than leaving it for integration day.

## Architecture

### Crate layout

Every crate depends on `pgprox-core` and, with three exceptions, on nothing
else in the workspace: `pgprox-session` composes `pgprox-proto`, `pgprox-pool`
and `pgprox-route`; `bin/pgprox` composes everything; `bin/pgload`, the load
client, composes `pgprox-proto`, `pgprox-load`, `pgprox-auth` and `pgprox-tls`
to speak the real wire protocol against a real client TLS configuration
without ever being a dependency of the proxy itself. See
[architecture.md](architecture.md) for the current, kept-in-sync statement of
this rule; what follows here is the shape it had at MVP, kept for the crates
it still describes correctly. That is what makes parallel development work: a
module owner codes against traits and fakes, not against another team's
half-finished crate.

```
pgprox/
  crates/
    pgprox-core/      contracts only: traits, DTOs, errors, IDs, buffer slab. No I/O.
    pgprox-proto/     Postgres wire codec, both directions, frame-level passthrough
    pgprox-tls/       rustls setup, FIPS feature gate, cert hot-reload
    pgprox-auth/      JWT extraction, sidecar gRPC client, grant cache with singleflight
    pgprox-pool/      upstream pools, lifecycle, idle reap, pinning, prepared-stmt mapping
    pgprox-route/     target selection, statement classification, replica LSN watermarks
    pgprox-cluster/   SWIM gossip, membership, quota leases, tenant reservations
    pgprox-config/    ConfigSource providers, schema validation, hot reload
    pgprox-observe/   metrics, tracing, log init, health endpoints
    pgprox-admin/     HTTP/JSON API and SHOW pseudo-database
    pgprox-session/   per-client state machine, relay loop (depends on proto + pool + route)
    pgprox-cache/     query cache, trait stub in MVP
  bin/pgprox/         composition root, wires concrete impls into the traits
  tests/conformance/  protocol behaviour vs real Postgres 17 and 18
  tests/sim/          deterministic cluster simulation
  deploy/             Helm chart, Dockerfiles (fips and default stages)
```

`pgprox-session` is the only crate that composes several others, and `bin/pgprox`
is the only place concrete types meet. Everything else is independently
testable.

### Contracts in pgprox-core

These land first and change rarely. Each has an in-memory fake shipped in the
same crate behind a `test-fakes` feature, so a module owner can build and test
without any other crate being finished.

```rust
#[async_trait]
pub trait CredentialResolver: Send + Sync {
    async fn resolve(&self, req: AuthRequest) -> Result<Grant, AuthError>;
}

pub struct Grant {
    pub tenant: TenantId,
    pub primary: Backend,
    pub replicas: Vec<Backend>,
    pub pool: PoolHints,     // max upstream, pool mode override, timeouts
    pub ttl: Duration,
    pub claims: ClaimSet,    // parsed, not verified: for authz policy and logging
}

pub struct Backend {
    pub host: String, pub port: u16, pub db: String,
    pub user: String, pub password: SecretString, pub tls: TlsMode,
}

#[async_trait]
pub trait UpstreamPool: Send + Sync {
    async fn acquire(&self, key: &PoolKey, lease: &QuotaLease, deadline: Instant)
        -> Result<UpstreamGuard, PoolError>;
    fn stats(&self) -> PoolStats;
}

#[async_trait]
pub trait ClusterCoordinator: Send + Sync {
    fn membership(&self) -> MembershipView;
    fn home_node(&self, tenant: &TenantId) -> NodeId;
    async fn request_quota(&self, server: &ServerId, want: u32) -> Result<QuotaLease, QuotaError>;
    fn release_quota(&self, lease: QuotaLease);
    fn digest(&self) -> ClusterDigest;          // gossiped aggregate, powers admin reads
    fn subscribe(&self) -> watch::Receiver<ClusterEvent>;
}

#[async_trait]
pub trait ConfigSource: Send + Sync {
    async fn load(&self) -> Result<Config, ConfigError>;
    fn watch(&self) -> watch::Receiver<Arc<Config>>;
}

pub trait Router: Send + Sync {
    fn route(&self, ctx: &RouteCtx) -> RouteTarget;   // Primary | Replica(id) | Pinned
}
```

`SecretString` wraps credentials with a `Debug` impl that redacts and a `Drop`
that zeroes. Nothing in the codebase may log a `Backend` without going through
its redacting formatter.

### Testability as an architectural constraint

The 95% coverage floor from a sub-two-minute test run is only reachable if
almost no logic is trapped behind a socket. That is a design constraint, not a
testing afterthought, so it belongs here:

- **Sans-I/O state machines.** The protocol codec, the session state machine, the
  pin detector, the statement classifier, and the quota arithmetic are pure
  functions of `(state, input event) -> (state, output actions)`. They never
  touch a socket, a clock, or a syscall. This is where the bulk of the logic and
  the bulk of the coverage lives, and it tests at memory speed.
- **I/O is a thin shell.** The layer that actually reads and writes is generic
  over `AsyncRead + AsyncWrite + Unpin`, so tests drive it with
  `tokio::io::duplex` or `tokio_test::io::Builder` and never open a port. The
  shell is small enough that covering it is cheap.
- **Time is injected.** A `Clock` trait in core, with `tokio::time::pause()` for
  most tests and a virtual clock for the cluster simulation. No test ever sleeps
  in wall-clock time, which is the usual reason suites blow past two minutes.
- **The composition root is five lines.** `bin/pgprox/src/main.rs` parses argv
  and calls `pgprox_app::run(config, deps)`. All wiring logic lives in a library
  target that unit tests can call with fakes. Only `main.rs` is excluded from
  coverage.

### Connection lifecycle

1. TCP accept. Read the first 8 bytes. Dispatch on `SSLRequest` (80877103),
   `GSSENCRequest` (80877104, answer `N`), `CancelRequest` (80877102), or a raw
   `StartupMessage`.
2. TLS handshake. If `require_tls` is set and the client skipped `SSLRequest`,
   reply with an `ErrorResponse` explaining why rather than dropping the socket.
3. `StartupMessage`. Accept protocol 3.0. If the client asks for 3.2 and we do
   not yet implement the wider cancel keys, reply `NegotiateProtocolVersion`
   down to 3.0, which every 3.2-capable driver handles by design.
4. `AuthenticationCleartextPassword`, and the client's `PasswordMessage` carries
   the JWT. If the startup user matches a configured static-credential rule, send
   `AuthenticationSASL` and run SCRAM-SHA-256 locally instead.
5. Resolve. Check the grant cache keyed by `sha256(token) || startup_db ||
   startup_user`. On miss,
   call the sidecar through a singleflight so a thundering herd of reconnects
   produces one RPC. Cache TTL is `min(grant.ttl, exp - now, configured_cap)`.
6. `AuthenticationOk`, the harvested `ParameterStatus` set, `BackendKeyData` with
   our own key, `ReadyForQuery('I')`. No upstream connection has been opened yet.
7. Relay. Client bytes are inspected only enough to find transaction boundaries
   and pin triggers; result rows are forwarded as opaque frames.

`ParameterStatus` values (`server_version`, `client_encoding`, `DateStyle`, and
the rest) come from a probe connection opened when a pool is first created, then
cached per `(host, db)`. This is the one place we need an upstream connection
before a client has issued a query.

### Transaction multiplexing and pinning

The authoritative signal for releasing an upstream connection is the transaction
status byte in `ReadyForQuery`: `I` idle, `T` in transaction, `E` failed
transaction. Release only on `I`, only with no extended-query sequence
outstanding, and only when the session is not pinned.

Pin triggers, each recorded with a reason for the `pgprox_pin_total{reason}`
metric:

- `LISTEN` / `UNLISTEN`, or any inbound `NotificationResponse`
- session-scoped advisory locks (`pg_advisory_lock`, not the `_xact_` variants)
- `CREATE TEMP TABLE` and anything touching the temp schema
- `DECLARE ... CURSOR WITH HOLD`
- SQL-level `PREPARE`
- `SET` of a parameter outside the replayable allowlist
- `COPY` in progress, which is naturally pinned until the stream ends

Session parameters inside the allowlist (`search_path`, `TimeZone`,
`application_name`, `statement_timeout`, `DateStyle`, `extra_float_digits`,
`client_encoding`, `lock_timeout`, `idle_in_transaction_session_timeout`) are
recorded per session and replayed on acquire when the target connection's
current values differ. `SET LOCAL` is transaction-scoped and needs no replay.
`RESET` and `RESET ALL` clear the recorded set.

**Protocol-level prepared statements are MVP scope, not optional.** Every modern
driver (pgx, asyncpg, JDBC, npgsql, SQLAlchemy) uses named `Parse`. Without this
the pool falls back to pinning almost every session and the whole design
collapses to session pooling. The mechanism: keep a per-upstream-connection map
of `global_stmt_name -> sql_hash`, rewrite the client's local statement name to a
global name derived from the SQL hash, and on acquire replay any `Parse` the
target connection does not already hold. Evict by LRU with a configurable
per-connection cap.

### Cancellation across nodes

We hand the client our own `BackendKeyData`, so the cancel key is ours to design.
Encode `node_id` in the high bits and a per-node counter in the rest. A
`CancelRequest` can land on any pod; the receiving node decodes the node ID, and
if it is not the owner, forwards the cancel over the peer channel. The owner maps
the key to the live upstream connection and issues a real `CancelRequest` to
Postgres using that connection's genuine backend key. Without this, cancellation
silently breaks the moment there is more than one pod, so it ships in MVP.

### Cluster: membership, quota, affinity

**Membership.** SWIM gossip over UDP using `foca`, seeded from the headless
Service DNS. One-second protocol period, sub-second failure detection. Each
message piggybacks a compact per-node digest: per-server upstream counts, client
counts, per-tenant usage for the tenants this node homes, lease state, and drain
mode. That digest is what lets any pod answer cluster-wide aggregate questions
locally.

**Quota.** For upstream server `S` with configured cap `C` (set to
`max_connections` minus a reserve for superuser and maintenance):

- Guaranteed share `G = floor(C * guaranteed_fraction / N)` where `N` is live
  members and `guaranteed_fraction` defaults to 0.5. A node may always open up to
  `G` connections with no coordination at all.
- The remaining `C - N*G` is a free pool leased by the leader, which is simply
  the lowest node ID in the current stable membership view. Leases carry
  `(server, count, expires_at)` with a 5s TTL, renewed at 2s.
- A node that becomes unreachable has its leases expire, returning capacity
  within one TTL with no explicit action.
- On leader change, the new leader rebuilds lease state from gossip digests and
  waits one full lease TTL before granting anything new from the free pool. That
  makes over-granting impossible across a leader transition.

The invariant to property-test: **the sum of guaranteed shares plus outstanding
leases never exceeds `C`, under arbitrary partitions, restarts, and leader
churn.**

**Affinity.** Tenant `T`'s home node is `argmax` over live nodes of
`hash(node_id, tenant_id)` (rendezvous hashing), so a membership change rehomes
only the tenants that lived on the departed node. The home node may reserve up to
`tenant_home_share` (default 0.8) of that tenant's upstream budget. Other nodes
share the rest and, when they hit it, queue for an existing upstream connection
rather than opening a new one. Reservations are use-it-or-lose-it: if the home
node's gossiped usage stays below its reservation for
`reservation_decay_rounds` (default 3), peers claim the slack.

**Opportunistic shedding.** On a non-home node, a client that has been idle at
`ReadyForQuery('I')` for longer than `shed_idle_threshold` (default 30s), whose
tenant's home node reports headroom, may be closed with `ErrorResponse` SQLSTATE
`57P01` so the driver reconnects cleanly and gets another roll of the LB dice.
Guard rails, all configurable and all with a global kill switch:

- rate limit per tenant (default 1 shed/sec) and a cap on the fraction of a
  tenant's clients shed per minute
- never shed within `settle_window` of a membership change
- never shed toward a node that is draining
- never shed a pinned or in-transaction session

The same mechanism, with longest-idle-first ordering, handles socket pressure:
above `max_client_conns * 0.9` the node evicts idle clients, preferring tenants
that already have presence on peers.

### Read replica routing

Route target is decided once per transaction, at the first statement.

**Watermarks.** After a write transaction commits on the primary, the session
records an LSN floor. We obtain it by appending `SELECT pg_current_wal_lsn()` to
the commit round trip for sessions in replica-eligible mode, which costs one
extra statement per write transaction and no extra round trip. A background
poller queries `pg_last_wal_replay_lsn()` and `pg_is_in_recovery()` on each
replica every 250ms (`POLL_INTERVAL`, a constant, not a configured value) into
a lock-free cell the router reads with no await. A replica is eligible for a
given session only if its replayed LSN is at or past that session's watermark.

ADR 0009 also describes a bounded-staleness opt-in for tenants preferring
throughput over strict read-your-writes, where eligibility would be
`lag < max_replica_lag` and no watermark tracked. See that ADR's Outstanding
section: it is not implemented. Every replica-eligible session today routes
under the strict watermark rule above; there is no time-based alternative to
opt into.

**Classification.** A fast token-prefix classifier, not a full SQL parser. It
must get these right, and the default for anything it is not confident about is
the primary:

- `WITH` CTEs containing `INSERT` / `UPDATE` / `DELETE` / `MERGE` are writes
- `SELECT ... FOR UPDATE` / `FOR SHARE` are writes, they take locks
- `EXPLAIN ANALYZE` is a write
- `SELECT` calling a volatile function is unknown, so primary
- `BEGIN READ ONLY` marks the whole transaction replica-eligible

Explicit overrides: `SET pgprox.route = 'replica' | 'primary' | 'auto'` for the
session, and a leading `/* pgprox:replica */` comment for one statement. Pinned
and session-mode connections always use the primary unless explicitly marked
read-only.

### Memory at 100k connections

A connection holds no I/O buffer while idle. It borrows a 16 KiB buffer from a
sharded slab when the socket becomes readable and returns it once quiescent, so
an idle client costs a socket plus a state struct of roughly 200 bytes instead of
32 KiB. That is the difference between ~200 MB and ~3 GB of userspace at 100k
connections, and it is a small amount of machinery: the borrow/return points are
the two ends of the relay loop.

Be aware that kernel socket memory does not go away with this trick. At 100k
sockets, expect 1 to 3 GB of kernel buffers depending on `net.ipv4.tcp_rmem` and
`tcp_wmem` minimums, which need tuning in the pod's sysctls. File descriptors
need `ulimit -n` around 262144. Outbound ephemeral ports are not a concern at 5k
connections per upstream host, since the limit applies per destination tuple.

### Config and drain

`ConfigSource` is a trait, shaped to add a provider without a rewrite; today it
has one implementation, `FileSource`, which watches the mount directory
(ConfigMap updates swap a symlink, so watching the file itself misses changes).
An etcd-watch provider and an HTTP-poll provider were the design's intended
reach for a non-k8s deployment, feature-gated so the default build carries
neither; neither is built. Config is declarative and versioned; drain is
desired state, not an imperative RPC, so it survives a pod restart and shows up
in git.

```yaml
nodes:
  pgprox-2: { mode: drain }        # or: active, quiesce
```

Drain sequence:

1. Node flips `/readyz` to failing, so kubernetes pulls it from Service
   endpoints and no new TCP arrives.
2. Gossip announces `draining`. Peers exclude it from rendezvous hashing, so its
   tenants rehome, and they reclaim its quota reservations as they free.
3. Idle clients are closed immediately with `57P01`. In-flight transactions run
   to completion and close at their next `ReadyForQuery('I')`.
4. After `drain_grace` (default 60s), the remainder are force-closed.
5. A `preStop` hook triggers the drain and sleeps long enough for the sequence to
   finish before SIGTERM lands.

`POST /v1/drain` exists for interactive use and writes the same state with a TTL,
so an operator's manual drain does not silently persist forever.

### Observability

**Metrics.** Per-node and per-server aggregates go to Prometheus:
`pgprox_client_conns{node,state}`, `pgprox_upstream_conns{node,server,state}`,
`pgprox_quota_leased{node,server}`, `pgprox_wait_seconds` (time blocked acquiring
an upstream connection, the single most important latency signal),
`pgprox_query_duration_seconds{route}`, `pgprox_pin_total{reason}`,
`pgprox_shed_total{reason}`, `pgprox_auth_cache{result}`,
`pgprox_replica_lag_bytes{replica}`, `pgprox_cluster_members`, and
`pgprox_cluster_view_hash` so a mismatch across pods surfaces split brain.

With 5,000 tenants, a `tenant` label would blow up cardinality. Per-tenant detail
lives in the admin API and `SHOW` output instead, with a configurable allowlist
for the handful of tenants worth a Prometheus series.

**Tracing.** OTel spans for connection lifecycle and per transaction, carrying
`tenant_id`, `node_id`, `pool_key`, and route target. Sample at 1% by default,
always sample on error.

**Logs.** JSON with a `conn_id` of `base32(node_id || counter)`, so a log line
identifies its pod without a lookup.

**Admin surfaces, cluster-scoped by default.** Hitting any pod gives the whole
cluster's truth: aggregates answer from the local gossip digest, and only
drill-downs like listing sessions fan out to peers.

- `GET /v1/cluster`, `/v1/pools`, `/v1/tenants/{id}`, `/v1/clients`, `/v1/config`
- `POST /v1/drain`, `/v1/undrain`, `/v1/pools/{key}/reset`
- `?scope=local` on any read for single-pod detail
- an OpenAPI document generated from the handlers, so tooling and agents get a
  typed contract rather than scraped text

The `SHOW` pseudo-database speaks the same data to `psql`: `SHOW POOLS`,
`SHOW SERVERS`, `SHOW CLIENTS`, `SHOW PEERS`, `SHOW QUOTA`, `SHOW TENANTS`,
`SHOW CONFIG`, `SHOW STATS`, each with a `SHOW LOCAL ...` variant. The
PgBouncer-compatible subset keeps existing dashboards working.

### FIPS

Default builds use `rustls` with the `aws-lc-rs` provider. `--features fips`
swaps in the FIPS module (`aws-lc-fips-sys`, FIPS 140-3 certificate #4816), calls
`rustls::crypto::default_fips_provider()`, and asserts `ServerConfig::fips()` and
`ClientConfig::fips()` at startup, refusing to boot if either is false. Two
Dockerfile stages, since the FIPS module needs cmake, Go, and clang that the
default build does not.

One simplification worth noting: because the sidecar owns JWT signature
verification, the proxy never verifies a signature. It parses claims for policy
and logging and hashes the token for the cache key. That keeps the FIPS crypto
boundary limited to TLS plus SHA-256, and sidesteps the awkward question of
EdDSA's status in validated modules. The proxy still enforces an algorithm
allowlist on the header (RS256/384/512, PS256, ES256/384) as defence in depth.

Expect FIPS mode to drop ChaCha20-Poly1305 and restrict TLS 1.2 to ECDHE suites
with extended master secret enforced. Verify client driver compatibility against
that suite list before committing to FIPS in production.

## Milestones and parallel workstreams

M-1 and M0 are hard barriers: one agent each, everything else waits. After that,
five tracks run independently against the fakes in `pgprox-core`.

Every milestone below carries an executable completion condition in
`product/roadmap.md`, a command that exits zero when the milestone is done. That
is what `/goal` hands its checker, so vague milestones cannot be driven
autonomously and this is not optional bookkeeping.

**M-1. The AI development system.** No Rust yet. Standards, product docs, the
first ADRs (one per row of the decisions table), root and per-crate `AGENTS.md`
with their `CLAUDE.md` imports, the eight skills in Agent Skills format, the
`scripts/` enforcement set, and `pre-commit` plus CI wiring. Validate it two ways:
have an agent build something small and throwaway under the system to confirm the
checks actually fire and the standards actually get followed, and run the same
exercise on a second tool (Codex CLI or Cursor) to confirm portability is real
rather than assumed. A spec system nobody tested is a spec system nobody follows,
and a portable system nobody tested on a second tool is not portable.

**M0. Contracts and quality gates.** `pgprox-core` complete: traits above, DTOs,
error taxonomy, ID types, `SecretString`, `Clock`, buffer slab, and a working
in-memory fake for every trait. Plus the entire quality apparatus described in
the next section, live and enforcing on an empty codebase: pre-commit hook,
coverage gate at 95% per crate, clippy lint set, `cargo-deny`, CI pipeline.
Nothing else starts until this compiles, its fakes pass their own tests, and the
gate is green. Standing the gate up on day one costs a day; retrofitting it onto
five crates at 60% coverage costs weeks, and every track that starts before it
exists will need rework.

Then, in parallel. Each track owns its crate's coverage: a track's work does not
merge below 95% on its own crates, so no track can borrow another's number and
nobody inherits a coverage debt at integration time.

- **Track A, protocol (`pgprox-proto`, `pgprox-tls`).** Frame codec both
  directions, startup and auth flows, extended query sequences, COPY, negotiation
  and cancellation. Largest and most self-contained piece. Validated by the
  conformance suite against real Postgres 17 and 18 in testcontainers.
- **Track B, auth (`pgprox-auth`).** The `.proto` contract is the first
  deliverable and should be agreed with the sidecar team immediately, since it is
  the one cross-team interface. Then the tonic client over UDS, grant cache,
  singleflight, negative caching, and a mock sidecar binary for everyone else's
  integration tests.
- **Track C, cluster (`pgprox-cluster`).** Gossip, leases, leader election,
  rendezvous hashing, reservations, shed decisions. Needs no Postgres at all, so
  it is developed entirely against the deterministic simulation harness.
- **Track D, operations (`pgprox-config`, `pgprox-observe`, `pgprox-admin`).**
  Providers, hot reload, metric and span registry, admin handlers and `SHOW`
  parser wired to the core traits' fakes.
- **Track E, pooling (`pgprox-pool`, `pgprox-route`).** Pool lifecycle, idle
  reap, pin bookkeeping, prepared-statement mapping, statement classifier,
  replica poller and watermark logic. Tests against a fake upstream first, real
  Postgres once Track A lands.

**M6. Integration.** `pgprox-session` and `bin/pgprox` compose the real
implementations. End to end: docker-compose with 3 proxy nodes, a primary, 2
replicas, the mock sidecar, and pgbench.

**M7. Scale and performance hardening.** The reference workload description and
replay harness, the first semantic coverage report, allocation budget tests on
the declared hot paths, and `iai` benchmarks wired into PR CI. Then buffer
reclaim under real load, the 100k-connection harness (pgbench cannot generate
that, so a purpose-built client is needed), sysctl and fd tuning, and RSS and
added-latency measurement at p50 and p99. Order matters here: build the
measurement before the optimization, or the optimization is guesswork.

**M8. FIPS and release.** FIPS build stage, cipher suite compatibility matrix
across drivers, Helm chart, `preStop` and probe wiring, rolling upgrade rehearsal.

**M9, post-MVP. Query cache.** `pgprox-cache` behind the trait stubbed in M0.
Keyed by tenant, normalized SQL, parameter values, and `search_path`.
Invalidation by TTL first, table-dependency tracking later.

## Dependency picks

`tokio`, `bytes` for zero-copy framing, `rustls` + `tokio-rustls` + `aws-lc-rs`,
`tonic` + `prost`, `foca` for SWIM, `axum` for admin HTTP, the `metrics` facade
with `metrics-exporter-prometheus`, `tracing` + `opentelemetry`, `serde` with
`figment` for layered config, `notify` for the ConfigMap directory watch.

Prior art worth reading before writing the protocol layer: pgdog and pgcat are
both Rust Postgres proxies with transaction pooling, and pgbouncer 1.21+ for the
prepared-statement mapping approach.

## Testing and quality gates

Three tiers, split by what they cost to run. The rule is that the pre-commit
tier alone carries the 95% number, so coverage is never waiting on Docker.

### Tier 1: pre-commit, budget 2 minutes

Runs on staged changes, blocks the commit, and is the tier that enforces
coverage.

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo llvm-cov nextest --lib --bins --fail-under-lines 95
gitleaks protect --staged --redact
```

Hitting two minutes with an instrumented build takes deliberate setup:

- **`cargo-nextest`** instead of `cargo test`. It runs each test in its own
  process with real parallelism and no serialized harness startup, which is
  typically a 2 to 3x wall-clock win on a workspace this shape.
- **`cargo-llvm-cov`** rather than tarpaulin. Source-based LLVM coverage is both
  accurate on `async` code and cheaper to collect. Point it at a dedicated
  `CARGO_TARGET_DIR` so the instrumented build cache stops thrashing against the
  normal dev cache; without this the hook rebuilds the world on every commit.
- **A fast linker** (`mold`, or `lld` on macOS) in `.cargo/config.toml`. Link
  time dominates a many-crate workspace and this is close to free.
- **`opt-level = 1` for `[profile.test]`.** The cluster simulation and the
  property tests are compute-bound; unoptimized builds make them minutes slower
  than the compile time saved.
- **No sleeps, no sockets, no containers in tier 1.** Enforced by convention and
  by a `#[cfg(feature = "integration")]` gate: anything needing Docker or a real
  port lives behind that feature and is excluded from the hook.

If the budget starts slipping as the workspace grows, the escape hatch is to run
the hook only over crates touched by the staged diff (`cargo llvm-cov -p`),
keeping the full-workspace gate in CI.

### Coverage enforcement details

- **Per crate, not workspace-wide.** A single global number lets `pgprox-proto`
  at 99% mask `pgprox-cluster` at 70%. CI parses `cargo llvm-cov --json` and
  asserts every crate independently clears 95%.
- **Exclusions, kept to a minimum and reviewed:** `bin/pgprox/src/main.rs`, and
  prost/tonic generated code under `OUT_DIR`, via `--ignore-filename-regex`.
  Nothing else. In particular, error paths are not excluded; the fakes exist
  precisely so failure branches are reachable.
- **Line coverage is a floor, not a goal.** 95% coverage with weak assertions is
  a well-known failure mode. `cargo-mutants` runs nightly against the pure state
  machines (protocol, pin detection, classifier, quota arithmetic) and reports
  surviving mutants. Those are the modules where a silent logic bug is most
  expensive, and mutation score is the honest measure of whether the tests
  actually check anything.
- **Property tests count toward coverage and toward confidence.** `proptest`
  over the codec (round-trip any frame), over the classifier (never classify a
  DML-bearing CTE as read-only), and over quota arithmetic.

### Tier 2: pre-push and PR CI

No time budget. Everything in tier 1 plus:

- **Protocol conformance** against Postgres 17 and 18 in testcontainers, driven
  by psql, pgx, asyncpg, JDBC, and npgsql, so driver-specific behaviour
  (especially named prepared statements and pipelining) is verified rather than
  assumed.
- **Cluster invariants** in deterministic simulation with a virtual clock and an
  injectable network that partitions, delays, drops, and reorders. Property test
  the quota invariant (guaranteed plus leased never exceeds the cap) over
  thousands of randomized schedules including leader loss and simultaneous
  restarts. This is the class of bug that never reproduces in staging, so it has
  to be found here. Seeds of any failure get committed as regression cases.
- **Pooling correctness**: a client that interleaves session-scoped features with
  plain queries across many concurrent connections, asserting pinning fires when
  it must and that no session ever observes another's state (temp tables,
  `search_path`, prepared statements, transaction status).
- **Replica consistency**: write then immediately read in a loop against a
  replica with injected lag, asserting a read never lands on a replica behind the
  session watermark.
- **Drain**: under sustained pgbench load across 3 nodes, drain one and assert
  zero failed transactions, only clean reconnects, and that connections
  redistribute.
- **Supply chain and security**: `cargo deny check` (RustSec advisories,
  licenses, banned crates, source allowlist), `cargo audit`, and Semgrep with the
  Rust ruleset. CodeQL on the default branch.
- **Fuzzing**: `cargo-fuzz` targets on the wire decoder and the statement
  classifier. Both parse untrusted bytes arriving from the internet, so this is a
  security control rather than a nicety. Short runs (60s per target) gate PRs,
  long runs go nightly, and the corpus is committed.

### Tier 3: nightly and pre-release

Mutation testing, long fuzz runs, the FIPS build and its driver cipher-suite
compatibility matrix, and the scale test: 100k connections against one node,
measuring RSS (target under 500 MB userspace), added p99 latency (target under
1ms over a direct connection), and upstream connection count, which must never
exceed the configured cap.

### Hot-path coverage: the other kind of coverage

Line coverage answers "did some test touch this line". It says nothing about
whether that line runs a billion times a day or twice a year, and a proxy lives
or dies on that distinction. This is a separate discipline with a separate
artifact, and it never runs in pre-commit.

**The reference workload.** Everything below measures against one committed,
versioned workload description: a realistic tenant mix (a handful of hot tenants,
a long tail of near-idle ones), a query shape distribution, connection churn
rate, transaction size distribution, and replica read fraction. Without a fixed
reference, profiles are not comparable across weeks and the whole exercise turns
into anecdote.

**The semantic coverage report.** Replay the reference workload against an
instrumented binary and keep the LLVM *execution counts*, not the hit/miss
booleans. That produces a per-function cost profile, which is then cross
referenced into three lists that each imply a different action:

- **Hot and under-tested**: high execution count, low assertion density or
  surviving `cargo-mutants` mutants. This is the highest-risk code in the
  repository and the list that earns new tests first. It is a strictly better
  prioritization signal than uncovered-line count, which tends to point at error
  paths nobody will ever hit.
- **Hot and expensive**: high count multiplied by per-call cost. The optimization
  queue, ordered by total contribution rather than by which code looks
  interesting. This is the direct answer to finding optimization opportunities.
- **Cold and complex**: near-zero execution count but high complexity or visible
  hand-optimization. Candidates for simplification or deletion. Speculative
  optimization in proxies is common and this catches it.

**Allocation budgets as assertions.** For each declared hot path, assert
allocation *counts* rather than timings, using `dhat-rs` in a normal test.
"Relaying a 1 KiB `DataRow` allocates zero times." "Acquiring from a warm pool
allocates at most twice." These are deterministic and therefore stable in CI,
where wall-clock measurements are not.

**Instruction-count benchmarks.** `iai-callgrind` produces deterministic
instruction counts under Valgrind, so a 3% regression is detectable on noisy
shared CI runners where `criterion` would report noise. Gate PRs on `iai` for the
declared hot paths, keep `criterion` for wall-clock numbers that inform rather
than gate.

**The declared hot path inventory**, kept in `standards/` with explicit budgets,
because "hot path" has to be a written list or it becomes an opinion:

1. The steady-state relay loop in both directions
2. Frame boundary scanning (message type byte plus length)
3. `ReadyForQuery` transaction-status handling and the pool release decision
4. Warm-pool acquire
5. Route decision: classification plus replica eligibility
6. Grant cache lookup on connect
7. Gossip digest encode and decode

**Continuous profiling.** `samply` or `perf` with flamegraphs for CPU, `dhat` for
allocation shape, and `pprof-rs` exposed behind an authenticated admin endpoint so
a production node can be profiled on demand without a redeploy. That last one is
worth the small complexity: the interesting performance problems in a
multitenant proxy only appear under a real tenant mix.

**PGO, and possibly BOLT.** The reference workload profile is already being
collected for coverage, so feeding it into a profile-guided optimization build
costs almost nothing extra. Branch-heavy code like a protocol codec typically
picks up 5 to 15%. Evaluate BOLT afterward only if measurement justifies the
build complexity.

**Cadence.** `iai` benchmarks and allocation budget tests run per PR, since they
are fast. The full semantic coverage report and flamegraphs run nightly. The PGO
build and the 100k-connection scale run happen pre-release.

### Static analysis configuration

- `#![forbid(unsafe_code)]` in every crate. If the buffer slab ever seems to need
  unsafe, that is a design review, not a local exemption.
- Workspace lints in `Cargo.toml`: `clippy::all` and `clippy::pedantic` denied,
  with a short, commented allowlist for the pedantic lints that fight async
  code. Also deny `clippy::unwrap_used` and `clippy::expect_used` outside
  `#[cfg(test)]`, `clippy::todo`, `clippy::dbg_macro`, and
  `clippy::print_stdout`.
- `rustfmt.toml` checked in, non-negotiable, applied by the hook.
- `cargo-deny` config with an explicit license allowlist and a source allowlist
  pinned to crates.io, so a dependency cannot quietly start pulling from a git
  URL.
- MSRV pinned in `Cargo.toml` and verified in CI, since the FIPS toolchain
  constrains what the build image can carry.

Tooling in M-1: `pre-commit`. Tooling in M0: `cargo-nextest`, `cargo-llvm-cov`,
`cargo-deny`, `cargo-audit`, `cargo-fuzz`, `cargo-mutants`, `gitleaks`, `mold`.
Tooling in M7:
`iai-callgrind`, `criterion`, `dhat-rs`, `samply`, `pprof-rs`, and
`cargo-pgo`.

## Open items to settle during M0

Reviewed in `M14.5`. Two of the three were answered by work done since and were
still listed as open; one remains genuinely outside this repository. Each says
which it is, because a list of open questions that keeps answered ones on it
stops being read.

- ~~The sidecar `.proto` needs sign-off from whoever owns the sidecar.~~
  **Settled, differently from how it was asked.** ADR 0017 decided that this
  repository owns the contract, and `proto/pgprox/auth/v1/auth.proto` carries
  `STATUS: FROZEN` at v1. The premise was that the sidecar is the one interface
  we do not control; the decision was to control it. What survives is the
  discipline in [../standards/contracts.md](../standards/contracts.md): field
  numbers are never reused, fields are never removed, and a change needs
  agreement from the sidecar owners before the Rust side moves.
- Upstream `max_connections` and the reserve to subtract from it, per server
  class, so quota caps can be configured rather than guessed.
  **Half done, and the half that is left needs an owner outside this repo.**
  The mechanism exists: `max_connections` and `guaranteed_fraction` are fields
  on the config document, so a cap is configured and not baked in. The values
  per server class are not something this repository can know. Still open, and
  it is the only one of the three that is.
- ~~Whether any tenant needs `LISTEN`/`NOTIFY` at scale.~~
  **The consequence half is measured; the population half needs real tenants.**
  `M11.7` ran the curve: a pinned session costs `0.650` upstream connections,
  linearly, with no knee and no safe fraction, so the sizing model does not need
  revisiting so much as one extra term, `upstream = c0 + (1 - 1/r0) * pins`.
  With every session pinned the fleet held one upstream connection per client,
  which is ADR 0001's "collapses back to session pooling" as an identity. What
  this repository still cannot answer is what fraction of real tenants do it.
  See [perf/run-2026-07-31-pinning-curve.md](perf/run-2026-07-31-pinning-curve.md).
