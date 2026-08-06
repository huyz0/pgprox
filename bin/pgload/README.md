# pgload

The load client. Replays the reference workload against a proxy node or against
Postgres directly, and writes what happened as JSON.

`scripts/scale.sh` runs it twice, once through the proxy and once straight at
the database. The difference between the two reports is the added latency the
project is judged on.

## What is here and what is not

The workload, the sampler and the report are in `pgprox-load` and perform no
I/O. This crate is the thin part that puts that stream on a socket.

Everything `main.rs` would otherwise hold is in the library target, where a
test can call it.

## It speaks the protocol properly

`client` runs a real startup, a real SCRAM or MD5 exchange, and real extended
query sequences, because a load client that cut corners would measure a path no
driver takes.

That is why this binary depends on `pgprox-auth` and `pgprox-tls`: it needs the
client half of SCRAM and a client TLS configuration. It is never a dependency
of the proxy, and the dependency only goes this way.

## Where it sits

Composes `pgprox-load`, `pgprox-proto`, `pgprox-auth`, `pgprox-tls` and
`pgprox-core`. One of the crates permitted to compose others, on the grounds
that it speaks the wire protocol to measure the proxy rather than to be part of
it.

## Running it

It ships inside the proxy image so a scale run can generate load from inside
the container network, which has far more ephemeral ports than the host and
skips the published-port forwarder. See
[Performance](../../docs/performance.md) for what has been measured with it.
