//! Encoding backend messages.
//!
//! The proxy speaks as a server to its clients, so it writes these itself
//! rather than only forwarding them.
//!
//! Every function appends to a caller-supplied buffer, so the relay path can
//! reuse a pooled buffer instead of allocating per message.

use pgprox_core::error::ClientError;
use pgprox_core::ids::ConnId;

use crate::backend::{TxStatus, key_from_conn_id};
use crate::frame::{LEN_PREFIX, Tag};

/// Protocol major version 3, minor 0. What the proxy speaks.
pub const PROTOCOL_3_0: i32 = 196_608;

/// Protocol major version 3, minor 2, introduced in Postgres 18.
pub const PROTOCOL_3_2: i32 = 196_610;

/// Writes a tagged message, filling in the length prefix afterwards.
///
/// The length counts itself and the body but not the tag, which is the detail
/// that makes hand-writing these error-prone. Doing it in one place means
/// getting it right once.
fn tagged(out: &mut Vec<u8>, tag: Tag, body: impl FnOnce(&mut Vec<u8>)) {
    out.push(tag.get());
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

/// `R` with subtype 0: authentication succeeded.
pub fn authentication_ok(out: &mut Vec<u8>) {
    tagged(out, Tag::AUTHENTICATION, |b| {
        b.extend_from_slice(&0_i32.to_be_bytes());
    });
}

/// `R` with subtype 3: send the password in the clear.
///
/// This is how a JWT arrives. It requires TLS on the frontend, which the
/// listener enforces before ever sending this.
pub fn authentication_cleartext_password(out: &mut Vec<u8>) {
    tagged(out, Tag::AUTHENTICATION, |b| {
        b.extend_from_slice(&3_i32.to_be_bytes());
    });
}

/// `S`: a runtime parameter and its value.
pub fn parameter_status(out: &mut Vec<u8>, name: &str, value: &str) {
    tagged(out, Tag::PARAMETER_STATUS, |b| {
        cstr(b, name);
        cstr(b, value);
    });
}

/// `K`: the cancellation key for this session.
///
/// The proxy issues its own key with the owning node encoded in it, so a
/// `CancelRequest` arriving at any pod can be forwarded to the pod that owns
/// the connection.
pub fn backend_key_data(out: &mut Vec<u8>, conn: ConnId) {
    let (process_id, secret) = key_from_conn_id(conn);
    tagged(out, Tag::BACKEND_KEY_DATA, |b| {
        b.extend_from_slice(&process_id.to_be_bytes());
        b.extend_from_slice(&secret.to_be_bytes());
    });
}

/// `Z`: ready for query, carrying the transaction status.
pub fn ready_for_query(out: &mut Vec<u8>, status: TxStatus) {
    let byte = match status {
        TxStatus::Idle => b'I',
        TxStatus::InTransaction => b'T',
        TxStatus::Failed => b'E',
    };
    tagged(out, Tag::READY_FOR_QUERY, |b| b.push(byte));
}

/// `T`: the shape of a result the proxy answered itself.
///
/// Every column is text (`OID` 25), because the only results this proxy
/// produces are `SHOW` output, and a `SHOW` is text. A relayed result is never
/// built here: the server's own `RowDescription` passes through untouched.
pub fn row_description(out: &mut Vec<u8>, columns: &[&str]) {
    tagged(out, Tag::ROW_DESCRIPTION, |b| {
        let count = i16::try_from(columns.len()).unwrap_or(i16::MAX);
        b.extend_from_slice(&count.to_be_bytes());
        // Truncated to the count, not merely counted. A saturating conversion
        // that still writes every item produces a message whose header and body
        // disagree, and the client reads the next message from the middle of
        // this one. Unreachable from any caller here, and two lines, and
        // `encode_frontend::bind_with_parameters` already does it this way.
        for column in columns.iter().take(usize::try_from(count).unwrap_or(0)) {
            cstr(b, column);
            // No table and no column: this row came from the proxy, not from a
            // relation, and claiming otherwise would make a client's metadata
            // lookup point at something that does not exist.
            b.extend_from_slice(&0_i32.to_be_bytes());
            b.extend_from_slice(&0_i16.to_be_bytes());
            // text
            b.extend_from_slice(&25_i32.to_be_bytes());
            b.extend_from_slice(&(-1_i16).to_be_bytes());
            b.extend_from_slice(&(-1_i32).to_be_bytes());
            // Text format, matching the type above.
            b.extend_from_slice(&0_i16.to_be_bytes());
        }
    });
}

/// `D`: one row of a result the proxy answered itself.
pub fn data_row(out: &mut Vec<u8>, values: &[String]) {
    tagged(out, Tag::DATA_ROW, |b| {
        let count = i16::try_from(values.len()).unwrap_or(i16::MAX);
        b.extend_from_slice(&count.to_be_bytes());
        for value in values.iter().take(usize::try_from(count).unwrap_or(0)) {
            b.extend_from_slice(&i32::try_from(value.len()).unwrap_or(i32::MAX).to_be_bytes());
            b.extend_from_slice(value.as_bytes());
        }
    });
}

/// `C`: the tag that ends a result the proxy answered itself.
pub fn command_complete(out: &mut Vec<u8>, tag: &str) {
    tagged(out, Tag::COMMAND_COMPLETE, |b| cstr(b, tag));
}

/// `E`: an error, built from the shared taxonomy.
///
/// The SQLSTATE comes from [`ClientError::sqlstate`] and the text from
/// [`ClientError::client_message`], so the vague-to-clients and detailed-to-
/// operators split is impossible to get wrong here: this function has no access
/// to the operator detail.
pub fn error_response(out: &mut Vec<u8>, error: &ClientError) {
    tagged(out, Tag::ERROR_RESPONSE, |b| {
        b.push(b'S');
        cstr(b, "ERROR");
        // `V` is the non-localized severity, which drivers prefer to parse.
        b.push(b'V');
        cstr(b, "ERROR");
        b.push(b'C');
        cstr(b, error.sqlstate().as_str());
        b.push(b'M');
        cstr(b, error.client_message());
        b.push(0);
    });
}

/// `v`: the server supports a lower minor version than the client asked for.
///
/// Sent when a client requests 3.2 and this proxy speaks 3.0. Every 3.2-capable
/// driver handles this by design, which is what makes supporting only 3.0
/// safe for now.
pub fn negotiate_protocol_version(out: &mut Vec<u8>, minor: i32, unrecognized: &[&str]) {
    tagged(out, Tag::NEGOTIATE_PROTOCOL_VERSION, |b| {
        b.extend_from_slice(&minor.to_be_bytes());
        let count = i32::try_from(unrecognized.len()).unwrap_or(i32::MAX);
        b.extend_from_slice(&count.to_be_bytes());
        for option in unrecognized
            .iter()
            .take(usize::try_from(count).unwrap_or(0))
        {
            cstr(b, option);
        }
    });
}

/// `R` with subtype 10: begin SASL, offering these mechanisms.
///
/// The list is written in the order given, which is the order a client is
/// expected to prefer. `SCRAM-SHA-256-PLUS` is deliberately not something this
/// proxy offers: it terminates TLS itself, so the binding a client would verify
/// is to the proxy rather than to the database.
pub fn authentication_sasl(out: &mut Vec<u8>, mechanisms: &[&str]) {
    tagged(out, Tag::AUTHENTICATION, |b| {
        b.extend_from_slice(&10_i32.to_be_bytes());
        for mechanism in mechanisms {
            cstr(b, mechanism);
        }
        // The list is itself null-terminated, on top of each entry being so. A
        // client reading one terminator and stopping would hang waiting for a
        // message the server already sent.
        b.push(0);
    });
}

/// `R` with subtype 11: a SASL challenge carrying the server-first message.
///
/// The payload is not null-terminated: SASL data is length-counted by the
/// enclosing frame, and a terminator would end up inside the client's
/// `AuthMessage` and break every proof it computes.
pub fn authentication_sasl_continue(out: &mut Vec<u8>, payload: &str) {
    tagged(out, Tag::AUTHENTICATION, |b| {
        b.extend_from_slice(&11_i32.to_be_bytes());
        b.extend_from_slice(payload.as_bytes());
    });
}

/// `R` with subtype 12: SASL succeeded, carrying the server-final message.
///
/// Followed by an `AuthenticationOk`, which is what actually ends
/// authentication. A client that got this and nothing else would wait forever.
pub fn authentication_sasl_final(out: &mut Vec<u8>, payload: &str) {
    tagged(out, Tag::AUTHENTICATION, |b| {
        b.extend_from_slice(&12_i32.to_be_bytes());
        b.extend_from_slice(payload.as_bytes());
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod answered_tests {
    use super::*;

    /// Splits one frame into its tag and body.
    fn frame(bytes: &[u8]) -> (Tag, Vec<u8>) {
        let len = u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize;
        assert_eq!(len + 1, bytes.len(), "the length prefix is wrong");
        (Tag(bytes[0]), bytes[5..].to_vec())
    }

    #[test]
    fn a_count_never_disagrees_with_the_list_it_counts() {
        // The count is an i16 and the list is a slice, so past 32767 the
        // conversion saturates. Writing every item anyway produces a message
        // whose header says one thing and whose body says another, and a client
        // reading it takes the next message from the middle of this one.
        //
        // No caller here can reach it: everything this proxy answers itself is
        // SHOW output. It is asserted rather than argued because the correct
        // version of this loop is already in the workspace, in
        // `encode_frontend::bind_with_parameters`, so three of the four
        // encoders had simply not been given it.
        let too_many = usize::try_from(i16::MAX).unwrap() + 1;

        let values: Vec<String> = (0..too_many).map(|_| String::new()).collect();
        let mut out = Vec::new();
        data_row(&mut out, &values);
        let (_, body) = frame(&out);

        let declared = i16::from_be_bytes(body[..2].try_into().unwrap());
        assert_eq!(declared, i16::MAX);
        // Every value is empty, so each is exactly its four-byte length.
        let written = (body.len() - 2) / 4;
        assert_eq!(
            written,
            usize::try_from(declared).unwrap(),
            "the body carries {written} values and the header claims {declared}"
        );

        // Same shape, same fix, for a row description.
        let names: Vec<&str> = vec![""; too_many];
        let mut out = Vec::new();
        row_description(&mut out, &names);
        let (_, body) = frame(&out);

        let declared = i16::from_be_bytes(body[..2].try_into().unwrap());
        assert_eq!(declared, i16::MAX);
        // An empty name is one null byte, then eighteen bytes of fixed fields.
        let written = (body.len() - 2) / 19;
        assert_eq!(written, usize::try_from(declared).unwrap());
    }

    #[test]
    fn a_row_description_names_its_columns_as_text() {
        // Everything this proxy answers itself is SHOW output, and a SHOW is
        // text. A column claiming another type would have a client parse the
        // bytes as something they are not.
        let mut out = Vec::new();
        row_description(&mut out, &["database", "pool_mode"]);

        let (tag, body) = frame(&out);
        assert_eq!(tag, Tag::ROW_DESCRIPTION);
        assert_eq!(i16::from_be_bytes(body[..2].try_into().unwrap()), 2);
        assert!(body.windows(9).any(|w| w == b"database\0"));
        // The type OID follows the name, the table OID and the column index.
        let at = 2 + "database\0".len() + 4 + 2;
        assert_eq!(i32::from_be_bytes(body[at..at + 4].try_into().unwrap()), 25);
    }

    #[test]
    fn a_column_carries_all_seven_of_its_fields() {
        // The test above reads the type OID by counting past the fields before
        // it, which only says where that OID is. What follows it matters just
        // as much: `typlen` and `typmod` are both -1, meaning a variable-length
        // value with no modifier, and a client that read 1 there would take the
        // next byte as the start of the next column.
        let mut out = Vec::new();
        row_description(&mut out, &["node"]);

        let (_, body) = frame(&out);
        let at = 2 + "node\0".len();
        assert_eq!(
            &body[at..],
            &[
                0, 0, 0, 0, // no table OID: this row came from the proxy
                0, 0, // and so no column index within one
                0, 0, 0, 25, // text
                0xff, 0xff, // typlen -1, a varlena
                0xff, 0xff, 0xff, 0xff, // typmod -1, no modifier
                0, 0, // text format, matching the type
            ]
        );
    }

    #[test]
    fn a_data_row_carries_its_values_with_lengths() {
        let mut out = Vec::new();
        data_row(&mut out, &["acme".to_owned(), "transaction".to_owned()]);

        let (tag, body) = frame(&out);
        assert_eq!(tag, Tag::DATA_ROW);
        assert_eq!(i16::from_be_bytes(body[..2].try_into().unwrap()), 2);
        assert_eq!(i32::from_be_bytes(body[2..6].try_into().unwrap()), 4);
        assert_eq!(&body[6..10], b"acme");
    }

    #[test]
    fn an_empty_result_is_a_row_description_with_no_rows() {
        // Which is what a SHOW that matched nothing looks like. A client that
        // got no RowDescription at all would have no columns to render.
        let mut description = Vec::new();
        row_description(&mut description, &["node"]);
        let mut completion = Vec::new();
        command_complete(&mut completion, "SHOW");

        assert_eq!(frame(&description).0, Tag::ROW_DESCRIPTION);
        assert_eq!(frame(&completion).0, Tag::COMMAND_COMPLETE);
    }

    #[test]
    fn a_command_complete_carries_its_tag() {
        let mut out = Vec::new();
        command_complete(&mut out, "SHOW");

        let (tag, body) = frame(&out);
        assert_eq!(tag, Tag::COMMAND_COMPLETE);
        assert_eq!(body, b"SHOW\0");
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::backend::{self, AuthRequest, BackendMessage};
    use crate::frame::{DEFAULT_MAX_FRAME, Decoded, decode};
    use pgprox_core::error::AuthRejection;
    use pgprox_core::ids::{NodeId, ServerId};

    /// Decodes exactly one message from `bytes`, asserting nothing is left over.
    fn round_trip(bytes: &[u8]) -> BackendMessage<'_> {
        let Decoded::Frame(frame, consumed) = decode(bytes, DEFAULT_MAX_FRAME).unwrap() else {
            unreachable!("encoded bytes should decode");
        };
        assert_eq!(consumed, bytes.len(), "length prefix disagrees with output");
        backend::decode(&frame).unwrap()
    }

    #[test]
    fn authentication_ok_round_trips() {
        let mut out = Vec::new();
        authentication_ok(&mut out);
        assert_eq!(
            round_trip(&out),
            BackendMessage::Authentication(AuthRequest::Ok)
        );
    }

    #[test]
    fn cleartext_password_request_round_trips() {
        let mut out = Vec::new();
        authentication_cleartext_password(&mut out);
        assert_eq!(
            round_trip(&out),
            BackendMessage::Authentication(AuthRequest::CleartextPassword)
        );
    }

    #[test]
    fn parameter_status_round_trips() {
        let mut out = Vec::new();
        parameter_status(&mut out, "server_version", "18.4");
        assert_eq!(
            round_trip(&out),
            BackendMessage::ParameterStatus {
                name: "server_version",
                value: "18.4",
            }
        );
    }

    #[test]
    fn backend_key_data_round_trips_through_the_wire_and_back_to_a_conn_id() {
        // The whole cross-pod cancellation path in one assertion.
        let conn = ConnId::new(NodeId::new(3), 0xDEAD_BEEF);
        let mut out = Vec::new();
        backend_key_data(&mut out, conn);

        let BackendMessage::BackendKeyData { process_id, secret } = round_trip(&out) else {
            unreachable!()
        };
        assert_eq!(backend::conn_id_from_key(process_id, secret), conn);
        assert_eq!(
            backend::conn_id_from_key(process_id, secret).node(),
            NodeId::new(3)
        );
    }

    #[test]
    fn ready_for_query_round_trips_every_status() {
        for status in [TxStatus::Idle, TxStatus::InTransaction, TxStatus::Failed] {
            let mut out = Vec::new();
            ready_for_query(&mut out, status);
            assert_eq!(round_trip(&out), BackendMessage::ReadyForQuery(status));
        }
    }

    #[test]
    fn error_response_carries_the_mapped_sqlstate() {
        // The mapping is the shared one, so an encoder cannot invent a code.
        let cases = [
            (ClientError::Draining, "57P01"),
            (ClientError::TlsRequired, "28000"),
            (
                ClientError::AuthRefused(AuthRejection::TokenExpired),
                "28000",
            ),
            (ClientError::SidecarUnavailable, "08006"),
        ];

        for (error, expected_code) in cases {
            let mut out = Vec::new();
            error_response(&mut out, &error);

            let BackendMessage::ErrorResponse(fields) = round_trip(&out) else {
                unreachable!()
            };
            assert_eq!(fields.code, expected_code, "wrong code for {error}");
            assert_eq!(fields.severity, "ERROR");
            assert_eq!(fields.message, error.client_message());
        }
    }

    #[test]
    fn an_encoded_error_never_carries_operator_detail() {
        // This function has no access to the operator form, which is what makes
        // the leak impossible rather than merely avoided.
        let error = ClientError::UpstreamAtCap {
            server: ServerId::new("db-secret.internal", 5432),
            cap: 4096,
        };
        let mut out = Vec::new();
        error_response(&mut out, &error);

        let rendered = String::from_utf8_lossy(&out);
        assert!(!rendered.contains("db-secret.internal"), "{rendered}");
        assert!(!rendered.contains("4096"), "{rendered}");
        assert!(rendered.contains("53300"));
    }

    #[test]
    fn negotiate_protocol_version_round_trips() {
        let mut out = Vec::new();
        negotiate_protocol_version(&mut out, 0, &[]);
        assert_eq!(
            round_trip(&out),
            BackendMessage::NegotiateProtocolVersion { minor: 0 }
        );
    }

    #[test]
    fn negotiate_lists_options_the_server_did_not_recognise() {
        let mut out = Vec::new();
        negotiate_protocol_version(&mut out, 0, &["_pq_.some_option", "_pq_.another"]);

        let BackendMessage::NegotiateProtocolVersion { minor } = round_trip(&out) else {
            unreachable!()
        };
        assert_eq!(minor, 0);
        let rendered = String::from_utf8_lossy(&out);
        assert!(rendered.contains("_pq_.some_option"));
        assert!(rendered.contains("_pq_.another"));
    }

    #[test]
    fn the_length_prefix_counts_itself_but_not_the_tag() {
        // The detail everyone gets wrong once. Encoded by hand here so the
        // assertion does not depend on the same helper being correct.
        let mut out = Vec::new();
        ready_for_query(&mut out, TxStatus::Idle);

        assert_eq!(out.len(), 6, "tag + 4 length bytes + 1 status byte");
        assert_eq!(out[0], b'Z');
        assert_eq!(u32::from_be_bytes([out[1], out[2], out[3], out[4]]), 5);
        assert_eq!(out[5], b'I');
    }

    #[test]
    fn messages_append_rather_than_replace() {
        // The relay path reuses a pooled buffer, so encoding must never assume
        // it owns the whole thing.
        let mut out = b"already here".to_vec();
        let before = out.len();
        ready_for_query(&mut out, TxStatus::Idle);

        assert_eq!(&out[..before], b"already here");
        assert_eq!(
            round_trip(&out[before..]),
            BackendMessage::ReadyForQuery(TxStatus::Idle)
        );
    }

    #[test]
    fn a_full_startup_response_decodes_message_by_message() {
        // What a client receives after authenticating, in order.
        let conn = ConnId::new(NodeId::new(1), 42);
        let mut out = Vec::new();
        authentication_ok(&mut out);
        parameter_status(&mut out, "server_version", "18.4");
        parameter_status(&mut out, "client_encoding", "UTF8");
        backend_key_data(&mut out, conn);
        ready_for_query(&mut out, TxStatus::Idle);

        let mut rest = out.as_slice();
        let mut seen = Vec::new();
        while !rest.is_empty() {
            let Decoded::Frame(frame, consumed) = decode(rest, DEFAULT_MAX_FRAME).unwrap() else {
                unreachable!("stream ended mid-frame")
            };
            seen.push(frame.tag());
            rest = &rest[consumed..];
        }

        assert_eq!(
            seen,
            vec![
                Tag::AUTHENTICATION,
                Tag::PARAMETER_STATUS,
                Tag::PARAMETER_STATUS,
                Tag::BACKEND_KEY_DATA,
                Tag::READY_FOR_QUERY,
            ]
        );
    }

    #[test]
    fn protocol_version_constants_match_the_wire_encoding() {
        // Major in the high 16 bits, minor in the low 16.
        assert_eq!(PROTOCOL_3_0, 3 << 16);
        assert_eq!(PROTOCOL_3_2, (3 << 16) | 2);
    }

    #[test]
    fn a_sasl_request_lists_its_mechanisms_and_terminates_the_list() {
        let mut out = Vec::new();
        authentication_sasl(&mut out, &["SCRAM-SHA-256"]);

        assert_eq!(
            round_trip(&out),
            BackendMessage::Authentication(AuthRequest::Sasl)
        );
        assert!(
            out.ends_with(b"SCRAM-SHA-256\0\0"),
            "the mechanism list was not terminated, so a client would hang: {out:?}"
        );
    }

    #[test]
    fn a_sasl_request_can_offer_more_than_one_mechanism() {
        let mut out = Vec::new();
        authentication_sasl(&mut out, &["SCRAM-SHA-256", "SOMETHING-ELSE"]);
        assert!(out.ends_with(b"SCRAM-SHA-256\0SOMETHING-ELSE\0\0"));
    }

    #[test]
    fn a_sasl_continue_carries_its_payload_unterminated() {
        // The terminator would land inside the client's AuthMessage and break
        // every proof computed from it, which presents as "the password is
        // wrong" against a password that is right.
        let mut out = Vec::new();
        authentication_sasl_continue(&mut out, "r=NONCE,s=U0FMVA==,i=4096");

        assert_eq!(
            round_trip(&out),
            BackendMessage::Authentication(AuthRequest::SaslContinue)
        );
        assert!(
            out.ends_with(b"i=4096"),
            "a terminator was appended: {out:?}"
        );
    }

    #[test]
    fn a_sasl_final_carries_its_payload_unterminated() {
        let mut out = Vec::new();
        authentication_sasl_final(&mut out, "v=U0lHTg==");

        assert_eq!(
            round_trip(&out),
            BackendMessage::Authentication(AuthRequest::SaslFinal)
        );
        assert!(out.ends_with(b"v=U0lHTg=="));
    }

    #[test]
    fn every_sasl_message_declares_its_own_length_correctly() {
        // A hand-written length prefix is the thing most worth checking, and
        // round_trip only proves the first message parses. This proves each
        // one consumes exactly what it wrote, with nothing left over.
        let mut out = Vec::new();
        authentication_sasl(&mut out, &["SCRAM-SHA-256"]);
        authentication_sasl_continue(&mut out, "r=NONCE,s=U0FMVA==,i=4096");
        authentication_sasl_final(&mut out, "v=U0lHTg==");
        authentication_ok(&mut out);

        let mut rest = out.as_slice();
        let mut seen = Vec::new();
        while !rest.is_empty() {
            let Decoded::Frame(frame, consumed) = decode(rest, DEFAULT_MAX_FRAME).unwrap() else {
                panic!("a message declared a length longer than it wrote");
            };
            backend::decode(&frame).unwrap();
            seen.push(frame.tag());
            rest = &rest[consumed..];
        }
        assert_eq!(seen.len(), 4, "the four messages did not decode as four");
    }
}
