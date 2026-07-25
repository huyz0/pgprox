//! Client-side conformance: the codec drives a real Postgres.
//!
//! The proxy binary does not exist until M6, so this is how the codec is
//! validated against a real server: speak to Postgres as a client using only
//! `pgprox-proto`, and assert every message decodes as expected.
//!
//! Requires Docker. Gated behind the `integration` feature so tier 1 never
//! waits on a container.
//!
//! Run with:
//!
//! ```text
//! PGPROX_PG_MAJOR=18 cargo nextest run -p pgprox-proto \
//!     --features integration --run-ignored all -E 'test(conformance_client)'
//! ```

#![cfg(feature = "integration")]
// An integration test is a separate crate target rather than a #[cfg(test)]
// module, so the workspace lints that ban these in production code apply here
// too. Panicking is how a test reports failure.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout
)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::Command;
use std::time::{Duration, Instant};

use pgprox_proto::backend::{self, AuthRequest, BackendMessage, TxStatus};
use pgprox_proto::encode::PROTOCOL_3_0;
use pgprox_proto::frame::{DEFAULT_MAX_FRAME, Decoded, Frame, Tag, decode};
use pgprox_proto::session::SessionState;

/// A Postgres the tests can connect to.
///
/// nextest runs each test in its own process, so a shared container cannot be
/// held in a static. Two modes instead:
///
/// - `PGPROX_PG_PORT` set: the harness already started one and owns its
///   lifecycle. This is what `scripts/conformance.sh` does, and it means one
///   container per version rather than one per test.
/// - unset: start a private container named for this process, for running a
///   single test directly during development.
struct Postgres {
    owned: Option<String>,
    port: u16,
}

impl Postgres {
    fn start(major: &str) -> Self {
        if let Ok(port) = std::env::var("PGPROX_PG_PORT") {
            let port = port.parse().expect("PGPROX_PG_PORT must be a port number");
            let pg = Self { owned: None, port };
            pg.wait_ready();
            return pg;
        }

        // Per-process name: a fixed one collides when nextest runs tests in
        // parallel, which is exactly what happened the first time.
        let name = format!("pgprox-conformance-{major}-{}", std::process::id());
        let out = Command::new("docker")
            .args([
                "run",
                "-d",
                "--rm",
                "--name",
                &name,
                "-e",
                "POSTGRES_HOST_AUTH_METHOD=trust",
                "-e",
                "POSTGRES_DB=conformance",
                "-P",
                &format!("postgres:{major}-alpine"),
            ])
            .output()
            .expect("failed to run docker");
        assert!(
            out.status.success(),
            "docker run failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        let port_out = Command::new("docker")
            .args(["port", &name, "5432/tcp"])
            .output()
            .expect("docker port failed");
        let mapped = String::from_utf8_lossy(&port_out.stdout);
        let port: u16 = mapped
            .lines()
            .next()
            .and_then(|line| line.rsplit(':').next())
            .and_then(|p| p.trim().parse().ok())
            .unwrap_or_else(|| panic!("could not parse mapped port from {mapped:?}"));

        let pg = Self {
            owned: Some(name),
            port,
        };
        pg.wait_ready();
        pg
    }

    /// Blocks until the server completes a startup exchange without an error.
    ///
    /// A container accepts TCP and answers a startup well before its databases
    /// exist, replying `57P03 the database system is starting up`. Sleeping a
    /// fixed amount instead of retrying is how this test suite would become
    /// intermittently red on a loaded machine.
    fn wait_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut last = String::from("never connected");
        while Instant::now() < deadline {
            match TcpStream::connect(("127.0.0.1", self.port)) {
                Ok(mut sock) => {
                    sock.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                    if send_startup(&mut sock, "postgres", "conformance").is_ok() {
                        let mut probe = Conn::new(sock);
                        match probe.read_until_ready() {
                            Ok(_) => return,
                            Err(e) => last = e.to_string(),
                        }
                    }
                }
                Err(e) => last = e.to_string(),
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        panic!(
            "Postgres on port {} not ready within 60s: {last}",
            self.port
        );
    }

    fn connect(&self) -> Conn {
        let mut sock = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        sock.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        send_startup(&mut sock, "postgres", "conformance").unwrap();
        let mut conn = Conn::new(sock);
        conn.read_until_ready().expect("startup should complete");
        conn
    }
}

impl Drop for Postgres {
    fn drop(&mut self) {
        if let Some(name) = &self.owned {
            let _ = Command::new("docker").args(["rm", "-f", name]).output();
        }
    }
}

/// Writes a startup packet: length prefix, version, then parameter pairs.
fn send_startup(sock: &mut TcpStream, user: &str, database: &str) -> std::io::Result<()> {
    let mut body = PROTOCOL_3_0.to_be_bytes().to_vec();
    for (name, value) in [("user", user), ("database", database)] {
        body.extend_from_slice(name.as_bytes());
        body.push(0);
        body.extend_from_slice(value.as_bytes());
        body.push(0);
    }
    body.push(0);

    let len = u32::try_from(body.len() + 4).unwrap();
    let mut packet = len.to_be_bytes().to_vec();
    packet.extend_from_slice(&body);
    sock.write_all(&packet)
}

/// A connection, buffering bytes and decoding frames with `pgprox-proto`.
struct Conn {
    sock: TcpStream,
    buf: Vec<u8>,
    /// The state machine under test, fed every frame in both directions.
    state: SessionState,
}

impl Conn {
    fn new(sock: TcpStream) -> Self {
        Self {
            sock,
            buf: Vec::new(),
            state: SessionState::new(),
        }
    }

    /// Reads until one complete frame is available, then returns its bytes.
    fn next_frame_bytes(&mut self) -> std::io::Result<Vec<u8>> {
        loop {
            if let Decoded::Frame(_, consumed) = decode(&self.buf, DEFAULT_MAX_FRAME)
                .expect("real Postgres must not produce an undecodable frame")
            {
                return Ok(self.buf.drain(..consumed).collect());
            }
            let mut chunk = [0_u8; 8192];
            let n = self.sock.read(&mut chunk)?;
            if n == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "server closed the connection",
                ));
            }
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }

    /// Reads frames until `ReadyForQuery`, returning every tag seen.
    fn read_until_ready(&mut self) -> std::io::Result<Vec<Tag>> {
        let mut tags = Vec::new();
        loop {
            let bytes = self.next_frame_bytes()?;
            let Decoded::Frame(frame, _) = decode(&bytes, DEFAULT_MAX_FRAME).unwrap() else {
                unreachable!("already known complete");
            };
            let msg = backend::decode(&frame).expect("backend message should decode");
            self.state.on_backend(&msg);
            tags.push(frame.tag());

            // Returned rather than panicked, so the readiness probe can treat
            // "the database system is starting up" as retry rather than fatal.
            // Tests unwrap, so a genuine error still fails loudly.
            if let BackendMessage::ErrorResponse(fields) = msg {
                return Err(std::io::Error::other(format!(
                    "server error {}: {}",
                    fields.code, fields.message
                )));
            }
            if matches!(msg, BackendMessage::ReadyForQuery(_)) {
                return Ok(tags);
            }
        }
    }

    /// Sends a simple query.
    fn query(&mut self, sql: &str) -> std::io::Result<Vec<Tag>> {
        let mut body = sql.as_bytes().to_vec();
        body.push(0);
        self.send(Tag::QUERY, &body)?;
        self.read_until_ready()
    }

    fn send(&mut self, tag: Tag, body: &[u8]) -> std::io::Result<()> {
        let mut out = vec![tag.get()];
        out.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_be_bytes());
        out.extend_from_slice(body);
        self.sock.write_all(&out)
    }
}

fn major() -> String {
    std::env::var("PGPROX_PG_MAJOR").unwrap_or_else(|_| "18".into())
}

fn cstr(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
    out.push(0);
}

#[test]
#[ignore = "requires docker"]
fn conformance_client_startup_and_simple_query() {
    let pg = Postgres::start(&major());
    let mut conn = pg.connect();

    // Startup left the session idle and releasable.
    assert_eq!(conn.state.tx_status(), TxStatus::Idle);
    assert!(
        conn.state.is_releasable(),
        "idle session should be releasable"
    );

    let tags = conn.query("SELECT 1").unwrap();
    assert!(tags.contains(&Tag::ROW_DESCRIPTION), "no RowDescription");
    assert!(tags.contains(&Tag::DATA_ROW), "no DataRow");
    assert!(tags.contains(&Tag::COMMAND_COMPLETE), "no CommandComplete");
    assert!(conn.state.is_releasable());
}

#[test]
#[ignore = "requires docker"]
fn conformance_client_reports_authentication_and_key_data() {
    let pg = Postgres::start(&major());
    let mut sock = TcpStream::connect(("127.0.0.1", pg.port)).unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    send_startup(&mut sock, "postgres", "conformance").unwrap();

    let mut conn = Conn::new(sock);
    let tags = conn.read_until_ready().unwrap();

    // Trust auth still sends AuthenticationOk, and every server sends its key.
    assert_eq!(tags.first(), Some(&Tag::AUTHENTICATION));
    assert!(tags.contains(&Tag::BACKEND_KEY_DATA), "no BackendKeyData");
    assert!(
        tags.iter().filter(|t| **t == Tag::PARAMETER_STATUS).count() > 3,
        "expected several ParameterStatus messages, got {tags:?}"
    );
}

#[test]
#[ignore = "requires docker"]
fn conformance_client_tracks_transaction_status() {
    // The release signal, against a real server rather than a synthesised one.
    let pg = Postgres::start(&major());
    let mut conn = pg.connect();

    conn.query("BEGIN").unwrap();
    assert_eq!(conn.state.tx_status(), TxStatus::InTransaction);
    assert!(!conn.state.is_releasable(), "released inside a transaction");

    conn.query("SELECT 1").unwrap();
    assert_eq!(conn.state.tx_status(), TxStatus::InTransaction);

    conn.query("COMMIT").unwrap();
    assert_eq!(conn.state.tx_status(), TxStatus::Idle);
    assert!(conn.state.is_releasable());
}

#[test]
#[ignore = "requires docker"]
fn conformance_client_tracks_a_failed_transaction() {
    // The case where the intuitive reading is wrong: a failed transaction is
    // not over, and the connection must stay held until the rollback.
    let pg = Postgres::start(&major());
    let mut conn = pg.connect();

    conn.query("BEGIN").unwrap();
    // Deliberately invalid, so read_until_ready's error panic must not fire.
    let mut body = b"SELECT * FROM table_that_does_not_exist".to_vec();
    body.push(0);
    conn.send(Tag::QUERY, &body).unwrap();

    let mut saw_error = false;
    loop {
        let bytes = conn.next_frame_bytes().unwrap();
        let Decoded::Frame(frame, _) = decode(&bytes, DEFAULT_MAX_FRAME).unwrap() else {
            unreachable!()
        };
        let msg = backend::decode(&frame).unwrap();
        conn.state.on_backend(&msg);
        if let BackendMessage::ErrorResponse(fields) = msg {
            assert_eq!(fields.code, "42P01", "expected undefined_table");
            saw_error = true;
        }
        if matches!(msg, BackendMessage::ReadyForQuery(_)) {
            break;
        }
    }

    assert!(saw_error, "server accepted a query against a missing table");
    assert_eq!(conn.state.tx_status(), TxStatus::Failed);
    assert!(
        !conn.state.is_releasable(),
        "released a failed transaction, which rejects every statement until rollback"
    );

    conn.query("ROLLBACK").unwrap();
    assert!(conn.state.is_releasable());
}

#[test]
#[ignore = "requires docker"]
fn conformance_client_extended_query_sequence() {
    // The path every modern driver takes, and the one that pins sessions when
    // prepared statement mapping is missing.
    let pg = Postgres::start(&major());
    let mut conn = pg.connect();

    let mut parse = Vec::new();
    cstr(&mut parse, "s1");
    cstr(&mut parse, "SELECT $1::int");
    parse.extend_from_slice(&0_i16.to_be_bytes());
    conn.send(Tag::PARSE, &parse).unwrap();

    let mut bind = Vec::new();
    cstr(&mut bind, "");
    cstr(&mut bind, "s1");
    bind.extend_from_slice(&0_i16.to_be_bytes()); // no format codes
    bind.extend_from_slice(&1_i16.to_be_bytes()); // one parameter
    bind.extend_from_slice(&1_i32.to_be_bytes()); // length 1
    bind.push(b'7');
    bind.extend_from_slice(&0_i16.to_be_bytes()); // no result formats
    conn.send(Tag::BIND, &bind).unwrap();

    let mut execute = Vec::new();
    cstr(&mut execute, "");
    execute.extend_from_slice(&0_i32.to_be_bytes());
    conn.send(Tag::EXECUTE, &execute).unwrap();

    conn.send(Tag::SYNC, &[]).unwrap();

    let tags = conn.read_until_ready().unwrap();
    assert!(tags.contains(&Tag::PARSE_COMPLETE), "no ParseComplete");
    assert!(tags.contains(&Tag::BIND_COMPLETE), "no BindComplete");
    assert!(tags.contains(&Tag::DATA_ROW), "no DataRow");
    assert!(conn.state.is_releasable(), "sequence did not close");
}

#[test]
#[ignore = "requires docker"]
fn conformance_client_copy_out_holds_the_session() {
    let pg = Postgres::start(&major());
    let mut conn = pg.connect();
    conn.query("CREATE TABLE copy_src (n int)").unwrap();
    conn.query("INSERT INTO copy_src SELECT generate_series(1, 100)")
        .unwrap();

    let mut body = b"COPY copy_src TO STDOUT".to_vec();
    body.push(0);
    conn.send(Tag::QUERY, &body).unwrap();

    let mut saw_copy_out = false;
    let mut held_during_copy = false;
    loop {
        let bytes = conn.next_frame_bytes().unwrap();
        let Decoded::Frame(frame, _) = decode(&bytes, DEFAULT_MAX_FRAME).unwrap() else {
            unreachable!()
        };
        let msg = backend::decode(&frame).unwrap();
        conn.state.on_backend(&msg);

        if matches!(msg, BackendMessage::CopyOutResponse) {
            saw_copy_out = true;
        }
        if saw_copy_out && frame.tag() == Tag::COPY_DATA {
            held_during_copy = true;
            assert!(!conn.state.is_releasable(), "released mid-COPY");
        }
        if matches!(msg, BackendMessage::ReadyForQuery(_)) {
            break;
        }
    }

    assert!(saw_copy_out, "server never entered COPY OUT");
    assert!(held_during_copy, "no COPY data arrived to check against");
    assert!(
        conn.state.is_releasable(),
        "COPY never released the session"
    );
}

#[test]
#[ignore = "requires docker"]
fn conformance_client_never_parses_a_data_row() {
    // The rule that keeps this a proxy rather than a bottleneck, checked
    // against rows a real server produced.
    let pg = Postgres::start(&major());
    let mut conn = pg.connect();

    let mut body = b"SELECT generate_series(1, 50), repeat('x', 100)".to_vec();
    body.push(0);
    conn.send(Tag::QUERY, &body).unwrap();

    let mut data_rows = 0;
    loop {
        let bytes = conn.next_frame_bytes().unwrap();
        let Decoded::Frame(frame, _) = decode(&bytes, DEFAULT_MAX_FRAME).unwrap() else {
            unreachable!()
        };
        let msg = backend::decode(&frame).unwrap();
        if frame.tag() == Tag::DATA_ROW {
            data_rows += 1;
            assert_eq!(
                msg,
                BackendMessage::Opaque(Tag::DATA_ROW),
                "a DataRow was parsed"
            );
        }
        if matches!(msg, BackendMessage::ReadyForQuery(_)) {
            break;
        }
    }

    assert_eq!(data_rows, 50, "expected 50 rows");
}

#[test]
#[ignore = "requires docker"]
fn conformance_client_authentication_is_reported_as_ok_under_trust() {
    let pg = Postgres::start(&major());
    let mut sock = TcpStream::connect(("127.0.0.1", pg.port)).unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    send_startup(&mut sock, "postgres", "conformance").unwrap();

    let mut conn = Conn::new(sock);
    let bytes = conn.next_frame_bytes().unwrap();
    let Decoded::Frame(frame, _) = decode(&bytes, DEFAULT_MAX_FRAME).unwrap() else {
        unreachable!()
    };
    assert_eq!(
        backend::decode(&frame).unwrap(),
        BackendMessage::Authentication(AuthRequest::Ok)
    );
}

#[test]
#[ignore = "requires docker"]
fn conformance_client_frames_survive_arbitrary_read_boundaries() {
    // Real TCP already chunks arbitrarily, but this forces one-byte reads so
    // reassembly is exercised at every boundary against real server output.
    let pg = Postgres::start(&major());
    let mut conn = pg.connect();

    let mut body = b"SELECT generate_series(1, 20)".to_vec();
    body.push(0);
    conn.send(Tag::QUERY, &body).unwrap();

    let mut buf: Vec<u8> = Vec::new();
    let mut tags = Vec::new();
    loop {
        let mut one = [0_u8; 1];
        let n = conn.sock.read(&mut one).unwrap();
        assert_eq!(n, 1, "server closed early");
        buf.push(one[0]);

        while let Decoded::Frame(frame, consumed) = decode(&buf, DEFAULT_MAX_FRAME).unwrap() {
            let tag = frame.tag();
            let msg = backend::decode(&frame).unwrap();
            tags.push(tag);
            let done = matches!(msg, BackendMessage::ReadyForQuery(_));
            buf.drain(..consumed);
            if done {
                assert!(tags.contains(&Tag::DATA_ROW));
                return;
            }
        }
    }
}

#[test]
#[ignore = "requires docker"]
fn conformance_client_reports_the_server_version_it_connected_to() {
    // Guards against the suite silently testing one version twice.
    let expected = major();
    let pg = Postgres::start(&expected);
    let mut sock = TcpStream::connect(("127.0.0.1", pg.port)).unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    send_startup(&mut sock, "postgres", "conformance").unwrap();

    let mut conn = Conn::new(sock);
    let mut server_version = None;
    loop {
        let bytes = conn.next_frame_bytes().unwrap();
        let Decoded::Frame(frame, _) = decode(&bytes, DEFAULT_MAX_FRAME).unwrap() else {
            unreachable!()
        };
        let msg = backend::decode(&frame).unwrap();
        if let BackendMessage::ParameterStatus { name, value } = msg
            && name == "server_version"
        {
            server_version = Some(value.to_owned());
        }
        if matches!(msg, BackendMessage::ReadyForQuery(_)) {
            break;
        }
    }

    let version = server_version.expect("no server_version parameter");
    assert!(
        version.starts_with(&expected),
        "asked for Postgres {expected} but connected to {version}"
    );
}

/// Keeps `Frame` referenced so the import list stays honest if tests change.
const _: fn(&Frame<'_>) = |_| {};
