# 0013. FunctionCall is relayed, never parsed, and always routes to the primary

Status: accepted

## Context

The fast-path `FunctionCall` message (`F`) invokes a function by OID without any
SQL text, and `FunctionCallResponse` (`V`) answers it. It predates the extended
query protocol and modern drivers rarely emit it, but libpq still exposes it
through `PQfn` and it remains part of protocol 3.0, so a proxy will eventually
see one.

The question is not whether to parse the payload. It is what a proxy does with a
statement it cannot classify.

The references split. pgdog models the message but keeps its body opaque.
pgbouncer does not special-case it at all, relaying it through the generic path.

## Decision

The payload is never parsed. `FunctionCall` and `FunctionCallResponse` are
relayed as opaque bodies, exactly like `DataRow`.

Both get a `Tag` constant and `FunctionCall` gets a `FrontendMessage` variant
with no payload field, so the session layer can see that one happened without
anything being able to read it.

A `FunctionCall` is always treated as unclassifiable, which means the primary.
It has no SQL text for the statement classifier to read, so there is nothing to
decide from, and `StmtClass::Unknown` already routes to the primary.

It also participates in the extended-sequence rule the same way a simple query
does: the connection is held until the `ReadyForQuery` that answers it.

## Consequences

- No parsing cost, and no OID-to-behaviour table to keep in step with Postgres.
- A tenant using `PQfn` gets correct results, at the cost of never reaching a
  replica. That is the safe direction and matches ADR 0009's rule: when the
  classifier cannot prove a statement is read-only, it goes to the primary.
- The function could be read-only in fact, so this costs some replica
  utilisation. Accepted, because the alternative is an OID allowlist that would
  need updating for every extension a tenant installs.
- Because the payload is opaque, a `FunctionCall` cannot pin a session the way
  `LISTEN` does even if the function called has session-scoped effects. A
  function that creates a temp table through the fast path would leak state
  across a pool switch. This is a known limitation rather than a solved problem,
  and it is why M5 should pin on `FunctionCall` if measurement shows real use.

## Alternatives rejected

**Parse the payload and classify by function OID.** Would allow replica routing
for known-safe functions. Rejected because the OID space is not fixed: every
extension adds functions, and an allowlist that lags a tenant's installation is
a correctness bug rather than a missed optimisation.

**Refuse `FunctionCall` outright.** Simplest, and it is what a proxy that has
never seen one might do by accident. Rejected because it breaks a working
application with an error rather than a slowdown, and the tenant cannot always
change their client.

**Ignore it entirely, relying on the opaque default.** This is where the code
already was. Rejected because a message with no `Tag` constant and no variant is
indistinguishable from an unknown one, so nothing can route it deliberately and
the routing decision above could not be expressed at all.
