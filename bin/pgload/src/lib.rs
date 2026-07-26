//! The load client.
//!
//! Replays the reference workload against a proxy node or against Postgres
//! directly, and writes what happened as JSON. `scripts/scale.sh` runs it twice
//! and the difference between the two reports is the added latency M7 is judged
//! on.
//!
//! Everything `main.rs` would otherwise hold is here, where a test can call it.

pub mod client;
pub mod options;
pub mod run;

pub use options::{LoadError, Options};

/// Parses arguments, runs the load, and writes the report.
///
/// # Errors
///
/// Fails on a bad command line, an unreadable workload, a target nothing could
/// connect to, and a report that could not be written.
pub fn run_with<I, S>(args: I) -> Result<(), LoadError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let options = Options::parse(args)?;
    init_logging();

    let runtime = tokio::runtime::Runtime::new().map_err(|error| LoadError::Arguments {
        detail: format!("could not start a runtime: {error}"),
    })?;
    let report = runtime.block_on(run::run(&options))?;

    tracing::info!(
        target_addr = %report.target,
        connections = report.connections,
        transactions = report.transactions,
        errors = report.errors,
        p50_us = report.latency.p50_us,
        p99_us = report.latency.p99_us,
        tps = report.throughput(),
        "run finished"
    );
    run::write_report(&report, &options.out)
}

/// Logs to stderr, so the report on disk stays a document rather than a stream
/// with log lines in it.
fn init_logging() {
    // Ignored on a second call, which is what a test that calls `run_with`
    // twice would do.
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a_bad_command_line_stops_before_a_runtime_is_started() {
        let error = run_with(["--nonsense"]).unwrap_err();
        assert!(format!("{error}").contains("--nonsense"), "{error}");
    }

    #[test]
    fn logging_can_be_installed_twice() {
        // `run_with` is called once per process in production and more than
        // once in a test binary.
        init_logging();
        init_logging();
    }
}
