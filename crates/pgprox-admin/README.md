# pgprox-admin

Two operator surfaces over one set of data: an HTTP/JSON API, and the `SHOW`
pseudo-database a client can query on the proxy's own port.

An operator with psql open reads `SHOW POOLS`. A script, a dashboard or an
agent reads `/v1/pools`. Neither scrapes the other's format, and neither can be
more current, because both read the same `Observatory`. See
[ADR 0018](../../docs/internal/product/decisions/0018-admin-reads-through-a-core-contract.md).

## Any pod answers for the fleet

Aggregates come from the node's own gossip digest, which already holds the
fleet's numbers, so there is no wrong pod to ask and no cost to asking one that
is not the leader. `?scope=local` and `SHOW LOCAL ...` narrow a read to the
node that answered.

Only drill-downs fan out to peers, and those are the ones that can come back
incomplete. A fan-out that lost a peer answers **206** with the rows it did
gather. An operator seeing 200 concludes the tenant has no clients; one seeing
500 concludes the proxy is broken. Neither is true, and the difference decides
what they do next.

## The route list is the router

Paths and handlers come from one declaration, so a path cannot be served
without appearing in the list, and the OpenAPI document is compared against
that list rather than against a hand-written copy.

Reads and writes are separate routers on purpose. Reading pool depths and
draining a node are not the same privilege, and a deployment that wants to
expose one without the other can.

## No response carries a credential

That holds structurally rather than by review: the `Observatory` types have no
field for one, so a handler cannot leak what it was never given.

Upstream hostnames are a different question. They appear in pool keys because
an operator debugging a pool needs them, which is one of the reasons the admin
port is not for tenants.

## Where it sits

Depends on `pgprox-core` and nothing else, like every other crate here. The
fan-in across pools, sessions and cluster state happens once, in `bin/pgprox`,
rather than in every handler.

## Reading it

`api` is the HTTP surface. `show` parses the `SHOW` grammar. `rows` renders
either one from the same data. `openapi` generates the document and is tested
against what the router actually serves.
