---
title: Getting started
description: "Bring the stack up on your machine, send a query through the proxy, and watch it pool."
---

By the end of this you will have three pgprox nodes, a Postgres primary, two
replicas and a mock token service running locally, and you will have sent a
query through the proxy and watched it multiplex.

You need Docker with Compose, and Rust 1.94 or later.

## Bring the stack up

```bash
git clone https://github.com/pgprox/pgprox.git
cd pgprox
scripts/e2e.sh up
```

The first run builds the image and takes a few minutes. When it finishes, six
containers are running and the proxy is listening on port 16432.

## Send it a query

The proxy expects a JWT in the password field. The mock token service accepts
any well-formed token and routes every tenant to the same database, so a
throwaway token works:

```bash
TOKEN=$(printf '%s' '{"alg":"RS256","typ":"JWT"}' | base64 -w0 | tr '+/' '-_' | tr -d '=')
TOKEN="$TOKEN.$(printf '%s' '{"sub":"acme"}' | base64 -w0 | tr '+/' '-_' | tr -d '=').sig"

PGPASSWORD="$TOKEN" psql -h 127.0.0.1 -p 16432 -U acme_app -d tenant_acme -c 'SELECT 1'
```

That is a real Postgres session through the proxy. Your driver needs no changes
and does not know pgprox is there.

## Watch it pool

Open twenty connections and ask the proxy what it is holding upstream:

```bash
for i in $(seq 20); do
  PGPASSWORD="$TOKEN" psql -h 127.0.0.1 -p 16432 -U acme_app -d tenant_acme \
    -c 'SELECT pg_sleep(2)' &
done

curl -s http://127.0.0.1:19090/metrics | grep -E 'pgprox_(client|upstream)_conns'
```

Client connections will show twenty. Upstream connections will show fewer,
because a connection is borrowed for a transaction and returned at its boundary
rather than held for the session. That ratio is the whole point of the proxy.

## Ask a node what it is doing

pgprox answers `SHOW` commands on the same port, in the shape pgbouncer does:

```bash
PGPASSWORD="$TOKEN" psql -h 127.0.0.1 -p 16432 -U acme_app -d tenant_acme \
  -c 'SHOW POOLS' -c 'SHOW CLIENTS' -c 'SHOW QUOTA'
```

`SHOW QUOTA` is pgprox's own and has no pgbouncer equivalent. It shows how the
upstream cap is divided across the three nodes, which is the part of the design
a single-node pooler has no need for.

## Tear it down

```bash
scripts/e2e.sh down
```

## What to read next

[Configuration](configuration.md) for the settings behind the stack you just
ran. The document those nodes read is `deploy/config/config.yaml`.

[Architecture](architecture.md) for why a connection gets returned at a
transaction boundary and what stops that breaking `LISTEN`, temp tables and
prepared statements.

## If it did not work

`scripts/e2e.sh` names the component that failed and prints what it last said,
rather than exiting with a status. Run it without arguments to get the full
assertion suite and a diagnosis.

The most common local failure is a port already in use. The stack publishes
16432, 16433, 16434 for the proxy nodes and 19090 to 19092 for their admin
ports.
