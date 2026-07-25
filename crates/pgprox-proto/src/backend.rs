//! Backend messages, server to client.
//!
//! Only the messages the proxy acts on are decoded. Everything else, including
//! every `DataRow`, is forwarded as an opaque [`crate::Frame`]. Parsing result
//! rows is the difference between a proxy and a bottleneck.

use pgprox_core::ids::ConnId;

use crate::frame::{Frame, Tag};
use crate::read::{FieldError, Reader};

/// The transaction status byte in `ReadyForQuery`.
///
/// This is the authoritative signal for releasing an upstream connection. Not
/// the SQL text, not a heuristic. See ADR 0001.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum TxStatus {
    /// `I`, idle. The only status at which a connection may be released.
    Idle,
    /// `T`, inside a transaction block.
    InTransaction,
    /// `E`, inside a failed transaction block, awaiting rollback.
    Failed,
}

impl TxStatus {
    /// Decodes the status byte.
    ///
    /// # Errors
    ///
    /// Fails on any byte Postgres does not define.
    pub const fn from_byte(byte: u8) -> Result<Self, BackendError> {
        match byte {
            b'I' => Ok(Self::Idle),
            b'T' => Ok(Self::InTransaction),
            b'E' => Ok(Self::Failed),
            other => Err(BackendError::UnknownTxStatus(other)),
        }
    }

    /// Whether an upstream connection may be released at this status.
    ///
    /// Only `Idle`. A connection released at `InTransaction` would hand another
    /// client a connection inside someone else's transaction, and one released
    /// at `Failed` would hand over a session that rejects every statement until
    /// it sees a rollback.
    #[must_use]
    pub const fn is_releasable(self) -> bool {
        matches!(self, Self::Idle)
    }
}

/// What an authentication request is asking for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum AuthRequest {
    /// 0, authentication succeeded.
    Ok,
    /// 3, send the password in the clear. This is how a JWT arrives.
    CleartextPassword,
    /// 5, send an MD5-hashed password, with the salt supplied.
    Md5Password,
    /// 10, begin SASL.
    Sasl,
    /// 11, a SASL challenge.
    SaslContinue,
    /// 12, SASL succeeded.
    SaslFinal,
    /// Any other subtype, kept so an unfamiliar method is reported rather than
    /// silently mishandled.
    Other(i32),
}

/// A decoded error or notice.
///
/// Fields are borrowed from the frame body.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[non_exhaustive]
pub struct ErrorFields<'a> {
    /// `C`, the SQLSTATE.
    pub code: &'a str,
    /// `M`, the primary message.
    pub message: &'a str,
    /// `S`, the severity, localized.
    pub severity: &'a str,
}

/// A decoded backend message.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum BackendMessage<'a> {
    /// `Z`, ready for query.
    ReadyForQuery(TxStatus),
    /// `R`, an authentication request.
    Authentication(AuthRequest),
    /// `S`, a runtime parameter changed.
    ParameterStatus {
        /// The parameter name.
        name: &'a str,
        /// Its new value.
        value: &'a str,
    },
    /// `K`, the cancellation key for this session.
    BackendKeyData {
        /// The server's process ID.
        process_id: i32,
        /// The secret needed to cancel.
        secret: i32,
    },
    /// `I`, the statement was empty.
    ///
    /// Postgres sends this *instead of* `CommandComplete`, so a proxy tracking
    /// statement completion must treat the two as one category.
    EmptyQueryResponse,
    /// `C`, a command finished.
    CommandComplete {
        /// The command tag, such as `SELECT 3` or `INSERT 0 1`.
        tag: &'a str,
    },
    /// `E`, an error.
    ErrorResponse(ErrorFields<'a>),
    /// `N`, a notice.
    NoticeResponse(ErrorFields<'a>),
    /// `A`, an asynchronous notification. Its presence pins the session.
    NotificationResponse {
        /// The notifying process.
        process_id: i32,
        /// The channel.
        channel: &'a str,
        /// The payload.
        payload: &'a str,
    },
    /// `G`, the server will accept COPY data.
    CopyInResponse,
    /// `H`, the server will send COPY data.
    CopyOutResponse,
    /// `W`, COPY in both directions.
    CopyBothResponse,
    /// `c`, the COPY stream ended.
    CopyDone,
    /// `v`, the server proposes a lower protocol version.
    NegotiateProtocolVersion {
        /// The highest minor version the server supports.
        minor: i32,
    },
    /// Anything else, forwarded without being parsed.
    Opaque(Tag),
}

/// Why a backend message could not be decoded.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BackendError {
    /// A field could not be read.
    #[error(transparent)]
    Field(#[from] FieldError),
    /// `ReadyForQuery` carried a status byte Postgres does not define.
    #[error("unknown transaction status byte {0:?}")]
    UnknownTxStatus(u8),
}

/// Decodes a backend frame.
///
/// Messages the proxy does not act on return [`BackendMessage::Opaque`] without
/// their bodies being examined.
///
/// # Errors
///
/// Fails when a message the proxy does act on is malformed.
pub fn decode<'a>(frame: &Frame<'a>) -> Result<BackendMessage<'a>, BackendError> {
    let mut r = Reader::new(frame.body());

    Ok(match frame.tag() {
        Tag::READY_FOR_QUERY => {
            BackendMessage::ReadyForQuery(TxStatus::from_byte(r.u8("tx_status")?)?)
        }
        Tag::AUTHENTICATION => BackendMessage::Authentication(match r.i32("auth_subtype")? {
            0 => AuthRequest::Ok,
            3 => AuthRequest::CleartextPassword,
            5 => AuthRequest::Md5Password,
            10 => AuthRequest::Sasl,
            11 => AuthRequest::SaslContinue,
            12 => AuthRequest::SaslFinal,
            other => AuthRequest::Other(other),
        }),
        Tag::PARAMETER_STATUS => BackendMessage::ParameterStatus {
            name: r.cstr("parameter_name")?,
            value: r.cstr("parameter_value")?,
        },
        Tag::BACKEND_KEY_DATA => BackendMessage::BackendKeyData {
            process_id: r.i32("process_id")?,
            secret: r.i32("secret_key")?,
        },
        Tag::COMMAND_COMPLETE => BackendMessage::CommandComplete {
            tag: r.cstr("command_tag")?,
        },
        Tag::EMPTY_QUERY_RESPONSE => BackendMessage::EmptyQueryResponse,
        Tag::ERROR_RESPONSE => BackendMessage::ErrorResponse(decode_error_fields(&mut r)?),
        Tag::NOTICE_RESPONSE => BackendMessage::NoticeResponse(decode_error_fields(&mut r)?),
        Tag::NOTIFICATION_RESPONSE => BackendMessage::NotificationResponse {
            process_id: r.i32("process_id")?,
            channel: r.cstr("channel")?,
            payload: r.cstr("payload")?,
        },
        Tag::COPY_IN_RESPONSE => BackendMessage::CopyInResponse,
        Tag::COPY_OUT_RESPONSE => BackendMessage::CopyOutResponse,
        Tag::COPY_BOTH_RESPONSE => BackendMessage::CopyBothResponse,
        Tag::COPY_DONE => BackendMessage::CopyDone,
        Tag::NEGOTIATE_PROTOCOL_VERSION => BackendMessage::NegotiateProtocolVersion {
            minor: r.i32("minor_version")?,
        },
        // DataRow lands here, deliberately. See the module docs.
        other => BackendMessage::Opaque(other),
    })
}

/// Reads the field list shared by `ErrorResponse` and `NoticeResponse`.
///
/// The list is `type byte` then a string, repeated, terminated by a zero byte.
/// Only the fields the proxy uses are kept; the rest are skipped rather than
/// collected, since the frame is forwarded verbatim anyway.
fn decode_error_fields<'a>(r: &mut Reader<'a>) -> Result<ErrorFields<'a>, BackendError> {
    let mut fields = ErrorFields::default();

    loop {
        let kind = r.u8("field_type")?;
        if kind == 0 {
            break;
        }
        let value = r.cstr("field_value")?;
        match kind {
            b'C' => fields.code = value,
            b'M' => fields.message = value,
            b'S' => fields.severity = value,
            _ => {}
        }
    }

    Ok(fields)
}

/// Rebuilds the connection ID from a cancel key the proxy issued.
///
/// The proxy hands clients its own `BackendKeyData`, so the key is ours to
/// design: the node is encoded in it, which is what lets a `CancelRequest`
/// arriving at any pod reach the pod that owns the connection.
#[must_use]
pub fn conn_id_from_key(process_id: i32, secret: i32) -> ConnId {
    #[allow(clippy::cast_sign_loss)]
    let raw = (u64::from(process_id as u32) << 32) | u64::from(secret as u32);
    ConnId::from_raw(raw)
}

/// Splits a connection ID into the two `i32` halves of `BackendKeyData`.
#[must_use]
pub fn key_from_conn_id(id: ConnId) -> (i32, i32) {
    #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    let pair = (
        (id.raw() >> 32) as u32 as i32,
        (id.raw() & 0xFFFF_FFFF) as u32 as i32,
    );
    pair
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use pgprox_core::ids::NodeId;

    fn frame(tag: Tag, body: &[u8]) -> Frame<'_> {
        Frame::new(tag, body)
    }

    #[test]
    fn ready_for_query_carries_the_release_signal() {
        // The authoritative signal for releasing an upstream connection.
        for (byte, expected, releasable) in [
            (b'I', TxStatus::Idle, true),
            (b'T', TxStatus::InTransaction, false),
            (b'E', TxStatus::Failed, false),
        ] {
            let body = [byte];
            let decoded = decode(&frame(Tag::READY_FOR_QUERY, &body)).unwrap();
            assert_eq!(decoded, BackendMessage::ReadyForQuery(expected));
            assert_eq!(
                expected.is_releasable(),
                releasable,
                "{expected:?} releasability is wrong"
            );
        }
    }

    #[test]
    fn only_idle_is_releasable() {
        // Releasing at InTransaction hands another client a connection inside
        // someone else's transaction. Releasing at Failed hands over a session
        // that rejects every statement until it sees a rollback.
        assert!(TxStatus::Idle.is_releasable());
        assert!(!TxStatus::InTransaction.is_releasable());
        assert!(!TxStatus::Failed.is_releasable());
    }

    #[test]
    fn an_undefined_status_byte_is_an_error() {
        let err = decode(&frame(Tag::READY_FOR_QUERY, b"X")).unwrap_err();
        assert_eq!(err, BackendError::UnknownTxStatus(b'X'));
    }

    #[test]
    fn a_truncated_ready_for_query_is_an_error_not_a_panic() {
        let err = decode(&frame(Tag::READY_FOR_QUERY, b"")).unwrap_err();
        assert!(matches!(err, BackendError::Field(_)), "{err:?}");
    }

    #[test]
    fn authentication_subtypes_decode() {
        for (subtype, expected) in [
            (0, AuthRequest::Ok),
            (3, AuthRequest::CleartextPassword),
            (5, AuthRequest::Md5Password),
            (10, AuthRequest::Sasl),
            (11, AuthRequest::SaslContinue),
            (12, AuthRequest::SaslFinal),
            (99, AuthRequest::Other(99)),
        ] {
            let body = i32::to_be_bytes(subtype);
            let decoded = decode(&frame(Tag::AUTHENTICATION, &body)).unwrap();
            assert_eq!(decoded, BackendMessage::Authentication(expected));
        }
    }

    #[test]
    fn parameter_status_decodes_both_strings() {
        let decoded = decode(&frame(Tag::PARAMETER_STATUS, b"server_version\x0018.4\x00")).unwrap();
        assert_eq!(
            decoded,
            BackendMessage::ParameterStatus {
                name: "server_version",
                value: "18.4",
            }
        );
    }

    #[test]
    fn backend_key_data_round_trips_through_a_conn_id() {
        // The property cancellation depends on across pods.
        let original = ConnId::new(NodeId::new(7), 0x1234_5678);
        let (process_id, secret) = key_from_conn_id(original);
        assert_eq!(conn_id_from_key(process_id, secret), original);
        assert_eq!(conn_id_from_key(process_id, secret).node(), NodeId::new(7));
    }

    #[test]
    fn a_cancel_key_survives_the_signed_round_trip() {
        // The wire format is two signed i32s. A counter with the high bit set
        // must not be mangled by the sign, or the cancel goes to the wrong pod.
        for node in [0_u16, 1, 40_000, u16::MAX] {
            for counter in [0_u64, 1, 0xFFFF_FFFF, 0x7FFF_FFFF_FFFF] {
                let original = ConnId::new(NodeId::new(node), counter);
                let (pid, secret) = key_from_conn_id(original);
                assert_eq!(
                    conn_id_from_key(pid, secret),
                    original,
                    "lost {node}/{counter}"
                );
            }
        }
    }

    #[test]
    fn backend_key_data_decodes_from_the_wire() {
        let mut body = 12_345_i32.to_be_bytes().to_vec();
        body.extend_from_slice(&(-42_i32).to_be_bytes());
        let decoded = decode(&frame(Tag::BACKEND_KEY_DATA, &body)).unwrap();
        assert_eq!(
            decoded,
            BackendMessage::BackendKeyData {
                process_id: 12_345,
                secret: -42,
            }
        );
    }

    #[test]
    fn an_empty_query_response_is_recognised() {
        // Sent instead of CommandComplete. Treating it as opaque means every
        // empty statement a driver sends is a miscounted completion.
        assert_eq!(
            decode(&frame(Tag::EMPTY_QUERY_RESPONSE, b"")).unwrap(),
            BackendMessage::EmptyQueryResponse
        );
    }

    #[test]
    fn empty_query_and_command_complete_are_both_completions() {
        // The category a proxy actually cares about.
        let empty = decode(&frame(Tag::EMPTY_QUERY_RESPONSE, b"")).unwrap();
        let complete = decode(&frame(Tag::COMMAND_COMPLETE, b"SELECT 1\x00")).unwrap();
        for msg in [empty, complete] {
            assert!(
                matches!(
                    msg,
                    BackendMessage::EmptyQueryResponse | BackendMessage::CommandComplete { .. }
                ),
                "{msg:?} is not a completion"
            );
        }
    }

    #[test]
    fn command_complete_carries_its_tag() {
        let decoded = decode(&frame(Tag::COMMAND_COMPLETE, b"SELECT 3\0")).unwrap();
        assert_eq!(decoded, BackendMessage::CommandComplete { tag: "SELECT 3" });
    }

    #[test]
    fn error_response_yields_the_sqlstate_and_message() {
        // Real shape: severity, code, message, then a terminator.
        let body = b"SERROR\0C42P01\0Mrelation \"nope\" does not exist\0\0";
        let BackendMessage::ErrorResponse(fields) =
            decode(&frame(Tag::ERROR_RESPONSE, body)).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(fields.code, "42P01");
        assert_eq!(fields.severity, "ERROR");
        assert!(fields.message.contains("does not exist"));
    }

    #[test]
    fn unknown_error_fields_are_skipped_not_rejected() {
        // Postgres adds fields over time. An unfamiliar one must not make the
        // message undecodable.
        let body = b"SERROR\0Zsomething-new\0C57P01\0Mshutting down\0Xanother\0\0";
        let BackendMessage::ErrorResponse(fields) =
            decode(&frame(Tag::ERROR_RESPONSE, body)).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(fields.code, "57P01");
        assert_eq!(fields.message, "shutting down");
    }

    #[test]
    fn an_error_response_with_no_fields_decodes() {
        let BackendMessage::ErrorResponse(fields) =
            decode(&frame(Tag::ERROR_RESPONSE, b"\0")).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(fields, ErrorFields::default());
    }

    #[test]
    fn an_unterminated_error_field_list_is_an_error() {
        let err = decode(&frame(Tag::ERROR_RESPONSE, b"C42P01\0")).unwrap_err();
        assert!(matches!(err, BackendError::Field(_)), "{err:?}");
    }

    #[test]
    fn notice_response_uses_the_same_field_list() {
        let body = b"SNOTICE\0C00000\0Mtable already exists\0\0";
        let BackendMessage::NoticeResponse(fields) =
            decode(&frame(Tag::NOTICE_RESPONSE, body)).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(fields.severity, "NOTICE");
    }

    #[test]
    fn notification_response_decodes() {
        // Its presence means the session used LISTEN, which pins it.
        let mut body = 999_i32.to_be_bytes().to_vec();
        body.extend_from_slice(b"jobs\0payload-here\0");
        let decoded = decode(&frame(Tag::NOTIFICATION_RESPONSE, &body)).unwrap();
        assert_eq!(
            decoded,
            BackendMessage::NotificationResponse {
                process_id: 999,
                channel: "jobs",
                payload: "payload-here",
            }
        );
    }

    #[test]
    fn copy_responses_decode() {
        // Bodies carry format information the proxy does not need; only the
        // mode transition matters.
        for (tag, expected) in [
            (Tag::COPY_IN_RESPONSE, BackendMessage::CopyInResponse),
            (Tag::COPY_OUT_RESPONSE, BackendMessage::CopyOutResponse),
            (Tag::COPY_BOTH_RESPONSE, BackendMessage::CopyBothResponse),
            (Tag::COPY_DONE, BackendMessage::CopyDone),
        ] {
            assert_eq!(decode(&frame(tag, b"\x00\x00\x01")).unwrap(), expected);
        }
    }

    #[test]
    fn negotiate_protocol_version_decodes() {
        let body = 0_i32.to_be_bytes();
        let decoded = decode(&frame(Tag::NEGOTIATE_PROTOCOL_VERSION, &body)).unwrap();
        assert_eq!(
            decoded,
            BackendMessage::NegotiateProtocolVersion { minor: 0 }
        );
    }

    #[test]
    fn a_data_row_is_never_parsed() {
        // The rule that keeps this a proxy rather than a bottleneck.
        let body = b"\x00\x01\x00\x00\x00\x05hello";
        assert_eq!(
            decode(&frame(Tag::DATA_ROW, body)).unwrap(),
            BackendMessage::Opaque(Tag::DATA_ROW)
        );
    }

    #[test]
    fn unrecognised_tags_pass_through_opaquely() {
        for tag in [Tag::ROW_DESCRIPTION, Tag::PARSE_COMPLETE, Tag(b'?')] {
            assert_eq!(
                decode(&frame(tag, b"anything at all")).unwrap(),
                BackendMessage::Opaque(tag)
            );
        }
    }

    #[test]
    fn decoding_never_panics_on_arbitrary_bodies() {
        let tags = [
            Tag::READY_FOR_QUERY,
            Tag::AUTHENTICATION,
            Tag::PARAMETER_STATUS,
            Tag::BACKEND_KEY_DATA,
            Tag::COMMAND_COMPLETE,
            Tag::ERROR_RESPONSE,
            Tag::NOTICE_RESPONSE,
            Tag::NOTIFICATION_RESPONSE,
            Tag::NEGOTIATE_PROTOCOL_VERSION,
            Tag::DATA_ROW,
        ];

        let mut seed = 0xDEAD_BEEF_CAFE_F00D_u64;
        for _ in 0..20_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let len = usize::try_from(seed % 24).unwrap();
            let body: Vec<u8> = (0..len)
                .map(|i| u8::try_from((seed >> (i % 8 * 8)) & 0xFF).unwrap())
                .collect();
            let tag = tags[usize::try_from(seed % 10).unwrap()];

            let _ = decode(&frame(tag, &body));
        }
    }
}
