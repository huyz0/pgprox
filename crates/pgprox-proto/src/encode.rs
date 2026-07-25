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
        for option in unrecognized {
            cstr(b, option);
        }
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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
}
