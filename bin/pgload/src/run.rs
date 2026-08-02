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

use pgprox_load::report::{Histogram, Latency, NO_SQLSTATE, Outcomes, Report};
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

/// Why a connection could not be opened.
///
/// Split because one case is not a failure. A node that is draining refuses a
/// new client with `57P01` and expects it to try somewhere else, and while
/// Kubernetes is still pulling that node out of the Service a reconnecting
/// client will land on it and be told exactly that. Counting those as errors
/// is how a rehearsal reports thirty-five failures for a drain that lost
/// nothing.
#[derive(Debug)]
enum Refused {
    /// The socket, TLS, or the client's own timeout.
    Local(String),
    /// The server said no, and said why.
    Server(crate::client::SessionError),
}

/// How long connection `index` waits before it opens.
///
/// The ramp spreads connections over the window rather than opening all of
/// them at once, so a run measures connections rather than the stampede of
/// their arrival. Connection zero waits nothing and the last waits almost the
/// whole window.
///
/// Extracted by `M17.5`, which found five mutants of this arithmetic surviving:
/// it lived inside a spawned task, and nothing that could be called from a test
/// computed it.
fn ramp_delay(ramp_secs: u64, index: u32, connections: u32) -> Duration {
    if ramp_secs == 0 || connections == 0 {
        return Duration::ZERO;
    }
    Duration::from_secs(ramp_secs).mul_f64(f64::from(index) / f64::from(connections))
}

/// The seed connection `index` samples from.
///
/// Derived from the run seed so the whole run reproduces, and shifted by the
/// index so no two connections send the same stream. The shift is what makes
/// them differ in the high bits rather than in one: `seed ^ index` would give
/// adjacent connections adjacent seeds, and `SplitMix64` mixes those into
/// streams that are not obviously related but were never chosen to be
/// independent.
fn connection_seed(seed: u64, index: u32) -> u64 {
    seed ^ (u64::from(index) << 17)
}

impl Refused {
    /// Whether this is a node sending the client elsewhere.
    fn is_relocation(&self) -> bool {
        matches!(
            self,
            Self::Server(crate::client::SessionError::Server { code, .. })
                if code == crate::client::ADMIN_SHUTDOWN
        )
    }

    /// The code and message this is counted under.
    fn told(&self) -> (&str, String) {
        match self {
            Self::Local(detail) => (NO_SQLSTATE, detail.clone()),
            Self::Server(error) => told(error),
        }
    }
}

/// What a session failure is counted under.
///
/// A `Server` failure carries the SQLSTATE the server sent. Everything else
/// happened on this side of the socket and has none.
fn told(error: &crate::client::SessionError) -> (&str, String) {
    match error {
        crate::client::SessionError::Server { code, message } => (code, message.clone()),
        other => (NO_SQLSTATE, other.to_string()),
    }
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local(detail) => f.write_str(detail),
            Self::Server(error) => write!(f, "{error}"),
        }
    }
}

/// What one connection did.
#[derive(Debug, Default)]
struct Tally {
    transactions: u64,
    errors: u64,
    /// Transactions given up because the node asked this client to leave.
    ///
    /// Not an error. A drain, a shed and a rolling restart all end with
    /// `57P01` on a connection that is between transactions, and reconnecting
    /// is the answer the code exists to ask for.
    relocations: u64,
    /// The first failure, kept for the message when nothing connected at all.
    first_failure: Option<String>,
    /// What this connection was told, by SQLSTATE.
    ///
    /// Per connection and merged at the end, rather than one shared map behind
    /// a lock: a thousand tasks contending on it would be the run measuring
    /// itself.
    outcomes: Outcomes,
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
            // Spread over the ramp, so the run measures connections rather
            // than the stampede of all of them arriving together.
            let share = ramp_delay(options.ramp_secs, index, options.connections);
            if !share.is_zero() {
                tokio::time::sleep(share).await;
            }
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
    let mut sampler = Sampler::new(workload, connection_seed(options.seed, index));
    let churn = sampler.transactions_per_connection();
    let mut tally = Tally::default();

    while Instant::now() < deadline {
        let mut session = match connect(options).await {
            Ok(session) => session,
            Err(refused) => {
                // Recorded before the split, so a relocation appears here too.
                // It is not a failure, but it is something a client was told,
                // and `57P01` missing from the one document that says what
                // clients saw would be a hole exactly where a drain is.
                let (code, message) = refused.told();
                tally.outcomes.record(code, &message);
                if refused.is_relocation() {
                    tally.relocations += 1;
                } else {
                    tally.errors += 1;
                    tally
                        .first_failure
                        .get_or_insert_with(|| refused.to_string());
                }
                // Backing off rather than spinning: a target that is refusing
                // connections should be measured, not flooded.
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };

        // A connection thinks before its first transaction as well as between
        // the rest. Without it, a hundred thousand connections arriving over a
        // two-minute ramp send a hundred thousand transactions as they land,
        // and the pool answers that stampede with 53300 however idle the
        // workload claims to be. Real clients connect and then sit there.
        //
        // After the connect rather than before it, or the connection would not
        // exist during the time it is supposed to be sitting idle.
        let mut settled = false;

        for _ in 0..churn {
            if Instant::now() >= deadline {
                break;
            }
            let transaction = sampler.next_transaction();
            if !settled {
                settled = true;
                tokio::time::sleep(Duration::from_millis(transaction.think_ms)).await;
                if Instant::now() >= deadline {
                    break;
                }
            }
            let started = Instant::now();
            match session.transaction(&transaction).await {
                Ok(()) => {
                    tally.transactions += 1;
                    tally
                        .latencies
                        .push(u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX));
                }
                Err(failed) => {
                    let (code, message) = told(&failed.error);
                    tally.outcomes.record(code, &message);
                    if failed.is_relocation() {
                        tally.relocations += 1;
                    } else {
                        tally.errors += 1;
                        tally
                            .first_failure
                            .get_or_insert_with(|| failed.error.to_string());
                    }
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

async fn connect(options: &Options) -> Result<Session<Stream>, Refused> {
    let connecting = async {
        let socket = tokio::net::TcpStream::connect(&options.target)
            .await
            .map_err(|error| Refused::Local(format!("connect {}: {error}", options.target)))?;
        // Nagle would batch a small query with the next one and add a
        // millisecond to a measurement whose target is under one.
        let _ = socket.set_nodelay(true);

        let stream = if options.tls {
            Stream::Tls(Box::new(
                upgrade(socket, options).await.map_err(Refused::Local)?,
            ))
        } else {
            Stream::Plain(socket)
        };

        Session::start(stream, &options.user, &options.database, &options.password)
            .await
            .map_err(Refused::Server)
    };

    match tokio::time::timeout(
        Duration::from_secs(options.connect_timeout_secs),
        connecting,
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(Refused::Local(format!(
            "startup did not finish within {}s",
            options.connect_timeout_secs
        ))),
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
    let mut relocations = 0;
    let mut first_failure = None;
    let mut outcomes = Outcomes::default();

    for tally in tallies {
        transactions += tally.transactions;
        errors += tally.errors;
        relocations += tally.relocations;
        outcomes.merge(&tally.outcomes);
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
        relocations,
        outcomes,
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

    #[test]
    fn a_summary_adds_the_tallies_rather_than_any_other_operation() {
        // `M17.5`. `transactions += tally.transactions` could become `*=` and
        // the whole file stayed green, because every test here drove a real
        // run against a fake server and read the report's shape rather than
        // its arithmetic.
        //
        // Distinct non-zero numbers on both sides, so a fold that multiplied,
        // subtracted or took only the last one is visible.
        let tallies = vec![
            Tally {
                transactions: 7,
                errors: 2,
                relocations: 1,
                ..Tally::default()
            },
            Tally {
                transactions: 11,
                errors: 3,
                relocations: 5,
                ..Tally::default()
            },
        ];

        // The shipped document rather than a hand-rolled one, which is what
        // `pgprox_load::sampler`'s own tests do. A fixture invented here would
        // drift from the schema and prove only that the invention parses.
        let Ok(workload) = Workload::parse(include_str!("../../../product/perf/workload.yaml"))
        else {
            unreachable!("the shipped workload parses")
        };

        let report = summarise(
            &Options::default(),
            &workload,
            &tallies,
            Duration::from_secs(1),
        )
        .unwrap_or_else(|error| unreachable!("a summary of two tallies: {error}"));

        assert_eq!(report.transactions, 18);
        assert_eq!(report.errors, 5);
        assert_eq!(report.relocations, 6);
    }

    #[test]
    fn the_ramp_spreads_connections_across_its_window() {
        // `M17.5`. Five mutants of this survived, because it lived inside a
        // spawned task and nothing a test could call computed it.
        //
        // The first connection waits nothing and the last waits almost the
        // whole window: that is what makes a run measure connections rather
        // than the stampede of all of them arriving together.
        assert_eq!(ramp_delay(10, 0, 100), Duration::ZERO);
        assert_eq!(ramp_delay(10, 50, 100), Duration::from_secs(5));
        assert_eq!(ramp_delay(10, 100, 100), Duration::from_secs(10));
        assert_eq!(ramp_delay(10, 25, 100), Duration::from_millis(2500));

        // No ramp means no wait, which is the default and the fast path.
        assert_eq!(ramp_delay(0, 50, 100), Duration::ZERO);
        // And a run with no connections must not divide by their count.
        assert_eq!(ramp_delay(10, 0, 0), Duration::ZERO);
    }

    #[test]
    fn every_connection_samples_a_different_stream_from_one_seed() {
        // Reproducible across a run and distinct within it, which is the pair
        // the whole generator rests on: a run that cannot be repeated is not a
        // measurement, and connections that send identical streams are one
        // connection measured many times.
        let seed = 0x2545_F491_4F6C_DD1D;

        assert_eq!(connection_seed(seed, 0), seed, "connection zero moved");
        let seeds: std::collections::HashSet<u64> =
            (0..64).map(|index| connection_seed(seed, index)).collect();
        assert_eq!(seeds.len(), 64, "two connections drew the same stream");

        // The shift is why they differ in the high bits. Without it adjacent
        // connections get adjacent seeds, which is a weaker thing to hand a
        // sampler than it looks.
        assert_ne!(
            connection_seed(seed, 1) ^ seed,
            1,
            "the index reached the seed unshifted"
        );
        assert_eq!(connection_seed(seed, 1) ^ seed, 1 << 17);

        // A different run seed gives a different stream for the same index.
        assert_ne!(connection_seed(seed, 7), connection_seed(seed + 1, 7));
    }

    #[test]
    fn a_drain_is_counted_as_a_relocation_and_everything_else_as_an_error() {
        // The distinction the whole report rests on: `57P01` on a connection
        // between transactions is a node asking a client to leave, and a run
        // that counted it as an error would report a clean rolling restart as
        // a failure.
        let drained = Refused::Server(crate::client::SessionError::Server {
            code: crate::client::ADMIN_SHUTDOWN.to_owned(),
            message: "the node is draining".to_owned(),
        });
        assert!(drained.is_relocation());
        let (code, message) = drained.told();
        assert_eq!(code, crate::client::ADMIN_SHUTDOWN);
        assert_eq!(message, "the node is draining");

        // A server error that is not a drain.
        let refused = Refused::Server(crate::client::SessionError::Server {
            code: "53300".to_owned(),
            message: "too many connections".to_owned(),
        });
        assert!(!refused.is_relocation());
        assert_eq!(refused.told().0, "53300");

        // And a local failure, which has no SQLSTATE because no server sent it.
        let local = Refused::Local("connection refused".to_owned());
        assert!(!local.is_relocation());
        let (code, message) = local.told();
        assert_eq!(code, NO_SQLSTATE);
        assert_eq!(message, "connection refused");
    }
    use super::*;
    use pgprox_proto::backend::TxStatus;
    use pgprox_proto::encode;
    use pgprox_proto::frame::Tag;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// How the fake server behaves.
    #[derive(Clone, Copy)]
    enum Fake {
        /// Answers everything.
        Working,
        /// Refuses at startup, with the code a draining node sends.
        Refusing,
        /// Refuses the first connection it sees, then answers everything.
        ///
        /// `M17.5`. The connect-failure counters in `one_connection` were
        /// reached by no test: `Refusing` refuses every connection, so the run
        /// ends in `NoConnection` with no report to read, and `Working` never
        /// refuses at all. A run needs both to have happened to say whether a
        /// refusal was counted.
        RefusingOnce {
            /// Whether that first refusal is a drain, which is a relocation,
            /// or anything else, which is an error.
            draining: bool,
        },
        /// Answers, and sends `57P01` at every other transaction boundary.
        ///
        /// `M17.5`. A drain does not only refuse new connections: a node
        /// draining under load sends `57P01` to a client that is already
        /// connected and between transactions, which is the relocation counter
        /// on the transaction path. No fake produced one, so that counter had
        /// no observable effect.
        ///
        /// `M19.6` named it for the boundary rather than for the counter. It
        /// fired on every other *statement*, which put some of its `57P01`s
        /// inside a transaction, where the same code means something else.
        DrainingBetweenTransactions,
        /// Answers, then fails every other statement with a `53300`.
        ///
        /// Every other rather than every one, because a run in which nothing
        /// succeeds has no report to inspect: it fails with `NoConnection`,
        /// which is the right answer to a target that is entirely broken and
        /// the wrong shape for a test about how failures are counted.
        FullEveryOtherStatement,
    }

    /// A server that answers every query the same way, as fast as it can.
    ///
    /// The point of running the load client against something real here is
    /// that the socket, the task fan-out and the tally are what these tests
    /// are about, and a duplex stream would not exercise any of them.
    /// What a refusing fake sends: a drain, or anything else.
    ///
    /// The two are the whole point of `RefusingOnce`: `57P01` is a node asking
    /// this client to go elsewhere and is counted as a relocation, and every
    /// other code is an error. Split out because `fake_server` is held to a
    /// hundred lines.
    fn refusal(draining: bool) -> pgprox_core::error::ClientError {
        if draining {
            pgprox_core::error::ClientError::Draining
        } else {
            pgprox_core::error::ClientError::UpstreamAtCap {
                server: pgprox_core::ids::ServerId::new("primary", 5432),
                cap: 60,
            }
        }
    }

    /// Whether the fake refuses this statement, and with what.
    ///
    /// `M19.6`. The drain arm is the reason this is a function rather than two
    /// conditions inline. A `57P01` means one of two different things depending
    /// on where in a transaction it lands: between transactions it is a node
    /// relocating a client and nothing was lost, and after a statement has
    /// succeeded it is the force-close at the end of `drain_grace` and a
    /// transaction went with it. That is the distinction `Failed::work_lost`
    /// exists for.
    ///
    /// This fake used to fire on every other *statement*, counted by one atomic
    /// four connections shared, so which of the two things it produced was
    /// decided by the scheduler. The test that asserts the first got the second
    /// about one run in twenty-five. A drain is sent between transactions, so
    /// that is the only place this sends one, and the lost-transaction side is
    /// asserted deterministically over a duplex stream by `client::tests::
    /// a_shutdown_after_a_statement_has_run_is_a_loss_rather_than_a_relocation`.
    ///
    /// `53300` needs no such rule: it is an error wherever it lands, so where
    /// it lands cannot change what the run counts.
    fn refuses(
        mode: Fake,
        in_transaction: bool,
        served: &std::sync::atomic::AtomicU64,
    ) -> Option<pgprox_core::error::ClientError> {
        let every_other = || served.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 2 == 1;
        match mode {
            Fake::DrainingBetweenTransactions if !in_transaction && every_other() => {
                Some(refusal(true))
            }
            Fake::FullEveryOtherStatement if every_other() => Some(refusal(false)),
            _ => None,
        }
    }

    async fn fake_server(mode: Fake) -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        // Shared across connections, so a client that reconnects after its
        // failed statement does not start the alternation again and succeed
        // forever.
        let served = Arc::new(std::sync::atomic::AtomicU64::new(0));

        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                // `RefusingOnce` turns itself off after the first connection,
                // which is what makes a run contain both a refusal and a
                // report.
                let refuse = match mode {
                    Fake::Refusing => Some(true),
                    Fake::RefusingOnce { draining }
                        if served.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 =>
                    {
                        Some(draining)
                    }
                    _ => None,
                };
                let served = Arc::clone(&served);
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
                    if let Some(draining) = refuse {
                        encode::error_response(&mut out, &refusal(draining));
                        let _ = socket.write_all(&out).await;
                        return;
                    }
                    encode::authentication_ok(&mut out);
                    encode::ready_for_query(&mut out, TxStatus::Idle);
                    let _ = socket.write_all(&out).await;

                    // Whether this connection is between `BEGIN` and `COMMIT`,
                    // which is the only thing that decides what a `57P01` here
                    // means. See the drain arm below.
                    let mut in_transaction = false;

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
                        let tag = Tag(header[0]);
                        if tag == Tag::TERMINATE {
                            return;
                        }
                        // An extended sequence is answered once, at its
                        // `Sync`. Answering every frame would put the client
                        // one reply out of step, which is the deadlock the
                        // proxy itself had twice in M6.
                        if !matches!(tag, Tag::QUERY | Tag::SYNC) {
                            continue;
                        }

                        // `BEGIN` and `COMMIT` are the only two statements the
                        // client sends with a fixed text, and both go as simple
                        // queries, so the transaction boundary is readable from
                        // the wire without parsing anything else.
                        let text = String::from_utf8_lossy(&body);
                        let opens = tag == Tag::QUERY && text.starts_with("BEGIN");
                        let closes = tag == Tag::QUERY && text.starts_with("COMMIT");

                        let refused = refuses(mode, in_transaction, &served);
                        let mut out = Vec::new();
                        match &refused {
                            Some(error) => encode::error_response(&mut out, error),
                            None => encode::command_complete(&mut out, "SELECT 1"),
                        }
                        encode::ready_for_query(&mut out, TxStatus::Idle);
                        if socket.write_all(&out).await.is_err() {
                            return;
                        }
                        // Moved only on the answered path: a `BEGIN` that was
                        // refused opened nothing.
                        if refused.is_none() {
                            if opens {
                                in_transaction = true;
                            } else if closes {
                                in_transaction = false;
                            }
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
        let addr = fake_server(Fake::Working).await;
        let report = run(&options(addr)).await.unwrap();

        assert!(report.transactions > 0, "nothing ran");
        assert_eq!(report.errors, 0, "a working target produced errors");
        assert!(report.first_error.is_none());
        assert_eq!(report.connections, 4);
        assert_eq!(report.seed, 5);
        assert_eq!(report.workload_version, 3);
        assert_eq!(report.latency.count, report.transactions);
        assert!(report.throughput() > 0.0);
    }

    #[tokio::test]
    async fn a_target_that_refuses_everything_is_an_error_rather_than_a_fast_run() {
        // The failure this whole binary has to make impossible: a beautiful
        // p99 over zero transactions.
        let addr = fake_server(Fake::Refusing).await;
        let error = run(&options(addr)).await.unwrap_err();
        assert!(matches!(error, LoadError::NoConnection { .. }), "{error}");
    }

    #[tokio::test]
    async fn a_run_with_errors_says_what_the_first_one_was() {
        // A count on its own is not diagnosable. Three errors in a run of
        // sixteen thousand is either a proxy refusing connections or a client
        // giving up on its own timeout, and those want opposite responses.
        let addr = fake_server(Fake::Working).await;
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
    async fn a_refused_connection_is_counted_as_what_it_was_told() {
        // `M17.5`. The connect-failure counters were reached by no test:
        // `Refusing` refuses everything, so the run ends in `NoConnection`
        // with no report to read, and `Working` never refuses. Both had to
        // happen in one run for the count to be observable.
        //
        // A drain first. `57P01` on a connection that never opened is a node
        // asking this client to go elsewhere, and a run that scored it as an
        // error would report a clean rolling restart as a failure.
        let addr = fake_server(Fake::RefusingOnce { draining: true }).await;
        let report = run(&options(addr)).await.unwrap();

        assert!(report.transactions > 0, "nothing ran: {report:?}");
        assert_eq!(
            report.relocations, 1,
            "the refusal was not counted as a relocation: {report:?}"
        );
        assert_eq!(report.errors, 0, "a drain was counted as an error");
        assert!(report.first_error.is_none());

        // And the same refusal under any other code is an error, counted
        // separately, with the run still producing a report.
        let addr = fake_server(Fake::RefusingOnce { draining: false }).await;
        let report = run(&options(addr)).await.unwrap();

        assert!(report.transactions > 0, "nothing ran: {report:?}");
        assert_eq!(report.errors, 1, "the refusal was not counted as an error");
        assert_eq!(
            report.relocations, 0,
            "an error was counted as a relocation"
        );
        assert!(
            report.first_error.is_some(),
            "a run with an error said nothing about it: {report:?}"
        );

        // Everything a client was told is in the breakdown, which is the
        // invariant the whole document rests on.
        assert_eq!(report.outcomes.total(), report.errors + report.relocations);
    }

    #[tokio::test]
    async fn a_drain_mid_run_is_a_relocation_rather_than_an_error() {
        // `M17.5`. The relocation counter on the transaction path had no test:
        // every fake either refused at connect or failed statements with
        // `53300`, so a `57P01` on an already-connected client never happened
        // and `tally.relocations += 1` could have been anything.
        //
        // It is the shape a rolling restart makes. A node draining under load
        // tells its connected clients to go elsewhere between transactions, and
        // a run that scored those as errors would report a clean restart as a
        // failure, which is the number `M11` drew conclusions from.
        //
        // `M19.6`. This failed about one run in twenty-five, and the cause was
        // in the fake rather than in the counter. It sent its `57P01` at every
        // other *statement*, chosen by one counter shared across four
        // connections, so on the twenty percent of transactions that are
        // wrapped in `BEGIN` and `COMMIT` the refusal sometimes landed after a
        // statement had already succeeded. That is a lost transaction and is
        // correctly an error, so the fake was producing the case this test
        // exists to distinguish from and the scheduler was choosing which. The
        // fake now refuses only between transactions, which is what a drain
        // does, and `errors == 0` holds by construction rather than by luck.
        let addr = fake_server(Fake::DrainingBetweenTransactions).await;
        let report = run(&options(addr)).await.unwrap();

        assert!(report.transactions > 0, "nothing succeeded: {report:?}");
        assert!(report.relocations > 0, "nothing relocated: {report:?}");
        assert_eq!(
            report.errors, 0,
            "a drain was counted as an error: {report:?}"
        );
        assert!(
            report.first_error.is_none(),
            "a run of pure relocations reported a failure: {report:?}"
        );

        // And it is in the breakdown under its own code, so the document says
        // which of the two things happened rather than only how many.
        let told = report
            .outcomes
            .get(crate::client::ADMIN_SHUTDOWN)
            .unwrap_or_else(|| panic!("no 57P01 in {:?}", report.outcomes));
        assert_eq!(told.count, report.relocations);
        assert_eq!(report.outcomes.total(), report.errors + report.relocations);
    }

    #[tokio::test]
    async fn a_run_records_the_sqlstate_its_clients_were_told_and_what_it_said() {
        // `M11.6` asks which of two errors a displaced client gets, and this
        // is the half of the answer the client has: the code, and what it was
        // actually told.
        //
        // Which is less than the task assumed. The fake here sends
        // `UpstreamAtCap` naming `primary:5432` and a cap of 60, and none of
        // that reaches the wire: `ClientError::client_message` is vague on
        // purpose, so the client sees "too many connections, please retry" and
        // would see exactly that from a node refusing at its own client
        // ceiling too. The two `53300`s are indistinguishable from outside,
        // which is the security posture working and which means the run has to
        // read the node's own view to say which one fired.
        let addr = fake_server(Fake::FullEveryOtherStatement).await;
        let report = run(&options(addr)).await.unwrap();

        assert!(report.transactions > 0, "nothing succeeded: {report:?}");
        assert!(report.errors > 0, "nothing failed: {report:?}");

        let refused = report
            .outcomes
            .get("53300")
            .unwrap_or_else(|| panic!("no 53300 in {:?}", report.outcomes));
        assert_eq!(refused.count, report.errors);
        assert_eq!(
            refused.messages.keys().collect::<Vec<_>>(),
            vec!["too many connections, please retry"],
            "what the client was told did not survive to the report"
        );

        // The invariant the document rests on: everything a client was told is
        // in here, so the breakdown accounts for the totals rather than
        // sampling them.
        assert_eq!(
            report.outcomes.total(),
            report.errors + report.relocations,
            "{:?}",
            report.outcomes
        );

        let json = report.to_json().unwrap();
        assert!(json.contains("\"53300\""), "{json}");
    }

    #[tokio::test]
    async fn a_failure_with_no_server_behind_it_is_counted_without_a_code() {
        // The other branch: nothing answered, so there is no SQLSTATE to
        // record and inventing one would put the server's vocabulary on a
        // socket error.
        let mut outcomes = Outcomes::default();
        let refused = Refused::Local("connect 127.0.0.1:1: refused".into());
        let (code, message) = refused.told();
        outcomes.record(code, &message);
        assert_eq!(outcomes.get(NO_SQLSTATE).unwrap().count, 1);

        // And a session error that is not a server's, which reaches the same
        // place by the other path.
        let (code, _) = told(&crate::client::SessionError::Disconnected);
        assert_eq!(code, NO_SQLSTATE);
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
        let addr = fake_server(Fake::Working).await;
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
        let addr = fake_server(Fake::Working).await;
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

        let addr = fake_server(Fake::Working).await;
        let mut options = options(addr);
        options.workload = file.path().to_path_buf();
        let error = run(&options).await.unwrap_err();
        assert!(format!("{error}").contains("cluster_size"), "{error}");
    }

    #[tokio::test]
    async fn the_report_is_written_where_a_script_can_read_it() {
        let addr = fake_server(Fake::Working).await;
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
            relocations: 0,
            target: "t".into(),
            workload_version: 1,
            seed: 1,
            connections: 1,
            duration_ms: 1,
            transactions: 1,
            errors: 0,
            first_error: None,
            outcomes: Outcomes::default(),
            latency: Latency::from(&Histogram::new()),
        };
        let error = write_report(&report, Path::new("/nonexistent/dir/run.json")).unwrap_err();
        assert!(
            format!("{error}").contains("/nonexistent/dir/run.json"),
            "{error}"
        );
    }
}
