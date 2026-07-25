//! SCRAM-SHA-256 against a real Postgres configured to require it.
//!
//! RFC vectors prove the arithmetic. This proves the arithmetic is the one
//! Postgres actually expects, which is a different claim: an implementation can
//! match published vectors and still fail against a real server by getting the
//! message framing or the `AuthMessage` assembly wrong.
//!
//! The wire framing is written out inline rather than pulled from
//! `pgprox-proto`. Depending on it would be a sideways crate dependency, which
//! `standards/contracts.md` forbids, and length-prefixed messages are twenty
//! lines.

#![cfg(feature = "integration")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::Command;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use pgprox_auth::scram;
use pgprox_testkit::{Readiness, classify_startup_reply};

/// Postgres 18 with `scram-sha-256` required for every host connection.
struct ScramPostgres {
    name: String,
    port: u16,
}

impl ScramPostgres {
    fn start() -> Self {
        let name = format!("pgprox-scram-{}", std::process::id());
        let _ = Command::new("docker").args(["rm", "-f", &name]).output();

        let out = Command::new("docker")
            .args([
                "run",
                "-d",
                "--rm",
                "--name",
                &name,
                "-e",
                "POSTGRES_PASSWORD=scram-test-password",
                "-e",
                "POSTGRES_DB=conformance",
                // Force SCRAM rather than trust, which is the whole point.
                "-e",
                "POSTGRES_HOST_AUTH_METHOD=scram-sha-256",
                "-e",
                "POSTGRES_INITDB_ARGS=--auth-host=scram-sha-256",
                "-P",
                "postgres:18-alpine",
            ])
            .output()
            .expect("docker must be runnable");
        assert!(
            out.status.success(),
            "docker run failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        let mapped = Command::new("docker")
            .args(["port", &name, "5432/tcp"])
            .output()
            .expect("docker port failed");
        let text = String::from_utf8_lossy(&mapped.stdout);
        let port = text
            .lines()
            .next()
            .and_then(|l| l.rsplit(':').next())
            .and_then(|p| p.trim().parse().ok())
            .unwrap_or_else(|| panic!("no mapped port in {text:?}"));

        let pg = Self { name, port };
        pg.wait_ready();
        pg
    }

    /// Polls until the server answers a startup, rather than sleeping. A
    /// container accepts TCP well before its databases exist.
    fn wait_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while Instant::now() < deadline {
            if let Ok(mut sock) = TcpStream::connect(("127.0.0.1", self.port)) {
                sock.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                if send_startup(&mut sock).is_ok() {
                    let mut buf = Vec::new();
                    if let Ok((tag, body)) = read_message(&mut sock, &mut buf) {
                        match classify_startup_reply(tag, &body) {
                            Readiness::Ready => return,
                            Readiness::Failed => panic!(
                                "Postgres rejected the probe: {}",
                                String::from_utf8_lossy(&body)
                            ),
                            Readiness::NotYet => {}
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        panic!("Postgres did not become ready");
    }

    fn connect(&self) -> TcpStream {
        let mut sock = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        sock.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        send_startup(&mut sock).unwrap();
        sock
    }
}

impl Drop for ScramPostgres {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.name])
            .output();
    }
}

/// Protocol 3.0 startup with user and database.
fn send_startup(sock: &mut TcpStream) -> std::io::Result<()> {
    let mut body = 196_608_i32.to_be_bytes().to_vec();
    for (k, v) in [("user", "postgres"), ("database", "conformance")] {
        body.extend_from_slice(k.as_bytes());
        body.push(0);
        body.extend_from_slice(v.as_bytes());
        body.push(0);
    }
    body.push(0);

    let mut packet = u32::try_from(body.len() + 4)
        .unwrap()
        .to_be_bytes()
        .to_vec();
    packet.extend_from_slice(&body);
    sock.write_all(&packet)
}

/// Reads one tagged message, returning its tag and body.
fn read_message(sock: &mut TcpStream, buf: &mut Vec<u8>) -> std::io::Result<(u8, Vec<u8>)> {
    loop {
        if buf.len() >= 5 {
            let len = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
            let total = 1 + len;
            if buf.len() >= total {
                let tag = buf[0];
                let body = buf[5..total].to_vec();
                buf.drain(..total);
                return Ok((tag, body));
            }
        }
        let mut chunk = [0_u8; 8192];
        let n = sock.read(&mut chunk)?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "server closed",
            ));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

fn send_tagged(sock: &mut TcpStream, tag: u8, body: &[u8]) -> std::io::Result<()> {
    let mut out = vec![tag];
    out.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_be_bytes());
    out.extend_from_slice(body);
    sock.write_all(&out)
}

/// Completes a SCRAM exchange.
///
/// The read buffer is threaded in rather than owned locally, because a read can
/// pull in bytes past the handshake and dropping them would lose the start of
/// the session. The real relay has the same hazard at every handoff.
fn authenticate(sock: &mut TcpStream, buf: &mut Vec<u8>, password: &str) -> Result<(), String> {
    // AuthenticationSASL, listing the mechanisms on offer.
    let (tag, body) = read_message(sock, buf).map_err(|e| e.to_string())?;
    assert_eq!(tag, b'R', "expected an Authentication message");
    let subtype = i32::from_be_bytes([body[0], body[1], body[2], body[3]]);
    assert_eq!(subtype, 10, "server did not ask for SASL");

    let offered = String::from_utf8_lossy(&body[4..]);
    assert!(
        offered.contains("SCRAM-SHA-256"),
        "server offered {offered:?}"
    );

    // SASLInitialResponse: mechanism name, then a length-prefixed payload.
    let nonce = scram::generate_nonce();
    // Empty username: Postgres takes it from the startup packet.
    let client_first = scram::client_first("", &nonce);
    let mut initial = b"SCRAM-SHA-256\0".to_vec();
    initial.extend_from_slice(&i32::try_from(client_first.len()).unwrap().to_be_bytes());
    initial.extend_from_slice(client_first.as_bytes());
    send_tagged(sock, b'p', &initial).map_err(|e| e.to_string())?;

    // AuthenticationSASLContinue, carrying server-first.
    let (tag, body) = read_message(sock, buf).map_err(|e| e.to_string())?;
    if tag == b'E' {
        return Err(format!("server error: {}", String::from_utf8_lossy(&body)));
    }
    assert_eq!(tag, b'R');
    let subtype = i32::from_be_bytes([body[0], body[1], body[2], body[3]]);
    assert_eq!(subtype, 11, "expected SASLContinue");

    let server_first = String::from_utf8_lossy(&body[4..]).into_owned();
    let parsed = scram::parse_server_first(&server_first, &nonce).map_err(|e| e.to_string())?;

    let keys = scram::ScramKeys::derive(password.as_bytes(), &parsed.salt, parsed.iterations)
        .map_err(|e| e.to_string())?;

    let final_bare = scram::client_final_without_proof(&parsed.nonce);
    let auth_message = scram::auth_message(
        &scram::client_first_bare("", &nonce),
        &server_first,
        &final_bare,
    );
    let proof = scram::client_proof(&keys, &auth_message);
    let client_final = format!("{final_bare},p={}", BASE64.encode(proof));
    send_tagged(sock, b'p', client_final.as_bytes()).map_err(|e| e.to_string())?;

    // AuthenticationSASLFinal, then AuthenticationOk.
    let (tag, body) = read_message(sock, buf).map_err(|e| e.to_string())?;
    if tag == b'E' {
        return Err(format!("server error: {}", String::from_utf8_lossy(&body)));
    }
    let subtype = i32::from_be_bytes([body[0], body[1], body[2], body[3]]);
    assert_eq!(subtype, 12, "expected SASLFinal");

    let server_final = String::from_utf8_lossy(&body[4..]).into_owned();
    scram::verify_server_final(&server_final, &keys, &auth_message).map_err(|e| e.to_string())?;

    let (tag, body) = read_message(sock, buf).map_err(|e| e.to_string())?;
    assert_eq!(tag, b'R');
    let subtype = i32::from_be_bytes([body[0], body[1], body[2], body[3]]);
    assert_eq!(subtype, 0, "expected AuthenticationOk");

    Ok(())
}

#[test]
#[ignore = "requires docker"]
fn scram_authenticates_against_real_postgres() {
    // The claim RFC vectors cannot make: that this is the exchange Postgres
    // expects, framing and AuthMessage assembly included.
    let pg = ScramPostgres::start();
    let mut sock = pg.connect();
    let mut buf = Vec::new();

    authenticate(&mut sock, &mut buf, "scram-test-password").expect("SCRAM should succeed");

    // Prove the session is genuinely usable afterwards, not merely that the
    // handshake did not error.
    loop {
        let (tag, _) = read_message(&mut sock, &mut buf).unwrap();
        if tag == b'Z' {
            break;
        }
    }
    let mut query = b"SELECT 1".to_vec();
    query.push(0);
    send_tagged(&mut sock, b'Q', &query).unwrap();

    let mut saw_row = false;
    loop {
        let (tag, _) = read_message(&mut sock, &mut buf).unwrap();
        if tag == b'D' {
            saw_row = true;
        }
        if tag == b'Z' {
            break;
        }
    }
    assert!(saw_row, "authenticated but could not query");
}

#[test]
#[ignore = "requires docker"]
fn scram_rejects_a_wrong_password() {
    // The half that matters more: an implementation that always succeeds would
    // pass the test above.
    let pg = ScramPostgres::start();
    let mut sock = pg.connect();

    let mut buf = Vec::new();
    let err = authenticate(&mut sock, &mut buf, "not-the-password")
        .expect_err("a wrong password must not authenticate");
    assert!(
        err.contains("28P01") || err.to_lowercase().contains("password"),
        "expected an authentication failure, got: {err}"
    );
}

#[test]
#[ignore = "requires docker"]
fn the_server_signature_is_actually_checked() {
    // Verifies our verification: a real server-final message must pass, and the
    // same message under different keys must not. Without this, verify could be
    // returning Ok unconditionally and both tests above would still pass.
    let pg = ScramPostgres::start();
    let mut sock = pg.connect();
    let mut buf = Vec::new();

    read_message(&mut sock, &mut buf).unwrap();
    let nonce = scram::generate_nonce();
    let client_first = scram::client_first("", &nonce);
    let mut initial = b"SCRAM-SHA-256\0".to_vec();
    initial.extend_from_slice(&i32::try_from(client_first.len()).unwrap().to_be_bytes());
    initial.extend_from_slice(client_first.as_bytes());
    send_tagged(&mut sock, b'p', &initial).unwrap();

    let (_, body) = read_message(&mut sock, &mut buf).unwrap();
    let server_first = String::from_utf8_lossy(&body[4..]).into_owned();
    let parsed = scram::parse_server_first(&server_first, &nonce).unwrap();

    let real =
        scram::ScramKeys::derive(b"scram-test-password", &parsed.salt, parsed.iterations).unwrap();
    let final_bare = scram::client_final_without_proof(&parsed.nonce);
    let auth_message = scram::auth_message(
        &scram::client_first_bare("", &nonce),
        &server_first,
        &final_bare,
    );
    let proof = scram::client_proof(&real, &auth_message);
    let client_final = format!("{final_bare},p={}", BASE64.encode(proof));
    send_tagged(&mut sock, b'p', client_final.as_bytes()).unwrap();

    let (tag, body) = read_message(&mut sock, &mut buf).unwrap();
    assert_eq!(tag, b'R');
    let server_final = String::from_utf8_lossy(&body[4..]).into_owned();

    assert!(
        scram::verify_server_final(&server_final, &real, &auth_message).is_ok(),
        "a genuine server signature was rejected"
    );

    let impostor = scram::ScramKeys::derive(b"different", &parsed.salt, parsed.iterations).unwrap();
    assert!(
        scram::verify_server_final(&server_final, &impostor, &auth_message).is_err(),
        "verification accepts any signature, so it proves nothing"
    );
}
