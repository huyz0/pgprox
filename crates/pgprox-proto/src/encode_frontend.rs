//! Encoding frontend messages.
//!
//! The proxy speaks as a client to Postgres as well as a server to its own
//! clients, so it has to write these too. Opening an upstream connection means
//! sending a `StartupMessage` and answering whatever authentication method the
//! server asks for.
//!
//! # Why these live here rather than in the crate that opens connections
//!
//! Because they are already here twice otherwise. The conformance client
//! hand-rolled its own startup packet, and `pgprox-session` was about to write
//! a second one. A length prefix computed in three places is a length prefix
//! that is wrong in one of them.
//!
//! Every function appends to a caller-supplied buffer, so a connection can
//! reuse one rather than allocating per message.

use crate::frame::{LEN_PREFIX, Tag};

/// Writes a tagged message, filling in the length prefix afterwards.
fn tagged(out: &mut Vec<u8>, tag: Tag, body: impl FnOnce(&mut Vec<u8>)) {
    out.push(tag.get());
    let len_at = out.len();
    out.extend_from_slice(&[0; LEN_PREFIX]);

    body(out);

    let len = u32::try_from(out.len() - len_at).unwrap_or(u32::MAX);
    out[len_at..len_at + LEN_PREFIX].copy_from_slice(&len.to_be_bytes());
}

/// Writes an untagged message: a length prefix and a body, nothing else.
///
/// The startup exchange has no type tags, which is why it needs its own
/// writer rather than reusing the one above.
fn untagged(out: &mut Vec<u8>, body: impl FnOnce(&mut Vec<u8>)) {
    let len_at = out.len();
    out.extend_from_slice(&[0; LEN_PREFIX]);

    body(out);

    let len = u32::try_from(out.len() - len_at).unwrap_or(u32::MAX);
    out[len_at..len_at + LEN_PREFIX].copy_from_slice(&len.to_be_bytes());
}

/// Appends a null-terminated string.
fn cstr(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(text.as_bytes());
    out.push(0);
}

/// The startup packet: a protocol version and a parameter list.
///
/// The list is terminated by an empty key on top of each entry's own
/// terminator. Without it the server waits for a parameter that never comes,
/// which presents as a connection that hangs rather than as an error.
pub fn startup_message(out: &mut Vec<u8>, version: i32, params: &[(&str, &str)]) {
    untagged(out, |b| {
        b.extend_from_slice(&version.to_be_bytes());
        for (name, value) in params {
            cstr(b, name);
            cstr(b, value);
        }
        b.push(0);
    });
}

/// `SSLRequest`, asking the server to start TLS.
pub fn ssl_request(out: &mut Vec<u8>) {
    untagged(out, |b| {
        b.extend_from_slice(&crate::startup::SSL_REQUEST_CODE.to_be_bytes());
    });
}

/// `CancelRequest`, carrying the key the server issued.
///
/// Sent on a fresh connection that carries nothing else: the key is the whole
/// credential, which is why the proxy issues unpredictable ones of its own.
pub fn cancel_request(out: &mut Vec<u8>, process_id: i32, secret: i32) {
    untagged(out, |b| {
        b.extend_from_slice(&crate::startup::CANCEL_REQUEST_CODE.to_be_bytes());
        b.extend_from_slice(&process_id.to_be_bytes());
        b.extend_from_slice(&secret.to_be_bytes());
    });
}

/// `p`: a password, in the clear.
///
/// Null-terminated, unlike the SASL payloads below. The asymmetry is
/// Postgres's, not this crate's.
pub fn password_message(out: &mut Vec<u8>, password: &str) {
    tagged(out, Tag::PASSWORD, |b| cstr(b, password));
}

/// `p`: `SASLInitialResponse`, naming a mechanism and carrying client-first.
pub fn sasl_initial_response(out: &mut Vec<u8>, mechanism: &str, initial: &str) {
    tagged(out, Tag::PASSWORD, |b| {
        cstr(b, mechanism);
        let len = i32::try_from(initial.len()).unwrap_or(i32::MAX);
        b.extend_from_slice(&len.to_be_bytes());
        b.extend_from_slice(initial.as_bytes());
    });
}

/// `p`: `SASLResponse`, carrying client-final and nothing else.
pub fn sasl_response(out: &mut Vec<u8>, payload: &str) {
    tagged(out, Tag::PASSWORD, |b| {
        b.extend_from_slice(payload.as_bytes());
    });
}

/// `Q`: a simple query.
pub fn query(out: &mut Vec<u8>, sql: &str) {
    tagged(out, Tag::QUERY, |b| cstr(b, sql));
}

/// `P`: prepare a statement under a name.
pub fn parse(out: &mut Vec<u8>, statement: &str, sql: &str) {
    tagged(out, Tag::PARSE, |b| {
        cstr(b, statement);
        cstr(b, sql);
        // No parameter type OIDs: the server infers them, which is what every
        // driver does unless it has a reason not to.
        b.extend_from_slice(&0_i16.to_be_bytes());
    });
}

/// `B`: bind a portal to a prepared statement, with no parameters.
///
/// No parameter values, because the only `Bind` this crate writes is one a
/// test or a replay produces. A client's own `Bind` is rewritten in place by
/// `rewrite::bind_statement` rather than re-encoded, so its parameters never
/// pass through here.
pub fn bind(out: &mut Vec<u8>, portal: &str, statement: &str) {
    tagged(out, Tag::BIND, |b| {
        cstr(b, portal);
        cstr(b, statement);
        // No format codes, no parameters, and no result format codes.
        b.extend_from_slice(&0_i16.to_be_bytes());
        b.extend_from_slice(&0_i16.to_be_bytes());
        b.extend_from_slice(&0_i16.to_be_bytes());
    });
}

/// `C`: close a prepared statement.
///
/// The protocol-level counterpart of SQL `DEALLOCATE`, and the only one that
/// can name a statement the proxy prepared on the client's behalf.
pub fn close_statement(out: &mut Vec<u8>, statement: &str) {
    tagged(out, Tag::CLOSE, |b| {
        b.push(b'S');
        cstr(b, statement);
    });
}

/// `E`: run a bound portal to completion.
///
/// A row limit of zero, which means "all of them" and is what every driver
/// sends unless it is using a cursor.
pub fn execute(out: &mut Vec<u8>, portal: &str) {
    tagged(out, Tag::EXECUTE, |b| {
        cstr(b, portal);
        b.extend_from_slice(&0_i32.to_be_bytes());
    });
}

/// `S`: end an extended query sequence.
pub fn sync(out: &mut Vec<u8>) {
    tagged(out, Tag::SYNC, |_| {});
}

/// `X`: end the session politely.
///
/// Worth sending rather than just closing the socket: the server logs an
/// unexpected EOF as an error, and a proxy that closed thousands of pooled
/// connections without this would fill the server's log with its own noise.
pub fn terminate(out: &mut Vec<u8>) {
    tagged(out, Tag::TERMINATE, |_| {});
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::frame::{DEFAULT_MAX_FRAME, Decoded, decode, decode_untagged};
    use crate::frontend::{self, FrontendMessage, Target};
    use crate::startup::{self, Startup, StartupParam};

    /// Decodes exactly one tagged message, asserting nothing is left over.
    fn round_trip(bytes: &[u8]) -> FrontendMessage<'_> {
        let Decoded::Frame(frame, consumed) = decode(bytes, DEFAULT_MAX_FRAME).unwrap() else {
            panic!("encoded bytes should decode");
        };
        assert_eq!(consumed, bytes.len(), "length prefix disagrees with output");
        frontend::decode(&frame).unwrap()
    }

    /// The same for an untagged one.
    fn round_trip_untagged(bytes: &[u8]) -> Startup<'_> {
        let Decoded::Frame(frame, consumed) = decode_untagged(bytes, DEFAULT_MAX_FRAME).unwrap()
        else {
            panic!("encoded bytes should decode");
        };
        assert_eq!(consumed, bytes.len(), "length prefix disagrees with output");
        startup::decode(frame.body()).unwrap()
    }

    #[test]
    fn a_startup_packet_round_trips_with_its_parameters() {
        let mut out = Vec::new();
        startup_message(
            &mut out,
            crate::encode::PROTOCOL_3_0,
            &[("user", "acme_app"), ("database", "acme")],
        );

        let Startup::StartupMessage { version, params } = round_trip_untagged(&out) else {
            panic!("a startup packet decoded as something else");
        };
        assert_eq!(version, crate::encode::PROTOCOL_3_0);
        assert_eq!(
            params,
            vec![
                StartupParam {
                    name: "user",
                    value: "acme_app"
                },
                StartupParam {
                    name: "database",
                    value: "acme"
                },
            ]
        );
    }

    #[test]
    fn a_startup_packet_terminates_its_parameter_list() {
        // Without the extra null the server waits for a parameter that never
        // arrives, which looks like a hung connection rather than an error.
        let mut out = Vec::new();
        startup_message(&mut out, crate::encode::PROTOCOL_3_0, &[("user", "u")]);
        assert!(out.ends_with(b"u\0\0"), "{out:?}");
    }

    #[test]
    fn a_startup_packet_with_no_parameters_still_decodes() {
        let mut out = Vec::new();
        startup_message(&mut out, crate::encode::PROTOCOL_3_0, &[]);
        assert!(matches!(
            round_trip_untagged(&out),
            Startup::StartupMessage { .. }
        ));
    }

    #[test]
    fn an_ssl_request_round_trips() {
        let mut out = Vec::new();
        ssl_request(&mut out);
        assert_eq!(round_trip_untagged(&out), Startup::SslRequest);
    }

    #[test]
    fn a_cancel_request_round_trips_to_the_connection_it_names() {
        use pgprox_core::ids::{ConnId, NodeId};

        let conn = ConnId::new(NodeId::new(6), 1234);
        let (process_id, secret) = crate::backend::key_from_conn_id(conn);

        let mut out = Vec::new();
        cancel_request(&mut out, process_id, secret);
        assert_eq!(
            round_trip_untagged(&out),
            Startup::CancelRequest { conn },
            "a cancel key did not survive the round trip, so a cancel would reach nobody"
        );
    }

    #[test]
    fn an_execute_names_its_portal_and_asks_for_every_row() {
        // Round-tripped through this crate's own decoder, which is the check a
        // hand-written length prefix needs.
        let mut out = Vec::new();
        execute(&mut out, "my_portal");

        let decoded = crate::frame::decode(&out, crate::frame::DEFAULT_MAX_FRAME).unwrap();
        let crate::frame::Decoded::Frame(frame, consumed) = decoded else {
            panic!("an execute did not decode as a frame");
        };
        assert_eq!(consumed, out.len());
        assert_eq!(frame.tag(), Tag::EXECUTE);
        assert_eq!(frame.body(), b"my_portal\0\0\0\0\0");
    }

    #[test]
    fn a_password_message_is_null_terminated_and_a_sasl_response_is_not() {
        // Postgres's asymmetry, not this crate's, and getting it backwards
        // means a proof computed over a payload with a stray null in it.
        let mut password = Vec::new();
        password_message(&mut password, "hunter2");
        assert!(password.ends_with(b"hunter2\0"));

        let mut sasl = Vec::new();
        sasl_response(&mut sasl, "c=biws,r=NONCE,p=UFJPT0Y=");
        assert!(sasl.ends_with(b"p=UFJPT0Y="));
    }

    #[test]
    fn every_password_shaped_message_decodes_as_one() {
        // All three share the tag `p`, and the decoder deliberately does not
        // look inside any of them: the body is a credential.
        let mut out = Vec::new();
        password_message(&mut out, "hunter2");
        assert_eq!(round_trip(&out), FrontendMessage::Password);

        let mut out = Vec::new();
        sasl_initial_response(&mut out, "SCRAM-SHA-256", "n,,n=,r=NONCE");
        assert_eq!(round_trip(&out), FrontendMessage::Password);

        let mut out = Vec::new();
        sasl_response(&mut out, "c=biws,r=NONCE,p=UFJPT0Y=");
        assert_eq!(round_trip(&out), FrontendMessage::Password);
    }

    #[test]
    fn a_sasl_initial_response_declares_its_payload_length() {
        let mut out = Vec::new();
        sasl_initial_response(&mut out, "SCRAM-SHA-256", "n,,n=,r=NONCE");

        // Tag, length, mechanism and terminator, then the declared length.
        let at = 1 + LEN_PREFIX + "SCRAM-SHA-256".len() + 1;
        let declared = i32::from_be_bytes(out[at..at + 4].try_into().unwrap());
        assert_eq!(declared, i32::try_from("n,,n=,r=NONCE".len()).unwrap());
    }

    #[test]
    fn a_query_round_trips() {
        let mut out = Vec::new();
        query(&mut out, "SELECT pg_last_wal_replay_lsn()");
        assert_eq!(
            round_trip(&out),
            FrontendMessage::Query {
                sql: "SELECT pg_last_wal_replay_lsn()"
            }
        );
    }

    #[test]
    fn a_parse_round_trips_under_its_global_name() {
        let mut out = Vec::new();
        parse(&mut out, "pgprox_1a2b", "SELECT $1");
        assert_eq!(
            round_trip(&out),
            FrontendMessage::Parse {
                statement: "pgprox_1a2b",
                sql: "SELECT $1"
            }
        );
    }

    #[test]
    fn closing_a_statement_names_a_statement_rather_than_a_portal() {
        // The two share a message and differ by one byte, and closing the
        // wrong one leaves the map believing a statement is gone when it is
        // not.
        let mut out = Vec::new();
        close_statement(&mut out, "pgprox_1a2b");
        assert_eq!(
            round_trip(&out),
            FrontendMessage::Close {
                target: Target::Statement,
                name: "pgprox_1a2b"
            }
        );
    }

    #[test]
    fn sync_and_terminate_carry_nothing() {
        let mut out = Vec::new();
        sync(&mut out);
        assert_eq!(round_trip(&out), FrontendMessage::Sync);

        let mut out = Vec::new();
        terminate(&mut out);
        assert_eq!(round_trip(&out), FrontendMessage::Terminate);
    }

    #[test]
    fn a_whole_upstream_handshake_decodes_message_by_message() {
        // The check a hand-written length prefix needs: round-tripping one
        // message only proves the first one is right.
        let mut out = Vec::new();
        startup_message(&mut out, crate::encode::PROTOCOL_3_0, &[("user", "u")]);

        let Decoded::Frame(_, consumed) = decode_untagged(&out, DEFAULT_MAX_FRAME).unwrap() else {
            panic!("the startup packet did not decode");
        };
        let mut rest = out.split_off(consumed);
        assert!(rest.is_empty());

        password_message(&mut rest, "hunter2");
        query(&mut rest, "SELECT 1");
        sync(&mut rest);
        terminate(&mut rest);

        let mut remaining = rest.as_slice();
        let mut count = 0;
        while !remaining.is_empty() {
            let Decoded::Frame(frame, consumed) = decode(remaining, DEFAULT_MAX_FRAME).unwrap()
            else {
                panic!("a message declared a length longer than it wrote");
            };
            frontend::decode(&frame).unwrap();
            remaining = &remaining[consumed..];
            count += 1;
        }
        assert_eq!(count, 4, "the four messages did not decode as four");
    }
}
