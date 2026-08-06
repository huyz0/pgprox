# pgprox

The proxy binary, and the one place concrete types meet.

Everything else in this workspace is written against a trait and tested against
a fake. Here the fakes are replaced, once, and the result is handed to a run
loop. That is the whole job.

## Why the wiring is a library

`main.rs` is the only file excluded from coverage, so anything in it is
untested by construction.

Keeping it to argument parsing and one call means the exclusion buys nothing.
The wiring in `lib.rs` and below is called by tests with fakes in place of
sockets, and `scripts/m6-complete.sh` fails if `main.rs` grows past a handful
of lines.

## What only exists here

Several things cannot live in a library crate and are worth knowing about:

`entropy` is the system randomness source. A cancel key is a bearer token, and
`pgprox-session` defines the trait but cannot choose a random number generator
on everyone's behalf.

`observatory` is the fan-in across pools, sessions and cluster state that
`pgprox-admin` reads through. It happens once, here, rather than in every
handler.

`gossip` is the transport. `pgprox-cluster` decides; this sends and receives.

`dial` opens upstream connections, which is the I/O half of a pool that
deliberately opens no sockets.

`fakepg` is a Postgres stand-in for tests that need a server to answer without
a container.

## Where it sits

One of two crates permitted to compose others, the other being
`pgprox-session`. It depends on all twelve library crates:

`pgprox-core` for the traits it satisfies with real implementations.
`pgprox-proto`, `pgprox-pool`, `pgprox-route` and `pgprox-session` for the
connection path. `pgprox-auth` for grants, `pgprox-cluster` for the cap,
`pgprox-cache` for cached answers, `pgprox-tls` for both TLS configurations,
`pgprox-config` for the document, `pgprox-observe` for metrics and health, and
`pgprox-admin` for the two operator surfaces.

`pgprox-load` is a dev dependency only, so the gossip allocation budget can
measure at the membership the reference workload declares.

## Running it

```bash
scripts/e2e.sh
```

Brings up three nodes, a primary, two replicas and a mock token service, then
asserts the properties the stack is meant to have.
[Getting started](../../docs/getting-started.md) is the longer version, and
[Configuration](../../docs/configuration.md) documents every flag this binary
takes.
