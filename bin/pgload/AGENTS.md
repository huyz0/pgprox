# bin/pgload

The load client. Opens N connections, replays the workload `pgprox-load`
samples, and writes a JSON report `scripts/scale.sh` reads.

## Rules specific to this crate

- Same split as `bin/pgprox`: `main.rs` holds nothing a test cannot call, and
  it is the only file excluded from coverage.
- Errors are counted, never swallowed. A run whose statements all failed must
  not report a wonderful p99, and a run where nothing connected is an error
  rather than a report.
- The report goes to a file and the logs go to stderr. A report sharing a
  stream with log lines is a report a script has to filter.
- This is a measurement tool, so it composes `pgprox-proto` and is listed as a
  composer in `docs/internal/product/architecture.md`. It is never a dependency of the proxy.

See [pgprox-load](../../crates/pgprox-load/AGENTS.md) for the workload itself.
