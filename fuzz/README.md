# Fuzzing

The wire decoder and the startup parser read bytes sent by anyone who can reach
the listener. Fuzzing them is a security control, not a nicety: a malformed
frame must never take down a node serving 100k other connections.

## Running

`cargo-fuzz` builds with libFuzzer, which needs a nightly toolchain:

```bash
rustup toolchain install nightly
cargo install cargo-fuzz
cargo +nightly fuzz run frame_decode -- -max_total_time=60
cargo +nightly fuzz run message_decode -- -max_total_time=60
```

Short runs gate pull requests; long runs happen nightly. See
`standards/testing.md`.

## Status

**These targets have not been executed.** No nightly toolchain is installed on
the development machine where they were written, so they are correct by
inspection only. Running them is outstanding and belongs in CI, where the
toolchain can be provisioned.

What does run today, on stable, is in `crates/pgprox-proto/tests/properties.rs`:
proptest generates structured and semi-structured input and asserts the same
no-panic and consumed-length properties. That is weaker than coverage-guided
fuzzing, which is why it is not a replacement.

## Corpus

Any crash found becomes a unit test in the module that owns the parser, and the
input is committed to `corpus/` so it is replayed on every later run.
