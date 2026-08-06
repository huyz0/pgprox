# pgprox-load

The reference workload, the sampler that turns it into work, and the report a
run produces. No I/O: `bin/pgload` is the part that owns sockets.

## Rules specific to this crate

- The workload document in `docs/internal/product/perf/workload.yaml` is a measurement
  baseline. Changing it invalidates every recorded run, so it changes in its
  own commit, with the reasoning, and the recorded runs are re-taken. Never
  change it to make a number look better.
- Sampling is deterministic given a seed. A run nobody can repeat is an
  anecdote, and the whole point of this crate is that two runs a week apart are
  comparable.
- The crate parses the committed workload in its own test suite. That test is
  the one that matters most here: if the file stops parsing, every number in
  `docs/internal/product/perf/` quietly loses its meaning.

See [docs/internal/standards/testing.md](../../docs/internal/standards/testing.md) for the hot-path
discipline this crate exists to serve.
