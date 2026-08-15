---
title: Pre-launch security checklist
description: "Every deployment-time decision docs/security.md and docs/admin.md argue for, as a checklist to work through before a tenant's token reaches a node, rather than three pages to read in full."
---

[Security](security.md) and [Admin and management](admin.md) state every
decision this needs from an operator, as prose. This is the same list,
extracted, so a launch review has something to go through item by item rather
than read three pages and hope nothing was missed. Every item links back to
where it is explained; nothing here is explained twice.

The admin-port item exists because this project got it wrong once — see
[what the admin surface does not
do](admin.md#what-the-admin-surface-does-not-do) for what happened.

## Transport

- [ ] The node was started with `--require-tls`, or
  `--insecure-plaintext-auth` was passed **and the reason is written down
  somewhere that is not this checkbox**. A node started with neither refuses
  to start on its own; this item is about whether the one that overrides that
  refusal was a deliberate choice. See [Configuration](configuration.md#tls).
- [ ] `--tls-cert` and `--tls-key` point at the certificate this deployment
  actually serves, not a self-signed one left over from testing, and
  something is watching for it to expire well before it does. A node refuses
  to start with a certificate outside its validity window, and refuses a
  rotated-in replacement the same way, leaving the previous one serving — but
  that previous one can itself run out while nothing rewrites the files on
  disk, and only a rewrite is checked. See [Admin and
  management](admin.md#changing-a-nodes-state) for the rotation mechanism.
- [ ] `--upstream-ca` is set if any backend a grant may resolve to asks for a
  verified connection. Without it the root store is empty and a verified
  backend fails to connect rather than connecting unverified — the safe
  direction, but worth knowing before it happens at launch. See
  [Configuration](configuration.md#tls).

## The sidecar and what it hands back

- [ ] The sidecar validates the JWT signature itself. pgprox checks the
  header's algorithm against an allowlist as defence in depth and verifies no
  signature — two validators that disagree about validity is a vulnerability,
  not redundancy, so there is exactly one that matters and it is not this
  process. See [Security](security.md#authenticating-a-client).
- [ ] `grant_ttl_cap` in `config.yaml` reflects how fast a revoked token
  should stop working, not the sidecar's default TTL. The cache clamps to the
  shortest of the sidecar's TTL, the token's own `exp`, and this cap — a
  generous sidecar answer cannot outlive what this says. See
  [Configuration](configuration.md#top-level).
- [ ] The sidecar's socket (`--sidecar`, default
  `/var/run/pgprox/sidecar.sock`) is not reachable by anything except this
  node. It hands back real backend passwords on every resolve.

## The static admin credential, if one is configured

- [ ] `--admin-user` is set only if this deployment actually needs SCRAM
  access for migrations, monitoring or a human operator — not as a default.
- [ ] Its password reaches the node through `PGPROX_ADMIN_PASSWORD`, never on
  the command line. Every process on the host can read `/proc/*/cmdline`. See
  [Security](security.md#operators).
- [ ] Whoever holds that password understands it reaches no tenant database —
  it authenticates against the node and gets the `SHOW` surface only. If a
  review is worried about it reaching tenant data, the answer is that it
  structurally cannot, which is worth confirming rather than assuming.

## The admin port

- [ ] The admin port (`--admin`, default `0.0.0.0:9090`) is not on the same
  network address a tenant application is given. The Helm chart's
  `adminService.enabled` is off by default for this reason; leave it off
  unless something specific needs an external address for it.
- [ ] A NetworkPolicy (or equivalent) restricts who can reach the admin port,
  not just the absence of an external Service. Pod IPs are routable whatever
  Kubernetes Services exist — not creating a Service is necessary and not
  sufficient. See [Admin and management](admin.md#what-the-admin-surface-does-not-do).
- [ ] Whoever can reach `POST /v1/drain` and `POST /v1/pools/.../reset`
  understands those are operational controls, not read-only endpoints. The
  two halves of the API are separate routers specifically so a deployment can
  expose reads without writes if that split matters here.

## Logging

- [ ] Query text logging is off (the default) unless a specific tenant has
  opted in and a specific incident needs it. It routinely carries customer
  data in literals, and turning up the log level fleet-wide during an
  incident does not turn this on as a side effect — it needs the tenant's own
  opt-in as well. See [Security](security.md#credentials-never-reach-a-log).
- [ ] Whatever aggregates this node's logs is not itself a place a credential
  could leak through — the proxy's own guarantee is that a password or token
  never reaches a log line it writes; a log shipper that re-serializes
  structured fields into something less careful is outside that guarantee.

## What this checklist is not

This is deployment configuration, not a substitute for reading [what pgprox
is not](security.md#what-this-is-not): it is not a firewall, not an
authorization layer, not a rate limiter, and not a boundary your credentials
do not already have. Two tenants sharing a database role are one security
domain to Postgres whatever this checklist says, and a review assuming
otherwise should stop and read that section before launch.
