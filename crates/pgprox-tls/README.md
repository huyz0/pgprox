# pgprox-tls

TLS configuration for the client listener and for upstream connections, plus
the FIPS build variant.

Small crate. Most of what it is for is two refusals.

## The FIPS assertion is the point

Building with `--features fips` swaps in the FIPS 140-3 validated `aws-lc-rs`
module. `assert_fips` then checks that the resulting configuration actually
reports `fips()`, and the process refuses to start if it does not.

That check is load-bearing. A binary that claims FIPS and quietly runs
non-validated crypto is worse than no FIPS binary, because it passes an audit
it should fail. See
[ADR 0010](../../docs/internal/product/decisions/0010-fips-build-variant.md)
and [the FIPS page](../../docs/fips.md).

## There is no way to skip verification

Upstream TLS verifies the certificate chain against a configured CA, and this
crate exposes no option to turn that off. Not behind a flag, not behind a
feature. Such a flag always ends up set in production.

## Certificate rotation

`CertReloader` re-reads the certificate and key on an interval and swaps them
in when they change. A rotation happens on the order of weeks and needs
noticing on the order of minutes, so the check is chosen for how little it
costs rather than how fast it reacts: two small files read and hashed. A
half-written file leaves the running certificate in place.

## Where it sits

Depends on `pgprox-core`. Used by `bin/pgprox` and by `bin/pgload`, which needs
a client configuration to measure a TLS deployment.

`#![forbid(unsafe_code)]` in its own source. This crate sits on the path a
client's first bytes take.
