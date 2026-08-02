//! A minimal Postgres server built on `pgprox-proto`, for driver conformance.
//!
//! The proxy binary does not exist until M6, so this is how the encoding side
//! of the codec gets tested against real drivers: stand up something that
//! speaks the protocol and let pgx, asyncpg, JDBC, npgsql and psql connect to
//! it.
//!
//! It answers a fixed result set rather than running SQL. What is under test is
//! the protocol exchange, not a query engine.
//!
//! Usage: `conformance_server [port]`, printing the bound port on stdout.

// A test harness, not production code. It panics to report failure and prints
// to stdout so the driver scripts can read the port.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout,
    clippy::print_stderr
)]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

use pgprox_core::ids::{ConnId, NodeId};
use pgprox_proto::backend::TxStatus;
use pgprox_proto::encode;
use pgprox_proto::frame::{DEFAULT_MAX_FRAME, Decoded, Tag, decode, decode_untagged};
use pgprox_proto::frontend::{self, FrontendMessage};
use pgprox_proto::startup::{self, Startup, VersionResponse};

fn main() {
    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);

    let listener = TcpListener::bind(("127.0.0.1", port)).expect("could not bind");
    let bound = listener.local_addr().unwrap().port();

    // The driver scripts read this to know where to connect.
    println!("{bound}");
    std::io::stdout().flush().unwrap();

    let mut counter = 0_u64;
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        counter += 1;
        let conn = ConnId::new(NodeId::new(1), counter);
        std::thread::spawn(move || {
            if let Err(e) = serve(stream, conn) {
                eprintln!("connection {conn}: {e}");
            }
        });
    }
}

/// Reads until `buf` holds a complete untagged message, then returns its body.
fn read_untagged(sock: &mut TcpStream, buf: &mut Vec<u8>) -> std::io::Result<Vec<u8>> {
    loop {
        if let Decoded::Frame(frame, consumed) = decode_untagged(buf, DEFAULT_MAX_FRAME)
            .map_err(|e| std::io::Error::other(e.to_string()))?
        {
            let body = frame.body().to_vec();
            buf.drain(..consumed);
            return Ok(body);
        }
        read_more(sock, buf)?;
    }
}

/// Reads until `buf` holds a complete tagged message, returning it.
fn read_tagged(sock: &mut TcpStream, buf: &mut Vec<u8>) -> std::io::Result<(Tag, Vec<u8>)> {
    loop {
        if let Decoded::Frame(frame, consumed) =
            decode(buf, DEFAULT_MAX_FRAME).map_err(|e| std::io::Error::other(e.to_string()))?
        {
            let out = (frame.tag(), frame.body().to_vec());
            buf.drain(..consumed);
            return Ok(out);
        }
        read_more(sock, buf)?;
    }
}

fn read_more(sock: &mut TcpStream, buf: &mut Vec<u8>) -> std::io::Result<()> {
    let mut chunk = [0_u8; 8192];
    let n = sock.read(&mut chunk)?;
    if n == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "client closed",
        ));
    }
    buf.extend_from_slice(&chunk[..n]);
    Ok(())
}

fn serve(mut sock: TcpStream, conn: ConnId) -> std::io::Result<()> {
    let mut buf = Vec::new();
    serve_startup(&mut sock, conn, &mut buf)?;
    serve_queries(&mut sock, &mut buf)
}

/// Handles everything up to the first `ReadyForQuery`.
fn serve_startup(sock: &mut TcpStream, conn: ConnId, buf: &mut Vec<u8>) -> std::io::Result<()> {
    sock.set_nodelay(true)?;

    // Startup phase. A client may send SSLRequest or GSSENCRequest first, and
    // each gets a single-byte answer before the real startup packet arrives.
    //
    // Only the version is kept from the decoded packet. Holding the whole
    // Startup would borrow from a buffer that goes out of scope each iteration.
    let version = loop {
        let body = read_untagged(sock, buf)?;
        match startup::decode(&body).map_err(|e| std::io::Error::other(e.to_string()))? {
            // 'N' declines without failing the connection. Drivers configured
            // for "prefer" fall back to plaintext, which is what this harness
            // wants: TLS is pgprox-tls's business, not the codec's.
            Startup::SslRequest | Startup::GssEncRequest => {
                sock.write_all(b"N")?;
            }
            Startup::CancelRequest { conn } => {
                eprintln!("cancel for {conn}");
                return Ok(());
            }
            Startup::StartupMessage { version, .. } => break version,
            _ => return Ok(()),
        }
    };

    let mut out = Vec::new();
    // No extensions: this server answers a harness rather than a client,
    // and the harness sends none. See `negotiate`'s own note.
    match startup::negotiate(version, false) {
        VersionResponse::Accept => {}
        VersionResponse::Negotiate { minor } => {
            encode::negotiate_protocol_version(&mut out, minor, &[]);
        }
        // VersionResponse is #[non_exhaustive] and an example is a separate
        // crate, so a wildcard is required. Refusing is the safe direction for
        // anything not explicitly handled.
        _ => {
            encode::error_response(
                &mut out,
                &pgprox_core::error::ClientError::ProtocolViolation("unsupported protocol version"),
            );
            sock.write_all(&out)?;
            return Ok(());
        }
    }

    encode::authentication_ok(&mut out);
    // Drivers require several of these at startup and hang without them.
    for (name, value) in [
        ("server_version", "18.0 (pgprox conformance harness)"),
        ("server_encoding", "UTF8"),
        ("client_encoding", "UTF8"),
        ("DateStyle", "ISO, MDY"),
        ("TimeZone", "UTC"),
        ("integer_datetimes", "on"),
        ("standard_conforming_strings", "on"),
        ("application_name", ""),
        ("is_superuser", "off"),
        ("session_authorization", "postgres"),
    ] {
        encode::parameter_status(&mut out, name, value);
    }
    encode::backend_key_data(&mut out, conn);
    encode::ready_for_query(&mut out, TxStatus::Idle);
    sock.write_all(&out)?;
    Ok(())
}

/// Answers messages until the client goes away.
fn serve_queries(sock: &mut TcpStream, buf: &mut Vec<u8>) -> std::io::Result<()> {
    // Query phase.
    //
    // Statement name to parameter count. A driver asks how many parameters a
    // statement takes and refuses to bind a different number, so answering a
    // fixed count breaks every query whose placeholder count differs. asyncpg
    // found this immediately.
    let mut statements: HashMap<String, StmtInfo> = HashMap::new();
    // Portal name to the statement it was bound to. Execute names a portal, not
    // a statement, so without this the harness cannot tell which SQL is running
    // and answers every extended query identically. Four drivers found that.
    let mut portals: HashMap<String, String> = HashMap::new();
    // Whether the current portal asked for binary results.
    let mut binary_results = false;
    // Which statement the current Execute is running.
    let mut executing_large = false;

    loop {
        let (tag, body) = match read_tagged(sock, buf) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e),
        };

        let frame = pgprox_proto::Frame::new(tag, &body);
        let msg = frontend::decode(&frame).map_err(|e| std::io::Error::other(e.to_string()))?;

        let mut out = Vec::new();
        match msg {
            FrontendMessage::Terminate => return Ok(()),

            FrontendMessage::Query { sql } => {
                // The simple query protocol is always text.
                if sql.contains("pgprox_large") {
                    large_row_description(&mut out);
                    for _ in 0..LARGE_RESULT_ROWS {
                        large_data_row(&mut out);
                    }
                    command_complete(&mut out, &format!("SELECT {LARGE_RESULT_ROWS}"));
                } else {
                    row_description(&mut out, false);
                    data_row(&mut out, false);
                    command_complete(&mut out, "SELECT 1");
                }
                encode::ready_for_query(&mut out, TxStatus::Idle);
            }

            // The extended query path. Every modern driver uses it, and its
            // messages are answered individually rather than as one block.
            FrontendMessage::Parse { statement, sql } => {
                statements.insert(
                    statement.to_owned(),
                    StmtInfo {
                        params: count_placeholders(sql),
                        large: sql.contains("pgprox_large"),
                    },
                );
                simple(&mut out, Tag::PARSE_COMPLETE);
            }
            FrontendMessage::Bind { portal, statement } => {
                // The result format codes live past the fields the decoder
                // exposes, and they are not optional to honour: asyncpg asks
                // for binary and then fails to decode a text answer.
                binary_results = bind_wants_binary(&body);
                portals.insert(portal.to_owned(), statement.to_owned());
                executing_large = statements.get(statement).is_some_and(|info| info.large);
                simple(&mut out, Tag::BIND_COMPLETE);
            }
            FrontendMessage::Close { .. } => simple(&mut out, Tag::CLOSE_COMPLETE),
            FrontendMessage::Describe { target, name } => match target {
                // Describing a statement yields its parameter types first.
                frontend::Target::Statement => {
                    let info = statements.get(name).copied().unwrap_or_default();
                    parameter_description(&mut out, info.params);
                    if info.large {
                        large_row_description(&mut out);
                    } else {
                        row_description(&mut out, binary_results);
                    }
                }
                _ => {
                    if executing_large {
                        large_row_description(&mut out);
                    } else {
                        row_description(&mut out, binary_results);
                    }
                }
            },
            FrontendMessage::Execute { portal, .. } => {
                let large = portals
                    .get(portal)
                    .and_then(|name| statements.get(name))
                    .is_some_and(|info| info.large);
                if large {
                    for _ in 0..LARGE_RESULT_ROWS {
                        large_data_row(&mut out);
                    }
                    command_complete(&mut out, &format!("SELECT {LARGE_RESULT_ROWS}"));
                } else {
                    data_row(&mut out, binary_results);
                    command_complete(&mut out, "SELECT 1");
                }
            }
            // Flush asks for buffered output without ending the sequence, so
            // it gets nothing here: this harness never buffers.
            FrontendMessage::Flush => {}

            // Sync ends a sequence; anything unhandled is answered the same way
            // so a driver is never left waiting.
            FrontendMessage::Sync | _ => encode::ready_for_query(&mut out, TxStatus::Idle),
        }

        if !out.is_empty() {
            sock.write_all(&out)?;
        }
    }
}

/// A message with an empty body.
fn simple(out: &mut Vec<u8>, tag: Tag) {
    out.push(tag.get());
    out.extend_from_slice(&4_u32.to_be_bytes());
}

/// Reads the result format codes from a `Bind` body.
///
/// The layout past the two names is: parameter format codes, then parameters,
/// then result format codes. Only the last matters here.
fn bind_wants_binary(body: &[u8]) -> bool {
    let mut r = pgprox_proto::Reader::new(body);
    let Ok(_portal) = r.cstr("portal") else {
        return false;
    };
    let Ok(_statement) = r.cstr("statement") else {
        return false;
    };

    let Ok(param_formats) = r.i16("param_format_count") else {
        return false;
    };
    for _ in 0..param_formats.max(0) {
        if r.i16("param_format").is_err() {
            return false;
        }
    }

    let Ok(params) = r.i16("param_count") else {
        return false;
    };
    for _ in 0..params.max(0) {
        let Ok(len) = r.i32("param_len") else {
            return false;
        };
        // -1 is SQL NULL, which carries no bytes.
        let Ok(len) = usize::try_from(len) else {
            continue;
        };
        if len > 0 && r.bytes(len, "param_value").is_err() {
            return false;
        }
    }

    let Ok(result_formats) = r.i16("result_format_count") else {
        return false;
    };
    // Zero codes means text for every column; one code applies to all.
    (0..result_formats.max(0)).any(|_| r.i16("result_format").is_ok_and(|f| f == 1))
}

/// `T`: one `int4` column named `n`, in text or binary format.
fn row_description(out: &mut Vec<u8>, binary: bool) {
    let mut body = 1_i16.to_be_bytes().to_vec();
    body.extend_from_slice(b"n\0");
    body.extend_from_slice(&0_i32.to_be_bytes()); // table oid
    body.extend_from_slice(&0_i16.to_be_bytes()); // column attnum
    body.extend_from_slice(&23_i32.to_be_bytes()); // int4
    body.extend_from_slice(&4_i16.to_be_bytes()); // type size
    body.extend_from_slice(&(-1_i32).to_be_bytes()); // type modifier
    body.extend_from_slice(&i16::from(binary).to_be_bytes());
    write_tagged(out, Tag::ROW_DESCRIPTION, &body);
}

/// `t`: `count` parameters, all `int4`.
fn parameter_description(out: &mut Vec<u8>, count: usize) {
    let mut body = i16::try_from(count).unwrap_or(0).to_be_bytes().to_vec();
    for _ in 0..count {
        body.extend_from_slice(&23_i32.to_be_bytes());
    }
    write_tagged(out, Tag(b't'), &body);
}

/// What the harness remembers about a prepared statement.
#[derive(Clone, Copy, Debug, Default)]
struct StmtInfo {
    params: usize,
    large: bool,
}

/// Counts the distinct `$N` placeholders in a statement.
///
/// Crude, and enough for a harness: it takes the highest N rather than parsing
/// SQL, which is what Postgres reports too.
fn count_placeholders(sql: &str) -> usize {
    let bytes = sql.as_bytes();
    let mut highest = 0_usize;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end > start
                && let Ok(n) = sql[start..end].parse::<usize>()
            {
                highest = highest.max(n);
            }
            i = end;
        } else {
            i += 1;
        }
    }
    highest
}

/// How many rows the harness answers with, and how wide each value is.
///
///
/// Drivers previously only ever asked for one small row, so nothing exercised a
/// response larger than a single TCP segment. See `PGPROX_DEPTH_LARGE_RESULT` in
/// the driver scripts.
const LARGE_RESULT_ROWS: usize = 2_000;
const LARGE_RESULT_WIDTH: usize = 4_096;

/// `D`: one column holding the integer 1, encoded as the client asked.
fn data_row(out: &mut Vec<u8>, binary: bool) {
    let value: Vec<u8> = if binary {
        1_i32.to_be_bytes().to_vec()
    } else {
        b"1".to_vec()
    };
    let mut body = 1_i16.to_be_bytes().to_vec();
    body.extend_from_slice(&i32::try_from(value.len()).unwrap().to_be_bytes());
    body.extend_from_slice(&value);
    write_tagged(out, Tag::DATA_ROW, &body);
}

/// `C`: the command tag.
fn command_complete(out: &mut Vec<u8>, tag: &str) {
    let mut body = tag.as_bytes().to_vec();
    body.push(0);
    write_tagged(out, Tag::COMMAND_COMPLETE, &body);
}

/// `T`: one wide `text` column, for the large-result path.
fn large_row_description(out: &mut Vec<u8>) {
    let mut body = 1_i16.to_be_bytes().to_vec();
    body.extend_from_slice(b"payload\0");
    body.extend_from_slice(&0_i32.to_be_bytes());
    body.extend_from_slice(&0_i16.to_be_bytes());
    body.extend_from_slice(&25_i32.to_be_bytes()); // text
    body.extend_from_slice(&(-1_i16).to_be_bytes());
    body.extend_from_slice(&(-1_i32).to_be_bytes());
    body.extend_from_slice(&0_i16.to_be_bytes());
    write_tagged(out, Tag::ROW_DESCRIPTION, &body);
}

/// `D`: one wide text value.
fn large_data_row(out: &mut Vec<u8>) {
    let value = vec![b'x'; LARGE_RESULT_WIDTH];
    let mut body = 1_i16.to_be_bytes().to_vec();
    body.extend_from_slice(&i32::try_from(value.len()).unwrap().to_be_bytes());
    body.extend_from_slice(&value);
    write_tagged(out, Tag::DATA_ROW, &body);
}

fn write_tagged(out: &mut Vec<u8>, tag: Tag, body: &[u8]) {
    out.push(tag.get());
    out.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_be_bytes());
    out.extend_from_slice(body);
}
