# Mission

pgprox sits between a very large fleet of Postgres clients and a small number of
upstream Postgres servers, each hosting up to 5,000 tenant databases with
distinct roles and passwords.

Clients authenticate with a JWT rather than a database password. An external
sidecar validates the token and returns the real host, database, user, and
password to connect with.

## The problem it solves

Downstream connections are cheap and numerous. Upstream connections are scarce
and capped. A single tenant's application fleet can open thousands of
connections; the Postgres server behind it has room for a few thousand across
every tenant it hosts.

pgprox absorbs that ratio through transaction-level multiplexing, and it holds
the upstream cap even though it runs as several independent pods that each keep
their own pools. Holding a global cap from processes that do not share memory is
the hard part of the design, and it is why there is a gossip and lease layer at
all.

## Who uses it

- **Tenant applications**, through any standard Postgres driver with no code
  changes. If a driver needs to know pgprox is there, something is wrong.
- **Platform operators**, human, through the admin API and `SHOW` commands.
- **Agents**, through the same machine-readable admin API. The surface was
  designed so an LLM can diagnose and operate the fleet without scraping text.

## What it must never do

- **Exceed an upstream connection cap.** Everything else degrades gracefully.
  This does not. Breaching the cap can lock out the operator and take the
  database down for every tenant on it.
- **Leak a credential.** The proxy holds credentials for every tenant database
  on the fleet. One leaked log line is a fleet-wide incident.
- **Serve a stale read to a session that expects read-your-writes.** Replica
  routing is an optimization, and a wrong answer is worse than a slow one.
- **Drop a connection that is mid-transaction** for any reason under its own
  control. Draining, rebalancing, and shedding all wait for a transaction
  boundary.
- **Take down a node because one client sent bad bytes.** A single malformed
  frame must not affect the other 100,000 connections on that pod.

## What good looks like

A tenant cannot tell pgprox is in the path except that their connections are
faster to establish and never rejected for lack of upstream capacity. An
operator upgrades a node during business hours without anyone noticing. The
upstream connection count sits flat under a load spike that doubles client
connections.
