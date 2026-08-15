# Changelog

Notable changes to pgprox, by release. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) loosely: what changed,
not why — the roadmap and the ADRs under `docs/internal/product/decisions/`
carry the reasoning, and this stays a record of what shipped.

Versions before 1.0 may change the wire-visible admin surface (`SHOW` columns,
HTTP API shapes) without a major-version bump. [Features and
limits](docs/features.md) is the source of truth for what is and is not
supported at any given commit; this file is a record of what changed, not a
substitute for it.

## [0.1.1] - 2026-08-15

Forty-three commits since `v0.1.0` was tagged, none of them a new feature and
none of them a wire-visible surface change beyond correcting what the surface
already claimed. `v0.1.0`'s tag stays where it is rather than moving; this is
what actually shipped on top of it.

### Fixed

- A pipelined statement that changed which connection it needed — most
  commonly a `LISTEN` pinning to the primary — could reuse a stale connection
  a prior statement in the same pipeline still held, sending every write
  after that point to a read-only replica.
- A write whose post-commit position could not be confirmed (a primary
  failover or reset in that window) could be followed by a read served from
  a replica behind it.
- A write sent as `Bind` of an already-prepared statement — the ordinary
  prepare-once, execute-many pattern most drivers use — did not invalidate
  the query cache, serving stale reads for up to the tenant's configured TTL.
- A node restarting inside the cluster's dead-node window could be marked
  stale by its peers and stay excluded from gossip, sometimes stuck
  draining, until it restarted again.
- `SHOW TENANTS` and `GET /v1/tenants` reported client connection counts
  rather than upstream connections held, and included tenants a node does
  not home, understating headroom and weakening the cluster's opportunistic
  shed.
- `SHOW CONFIG` and `GET /v1/config` reported `drain_grace` and
  `grant_ttl_cap` as changeable on reload; both are read once at startup.
- A force-close during a drain or shutdown closed a client's socket without
  telling it why, indistinguishable from a crash.
- A connection's configured maximum lifetime had no enforcement path, so a
  connection released and immediately reused never aged out.
- A route hint (`SET pgprox.route = ...`) silently absorbed whatever
  statement followed it in the same wire message instead of forwarding it.
- A tenant or server name containing a quote or backslash could break a
  node's whole Prometheus scrape, not just its own series.
- A tenant could be allowlisted under the name the aggregate bucket itself
  uses, colliding with it.
- The grant cache did not distinguish `startup_user`, letting two different
  startup users on the same token share a cached grant.
- A rejected route hint was silently dropped instead of reported to the
  client as an error.
- A client that disconnected mid-transaction leaked its cancel-key
  registration.
- A refused replica health probe dropped its connection without sending
  `Terminate`.
- `pgload`'s "no connection" error reported nothing was attempted on a run
  that was continuously refused; its "most recent failure" message could
  report an older failure over a newer one.
- The query cache's case-insensitive word matching used full Unicode
  case-folding, which for one character (`İ`) expands to two codepoints and
  could collide two different identifiers onto one cache key.

### Status at this tag

94 of 95 roadmap milestones are complete. The one still open:
[`M16`](docs/internal/product/roadmap.md), a 100,000-connection run serving
large result sets, blocked on the multi-machine setup the measurement needs.

## [0.1.0] - 2026-08-15

First tagged release.

### Added

- Transaction-level connection pooling, with automatic session pinning for
  `LISTEN`, session-scoped advisory locks, temp tables, `WITH HOLD` cursors,
  SQL-level `PREPARE` and any `SET` outside a small replayable allowlist. See
  [Features and limits](docs/features.md).
- Per-tenant JWT authentication, resolved by an operator-provided sidecar over
  a frozen gRPC contract (`proto/pgprox/auth/v1/auth.proto`). See [Going to
  production](docs/going-to-production.md) and
  [Architecture](docs/architecture.md#the-token-service).
- Cross-node upstream connection cap enforcement over gossip, holding one
  quota across a stateless fleet that shares no memory. See [Clustering and
  deployment](docs/clustering.md).
- Read replica routing with a per-session write-position watermark, so a read
  never lands on a replica behind that session's own writes. See [Read
  routing](docs/read-routing.md).
- An optional, per-tenant opt-in query cache with bounded staleness, off by
  default. See [Configuration](docs/configuration.md#query_cache).
- `SHOW` command compatibility with pgbouncer's five overlapping commands,
  plus four pgprox-only ones (`SHOW QUOTA`, `SHOW PEERS`, `SHOW TENANTS`,
  `SHOW CACHE`). See [Admin and management](docs/admin.md).
- An HTTP/JSON admin API mirroring the `SHOW` surface, for a script, a
  dashboard or an agent rather than a person at a terminal.
- Prometheus metrics, `/healthz`/`/readyz` probes, and structured logs. See
  [Operations](docs/operations.md).
- Zero-downtime draining for rolling upgrades: a draining node closes clients
  at their next transaction boundary rather than mid-transaction.
- Live configuration reload without a restart, rejecting an invalid document
  rather than taking the node down.
- A Helm chart and several `docker-compose` stacks for deployment. See
  `deploy/`.
- FIPS builds against a validated crypto module. See [FIPS builds](docs/fips.md).

### Security

- TLS is required whenever JWT authentication is reachable, enforced at
  startup rather than left to whoever writes the deployment config
  remembering `--require-tls`. A node started with neither `--require-tls`
  nor the explicit `--insecure-plaintext-auth` opt-out does not come up.
- Every credential is held in a type that redacts in `Debug` and `Display`
  and zeroes on drop, checked structurally by `scripts/check-secrets.sh` and
  end-to-end by `scripts/e2e.sh` grepping every node's logs for the token and
  password it authenticated with. See [Security](docs/security.md).

### Known limitations

[Features and limits](docs/features.md) has the full list. The ones most
likely to matter when deciding whether to adopt this:

- No statement-level pooling; transaction and session only.
- No automatic primary failover. pgprox is told where the primary is by the
  sidecar; it does not elect or promote one.
- No sharding.
- The query cache offers bounded staleness, not read-your-writes, and is not
  shared across nodes.
- A 100,000-connection run that also serves load has been measured for
  holding the connections; serving them at that scale needs load generators
  on machines this project's own CI does not have, and is not yet verified.

### Status at this tag

90 of 92 roadmap milestones are complete. The two still open:
[`M16`](docs/internal/product/roadmap.md), a 100,000-connection run serving
large result sets, blocked on the multi-machine setup the measurement needs;
and [`M88`](docs/internal/product/roadmap.md), a second reading of every
crate for correctness, completeness, design, performance and test quality,
landing its findings one commit at a time.
