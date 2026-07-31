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
    /// 10, begin SASL. The body lists the mechanisms the server offers.
    Sasl,
    /// 11, a SASL challenge carrying the server-first message.
    SaslContinue,
    /// 12, SASL succeeded, carrying the server-final message.
    SaslFinal,
    /// Any other subtype, kept so an unfamiliar method is reported rather than
    /// silently mishandled.
    Other(i32),
}

/// A decoded error or notice.
///
/// Fields are borrowed from the frame body. Every field Postgres defines is
/// captured, because these are what an operator reads when something breaks and
/// a proxy that drops them makes its own logs worse than connecting directly.
///
/// Absent fields are the empty string rather than an option: Postgres omits most
/// of them most of the time, and threading twenty options through call sites
/// buys nothing over checking for empty.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[non_exhaustive]
pub struct ErrorFields<'a> {
    /// `S`, the severity, localized to the server's language.
    pub severity: &'a str,
    /// `V`, the severity, never localized. Added in Postgres 9.6, so it is
    /// absent from older servers and `severity` is the fallback.
    pub severity_nonlocalized: &'a str,
    /// `C`, the SQLSTATE.
    pub code: &'a str,
    /// `M`, the primary message.
    pub message: &'a str,
    /// `D`, an optional secondary message with more detail.
    pub detail: &'a str,
    /// `H`, a suggestion about what to do about the problem.
    pub hint: &'a str,
    /// `P`, a one-based character index into the original query.
    pub position: &'a str,
    /// `p`, a position into `internal_query` rather than the client's query.
    pub internal_position: &'a str,
    /// `q`, the internally generated statement the error refers to.
    pub internal_query: &'a str,
    /// `W`, the call stack traceback.
    pub context: &'a str,
    /// `s`, the schema the error relates to.
    pub schema: &'a str,
    /// `t`, the table the error relates to.
    pub table: &'a str,
    /// `c`, the column the error relates to.
    pub column: &'a str,
    /// `d`, the data type the error relates to.
    pub datatype: &'a str,
    /// `n`, the constraint the error relates to.
    pub constraint: &'a str,
    /// `F`, the server source file reporting it.
    pub file: &'a str,
    /// `L`, the server source line.
    pub line: &'a str,
    /// `R`, the server routine reporting it.
    pub routine: &'a str,
}

/// The SASL mechanisms this proxy understands, most preferred first.
///
/// `SCRAM-SHA-256-PLUS` is deliberately absent. See ADR 0014.
pub const SUPPORTED_SASL_MECHANISMS: &[&str] = &["SCRAM-SHA-256"];

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
    /// `t`, the parameter types a described statement expects.
    ///
    /// The count is what matters: a driver asks and then refuses to bind a
    /// different number, which is how a wrong answer here surfaces. The type
    /// OIDs are borrowed rather than copied, since a proxy forwards them
    /// unchanged.
    ParameterDescription {
        /// How many parameters the statement takes.
        count: usize,
        /// The raw big-endian OID list, `count` entries of four bytes.
        oids: &'a [u8],
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
        Tag::PARAMETER_DESCRIPTION => {
            let declared = r.i16("parameter_count")?;
            // A negative count is malformed rather than "none": trusting it
            // would make the OID slice length nonsense.
            let count = usize::try_from(declared).map_err(|_| {
                BackendError::Field(FieldError::OutOfRange {
                    what: "parameter_count",
                    value: i64::from(declared),
                })
            })?;
            BackendMessage::ParameterDescription {
                count,
                oids: r.bytes(count * 4, "parameter_oids")?,
            }
        }
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
            b'S' => fields.severity = value,
            b'V' => fields.severity_nonlocalized = value,
            b'C' => fields.code = value,
            b'M' => fields.message = value,
            b'D' => fields.detail = value,
            b'H' => fields.hint = value,
            b'P' => fields.position = value,
            b'p' => fields.internal_position = value,
            b'q' => fields.internal_query = value,
            b'W' => fields.context = value,
            b's' => fields.schema = value,
            b't' => fields.table = value,
            b'c' => fields.column = value,
            b'd' => fields.datatype = value,
            b'n' => fields.constraint = value,
            b'F' => fields.file = value,
            b'L' => fields.line = value,
            b'R' => fields.routine = value,
            // Postgres adds field types over time. An unfamiliar one is skipped
            // rather than rejected, so a newer server cannot make an error
            // undecodable.
            _ => {}
        }
    }

    Ok(fields)
}

/// Reads the mechanism list from an `AuthenticationSASL` body.
///
/// The body is a run of null-terminated names ended by an empty one. Returns the
/// first name this proxy supports, or [`None`] when the server offers nothing
/// usable, which is a clearer failure than attempting a mechanism blindly.
///
/// # Errors
///
/// Fails when the list is unterminated or not UTF-8.
pub fn select_sasl_mechanism(body: &[u8]) -> Result<Option<&str>, BackendError> {
    select_mechanism_from(body, SUPPORTED_SASL_MECHANISMS)
}

/// [`select_sasl_mechanism`] against an explicit preference list.
///
/// Split out so the preference rule can be tested. `SUPPORTED_SASL_MECHANISMS`
/// holds one entry, and an ordering rule over a one-element list is a rule
/// nothing can disagree with: every ranking of it is the same ranking. `M14`
/// spent a milestone on assertions of exactly that shape, so the way to state
/// this one is against a list long enough to have an order.
///
/// # Errors
///
/// As [`select_sasl_mechanism`].
fn select_mechanism_from<'a>(
    body: &'a [u8],
    supported: &[&str],
) -> Result<Option<&'a str>, BackendError> {
    let mut r = Reader::new(body);
    // Skip the subtype the caller already read.
    r.i32("auth_subtype")?;

    // One pass, no vector. The offered list used to be collected so it could be
    // searched once per supported mechanism, which allocated on the
    // authentication path of every connection to hold at most a handful of
    // string slices.
    //
    // Server order is advisory; our preference decides, so a server listing a
    // mechanism we would rather not use cannot force it by putting it first.
    // That is what `rank` keeps: the index into `SUPPORTED_SASL_MECHANISMS`,
    // so a later offer replaces an earlier one only when we prefer it.
    let mut best: Option<(usize, &str)> = None;
    loop {
        if r.is_empty() {
            break;
        }
        let name = r.cstr("mechanism")?;
        if name.is_empty() {
            break;
        }
        if let Some(rank) = supported.iter().position(|w| *w == name)
            && best.is_none_or(|(seen, _)| rank < seen)
        {
            best = Some((rank, name));
        }
    }

    Ok(best.map(|(_, name)| name))
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
    use crate::frame::{Direction, Inspect, inspect_policy};
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
    fn a_sasl_offer_yields_the_mechanism_we_support() {
        let mut body = 10_i32.to_be_bytes().to_vec();
        body.extend_from_slice(b"SCRAM-SHA-256\x00\x00");
        assert_eq!(select_sasl_mechanism(&body).unwrap(), Some("SCRAM-SHA-256"));
    }

    #[test]
    fn our_preference_decides_rather_than_the_servers_order() {
        // A server listing a mechanism we would rather not use must not be able
        // to force it by putting it first.
        let mut body = 10_i32.to_be_bytes().to_vec();
        body.extend_from_slice(b"SCRAM-SHA-256-PLUS\x00SCRAM-SHA-256\x00\x00");
        assert_eq!(
            select_sasl_mechanism(&body).unwrap(),
            Some("SCRAM-SHA-256"),
            "the server's first choice overrode ours"
        );
    }

    #[test]
    fn our_preference_order_decides_and_not_just_our_membership() {
        // `SUPPORTED_SASL_MECHANISMS` has one entry, so the test above cannot
        // tell "we prefer ours" from "we take whichever of ours we see first".
        // Every ordering of a one-element list is the same ordering. `M14`
        // spent a milestone on assertions of that shape, so this one is made
        // against a list long enough to have an order.
        let supported = &["BEST", "WORSE"];

        let mut offer = 10_i32.to_be_bytes().to_vec();
        offer.extend_from_slice(b"WORSE\x00BEST\x00\x00");
        assert_eq!(
            select_mechanism_from(&offer, supported).unwrap(),
            Some("BEST"),
            "the server's order overrode ours"
        );

        // And the same list offered the other way round gives the same answer,
        // which is what makes the first assertion about preference rather than
        // about position.
        let mut offer = 10_i32.to_be_bytes().to_vec();
        offer.extend_from_slice(b"BEST\x00WORSE\x00\x00");
        assert_eq!(
            select_mechanism_from(&offer, supported).unwrap(),
            Some("BEST")
        );

        // A server offering only the one we like less still gets it: the rule
        // is a preference, not a requirement.
        let mut offer = 10_i32.to_be_bytes().to_vec();
        offer.extend_from_slice(b"WORSE\x00\x00");
        assert_eq!(
            select_mechanism_from(&offer, supported).unwrap(),
            Some("WORSE")
        );
    }

    #[test]
    fn a_repeated_offer_does_not_change_the_answer() {
        // The one thing the single-entry list can still say. Without the rank
        // comparison a later duplicate would overwrite the earlier match, which
        // is harmless here and would not be if the list ever grows.
        let mut body = 10_i32.to_be_bytes().to_vec();
        body.extend_from_slice(b"SCRAM-SHA-256\x00SCRAM-SHA-256\x00\x00");
        assert_eq!(select_sasl_mechanism(&body).unwrap(), Some("SCRAM-SHA-256"));
    }

    #[test]
    fn an_offer_with_nothing_usable_is_none_rather_than_a_guess() {
        // Clearer than attempting a mechanism blindly and failing later with a
        // confusing error.
        let mut body = 10_i32.to_be_bytes().to_vec();
        body.extend_from_slice(b"GSSAPI\x00EXTERNAL\x00\x00");
        assert_eq!(select_sasl_mechanism(&body).unwrap(), None);
    }

    #[test]
    fn an_unterminated_mechanism_list_is_an_error() {
        let mut body = 10_i32.to_be_bytes().to_vec();
        body.extend_from_slice(b"SCRAM-SHA-256");
        assert!(select_sasl_mechanism(&body).is_err());
    }

    #[test]
    fn channel_binding_is_not_offered() {
        // ADR 0014: it needs the TLS exporter and interacts with the FIPS suite
        // list. Absent deliberately, and stated here so it cannot creep in.
        assert!(!SUPPORTED_SASL_MECHANISMS.contains(&"SCRAM-SHA-256-PLUS"));
        assert_eq!(SUPPORTED_SASL_MECHANISMS, &["SCRAM-SHA-256"]);
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
    fn parameter_description_yields_its_count_and_oids() {
        // int4, then text.
        let mut body = 2_i16.to_be_bytes().to_vec();
        body.extend_from_slice(&23_i32.to_be_bytes());
        body.extend_from_slice(&25_i32.to_be_bytes());

        let BackendMessage::ParameterDescription { count, oids } =
            decode(&frame(Tag::PARAMETER_DESCRIPTION, &body)).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(count, 2);
        assert_eq!(oids.len(), 8, "the OID list should be four bytes each");
    }

    #[test]
    fn a_statement_with_no_parameters_describes_as_zero() {
        // The common case, and the one asyncpg refuses to bind against if the
        // count is wrong.
        let body = 0_i16.to_be_bytes();
        let BackendMessage::ParameterDescription { count, oids } =
            decode(&frame(Tag::PARAMETER_DESCRIPTION, &body)).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(count, 0);
        assert!(oids.is_empty());
    }

    #[test]
    fn a_parameter_description_shorter_than_its_count_is_an_error() {
        // Claiming three parameters and supplying one OID must not read past
        // the body.
        let mut body = 3_i16.to_be_bytes().to_vec();
        body.extend_from_slice(&23_i32.to_be_bytes());
        assert!(decode(&frame(Tag::PARAMETER_DESCRIPTION, &body)).is_err());
    }

    #[test]
    fn a_negative_parameter_count_is_an_error() {
        // Malformed rather than "none": trusting it makes the OID slice length
        // nonsense.
        let body = (-1_i16).to_be_bytes();
        let err = decode(&frame(Tag::PARAMETER_DESCRIPTION, &body)).unwrap_err();
        assert!(
            matches!(
                err,
                BackendError::Field(FieldError::OutOfRange {
                    what: "parameter_count",
                    value: -1
                })
            ),
            "reported as {err:?}, which sends an operator looking for a short read"
        );
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
    fn every_documented_error_field_is_captured() {
        // The shape a constraint violation actually has. Dropping these makes
        // the proxy's logs worse than connecting directly, which is the failure
        // this guards.
        let body = b"SERROR\x00VERROR\x00C23505\x00Mduplicate key value\x00DKey (id)=(1) already exists.\x00HConsider an upsert.\x00P42\x00qINSERT INTO t\x00p7\x00WPL/pgSQL function f()\x00spublic\x00torders\x00cid\x00dinteger\x00norders_pkey\x00Fnbtinsert.c\x00L199\x00R_bt_check_unique\x00\x00";

        let BackendMessage::ErrorResponse(f) = decode(&frame(Tag::ERROR_RESPONSE, body)).unwrap()
        else {
            unreachable!()
        };

        assert_eq!(f.severity, "ERROR");
        assert_eq!(f.severity_nonlocalized, "ERROR");
        assert_eq!(f.code, "23505");
        assert_eq!(f.message, "duplicate key value");
        assert_eq!(f.detail, "Key (id)=(1) already exists.");
        assert_eq!(f.hint, "Consider an upsert.");
        assert_eq!(f.position, "42");
        assert_eq!(f.internal_query, "INSERT INTO t");
        assert_eq!(f.internal_position, "7");
        assert_eq!(f.context, "PL/pgSQL function f()");
        assert_eq!(f.schema, "public");
        assert_eq!(f.table, "orders");
        assert_eq!(f.column, "id");
        assert_eq!(f.datatype, "integer");
        assert_eq!(f.constraint, "orders_pkey");
        assert_eq!(f.file, "nbtinsert.c");
        assert_eq!(f.line, "199");
        assert_eq!(f.routine, "_bt_check_unique");
    }

    #[test]
    fn lowercase_and_uppercase_field_types_are_distinct() {
        // The trap in this table: P is position and p is internal position,
        // s is schema and S is severity, c is column and C is the SQLSTATE.
        // Case-insensitive matching would silently swap them.
        let body = b"P1\x00p2\x00Sseverity\x00sschema\x00Ccode\x00ccolumn\x00\x00";
        let BackendMessage::ErrorResponse(f) = decode(&frame(Tag::ERROR_RESPONSE, body)).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(f.position, "1");
        assert_eq!(f.internal_position, "2");
        assert_eq!(f.severity, "severity");
        assert_eq!(f.schema, "schema");
        assert_eq!(f.code, "code");
        assert_eq!(f.column, "column");
    }

    #[test]
    fn absent_fields_are_empty_rather_than_missing() {
        // Postgres omits most fields most of the time.
        let BackendMessage::ErrorResponse(f) =
            decode(&frame(Tag::ERROR_RESPONSE, b"C42P01\x00Mnope\x00\x00")).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(f.code, "42P01");
        assert!(f.detail.is_empty());
        assert!(f.constraint.is_empty());
        assert!(f.routine.is_empty());
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
    fn what_is_not_inspected_is_not_decoded_either() {
        // The invariant a streaming relay rests on, and it is worth stating
        // because two lists in two modules happen to agree and nothing said so.
        //
        // `inspect_policy` decides how much of a body must be buffered; this
        // function decides how much is read. If a tag were `Inspect::None` here
        // and still decoded to something carrying a field, a caller that
        // streamed the body past without keeping it would hand `decode` an
        // empty slice and get a wrong answer or an error instead of `Opaque`.
        //
        // `M16.3` uses exactly that implication: an uninspected tag decodes
        // from no body at all.
        for code in 0_u8..=255 {
            let tag = Tag(code);
            if inspect_policy(Direction::Backend, tag) != Inspect::None {
                continue;
            }
            assert_eq!(
                decode(&frame(tag, b"")).unwrap(),
                BackendMessage::Opaque(tag),
                "{tag} is not inspected but decodes to something that read a body"
            );
        }
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
            Tag::PARAMETER_DESCRIPTION,
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
            let tag = tags[usize::try_from(seed % 11).unwrap()];

            let _ = decode(&frame(tag, &body));
        }
    }
}
