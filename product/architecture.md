# Architecture

Full design detail is in [plan.md](plan.md). This file is the crate map and the
rules that keep parallel development from colliding.

## Dependency rule

Every crate depends on `pgprox-core` and on nothing else in the workspace.

Two stated exceptions:

- `pgprox-session` composes `pgprox-proto`, `pgprox-pool`, and `pgprox-route`
- `bin/pgprox` composes everything

`pgprox-core` depends on no workspace crate and performs no I/O.

This is what lets five tracks run in parallel: a track codes against traits and
fakes, never against another track's half-finished crate. It is checked in CI,
not left to review.

## Crates

| Crate | Owns | Track |
| --- | --- | --- |
| `pgprox-core` | Traits, DTOs, errors, IDs, `SecretString`, `Clock`, buffer slab, fakes | M0 |
| `pgprox-proto` | Postgres wire codec, both directions, frame-level passthrough | A |
| `pgprox-tls` | rustls setup, FIPS feature gate, cert hot-reload | A |
| `pgprox-auth` | JWT extraction, sidecar gRPC client, grant cache | B |
| `pgprox-cluster` | SWIM gossip, membership, quota leases, tenant reservations | C |
| `pgprox-config` | `ConfigSource` providers, validation, hot reload | D |
| `pgprox-observe` | Metrics, tracing, log init, health | D |
| `pgprox-admin` | HTTP/JSON API and `SHOW` pseudo-database | D |
| `pgprox-pool` | Upstream pools, idle reap, pinning, prepared-statement mapping | E |
| `pgprox-route` | Target selection, statement classification, LSN watermarks | E |
| `pgprox-session` | Per-client state machine, relay loop | M6 |
| `pgprox-cache` | Query cache, trait stub until M9 | M9 |
| `bin/pgprox` | Composition root. Five lines in `main.rs`, logic in a lib target. | M6 |
| `pgprox-load` | The reference workload, its sampler, and the run report. No I/O. | M7 |
| `pgprox-testkit` | Test scaffolding: container readiness classification. Never a runtime dependency. | M1F |

## Layering inside a crate

Business logic is sans-I/O: a pure function of state and input events, with no
socket, clock, or syscall. The I/O shell that wraps it is generic over
`AsyncRead + AsyncWrite + Unpin`.

This is not a style preference. It is what makes 95% coverage reachable in a
two-minute test run, and it is what makes concurrency bugs findable by
deterministic test rather than by load. See
[../standards/async-concurrency.md](../standards/async-concurrency.md).

## The parts that are easy to get wrong

**Transaction boundaries.** The authoritative signal for releasing an upstream
connection is the transaction status byte in `ReadyForQuery`. Not the SQL text,
not a heuristic. Release only on `I`, with no extended-query sequence
outstanding, and only when unpinned.

**Prepared statements.** Every modern driver uses named `Parse`. Without
per-connection statement mapping the pool pins nearly every session and
transaction pooling collapses into session pooling. This is MVP scope, not an
optimization.

**Cancellation.** The proxy issues its own `BackendKeyData` with the node ID
encoded in it, so a `CancelRequest` landing on any pod can be forwarded to the
owner. Without this, cancellation silently breaks as soon as there is a second
pod.

**Quota.** Guaranteed share plus outstanding leases must never exceed the cap,
under partition, leader loss, and simultaneous restart. This is a property test
over a deterministic simulation, because it is the class of bug that never
reproduces in staging.

**Fan-out.** A tenant's clients can land on all five pods, so a tenant can have
up to five upstream pools. That is an efficiency cost, never a safety one: the
quota layer holds the cap regardless. Home-node reservations reduce the cost
without moving any client.
