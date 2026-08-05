---
title: FIPS builds
description: "Building pgprox against a FIPS 140-3 validated crypto module, what it costs in cipher suites, and how to verify the binary you got."
---

Some deployments need FIPS 140-3 validated cryptography. Most do not, and the
validated module is expensive enough to build that making everyone carry it
would be the wrong default.

So there is one codebase and two build profiles. `--features fips` swaps in the
validated `aws-lc-rs` module, and the binary refuses to start if the swap did
not take.

## Building it

With cargo, on a machine that has the toolchain:

```bash
cargo build --release -p pgprox --features fips
```

As a container image, which is what a deployment runs:

```bash
docker build --target fips -f deploy/Dockerfile -t pgprox:fips .
```

The `fips` target is a named stage. Nothing builds it unless it is asked for by
name, and it deliberately sits before the default runtime stage in the file,
because Docker builds the last stage when no target is given and the other order
would quietly make every build a FIPS build.

### What the build needs

The default build needs none of this. The validated module compiles AWS-LC from
source and runs it through `delocate`, which rewrites its assembly so the whole
module lands in one contiguous text section whose hash is checked at startup.

| Tool | Why |
| --- | --- |
| cmake | Configures the AWS-LC build |
| make | cmake generates Unix Makefiles here, and without make it fails as though cmake were the problem |
| Go | The delocate step is a Go program shipped with AWS-LC |
| clang | See below |

**The compiler is not optional.** gcc emits `.data.rel.ro.local` sections for
the module's relocatable read-only tables once optimization is on, and delocate
refuses any `.data` section in the module. A release build is optimized by
definition, so gcc cannot build this at all. The failure looks like this and is
easy to misread as a source problem:

```
error while processing "\t.section\t.data.rel.ro.local,\"aw\"\n"
on line 406498: ".data section found in module"
```

The Dockerfile sets `CC=clang CXX=clang++`, and `scripts/fips-check.sh` pins the
same thing rather than trusting whatever `cc` happens to be, so it gives the
same answer on a machine with a different gcc.

## What the binary does about it

Building with the feature is not the same as running validated crypto, and the
gap between those two is what the assertion exists to close.

Every TLS configuration the process builds, client and server, is checked with
`fips()` before it is returned. If a FIPS build produces a configuration that
does not report FIPS mode, the process refuses to start:

```
this binary was built with --features fips but the server configuration
does not report FIPS mode; refusing to start
```

A FIPS binary that silently falls back to non-validated crypto is worse than no
FIPS binary at all, because it passes an audit it should fail.

The feature is declared on the binary and forwarded to every crate that holds a
crypto provider, so one flag cannot leave half the process on the validated
module and half on the default one. TLS, the SCRAM implementation, the grant
cache's hashing and the cancel key's entropy source all come from the same
provider.

## Telling the two apart at runtime

Both images ship a binary with the same name at the same path with the same
entrypoint. The startup line is what distinguishes them:

```
crypto=aws-lc-rs-fips
```

against `crypto=aws-lc-rs` for a default build. Without that field the only way
to tell a FIPS pod from a default one is to go and look at how it was built.

## The crypto boundary is small on purpose

Because your token service owns JWT validation, the proxy's validated surface is
TLS plus SHA-256 for cache keys. It never verifies a signature, so the awkward
question of where EdDSA stands in validated modules never comes up.

That is a design consequence rather than a happy accident. Keeping signature
verification outside the proxy keeps the thing that has to be validated small.

## What it costs in cipher suites

FIPS mode drops ChaCha20-Poly1305 and restricts TLS 1.2 to ECDHE suites with
extended master secret enforced. The question that matters before committing to
FIPS in production is not what the provider offers, it is which client stops
working.

`scripts/cipher-matrix.sh` answers it by running both builds in one stack
against one Postgres and making each driver connect. The suite is read from the
proxy's log rather than from the driver, because only some drivers will say and
the server knows for all of them.

The recorded run is in
[`product/release/cipher-matrix.md`](../product/release/cipher-matrix.md).
Every driver that negotiated TLS 1.3 was unaffected, because all TLS 1.3 suites
are approved. The TLS 1.2 rows are the ones to read:

| Probe | Default build | FIPS build |
| --- | --- | --- |
| psql, TLS 1.2, AES | `TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384` | same |
| psql, TLS 1.2, ChaCha20 | `TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256` | **refused** |

Treat a passing row as a floor. It says nothing about a driver pinned to an
older TLS library than the one on the machine that ran the matrix, and that is
the case a FIPS migration actually breaks.

## What ships in the image

The FIPS image carries the proxy and nothing else. No mock sidecar, no openssl.
A test double for credential resolution inside a validated image is something an
auditor has to be told to ignore, and "told to ignore" is not a property worth
relying on.

That has a consequence for testing: the compose file that runs a FIPS node
beside a default one gets its certificates and its mock token service from a
separate container over a shared volume, because the image under test cannot
provide them. Bring that stack up with:

```bash
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.fips.yml up -d
```

## Verifying a build

```bash
scripts/fips-check.sh
```

It compiles the feature, runs the test that asks a real `ServerConfig` and
`ClientConfig` whether they report FIPS mode, and then builds the release
binary. Both halves of the test result are checked: that the suite passed, and
that the FIPS-gated test was among what ran, since a `#[cfg]` that stopped
matching would leave a green suite with nothing in it having asked the provider
anything.

The release build is checked separately from the test run for a specific reason.
A test passing under the test profile says nothing about a release build, and
the delocate failure above showed up in exactly that gap.

**This runs nightly and on request, not on every commit.** Building AWS-LC from
source takes minutes, and putting that on every push would slow the loop for
everyone to serve a subset of deployments. What every commit does carry is
`clippy --all-features`, so the feature always compiles; what it does not carry
is a run with the validated module linked.
