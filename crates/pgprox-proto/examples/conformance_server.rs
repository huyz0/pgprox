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
    sock.set_nodelay(true)?;
    let mut buf = Vec::new();

    // Startup phase. A client may send SSLRequest or GSSENCRequest first, and
    // each gets a single-byte answer before the real startup packet arrives.
    //
    // Only the version is kept from the decoded packet. Holding the whole
    // Startup would borrow from a buffer that goes out of scope each iteration.
    let version = loop {
        let body = read_untagged(&mut sock, &mut buf)?;
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
    match startup::negotiate_version(version) {
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

    // Query phase.
    loop {
        let (tag, body) = match read_tagged(&mut sock, &mut buf) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e),
        };

        let frame = pgprox_proto::Frame::new(tag, &body);
        let msg = frontend::decode(&frame).map_err(|e| std::io::Error::other(e.to_string()))?;

        let mut out = Vec::new();
        match msg {
            FrontendMessage::Terminate => return Ok(()),

            FrontendMessage::Query { .. } => {
                row_description(&mut out);
                data_row(&mut out, b"1");
                command_complete(&mut out, "SELECT 1");
                encode::ready_for_query(&mut out, TxStatus::Idle);
            }

            // The extended query path. Every modern driver uses it, and its
            // messages are answered individually rather than as one block.
            FrontendMessage::Parse { .. } => simple(&mut out, Tag::PARSE_COMPLETE),
            FrontendMessage::Bind { .. } => simple(&mut out, Tag::BIND_COMPLETE),
            FrontendMessage::Close { .. } => simple(&mut out, Tag::CLOSE_COMPLETE),
            FrontendMessage::Describe { target, .. } => match target {
                // Describing a statement yields its parameter types first.
                frontend::Target::Statement => {
                    parameter_description(&mut out);
                    row_description(&mut out);
                }
                _ => row_description(&mut out),
            },
            FrontendMessage::Execute { .. } => {
                data_row(&mut out, b"1");
                command_complete(&mut out, "SELECT 1");
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

/// `T`: one `int4` column named `n`.
fn row_description(out: &mut Vec<u8>) {
    let mut body = 1_i16.to_be_bytes().to_vec();
    body.extend_from_slice(b"n\0");
    body.extend_from_slice(&0_i32.to_be_bytes()); // table oid
    body.extend_from_slice(&0_i16.to_be_bytes()); // column attnum
    body.extend_from_slice(&23_i32.to_be_bytes()); // int4
    body.extend_from_slice(&4_i16.to_be_bytes()); // type size
    body.extend_from_slice(&(-1_i32).to_be_bytes()); // type modifier
    body.extend_from_slice(&0_i16.to_be_bytes()); // text format
    write_tagged(out, Tag::ROW_DESCRIPTION, &body);
}

/// `t`: one `int4` parameter.
fn parameter_description(out: &mut Vec<u8>) {
    let mut body = 1_i16.to_be_bytes().to_vec();
    body.extend_from_slice(&23_i32.to_be_bytes());
    write_tagged(out, Tag(b't'), &body);
}

/// `D`: one column holding `value`.
fn data_row(out: &mut Vec<u8>, value: &[u8]) {
    let mut body = 1_i16.to_be_bytes().to_vec();
    body.extend_from_slice(&i32::try_from(value.len()).unwrap().to_be_bytes());
    body.extend_from_slice(value);
    write_tagged(out, Tag::DATA_ROW, &body);
}

/// `C`: the command tag.
fn command_complete(out: &mut Vec<u8>, tag: &str) {
    let mut body = tag.as_bytes().to_vec();
    body.push(0);
    write_tagged(out, Tag::COMMAND_COMPLETE, &body);
}

fn write_tagged(out: &mut Vec<u8>, tag: Tag, body: &[u8]) {
    out.push(tag.get());
    out.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_be_bytes());
    out.extend_from_slice(body);
}
