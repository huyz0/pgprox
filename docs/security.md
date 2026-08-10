---
title: Security
description: "The threat model, how clients and operators authenticate, what a grant authorizes, and how credentials are kept out of logs."
---

Two properties define the threat model, and everything here follows from them.
The proxy holds credentials for every tenant database on the fleet, and it
parses bytes sent by anyone who can reach the listener.

That makes two failures worse than the rest: a leaked credential, which is a
fleet-wide incident from one log line, and a node taken down by malformed
input, which takes 100,000 unrelated connections with it.

## Authenticating a client

Tenants authenticate with a JWT in the password field. Any standard Postgres
driver can send one, which is what lets a tenant use a token without the driver
knowing anything about it.

The token travels in the clear inside the connection, so **TLS is required
whenever JWT authentication is in use**. A client that skips `SSLRequest` while
`require_tls` is set gets an `ErrorResponse` explaining why, not a silent
downgrade to plaintext.

Before the token goes anywhere, the proxy checks the header's algorithm against
an allowlist:

```
RS256  RS384  RS512  PS256  ES256  ES384
```

Asymmetric only. `none` is rejected, and so is the whole `HS*` family, because
an HMAC verification key is also a signing key: anything that can check an `HS*`
token can mint one. This costs one base64 decode and is defence in depth against
a token service that would have accepted something it should not.

Validation itself belongs to your token service. The proxy does not implement a
second validator, because two validators that disagree about whether a token is
valid is a vulnerability rather than redundancy.

### Operators

A static role authenticates with **SCRAM-SHA-256** instead, for migrations,
monitoring and a human with psql, none of which carry tokens.

A static user reaches no database. It authenticates against the node and gets
the `SHOW` surface, which reads the same data as the HTTP admin API. That is the
security position, not an implementation detail: an operator credential that
could reach a tenant's data would be a way around the entire token path.

Three smaller decisions on that path:

- The password comes from `PGPROX_ADMIN_PASSWORD`, not from a command line.
  Every process on the host can read `/proc/*/cmdline`.
- Only the derived SCRAM keys are kept. The password itself is not held after
  startup, and what is stored is what Postgres would store.
- A user that does not exist gets a fixed salt and a full exchange that fails at
  the end. Varying the salt, or failing early, answers "does this account
  exist?" for anyone who asks.

**`md5` is not supported.** Postgres deprecated it in 14.
**`SCRAM-SHA-256-PLUS` is not supported** either: channel binding would tie the
exchange to the proxy's TLS session rather than the database's, which states a
guarantee that is not being made.

## What a grant authorizes

The token service returns a grant, and that grant is the authorization decision.
It names the tenant, the primary and replicas, the role and password to connect
with, and the pool policy. The proxy enforces it and decides nothing about it.

Authorization inside the database stays with Postgres. The proxy does not
inspect statements for permission, does not rewrite them for row-level security,
and does not add predicates. It connects as the role the grant names, and that
role's privileges are what apply.

**Cached grants expire on the shortest clock available**:
`min(grant TTL, exp - now, configured cap)`. A revoked or expired token must not
keep working because a cache had a longer opinion than the token did. Refusals
are cached too, for a much shorter time, since a refusal can be reversed by
something outside this process and a long negative TTL makes that fix look
broken.

## Credentials never reach a log

Every password and token is wrapped in a type that redacts in both `Debug` and
`Display` and zeroes on drop. It has no `Deref`, no `AsRef<str>`, no `PartialEq`
and no conversion back to a string, so there is exactly one route to a real
value: an `expose()` call, which is greppable and is a review item at every site.

Three things hold that claim up.

**The type.** The struct carrying backend credentials has no derived `Debug`; the
hand-written one prints host, database and user, and never the password.

**A static check.** `scripts/check-secrets.sh` fails the build if the result of
`expose()` reaches a formatting macro. It does not prove a credential never
reaches a log, and says so: a value exposed into a local and formatted three
functions later is not caught. It closes the one shape the type system leaves
open.

**The end-to-end version.** `scripts/e2e.sh` brings the stack up, authenticates
with a real token against a backend with a real password, and greps every node's
logs for both. It runs a positive control first, because a search that finds
nothing is worth nothing until you have seen it find something.

Nothing writes a credential to disk. Not a temp file, not a debug dump, not a
core file, and core dumps are off in the deployment.

Query text gets the same care for a different reason: it routinely carries
customer data in literals. Recording it needs the log level turned up **and**
the tenant opted in, two separate switches, so that raising log levels
fleet-wide during an incident does not start capturing everyone's data as a side
effect.

## Untrusted input

Everything a client sends is untrusted, including the length prefix on a frame.
The classic way to lose is to trust a declared length and allocate it.

- **Lengths are checked before anything is allocated.** A client claiming a 2 GB
  message gets an error. The startup packet is held to 32 KiB rather than the
  gigabyte a `DataRow` may legitimately need, so an unauthenticated peer cannot
  make a node grow a buffer to whatever it felt like sending.
- **No panic on any path reachable from client bytes.** A malformed frame must
  not take down a node serving 100,000 other connections, which is why the
  decoder is fuzzed rather than only unit tested.
- **No unsafe code in the crates that read what a peer chose.**
  `pgprox-proto`, `pgprox-core`, `pgprox-route`, `pgprox-auth` and `pgprox-tls`
  each carry `#![forbid(unsafe_code)]` in their own source, where neither the
  workspace lint configuration nor an `#[allow]` can reach it. The failure mode
  of a decoder bug is a wrong answer, never memory corruption. Elsewhere unsafe
  is a governed exception with conditions and a script that enforces them.
- **Who chooses a map key decides its hasher.** A key a peer chooses keeps the
  seeded default hasher, which is what stops a client sending a thousand keys
  that land in one bucket and turning every lookup into a scan. Only keys this
  process hands out get the cheap unseeded one. A hash of peer input is still
  peer input.
- **The statement classifier resolves ambiguity toward the primary.** Guessing
  read-only on a statement it cannot place would be both a correctness bug and a
  freshness bug, so it never guesses.

## Transport

Client TLS is configured with a certificate and key on disk, watched for
rotation. Upstream TLS is asked for the way Postgres expects to be asked, with
an `SSLRequest` before any TLS record, and the chain is verified against a
configured CA. A server that answers "no TLS here" gets no further conversation,
because continuing would put that tenant's backend password on a plaintext
socket. There is no option to skip verification, not even behind a flag. Such a
flag always ends up set in production.

For regulated deployments, [FIPS builds](fips.md) swap in a validated crypto
module and assert at startup that it took.

## Cancel keys

A `CancelRequest` arrives on a fresh connection carrying nothing but a key, and
the protocol gives it 32 bits and no authentication. It is a bearer token:
whoever holds it can cancel that query.

So the secret half of a connection id comes from a CSPRNG rather than a counter.
With a counter, adding one to your own key gives you your neighbour's, and
"cancel your own query" quietly becomes "cancel anyone's". If the system entropy
source fails persistently, the connection is refused with an internal error
rather than handed a guessable key.

The mapping from key to query exists only between acquiring an upstream
connection and releasing it. Outside that window a cancel resolves to nothing
and is refused, because the connection a client was using a moment ago may
belong to someone else now, and cancelling their query is worse than cancelling
nothing.

## Supply chain

`cargo deny` runs in CI with an explicit license allowlist and a source
allowlist pinned to crates.io, so a dependency cannot quietly start pulling from
a git URL. `cargo audit` runs against the RustSec database. `gitleaks` runs in
the pre-commit hook, before a secret can reach history where removing it means a
rewrite.

One honest gap: the documentation site brings a Node toolchain and its own
lockfile, and `cargo deny` does not see any of it. That was accepted
deliberately rather than overlooked, and it is a real widening of the dependency
surface for a site that ships no code into the proxy.

## What this is not

- **Not a firewall or a WAF.** Statements are classified for routing, not
  inspected for attacks. There is no SQL injection detection and none is
  planned; that is the application's problem and Postgres's privileges are the
  backstop.
- **Not an authorization layer.** See above. Roles do that.
- **Not a rate limiter.** The upstream cap bounds concurrency. It does not bound
  how much work a tenant asks each connection to do.
- **Not a boundary your credentials do not have.** Two tenants sharing a
  database role are one security domain to Postgres, and the proxy cannot make
  them two. See [Multitenancy](multitenancy.md#where-the-boundary-actually-is).
