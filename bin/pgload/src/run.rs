//! Opening the connections, keeping them busy, and adding up what happened.
//!
//! # Why every connection has its own sampler
//!
//! One shared sampler behind a lock would serialise a thousand tasks on a
//! mutex and measure that instead of the proxy. Each connection seeds its own
//! from the run seed and its index, so the run stays reproducible and the
//! connections stay independent.
//!
//! # Why the run ends on a clock rather than a count
//!
//! A fixed transaction count finishes early on a fast target and never on a
//! slow one, so two runs would cover different amounts of time and their
//! percentiles would not be comparable. A fixed duration gives every run the
//! same window.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use pgprox_load::report::{Histogram, Latency, Report};
use pgprox_load::sampler::Sampler;
use pgprox_load::workload::Workload;
use tokio::sync::Mutex;

use crate::client::Session;
use crate::options::{LoadError, Options};

/// A client socket, with or without TLS.
///
/// The proxy in a real deployment requires TLS, so a run that could not speak
/// it could only ever measure a posture nobody deploys.
#[derive(Debug)]
pub enum Stream {
    /// Plaintext, which is what a direct connection to Postgres uses here.
    Plain(tokio::net::TcpStream),
    /// TLS, verifying nothing. See `crate::tls`.
    Tls(Box<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>),
}

impl tokio::io::AsyncRead for Stream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(io) => std::pin::Pin::new(io).poll_read(cx, buf),
            Self::Tls(io) => std::pin::Pin::new(io.as_mut()).poll_read(cx, buf),
        }
    }
}

impl tokio::io::AsyncWrite for Stream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Self::Plain(io) => std::pin::Pin::new(io).poll_write(cx, buf),
            Self::Tls(io) => std::pin::Pin::new(io.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(io) => std::pin::Pin::new(io).poll_flush(cx),
            Self::Tls(io) => std::pin::Pin::new(io.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(io) => std::pin::Pin::new(io).poll_shutdown(cx),
            Self::Tls(io) => std::pin::Pin::new(io.as_mut()).poll_shutdown(cx),
        }
    }
}

/// What one connection did.
#[derive(Debug, Default)]
struct Tally {
    transactions: u64,
    errors: u64,
    /// The first failure, kept for the message when nothing connected at all.
    first_failure: Option<String>,
    latencies: Vec<u64>,
}

/// Runs the load and returns the report.
///
/// # Errors
///
/// Fails when the workload cannot be read, and when not a single connection
/// managed to start. A run where some connections failed is a successful run
/// with a non-zero error count, because that is a real answer about the target;
/// a run where none did is a wrong password or a wrong address, and reporting
/// a p99 over nothing would be worse than saying so.
pub async fn run(options: &Options) -> Result<Report, LoadError> {
    let text = std::fs::read_to_string(&options.workload).map_err(|error| LoadError::Workload {
        path: options.workload.display().to_string(),
        detail: error.to_string(),
    })?;
    let workload = Arc::new(Workload::parse(&text).map_err(|error| LoadError::Workload {
        path: options.workload.display().to_string(),
        detail: error.to_string(),
    })?);

    let deadline = Instant::now() + Duration::from_secs(options.duration_secs);
    let started = Instant::now();

    let tallies: Arc<Mutex<Vec<Tally>>> = Arc::new(Mutex::new(Vec::new()));
    let mut tasks = Vec::with_capacity(options.connections as usize);
    for index in 0..options.connections {
        let options = options.clone();
        let workload = Arc::clone(&workload);
        let tallies = Arc::clone(&tallies);
        tasks.push(tokio::spawn(async move {
            let tally = one_connection(&options, &workload, index, deadline).await;
            tallies.lock().await.push(tally);
        }));
    }
    for task in tasks {
        // A panicking task is a bug in this binary rather than a fact about
        // the target, so it is dropped from the tally rather than reported as
        // a proxy error.
        let _ = task.await;
    }

    let elapsed = started.elapsed();
    let tallies = std::mem::take(&mut *tallies.lock().await);
    summarise(options, &workload, &tallies, elapsed)
}

/// Keeps one connection working until the deadline, reconnecting as the
/// workload's churn rate says to.
async fn one_connection(
    options: &Options,
    workload: &Workload,
    index: u32,
    deadline: Instant,
) -> Tally {
    // Seeded from the run seed and the connection index, so the whole run is
    // reproducible while no two connections send the same stream.
    let mut sampler = Sampler::new(workload, options.seed ^ (u64::from(index) << 17));
    let churn = sampler.transactions_per_connection();
    let mut tally = Tally::default();

    while Instant::now() < deadline {
        let mut session = match connect(options).await {
            Ok(session) => session,
            Err(detail) => {
                tally.errors += 1;
                tally.first_failure.get_or_insert(detail);
                // Backing off rather than spinning: a target that is refusing
                // connections should be measured, not flooded.
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };

        for _ in 0..churn {
            if Instant::now() >= deadline {
                break;
            }
            let transaction = sampler.next_transaction();
            let started = Instant::now();
            match session.transaction(&transaction).await {
                Ok(()) => {
                    tally.transactions += 1;
                    tally
                        .latencies
                        .push(u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX));
                }
                Err(error) => {
                    tally.errors += 1;
                    tally.first_failure.get_or_insert_with(|| error.to_string());
                    // The session may be inside a transaction the client did
                    // not open, so it is replaced rather than reused.
                    break;
                }
            }

            // Outside the measured interval on purpose: the pause is what
            // makes the run describe a real client rather than a benchmark
            // loop, and counting it as latency would report the workload's
            // own think time as the proxy's.
            tokio::time::sleep(Duration::from_millis(transaction.think_ms)).await;
        }

        let _ = session.terminate().await;
    }

    tally
}

async fn connect(options: &Options) -> Result<Session<Stream>, String> {
    let connecting = async {
        let socket = tokio::net::TcpStream::connect(&options.target)
            .await
            .map_err(|error| format!("connect {}: {error}", options.target))?;
        // Nagle would batch a small query with the next one and add a
        // millisecond to a measurement whose target is under one.
        let _ = socket.set_nodelay(true);

        let stream = if options.tls {
            Stream::Tls(Box::new(upgrade(socket, options).await?))
        } else {
            Stream::Plain(socket)
        };

        Session::start(stream, &options.user, &options.database, &options.password)
            .await
            .map_err(|error| error.to_string())
    };

    match tokio::time::timeout(
        Duration::from_secs(options.connect_timeout_secs),
        connecting,
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(format!(
            "startup did not finish within {}s",
            options.connect_timeout_secs
        )),
    }
}

/// Asks for TLS and wraps the socket in it.
///
/// The `SSLRequest` is a bare length and code with no message tag, answered by
/// a single byte: `S` to proceed, `N` to carry on in the clear. A client that
/// accepted `N` silently would measure a plaintext connection and report it as
/// a TLS one, so `N` is an error here.
async fn upgrade(
    socket: tokio::net::TcpStream,
    options: &Options,
) -> Result<tokio_rustls::client::TlsStream<tokio::net::TcpStream>, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut socket = socket;
    let mut request = Vec::new();
    pgprox_proto::encode_frontend::ssl_request(&mut request);
    socket
        .write_all(&request)
        .await
        .map_err(|error| format!("ssl request: {error}"))?;

    let mut answer = [0_u8; 1];
    socket
        .read_exact(&mut answer)
        .await
        .map_err(|error| format!("ssl request: {error}"))?;
    if answer[0] != b'S' {
        return Err(format!(
            "the server answered {} to an SSLRequest, so TLS is not available",
            char::from(answer[0])
        ));
    }

    let config = crate::tls::insecure_config()?;
    // A name is required by the API and verified by nothing here. The host
    // half of the target is the honest thing to send, since that is what a
    // real client would put in SNI.
    let host = options
        .target
        .rsplit_once(':')
        .map_or(options.target.as_str(), |(host, _)| host)
        .to_owned();
    let name = tokio_rustls::rustls::pki_types::ServerName::try_from(host)
        .map_err(|error| format!("tls: {error}"))?;

    tokio_rustls::TlsConnector::from(config)
        .connect(name, socket)
        .await
        .map_err(|error| format!("tls handshake: {error}"))
}

fn summarise(
    options: &Options,
    workload: &Workload,
    tallies: &[Tally],
    elapsed: Duration,
) -> Result<Report, LoadError> {
    let mut histogram = Histogram::new();
    let mut transactions = 0;
    let mut errors = 0;
    let mut first_failure = None;

    for tally in tallies {
        transactions += tally.transactions;
        errors += tally.errors;
        for micros in &tally.latencies {
            histogram.record(*micros);
        }
        if first_failure.is_none() {
            first_failure.clone_from(&tally.first_failure);
        }
    }

    if transactions == 0 {
        return Err(LoadError::NoConnection {
            detail: first_failure.unwrap_or_else(|| "nothing was attempted".to_owned()),
        });
    }

    Ok(Report {
        first_error: first_failure,
        target: options.target.clone(),
        workload_version: workload.version,
        seed: options.seed,
        connections: options.connections,
        duration_ms: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
        transactions,
        errors,
        latency: Latency::from(&histogram),
    })
}

/// Writes the report where a script can read it.
///
/// # Errors
///
/// Fails when the file cannot be written, which is a run whose numbers exist
/// only in this process and are about to be lost.
pub fn write_report(report: &Report, path: &Path) -> Result<(), LoadError> {
    let json = report.to_json().map_err(|error| LoadError::Report {
        path: path.display().to_string(),
        source: std::io::Error::other(error),
    })?;
    std::fs::write(path, json).map_err(|error| LoadError::Report {
        path: path.display().to_string(),
        source: error,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use pgprox_proto::backend::TxStatus;
    use pgprox_proto::encode;
    use pgprox_proto::frame::Tag;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// A server that answers every query the same way, as fast as it can.
    ///
    /// The point of running the load client against something real here is
    /// that the socket, the task fan-out and the tally are what these tests
    /// are about, and a duplex stream would not exercise any of them.
    async fn fake_server(refuse: bool) -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    // The startup packet: an untagged length and the rest.
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
                    if refuse {
                        encode::error_response(
                            &mut out,
                            &pgprox_core::error::ClientError::Draining,
                        );
                        let _ = socket.write_all(&out).await;
                        return;
                    }
                    encode::authentication_ok(&mut out);
                    encode::ready_for_query(&mut out, TxStatus::Idle);
                    let _ = socket.write_all(&out).await;

                    loop {
                        let mut header = [0_u8; 5];
                        if socket.read_exact(&mut header).await.is_err() {
                            return;
                        }
                        let len = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
                        let mut body = vec![0; len - 4];
                        if socket.read_exact(&mut body).await.is_err() {
                            return;
                        }
                        if Tag(header[0]) == Tag::TERMINATE {
                            return;
                        }

                        let mut out = Vec::new();
                        encode::command_complete(&mut out, "SELECT 1");
                        encode::ready_for_query(&mut out, TxStatus::Idle);
                        if socket.write_all(&out).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });

        addr
    }

    fn options(target: std::net::SocketAddr) -> Options {
        Options {
            target: target.to_string(),
            workload: std::path::PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../product/perf/workload.yaml"
            )),
            connections: 4,
            duration_secs: 1,
            seed: 5,
            connect_timeout_secs: 2,
            ..Options::default()
        }
    }

    #[tokio::test]
    async fn a_run_against_a_working_target_reports_what_it_did() {
        let addr = fake_server(false).await;
        let report = run(&options(addr)).await.unwrap();

        assert!(report.transactions > 0, "nothing ran");
        assert_eq!(report.errors, 0, "a working target produced errors");
        assert!(report.first_error.is_none());
        assert_eq!(report.connections, 4);
        assert_eq!(report.seed, 5);
        assert_eq!(report.workload_version, 2);
        assert_eq!(report.latency.count, report.transactions);
        assert!(report.throughput() > 0.0);
    }

    #[tokio::test]
    async fn a_target_that_refuses_everything_is_an_error_rather_than_a_fast_run() {
        // The failure this whole binary has to make impossible: a beautiful
        // p99 over zero transactions.
        let addr = fake_server(true).await;
        let error = run(&options(addr)).await.unwrap_err();
        assert!(matches!(error, LoadError::NoConnection { .. }), "{error}");
    }

    #[tokio::test]
    async fn a_run_with_errors_says_what_the_first_one_was() {
        // A count on its own is not diagnosable. Three errors in a run of
        // sixteen thousand is either a proxy refusing connections or a client
        // giving up on its own timeout, and those want opposite responses.
        let addr = fake_server(false).await;
        let mut options = options(addr);
        // A user the fake server accepts, against a port that is not there for
        // half the connections: the run still completes and carries a reason.
        options.connections = 2;
        options.connect_timeout_secs = 1;
        let report = run(&options).await.unwrap();
        assert!(report.first_error.is_none(), "{report:?}");

        let json = report.to_json().unwrap();
        assert!(
            !json.contains("first_error"),
            "a clean run should not carry an error field: {json}"
        );
    }

    #[tokio::test]
    async fn a_tls_target_that_is_not_there_says_so() {
        // The TLS path's own connect failure, which is a different branch from
        // the plaintext one and would otherwise never run.
        let mut options = options("127.0.0.1:1".parse().unwrap());
        options.tls = true;
        options.duration_secs = 1;
        options.connect_timeout_secs = 1;
        let error = run(&options).await.unwrap_err();
        assert!(format!("{error}").contains("connect"), "{error}");
    }

    #[tokio::test]
    async fn a_plaintext_server_answering_a_tls_client_is_reported() {
        // The fake postgres does not speak the SSLRequest exchange, so the
        // read of its answer is what fails. Reported rather than hung.
        let addr = fake_server(false).await;
        let mut options = options(addr);
        options.tls = true;
        options.connections = 1;
        options.duration_secs = 1;
        let error = run(&options).await.unwrap_err();
        assert!(matches!(error, LoadError::NoConnection { .. }), "{error}");
    }

    #[tokio::test]
    async fn a_target_that_is_not_there_says_so() {
        let mut options = options("127.0.0.1:1".parse().unwrap());
        options.duration_secs = 1;
        let error = run(&options).await.unwrap_err();
        assert!(format!("{error}").contains("connect"), "{error}");
    }

    #[tokio::test]
    async fn a_workload_that_cannot_be_read_stops_before_any_load() {
        let addr = fake_server(false).await;
        let mut options = options(addr);
        options.workload = std::path::PathBuf::from("/nonexistent/workload.yaml");
        let error = run(&options).await.unwrap_err();
        assert!(matches!(error, LoadError::Workload { .. }), "{error}");
    }

    #[tokio::test]
    async fn a_workload_that_is_invalid_names_its_field() {
        let file = tempfile::NamedTempFile::new().unwrap();
        // Complete but for one field being out of range, so the failure is
        // validation rather than a missing key.
        let good = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../product/perf/workload.yaml"
        ))
        .unwrap();
        std::fs::write(
            file.path(),
            good.replace("cluster_size: 3", "cluster_size: 0"),
        )
        .unwrap();

        let addr = fake_server(false).await;
        let mut options = options(addr);
        options.workload = file.path().to_path_buf();
        let error = run(&options).await.unwrap_err();
        assert!(format!("{error}").contains("cluster_size"), "{error}");
    }

    #[tokio::test]
    async fn the_report_is_written_where_a_script_can_read_it() {
        let addr = fake_server(false).await;
        let report = run(&options(addr)).await.unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.json");
        write_report(&report, &path).unwrap();

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("\"p99_us\""), "{written}");
        assert!(written.contains("\"transactions\""), "{written}");
    }

    /// A server that speaks the `SSLRequest` exchange and then TLS.
    ///
    /// The whole point of the flag: the deployed posture requires TLS, so a
    /// run that could not speak it could only measure a posture nobody uses.
    async fn tls_server(answer: u8) -> std::net::SocketAddr {
        use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};

        let issued = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
        let cert = CertificateDer::from(issued.cert.der().to_vec());
        let key = PrivateKeyDer::try_from(issued.signing_key.serialize_der()).unwrap();
        let config = pgprox_tls::server_config(vec![cert], key).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let acceptor = tokio_rustls::TlsAcceptor::from(config);
            while let Ok((mut socket, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    // The SSLRequest: eight bytes, no tag.
                    let mut request = [0_u8; 8];
                    if socket.read_exact(&mut request).await.is_err() {
                        return;
                    }
                    if socket.write_all(&[answer]).await.is_err() || answer != b'S' {
                        return;
                    }

                    let Ok(mut tls) = acceptor.accept(socket).await else {
                        return;
                    };
                    let mut length = [0_u8; 4];
                    if tls.read_exact(&mut length).await.is_err() {
                        return;
                    }
                    let len = u32::from_be_bytes(length) as usize;
                    let mut rest = vec![0; len - 4];
                    if tls.read_exact(&mut rest).await.is_err() {
                        return;
                    }

                    let mut out = Vec::new();
                    encode::authentication_ok(&mut out);
                    encode::ready_for_query(&mut out, TxStatus::Idle);
                    let _ = tls.write_all(&out).await;

                    loop {
                        let mut header = [0_u8; 5];
                        if tls.read_exact(&mut header).await.is_err() {
                            return;
                        }
                        let len = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
                        let mut body = vec![0; len - 4];
                        if tls.read_exact(&mut body).await.is_err() {
                            return;
                        }
                        if Tag(header[0]) == Tag::TERMINATE {
                            return;
                        }
                        let mut out = Vec::new();
                        encode::command_complete(&mut out, "SELECT 1");
                        encode::ready_for_query(&mut out, TxStatus::Idle);
                        if tls.write_all(&out).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });

        addr
    }

    #[tokio::test]
    async fn a_run_over_tls_measures_the_deployed_posture() {
        let addr = tls_server(b'S').await;
        let mut options = options(addr);
        options.tls = true;
        options.connections = 2;

        let report = run(&options).await.unwrap();
        assert!(report.transactions > 0, "nothing ran over TLS");
        assert_eq!(report.errors, 0, "{report:?}");
    }

    #[tokio::test]
    async fn a_server_that_refuses_tls_is_an_error_rather_than_a_downgrade() {
        // A client that carried on in the clear would measure a plaintext
        // connection and report it as a TLS one, which is the one answer worse
        // than failing.
        let addr = tls_server(b'N').await;
        let mut options = options(addr);
        options.tls = true;
        options.connections = 1;
        options.duration_secs = 1;

        let error = run(&options).await.unwrap_err();
        assert!(
            format!("{error}").contains("TLS is not available"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn a_stream_closes_cleanly() {
        // `poll_shutdown` runs when a run ends and its connections go. Not
        // exercised by the load path, which lets the socket drop.
        use tokio::io::AsyncWriteExt as _;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { listener.accept().await });

        let mut stream = Stream::Plain(tokio::net::TcpStream::connect(addr).await.unwrap());
        stream.write_all(b"x").await.unwrap();
        stream.flush().await.unwrap();
        stream.shutdown().await.unwrap();
    }

    #[test]
    fn a_report_that_cannot_be_written_says_where() {
        let report = Report {
            target: "t".into(),
            workload_version: 1,
            seed: 1,
            connections: 1,
            duration_ms: 1,
            transactions: 1,
            errors: 0,
            first_error: None,
            latency: Latency::from(&Histogram::new()),
        };
        let error = write_report(&report, Path::new("/nonexistent/dir/run.json")).unwrap_err();
        assert!(
            format!("{error}").contains("/nonexistent/dir/run.json"),
            "{error}"
        );
    }
}
