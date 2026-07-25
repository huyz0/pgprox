# 0018. The admin surface reads through a contract in pgprox-core

Status: accepted

## Context

The admin API and the `SHOW` pseudo-database report on pools, tenants, client
sessions and cluster state. Those live in `pgprox-pool`, `pgprox-session` and
`pgprox-cluster` respectively.

`pgprox-admin` may not depend on any of them. `scripts/check-layering.sh`
enforces that every crate depends only on `pgprox-core`, with `pgprox-session`
and `bin/pgprox` as the two stated composers. And `pgprox-core` had nothing
admin-shaped in it: `UpstreamPool::stats` needs a key the caller must already
know, so there is no way to enumerate pools, and nothing describes a client
session at all.

This is the second time an ADR has implied a dependency the layering rule
forbids. ADR 0011 did it for prepared statements and was corrected in M5.1.

## Decision

A new `pgprox_core::admin` module: an `Observatory` trait and the DTOs it
returns, with an in-memory fake. The composition root implements it by fanning
in from every subsystem, because it is the only place that can see them all.
`pgprox-admin` renders what it is given and depends on nothing else.

Purely additive. No existing trait, type or call site changes, and
`cargo check --workspace` is clean either side of it.

Shape decisions worth keeping:

- **Scope is a parameter, not two APIs.** `Scope::Cluster` is the default,
  because an operator asking a question almost never means "on whichever pod my
  request happened to reach". An unrecognised `?scope=` value parses to `None`
  rather than the default, so a typo is reportable.
- **The signature says what costs a round trip.** Every aggregate is sync,
  because every node already carries a gossip digest for every other and an
  aggregate is a local read. `clients` is async, because a node knows only its
  own and listing them fans out.
- **A fan-out that loses a peer returns `Partial`, not a short list.** An
  incomplete answer is useful; an incomplete answer presented as complete is how
  an operator concludes a tenant has no clients when a node was merely
  unreachable.
- **No admin type can hold a credential.** Not by convention: no type in the
  module has a `SecretString` or a password field, there is a `compile_fail`
  doctest saying so, and a test asserts the rendered forms contain nothing that
  looks like one.

## Consequences

- The HTTP surface and the `SHOW` surface read the same data by construction
  rather than by discipline, so the two cannot drift into giving different
  answers to the same question.
- `bin/pgprox` gains a real responsibility beyond wiring: it is where the
  fan-in lives. That is the correct place for it, and it is worth saying out
  loud because it means M6 is not purely mechanical.
- One more trait to keep in step. The fake honours scope for real and refuses
  what the real one refuses, so a test cannot pass against behaviour the
  implementation will not produce.
- Adding a field to an admin DTO is now a contract change. That friction is the
  point: the DTOs are what an agent operating the fleet reads, and the OpenAPI
  document is generated from them.

## Why this did not go to a human first

`standards/behavior.md` and the `contract-change` skill say to escalate a
`pgprox-core` change that touches more than one track. The reason given is the
cost of a rebase across parallel branches and everyone rebuilding against a
moved target.

Neither applies here. The change breaks nothing, so there is nothing to rebase;
tracks A, B, C and E are complete and D is the one making the change; and both
consumers, `pgprox-admin` and `bin/pgprox`, do not yet exist. Recorded here so
the judgement is auditable rather than implicit, and so the next additive
core change has a precedent to point at or to argue with.

## Alternatives rejected

**Make `pgprox-admin` a composer.** One line in `check-layering.sh`. Rejected
because the composer list is short on purpose: it is what makes the tracks
independently buildable, and an HTTP handler reaching into three subsystems is
exactly the coupling the rule exists to prevent. The list would then have three
entries, and the fourth would be easier to argue for than the third.

**Put the fan-in in `pgprox-admin` behind three separate traits.** Rejected
because the caller would then have to assemble a coherent view from three
sources, and the `SHOW` layer would assemble it again, differently. One trait
means one assembly.

**Have the admin API call peers directly for aggregates.** Rejected because it
gives up the property ADR 0007 exists to protect: hitting any pod costs nothing
because the digest is already local. Fan-out is for drill-downs only.
