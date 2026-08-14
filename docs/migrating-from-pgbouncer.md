---
title: Migrating from PgBouncer
description: "What in pgbouncer.ini has a pgprox equivalent, what does not, and what to check before pointing an existing pgbouncer deployment's traffic at pgprox instead."
---

pgbouncer solves connection pooling for one database with one credential
model. pgprox solves the same problem for a fleet where credentials differ per
tenant, arrive as a JWT, and a connection cap has to hold across several proxy
nodes that share no memory. If that is not your situation — one database, a
handful of static roles — pgbouncer is very likely still the right answer;
see [Performance](performance.md#against-other-poolers) for what
that costs you either way.

If it is your situation, this is what changes.

## The dashboard mostly does not

`SHOW POOLS`, `SHOW SERVERS`, `SHOW CLIENTS`, `SHOW STATS` and `SHOW CONFIG`
keep pgbouncer's column names and column order on purpose, so a dashboard
built against pgbouncer reads them unchanged once it is pointed at pgprox's
client port instead. `SHOW QUOTA`, `SHOW PEERS`, `SHOW TENANTS` and
`SHOW CACHE` are pgprox-only and have nothing to migrate — see [Admin and
management](admin.md#show-on-the-client-port) for all nine.

Two differences worth checking your dashboard for even though the columns
match: `SHOW SERVERS` is one row per upstream **connection**, and if your
dashboard sums it across nodes for a fleet-wide number, pgprox already gives
you that fleet-wide number on any node — `SHOW QUOTA` — so summing `SHOW
SERVERS` across nodes yourself now double-counts nothing pgbouncer had to
worry about with one process. And an unrecognised `SHOW` is an error naming
what it could have been, not an empty result — if your tooling treats an empty
result and an error the same way, it will not any more.

## What has no equivalent: the credential model

This is the one that is not a mapping, because pgbouncer and pgprox answer a
different question. pgbouncer's `auth_type` authenticates against a
`userlist.txt` or an `auth_query` you point at your own database — a static
list of roles and password hashes pgbouncer already knows. pgprox has no
static list. Every non-static connection presents a JWT, and pgprox asks a
sidecar you write what that token means: which tenant, which backend
role and database, which password. There is no `userlist.txt` file to convert;
there is a service to build. See [Going to production](going-to-production.md)
for the two RPCs it implements.

The one pgbouncer credential shape pgprox keeps is a **static role**
authenticated with SCRAM — `--admin-user`, for a migration, a monitoring
job or a human with `psql`, the same shape as pgbouncer's `admin_users` /
`stats_users`. It reaches no database; it gets the `SHOW` surface. If your
current deployment has application roles going through pgbouncer's static
`userlist.txt` rather than a per-request credential, every one of those has to
become either a JWT-issuing path through your sidecar or stay a static user
that authenticates but never touches tenant data.

## Settings with a direct equivalent

| `pgbouncer.ini` | pgprox | Notes |
| --- | --- | --- |
| `pool_mode = transaction` | Default, and the only mode most tenants want | pgprox has no `pool_mode` setting; transaction pooling is the design, not a choice, with automatic pinning to session behaviour for the features that need it. See [Features and limits](features.md#when-a-session-pins). |
| `pool_mode = session` | A tenant's grant sets `pool.mode = SESSION` | Per-tenant, not per-database: the sidecar's `Resolve` answer carries it, not a config file. |
| `max_client_conn` | `max_client_conns` in `config.yaml` | Per node, not per fleet: a three-node fleet at `max_client_conns: 10000` can hold 30,000 clients total. |
| `default_pool_size` | `servers[].max_connections`, divided by `guaranteed_fraction` | pgbouncer's is per pgbouncer process; pgprox's is the fleet's total for that server, divided across nodes by a rule rather than a fixed per-node number. See [Configuration](configuration.md#servers). |
| `server_idle_timeout` | Not yet exposed as a per-server setting | pgprox reaps idle upstream connections on its own tick; there is no per-server override today. |
| `listen_addr` / `listen_port` | `--listen` | Defaults to `0.0.0.0:6432`, the same non-conflicting port pgbouncer conventionally uses. |
| `admin_users` / `stats_users` | `--admin-user`, one name, password from `PGPROX_ADMIN_PASSWORD` | pgbouncer allows a list; pgprox takes one static admin identity per node today. |
| `client_tls_sslmode` | `--require-tls`, `--tls-cert`, `--tls-key` | See the next section — this one changed meaningfully. |
| `server_tls_sslmode` | The grant's `tls` field, plus `--upstream-ca` | Set per backend by your sidecar, not by a pgprox config file, because the backend a token resolves to can differ per tenant. |

## TLS: check this one, do not assume it

pgbouncer's TLS is opt-in per direction and pgprox's frontend TLS decision
changed recently: a node started with `--require-tls` off and no
`--insecure-plaintext-auth` now refuses to start rather than accepting JWT
logins in the clear. If your pgbouncer deployment ran `client_tls_sslmode =
disable` because the network path was already trusted, carrying that
assumption straight to pgprox means passing `--insecure-plaintext-auth`
explicitly rather than getting the old pgbouncer behaviour by doing nothing.
See [Configuration](configuration.md#tls) for why, and check whether that
assumption should really carry over before you make it — a JWT in the
password field is not the same risk as a pgbouncer static password was on
that same trusted network.

## What to check before cutting traffic over

- **Every application role's connection string still works unchanged** —
  pgprox speaks the wire protocol, so a driver needs no changes, but the
  *password* field's meaning changed from a static credential to a bearer
  token. Confirm your application fleet is issuing JWTs, not the old
  passwords, before the cutover, not after.
- **Anything relying on `LISTEN`/`NOTIFY`, session-scoped advisory locks, temp
  tables, `WITH HOLD` cursors or SQL-level `PREPARE`** still works the same
  way functionally — pgprox pins the session rather than refusing the
  feature — but a pin is expensive and counted; see
  [Operations](operations.md#reading-a-rising-pin-rate) for what a rising pin
  rate after cutover is telling you.
- **Your upstream connection cap**, `servers[].max_connections`, is the
  fleet's total across every pgprox node, not one process's — do not just
  copy `default_pool_size` from a single-process pgbouncer deployment without
  accounting for how many pgprox nodes will be dividing it.
- **The admin port is not the client port.** pgbouncer's console database and
  its client-facing pooling share one listener; pgprox splits them, and the
  HTTP admin API on `--admin` has no authentication of its own. See [Admin and
  management](admin.md#what-the-admin-surface-does-not-do) before you expose
  it the way you exposed pgbouncer's console.
