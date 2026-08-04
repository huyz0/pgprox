# 0024. A cache key names the connection that would have answered

Status: accepted

Amends ADR [0021](0021-the-query-cache-promises-bounded-staleness.md), which
this does not weaken: bounded staleness is still the whole promise, and this is
about which question an entry is the answer to.

## Context

`CacheKey` was tenant, normalized SQL, bound parameters and `search_path`. The
type's own documentation says "Every field is part of the key. Dropping one is
how a cache starts returning another tenant's data", and `search_path` is there
because ADR 0021 worked out that identical SQL under two paths names two
different sets of tables.

The same argument reaches two more fields and was not carried that far.

A tenant is not a database and is not a role. A grant resolves per token and
per **startup database**, and what it resolves to is a
`Backend { server, database, user }`. `PoolKey` is built from all three,
because a connection to one is not a connection to another. So within a single
tenant:

| Two sessions differing in | Same SQL, same `search_path`, answers |
| --- | --- |
| the database the grant resolved to | different tables entirely |
| the role the grant resolved to | different rows, under row-level security |
| nothing | the same |

The first is not exotic. It is what a tenant with an application database and a
reporting database looks like, and the proxy's own pool layer already models it.
The second is what row-level security and column privileges are for: `SELECT *
FROM orders` under `acme_app` and under `acme_readonly` are two questions, and
an entry recorded for one served to the other publishes rows that role cannot
see.

Both were sharing one entry.

## Decision

`CacheKey` carries `database` and `user`, filled from the resolved grant.

**From the grant, not from the startup packet.** The startup database is what
the client asked for; the grant's is what the sidecar resolved it to, and that
is where the rows actually came from. The two are not required to match, and
keying on the request rather than the answer would put the mapping's own
behaviour inside the key.

**Not the server.** A tenant's primary and its replicas hold the same data by
construction, and ADR 0009 already gates whether a replica may answer at all.
Adding the server would split every entry across the set of hosts that can
legitimately produce it, which costs hit rate and buys nothing: two hosts
serving the same database as the same role are answering the same question.

The pair is exactly what `PoolKey` carries minus the server, which is the same
observation from the other side: an entry is the answer a particular kind of
connection would have given, and these are the fields that decide which kind.

## Alternatives rejected

**Key on `PoolKey` itself.** Tempting, and wrong twice. It carries the server,
which splits entries for the reason above, and it does not carry the tenant,
which is the field that stops the worst crossing of all. The overlap is a
coincidence of what identifies a connection, not a shared definition.

**Derive them from the tenant.** Only correct if a tenant were one database and
one role, and the pool layer already assumes it is not. An assumption that two
layers disagree about is the shape of every finding in M24.

**Invalidate on role change instead.** There is no event to invalidate on: a
second session simply arrives with a different grant, and nothing about it says
the first one existed.

**Leave it and document the limit.** ADR 0021 is explicit that the cache
promises bounded staleness, which is a promise about *age*. Serving another
role's rows is not stale data, it is the wrong data, and no TTL bounds it. It
is not the kind of thing a document can make acceptable.

## Consequences

Two `Arc<str>` per key, 32 bytes, on a structure already carrying three. The
byte budget accounts for key size, so the entry count at a given budget falls by
that much, which at any realistic result size is not measurable.

Entries no longer cross between a tenant's databases or roles, which for a
tenant using one of each changes nothing at all, and for a tenant using two is
the difference between a hit rate and a defect.

`M24.4` fills the fields at the one construction site that has a grant. Two
tests in `pgprox-cache` prove the fields are part of the key, and one in
`bin/pgprox` proves they arrive from the grant rather than being invented.
