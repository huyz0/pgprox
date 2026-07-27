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
pub mod tls;

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
    fn a_whole_run_writes_its_report() {
        // `run_with` is what `main` calls, and it is the one path that builds
        // a runtime, runs the load and writes the file. Everything it does was
        // covered piecewise and never end to end.
        use std::io::Write as _;

        // A server on its own runtime, since `run_with` builds its own.
        let (addr_tx, addr_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                addr_tx.send(listener.local_addr().unwrap()).unwrap();
                loop {
                    let Ok((mut socket, _)) = listener.accept().await else {
                        return;
                    };
                    tokio::spawn(async move {
                        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
                        let mut length = [0_u8; 4];
                        if socket.read_exact(&mut length).await.is_err() {
                            return;
                        }
                        let len = u32::from_be_bytes(length) as usize;
                        let mut rest = vec![0; len - 4];
                        if socket.read_exact(&mut rest).await.is_err() {
                            return;
                        }
                        let mut out = Vec::new();
                        pgprox_proto::encode::authentication_ok(&mut out);
                        pgprox_proto::encode::ready_for_query(
                            &mut out,
                            pgprox_proto::backend::TxStatus::Idle,
                        );
                        let _ = socket.write_all(&out).await;

                        loop {
                            let mut header = [0_u8; 5];
                            if socket.read_exact(&mut header).await.is_err() {
                                return;
                            }
                            let len = u32::from_be_bytes(header[1..].try_into().unwrap_or([0; 4]))
                                as usize;
                            let mut body = vec![0; len.saturating_sub(4)];
                            if socket.read_exact(&mut body).await.is_err() {
                                return;
                            }
                            let mut out = Vec::new();
                            pgprox_proto::encode::command_complete(&mut out, "SELECT 1");
                            pgprox_proto::encode::ready_for_query(
                                &mut out,
                                pgprox_proto::backend::TxStatus::Idle,
                            );
                            if socket.write_all(&out).await.is_err() {
                                return;
                            }
                        }
                    });
                }
            });
        });
        let addr = addr_rx.recv().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("run.json");
        let workload = dir.path().join("workload.yaml");
        let mut file = std::fs::File::create(&workload).unwrap();
        file.write_all(
            include_str!("../../../product/perf/workload.yaml")
                .replace("min_ms: 50", "min_ms: 1")
                .replace("max_ms: 500", "max_ms: 5")
                .as_bytes(),
        )
        .unwrap();

        run_with([
            "--target",
            &addr.to_string(),
            "--connections",
            "2",
            "--duration",
            "1",
            "--workload",
            workload.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .unwrap();

        let written = std::fs::read_to_string(&out).unwrap();
        assert!(written.contains("\"transactions\""), "{written}");
    }

    #[test]
    fn logging_can_be_installed_twice() {
        // `run_with` is called once per process in production and more than
        // once in a test binary.
        init_logging();
        init_logging();
    }
}
