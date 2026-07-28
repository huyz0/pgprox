//! Writes a starting corpus for the fuzz targets.
//!
//!   cargo run -p pgprox-proto --example `seed_corpus` -- fuzz/corpus
//!
//! # Why this is generated rather than extracted
//!
//! `M1F.25` asked for the reference proxies' protocol fixtures to be copied
//! into the corpus, so their accumulated edge cases would become ours. There
//! are none to copy. pgdog builds its messages in Rust and round-trips them,
//! pgbouncer and odyssey drive real servers through their integration suites,
//! and none of the three ships a file of wire bytes.
//!
//! What they do carry is a list of what they thought worth testing, and that
//! is the part worth having. Every shape below is one the references exercise:
//! the authentication ladder, the extended-query sequence, the messages whose
//! length field can disagree with their content, and the startup packet, which
//! is the one parser an unauthenticated peer reaches.
//!
//! A corpus is a starting point rather than a test. libFuzzer mutates these;
//! their value is that the first mutation begins from a frame that decodes
//! rather than from random bytes that do not.

use std::io::Write;

use pgprox_core::error::ClientError;
use pgprox_core::ids::{ConnId, NodeId};
use pgprox_proto::backend::TxStatus;
use pgprox_proto::{encode, encode_frontend};

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let root = args.next().unwrap_or_else(|| "fuzz/corpus".to_owned());

    let frames = frames();
    let messages = messages();

    write_all(&format!("{root}/frame_decode"), &frames)?;
    // The message target reads a tag byte and treats the rest as a body, so a
    // whole frame is the wrong shape for it and every frame here would decode
    // as a tag plus nonsense.
    write_all(&format!("{root}/message_decode"), &messages)?;

    // Nothing is printed. The workspace denies writing to either standard
    // stream outside a binary that exists to, and `scripts/fuzz.sh` reports
    // the outcome from the exit status anyway.
    Ok(())
}

/// Whole frames, as they arrive on the wire.
fn frames() -> Vec<(String, Vec<u8>)> {
    let mut out = startup_frames();
    out.extend(auth_frames());
    out.extend(extended_frames());
    out.extend(result_frames());
    out
}

/// The startup exchange, which is the only part of the protocol an
/// unauthenticated peer can reach and therefore the part that matters most.
fn startup_frames() -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    out.push((
        "startup-3.0".into(),
        built(|b| {
            encode_frontend::startup_message(
                b,
                pgprox_proto::encode::PROTOCOL_3_0,
                &[("user", "acme_app"), ("database", "tenant_acme")],
            );
        }),
    ));
    out.push((
        "startup-options".into(),
        built(|b| {
            encode_frontend::startup_message(
                b,
                pgprox_proto::encode::PROTOCOL_3_0,
                &[
                    ("user", "acme_app"),
                    ("options", "-c search_path=tenant,public"),
                ],
            );
        }),
    ));
    out.push(("ssl-request".into(), built(encode_frontend::ssl_request)));
    out.push((
        "cancel-request".into(),
        built(|b| {
            encode_frontend::cancel_request(b, 4242, 0x0BAD_F00D_u32.cast_signed());
        }),
    ));

    out
}

/// The authentication ladder.
///
/// Every proxy that gets SCRAM wrong gets it wrong here, and all three
/// references test each rung.
fn auth_frames() -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    out.push(("auth-ok".into(), built(encode::authentication_ok)));
    out.push((
        "auth-cleartext".into(),
        built(encode::authentication_cleartext_password),
    ));
    out.push((
        "auth-sasl".into(),
        built(|b| {
            encode::authentication_sasl(b, &["SCRAM-SHA-256"]);
        }),
    ));
    out.push((
        "auth-sasl-continue".into(),
        built(|b| {
            encode::authentication_sasl_continue(b, "r=abc,s=def,i=4096");
        }),
    ));
    out.push((
        "auth-sasl-final".into(),
        built(|b| {
            encode::authentication_sasl_final(b, "v=xyz");
        }),
    ));
    out.push((
        "password".into(),
        built(|b| {
            encode_frontend::password_message(b, "a-token");
        }),
    ));
    out.push((
        "sasl-initial".into(),
        built(|b| {
            encode_frontend::sasl_initial_response(b, "SCRAM-SHA-256", "n,,n=,r=abc");
        }),
    ));

    out
}

/// The extended query sequence, in the order a driver sends it.
fn extended_frames() -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    out.push((
        "parse".into(),
        built(|b| {
            encode_frontend::parse(b, "s1", "SELECT $1");
        }),
    ));
    out.push((
        "parse-unnamed".into(),
        built(|b| {
            encode_frontend::parse(b, "", "SELECT 1");
        }),
    ));
    out.push(("bind".into(), built(|b| encode_frontend::bind(b, "", "s1"))));
    out.push(("execute".into(), built(|b| encode_frontend::execute(b, ""))));
    out.push((
        "close-statement".into(),
        built(|b| {
            encode_frontend::close_statement(b, "s1");
        }),
    ));
    out.push(("sync".into(), built(encode_frontend::sync)));
    out.push(("terminate".into(), built(encode_frontend::terminate)));
    out.push((
        "query".into(),
        built(|b| {
            encode_frontend::query(b, "SELECT 1");
        }),
    ));
    // Several statements in one frame, which is what makes the simple protocol
    // harder than it looks: a classifier that reads only the first is wrong.
    out.push((
        "query-multi".into(),
        built(|b| {
            encode_frontend::query(b, "BEGIN; SET search_path = a; SELECT 1; COMMIT");
        }),
    ));

    out
}

/// The results, including the two whose length fields describe a count that
/// has to agree with what follows.
fn result_frames() -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let conn = ConnId::new(NodeId::new(1), 0x00AB_CDEF);
    out.push((
        "row-description".into(),
        built(|b| {
            encode::row_description(b, &["id", "name"]);
        }),
    ));
    out.push((
        "data-row".into(),
        built(|b| {
            encode::data_row(b, &["1".to_owned(), "acme".to_owned()]);
        }),
    ));
    out.push((
        "command-complete".into(),
        built(|b| {
            encode::command_complete(b, "SELECT 1");
        }),
    ));
    out.push((
        "ready-idle".into(),
        built(|b| {
            encode::ready_for_query(b, TxStatus::Idle);
        }),
    ));
    out.push((
        "ready-in-transaction".into(),
        built(|b| {
            encode::ready_for_query(b, TxStatus::InTransaction);
        }),
    ));
    out.push((
        "ready-failed".into(),
        built(|b| {
            encode::ready_for_query(b, TxStatus::Failed);
        }),
    ));
    out.push((
        "parameter-status".into(),
        built(|b| {
            encode::parameter_status(b, "server_version", "17.2");
        }),
    ));
    out.push((
        "backend-key-data".into(),
        built(|b| {
            encode::backend_key_data(b, conn);
        }),
    ));
    out.push((
        "error-response".into(),
        built(|b| {
            encode::error_response(b, &ClientError::Draining);
        }),
    ));
    out.push((
        "negotiate-version".into(),
        built(|b| {
            encode::negotiate_protocol_version(b, 2, &["_pq_.unknown"]);
        }),
    ));

    // Two frames in one buffer, which is what a real read looks like and what
    // a decoder that ignores `consumed` gets wrong.
    out.push((
        "pipelined".into(),
        built(|b| {
            encode_frontend::parse(b, "s1", "SELECT $1");
            encode_frontend::bind(b, "", "s1");
            encode_frontend::execute(b, "");
            encode_frontend::sync(b);
        }),
    ));

    out
}

/// Bodies with a tag byte in front, which is the shape `message_decode` reads.
fn messages() -> Vec<(String, Vec<u8>)> {
    // Every frame above, minus its four-byte length. The target builds its own
    // `Frame`, so the length would be read as body.
    frames()
        .into_iter()
        .filter_map(|(name, bytes)| {
            // An untagged frame, such as the startup packet, has no tag byte
            // to strip and belongs to the other target.
            let tagged = bytes.first().is_some_and(u8::is_ascii_alphanumeric);
            if !tagged || bytes.len() < 5 {
                return None;
            }
            let mut body = vec![bytes[0]];
            body.extend_from_slice(&bytes[5..]);
            Some((name, body))
        })
        .collect()
}

fn built(build: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut out = Vec::new();
    build(&mut out);
    out
}

fn write_all(dir: &str, inputs: &[(String, Vec<u8>)]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (name, bytes) in inputs {
        let mut file = std::fs::File::create(format!("{dir}/{name}"))?;
        file.write_all(bytes)?;
    }
    Ok(())
}
