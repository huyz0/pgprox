//! A Postgres that authenticates anyone and answers every query the same way.
//!
//! A real socket speaking the real protocol: everything the proxy sends it is
//! decoded by this project's own decoder, and everything it sends back goes
//! through the proxy's relay untouched.
//!
//! Test-only, and its own module because two test modules need it. It lived
//! inside `serve.rs`'s tests until `M17.4`, which is why `observatory.rs` had
//! no way to hold a real upstream connection: `reset_pool` closes idle
//! connections, a pool has none until something opens one, and opening one
//! means completing the startup handshake. The mutant that deleted the reset's
//! `idle_timeout` therefore survived, not because the behaviour is untestable
//! but because the only thing that could test it was in the wrong file.
//!
//! # Why a socket rather than a duplex
//!
//! The connector under test dials. `PgConnector` is generic over `Upstream`,
//! but the node wires it to the concrete `TcpUpstream`, so a test that wants a
//! connection the *node's own pool* holds has to give it an address. Every
//! other layer is tested against a duplex pair, and that is still right for
//! them.
//!
//! # What it answers
//!
//! Enough to be a server and no more: the startup handshake, a canned
//! completion for any query, the two replication positions the replica poller
//! asks for, and a copy-in that stays silent until the client finishes. The
//! transaction status is tracked, because that is what the relay releases on
//! and a fake that always answered `Idle` would make every session look
//! releasable mid-transaction.

// Test-only code, held to the same lints as the test modules it serves rather
// than to production's. `unwrap` on a bind that cannot fail, in a fake whose
// job is to be simple enough to read, is the same trade every `mod tests` in
// this crate already makes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::net::SocketAddr;
use std::time::Duration;

use pgprox_core::ids::{ConnId, NodeId};
use pgprox_proto::backend::TxStatus;
use pgprox_proto::encode;
use pgprox_proto::frame::Tag;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// How far the fake replica says it has replayed.
pub const REPLICA_REPLAYED: &str = "16/B374D848";

/// What this fake records when a client says goodbye properly.
///
/// A `Terminate` has an empty body, so recording the body would be recording
/// the empty string and no test could tell it from anything else. `M20.4`.
pub const TERMINATED: &str = "<terminate>";

/// Where the fake primary says the last write landed, which is ahead of it.
pub const PRIMARY_WRITTEN: &str = "16/C0000000";

/// Every statement each fake server was sent, by port.
///
/// A test asserting that something was replayed has to see what reached the
/// server, and the alternative is a fake per test that reads the same bytes a
/// different way.
fn seen() -> &'static std::sync::Mutex<std::collections::HashMap<u16, Vec<String>>> {
    static SEEN: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<u16, Vec<String>>>,
    > = std::sync::OnceLock::new();
    SEEN.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Records one thing a fake server was sent.
///
/// Exposed rather than the map, so a second fake in another module can share
/// the record without sharing the lock: `serve.rs` has one that speaks the
/// extended protocol and files a tag per frame rather than a statement.
pub fn record(addr: SocketAddr, entry: String) {
    seen()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(addr.port())
        .or_default()
        .push(entry);
}

/// What one fake server was sent.
#[must_use]
pub fn statements_seen(addr: SocketAddr) -> Vec<String> {
    seen()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&addr.port())
        .cloned()
        .unwrap_or_default()
}

/// Starts one, and answers immediately.
pub async fn fake_postgres() -> SocketAddr {
    fake_postgres_after(Duration::ZERO).await
}

/// A fake upstream that waits before answering its first connection.
///
/// Models the proxy-side work a client waits through after it has
/// authenticated: resolving a grant, fetching server parameters, taking a
/// connection from a pool. The client owes nothing during it.
#[allow(clippy::too_many_lines)]
pub async fn fake_postgres_after(delay: Duration) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                // The startup packet.
                let mut len = [0_u8; 4];
                if socket.read_exact(&mut len).await.is_err() {
                    return;
                }
                let mut body = vec![0; u32::from_be_bytes(len) as usize - 4];
                let _ = socket.read_exact(&mut body).await;

                let mut out = Vec::new();
                encode::authentication_ok(&mut out);
                encode::parameter_status(&mut out, "server_version", "17.2");
                encode::backend_key_data(&mut out, ConnId::new(NodeId::new(9), 0x00AB_CDEF));
                encode::ready_for_query(&mut out, TxStatus::Idle);
                let _ = socket.write_all(&out).await;

                // Then one canned answer per query.
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

                    // Said goodbye rather than vanished, which is the whole of
                    // what `M20.4` is about. Recorded and then gone: there is
                    // nothing to answer a `Terminate` with.
                    if Tag(header[0]) == Tag::TERMINATE {
                        record(addr, TERMINATED.to_owned());
                        return;
                    }

                    let mut out = Vec::new();
                    // The replica poller's own question, answered as a replica
                    // answers it: a replay position and t for
                    // pg_is_in_recovery. Every other query gets the canned
                    // completion below.
                    let sql = String::from_utf8_lossy(&body)
                        .trim_end_matches('\0')
                        .to_owned();
                    record(addr, sql.clone());

                    if sql.contains("pg_last_wal_replay_lsn") {
                        out.extend_from_slice(&text_row(&[Some(REPLICA_REPLAYED), Some("t")]));
                        encode::ready_for_query(&mut out, TxStatus::Idle);
                        if socket.write_all(&out).await.is_err() {
                            return;
                        }
                        continue;
                    }
                    // The position the proxy asks for after a write, ahead of
                    // anything the replica reports having replayed.
                    if sql.contains("pg_current_wal_insert_lsn") {
                        out.extend_from_slice(&text_row(&[Some(PRIMARY_WRITTEN)]));
                        encode::ready_for_query(&mut out, TxStatus::Idle);
                        if socket.write_all(&out).await.is_err() {
                            return;
                        }
                        continue;
                    }
                    // A copy-in, answered as the server answers one: an
                    // invitation, then nothing until the client says it is
                    // done.
                    if sql.contains("COPY") && sql.contains("FROM STDIN") {
                        if serve_copy_in(&mut socket).await.is_err() {
                            return;
                        }
                        continue;
                    }

                    // The transaction status is what the relay releases on, so
                    // the fake has to track it: answering Idle to a BEGIN
                    // would make every session look releasable while it was
                    // mid-transaction.
                    if sql.contains("BEGIN") {
                        in_transaction = true;
                    } else if sql.contains("COMMIT") || sql.contains("ROLLBACK") {
                        in_transaction = false;
                    }

                    // A row description, for anything that returns rows.
                    // Postgres sends one for every simple query with a result,
                    // and a fake that did not send it stored a payload shape no
                    // server produces: `M9.27` is what that hid.
                    if sql.to_uppercase().starts_with("SELECT") {
                        out.extend_from_slice(&row_description());
                    }
                    out.push(Tag::COMMAND_COMPLETE.get());
                    let text = b"SELECT 1\0";
                    out.extend_from_slice(&u32::try_from(text.len() + 4).unwrap().to_be_bytes());
                    out.extend_from_slice(text);
                    encode::ready_for_query(
                        &mut out,
                        if in_transaction {
                            TxStatus::InTransaction
                        } else {
                            TxStatus::Idle
                        },
                    );
                    if socket.write_all(&out).await.is_err() {
                        return;
                    }
                }
            });
        }
    });

    addr
}

/// Answers a `COPY ... FROM STDIN` the way a server does: an invitation,
/// silence until the client finishes, then a completion.
///
/// Silence is the point. A server that answered a copy-in immediately would not
/// reproduce the deadlock this fake exists to catch.
///
/// `M88.18`. A real server does not answer `CopyDone` and `CopyFail` the same
/// way: `CopyDone` completes the copy, `CopyFail` aborts it with an
/// `ErrorResponse` carrying `57014` (`query_canceled`, the code real Postgres
/// uses here) and nothing was copied. Answering both with the same
/// `CommandComplete` would make a test that aborts a copy with `CopyFail`
/// indistinguishable, through this fake, from one that let it finish.
async fn serve_copy_in(socket: &mut tokio::net::TcpStream) -> std::io::Result<()> {
    let mut invitation = vec![Tag::COPY_IN_RESPONSE.get()];
    // Length, the overall format (text), and no per-column formats.
    invitation.extend_from_slice(&7_u32.to_be_bytes());
    invitation.extend_from_slice(&[0, 0, 0]);
    socket.write_all(&invitation).await?;

    let mut failed = false;
    loop {
        let mut header = [0_u8; 5];
        socket.read_exact(&mut header).await?;
        let len = u32::from_be_bytes(header[1..].try_into().unwrap_or([0; 4])) as usize;
        let mut chunk = vec![0; len.saturating_sub(4)];
        socket.read_exact(&mut chunk).await?;
        if header[0] == Tag::COPY_FAIL.get() {
            failed = true;
            break;
        }
        if header[0] != Tag::COPY_DATA.get() {
            break;
        }
    }

    let mut done = Vec::new();
    if failed {
        done.push(Tag::ERROR_RESPONSE.get());
        let mut body = vec![b'S'];
        body.extend_from_slice(b"ERROR\0");
        body.push(b'C');
        body.extend_from_slice(b"57014\0");
        body.push(b'M');
        body.extend_from_slice(b"COPY from stdin failed: aborted by client\0");
        body.push(0);
        done.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_be_bytes());
        done.extend_from_slice(&body);
    } else {
        done.push(Tag::COMMAND_COMPLETE.get());
        let text = b"COPY 2\0";
        done.extend_from_slice(&u32::try_from(text.len() + 4).unwrap().to_be_bytes());
        done.extend_from_slice(text);
    }
    encode::ready_for_query(&mut done, TxStatus::Idle);
    socket.write_all(&done).await
}

/// A `RowDescription` for one text column.
///
/// What a real server sends before the rows of any query that returns them. It
/// is in the payload the cache stores, and whether it is there decides whether
/// an entry can answer a given client: a simple query is always owed one, and
/// an extended sequence only if it asked. See ADR 0022.
#[must_use]
pub fn row_description() -> Vec<u8> {
    let mut body = 1_i16.to_be_bytes().to_vec();
    body.extend_from_slice(b"abalance\0");
    body.extend_from_slice(&0_i32.to_be_bytes()); // table oid
    body.extend_from_slice(&0_i16.to_be_bytes()); // column
    body.extend_from_slice(&23_i32.to_be_bytes()); // int4
    body.extend_from_slice(&4_i16.to_be_bytes()); // width
    body.extend_from_slice(&(-1_i32).to_be_bytes()); // type modifier
    body.extend_from_slice(&0_i16.to_be_bytes()); // text format
    let mut out = vec![Tag::ROW_DESCRIPTION.get()];
    out.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_be_bytes());
    out.extend_from_slice(&body);
    out
}

/// One `DataRow` carrying text values.
#[must_use]
pub fn text_row(values: &[Option<&str>]) -> Vec<u8> {
    let mut body = i16::try_from(values.len()).unwrap().to_be_bytes().to_vec();
    for value in values {
        match *value {
            None => body.extend_from_slice(&(-1_i32).to_be_bytes()),
            Some(text) => {
                body.extend_from_slice(&i32::try_from(text.len()).unwrap().to_be_bytes());
                body.extend_from_slice(text.as_bytes());
            }
        }
    }
    let mut out = vec![Tag::DATA_ROW.get()];
    out.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_be_bytes());
    out.extend_from_slice(&body);
    out
}
