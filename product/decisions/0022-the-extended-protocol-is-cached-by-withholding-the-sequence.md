# 0022. The extended protocol is cached by withholding the sequence

Status: accepted

## Context

`M9.7` cached the simple protocol only. `M9.10` then measured what the cache
was worth and found the ceiling was in the workload rather than in the cache:
half the reference workload goes through the extended protocol, and every one
of those statements is a miss. Every mainstream driver uses the extended
protocol for anything with a parameter, so a cache that cannot serve it is a
cache most traffic misses.

`M9.12` gave `pgprox-proto` a way to read a `Bind`'s parameter values, which is
what a key needs. It does not say where the key is built, and the obvious answer
turns out to be wrong in three separate ways.

The obvious answer is to key on the SQL and the parameters at the `Execute`, the
way `M9.7` keys on the SQL at the `Query`, and serve there.

**A hit at the `Execute` has already taken a connection.** A `Parse` or a `Bind`
is forwarded upstream as it arrives, and forwarding acquires. By the time the
`Execute` is decoded the session is holding a pooled connection and has already
paid for it. `M7.56` measured 45% of this proxy's CPU in the upstream pool's
lock, and `M9` was moved up the roadmap because a cache hit is a statement that
never acquires. A hit that acquires anyway saves the database's execution and
none of the proxy's work, which is not the thing being bought.

**The answer to a sequence is not a function of the SQL and the parameters.**
Whether the client is owed a `RowDescription` depends on whether it sent a
`Describe`. Whether the last frame is a `CommandComplete` or a
`PortalSuspended` depends on whether the `Execute` carried a row limit. Two
drivers can ask the same question with different message sequences, so bytes
recorded from one and replayed to the other desynchronise the protocol. That is
worse than a miss by the same distance a wrong answer is worse than a slow one.

**An `Execute` has no terminator.** Its answer ends with a `CommandComplete`
and the `ReadyForQuery` arrives for the `Sync` that follows. Serving the
`Execute` leaves a `Sync` the client is still owed an answer to, on a session
that has a bound portal upstream which nobody is going to execute.

The constraint on any fix is the session future, which is 5,064 bytes against a
5,120 byte ceiling, because one of these exists per connection and 100k of them
is the design point.

## Decision

**An extended-query sequence for an opted-in tenant is withheld from the
upstream until the client ends it, and a hit is assembled from the sequence the
client actually sent.**

Four rules, each carrying one of the problems above.

**Withholding starts only from an idle session.** No connection held, no
transaction open, the tenant opted in, and the statement passing the
cacheability rule. Nothing is withheld from a session in any other state, so a
sequence that is withheld is one whose `ReadyForQuery` is `'I'` by construction.
This is what makes the last rule sound.

**What is cached is the statement's answer and nothing about the sequence.** The
`RowDescription` when the server sent one, the `DataRow`s, and the
`CommandComplete`. Never a `ParseComplete`, a `BindComplete` or a
`ReadyForQuery`, because those are answers to the client's framing rather than
to its question. One payload therefore serves both protocols: an entry a `psql`
session filled answers a JDBC client asking the same thing, and the simple path
synthesises its own `ReadyForQuery` for the same reason the extended path does.

**A hit is assembled frame by frame from what the client sent.** A
`ParseComplete` for its `Parse`, a `BindComplete` for its `Bind`, the cached
`RowDescription` for its `Describe`, the rows and the `CommandComplete` for its
`Execute`, and a `ReadyForQuery('I')` for its `Sync`. The proxy already
synthesises a `ParseComplete` when a pooled connection turns out to hold the
statement a `Parse` names, so this is the same move at a larger scale.

**Anything not covered replays and carries on.** The withheld frames go
upstream in the order they arrived and the sequence proceeds exactly as it does
today. A `Flush` replays, because a client that sends one is waiting for an
answer now. A row limit on the `Execute` replays, because a suspended portal is
not an answer. A `Describe` of a statement rather than a portal replays,
because its answer includes a `ParameterDescription` that is not in the payload.
A miss at the `Sync` replays. So does anything else at all: the machine names
what it can withhold and everything else is a replay, rather than the other way
round.

## Consequences

- A hit costs no pool operation, no upstream connection and no round trip,
  which is the whole point and is what separates this from the alternative
  below.
- Withholding rests on the protocol rule that a frontend must send a `Sync` or
  a `Flush` before it examines the results of an extended-query command. A
  client that expects an answer to a bare `Execute` hangs against this proxy.
  That is the same hazard as the `Flush` deadlock M8 found, and the reason it is
  acceptable here is narrower than the rule: only an opted-in tenant's
  cacheable statements on an idle session are ever withheld, and a `Flush`
  replays.
- A miss copies the sequence's frames once before replaying them. That is a
  memcpy per frame on statements that were already allocating a normalised SQL
  string, and it happens only for tenants that opted in.
- The session future grows by one pointer, the `Option<Box<..>>` holding a
  withheld sequence, on every session whether or not it is used. Every byte of
  the sequence itself is behind that pointer, the way `M9.7`'s recording is.
- The payload changes shape for the simple protocol too, so `M9.7`'s entries
  are not the entries this stores. Nothing has to migrate, since a cache is
  cold on start by definition, but a mixed fleet mid-upgrade holds two shapes
  and each node only reads its own.
- A `Describe` of a statement inside the same sequence as an `Execute` is never
  served. No mainstream driver does that: the ones that describe a statement do
  it in a prepare round trip of its own, which has no `Execute` in it and
  nothing to serve anyway.
- The relay now generates a `ReadyForQuery` of its own, which means it is
  asserting a transaction status rather than relaying one. It is only ever `'I'`
  and only ever on a session that held no connection, and if that invariant is
  ever broken the failure is a client that believes a transaction ended when it
  did not. The invariant is a test rather than a paragraph.

## Alternatives rejected

**Serve at the `Execute` and let the rest of the sequence go upstream.** The
much smaller change: keep forwarding `Parse`, `Bind` and `Describe`, withhold
only the `Execute`, and queue the cached rows just before the terminator the
client is waiting for. It is genuinely appealing, it cannot hang, it needs no
buffering, and it does save the database from executing the query. It loses
because the connection has already been acquired and the round trip has already
been paid for, so it moves work off the database and none off the proxy, and the
proxy's own CPU is what `M7.56` measured and what M9 exists to reduce. Worth
revisiting if the constraint ever turns out to be the database instead.

**Give up the hit when a `Sync` follows.** The status quo, dressed as a
decision: never serve the extended protocol. Zero risk, and it leaves most
driver traffic uncacheable, which is the ceiling `M9.10` recorded.

**Cache the whole byte stream of the sequence's answer and put the sequence's
shape in the key.** No synthesis at all: what is stored is what the client saw,
which is the property that makes the simple path easy to be confident about.
It loses on the key. The shape has to be fingerprinted completely or two clients
with different framing collide, and completeness here is an enumeration of
protocol variations nobody can prove they finished. It also fragments the cache:
two drivers asking one question hold two entries, and neither can use the
other's.

**Answer the `Sync` locally without withholding anything.** The first of the two
options the backlog named. It needs no buffer and no replay path. It loses
because it leaves an upstream connection mid-sequence with a bound portal that
will never be executed, and because the ordering problem remains: the client is
owed a `ParseComplete` and a `BindComplete` from upstream that have not arrived
when the cached rows are ready to send.
