# Fuzzing

The wire decoder and the startup parser read bytes sent by anyone who can reach
the listener. Fuzzing them is a security control, not a nicety: a malformed
frame must never take down a node serving 100k other connections.

## Running

`cargo-fuzz` builds with libFuzzer, which needs a nightly toolchain:

```bash
rustup toolchain install nightly
cargo install cargo-fuzz

scripts/fuzz.sh          # 60 seconds per target
scripts/fuzz.sh 600      # ten minutes each, for a nightly
```

The script seeds the corpus and runs all three. A single target, when one of
them has found something and the input needs replaying:

```bash
cargo +nightly fuzz run classify fuzz/artifacts/classify/crash-<hash>
```

Short runs gate pull requests; long runs happen nightly. See
`docs/internal/standards/testing.md`.

## Status

Run for the first time in M8, and `scripts/fuzz.sh` is what runs them now:
it seeds the corpus, then gives each target a time budget. Sixty seconds each
for a quick pass, longer for a nightly.

`frame_decode` and `message_decode` came back clean, at 1.2 million and 380
thousand executions a second respectively. `classify` found two things, both in
its own oracle rather than in the classifier.

The oracle greps for a DML keyword outside anything quoted and asserts that a
statement containing one is never classified read-only. It skipped quotes and
not comments, so `---kk...update;` read as a statement mentioning `update`
when `--` had opened a comment and the keyword was inside it. Then, with line
comments handled, `/* /* merge */ */` did the same thing: Postgres nests block
comments and the oracle did not, so it thought the comment had closed early.

Both have the same shape and it is worth naming. An oracle that skips *less*
than the thing it checks sees more keywords than are really there, and reports
the checker's correctness as a bug. The oracle has to skip at least as much as
the scanner it is checking. The classifier was right both times, which is the
reassuring part: it is the path that decides whether a statement may go to a
replica.

## The corpus

Seeded by `crates/pgprox-proto/examples/seed_corpus.rs` and not committed. One
file per message shape the proxy can encode, chosen from what pgdog, pgbouncer
and odyssey test for: the authentication ladder, the extended-query sequence,
the messages whose length field can disagree with their content, and the
startup packet.

`M1F.25` asked for those proxies' fixtures to be copied in. There are none to
copy: all three build their messages in code or drive real servers. What they
carry is a list of what they thought worth testing, and that is the part the
seeder reproduces.

What does run today, on stable, is in `crates/pgprox-proto/tests/properties.rs`
and `crates/pgprox-route/tests/properties.rs`: proptest generates structured and
semi-structured input and asserts the same properties. The route one carries the
same differential oracle as the `classify` target, and it has already earned its
place: it found a dollar-quote tag validation bug that classified
`SELECT $1 INSERT $$` as a replica-eligible read. That is weaker than
coverage-guided
fuzzing, which is why it is not a replacement.

## Corpus

Any crash found becomes a unit test in the module that owns the parser, and the
input is committed to `corpus/` so it is replayed on every later run.
