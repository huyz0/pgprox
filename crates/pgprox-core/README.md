# pgprox-core

Every other crate in this workspace depends on this one. This one depends on
none of them, and performs no I/O.

It holds what crosses a boundary: the traits, the types passed through them,
the error types, the ID newtypes, and a working in-memory fake for every trait.

That constraint is why several people can work on this at once. A crate is
written against a trait and tested against the fake, never against another
crate's half-finished implementation.

## The contracts

| Trait | Implemented by | Fake |
| --- | --- | --- |
| `Clock` | this crate | `clock::FakeClock` |
| `CredentialResolver` | `pgprox-auth` | `auth::FakeCredentialResolver` |
| `UpstreamPool` | `pgprox-pool` | `pool::FakeUpstreamPool` |
| `ClusterCoordinator` | `pgprox-cluster` | `cluster::FakeClusterCoordinator` |
| `ConfigSource` | `pgprox-config` | `config::FakeConfigSource` |
| `Router` | `pgprox-route` | `route::FakeRouter` |
| `QueryCache` | `pgprox-cache` | `cache::FakeQueryCache` |
| `Observatory` | `bin/pgprox` | `admin::FakeObservatory` |

The fakes are behind the `test-fakes` feature, so an ordinary build does not
carry them.

## Some behaviour lives here too

Types cross boundaries and most logic does not, but a few decisions are here
because having two of them would be worse than the layering violation:

`route::decide` makes the routing decision itself, so the real router and every
fake reach the same answer. `sql::Lexer` reads untrusted SQL, and one lexer
means one set of edge cases. `buf` is the buffer slab an idle connection gives
its buffers back to. `hash` names which maps may use the fast unseeded hasher
and which must keep the seeded default, which is a security rule rather than a
performance one.

## Changing anything here

A trait change is one commit: the trait, every fake, every implementor, and an
ADR. `scripts/check-core-contract.sh` holds the mechanical half of that, and
[contracts.md](../../docs/internal/standards/contracts.md) explains the rest.

`SecretString` lives here and is the reason no credential reaches a log. It
redacts in `Debug` and `Display`, zeroes on drop, and has exactly one way out.
