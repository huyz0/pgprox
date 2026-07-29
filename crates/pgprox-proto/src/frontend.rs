//! Frontend messages, client to server.
//!
//! Statement and portal names are the reason this module exists. Transaction
//! pooling requires rewriting them, because every modern driver uses named
//! `Parse` and a pooled connection may never have seen the original. See
//! ADR 0011.

use crate::frame::{Frame, Tag};
use crate::read::{FieldError, Reader};

/// Whether a `Describe` or `Close` targets a prepared statement or a portal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Target {
    /// `S`, a prepared statement.
    Statement,
    /// `P`, a portal.
    Portal,
}

impl Target {
    /// Decodes the target byte.
    ///
    /// # Errors
    ///
    /// Fails on any byte other than `S` or `P`.
    pub const fn from_byte(byte: u8) -> Result<Self, FrontendError> {
        match byte {
            b'S' => Ok(Self::Statement),
            b'P' => Ok(Self::Portal),
            other => Err(FrontendError::UnknownTarget(other)),
        }
    }
}

/// A decoded frontend message.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum FrontendMessage<'a> {
    /// `Q`, simple query.
    Query {
        /// The SQL text.
        sql: &'a str,
    },
    /// `P`, prepare a statement.
    Parse {
        /// The statement name. Empty means the unnamed statement.
        statement: &'a str,
        /// The SQL text.
        sql: &'a str,
    },
    /// `B`, bind a portal to a statement.
    Bind {
        /// The portal name. Empty means the unnamed portal.
        portal: &'a str,
        /// The statement being bound.
        statement: &'a str,
    },
    /// `E`, execute a portal.
    Execute {
        /// The portal name.
        portal: &'a str,
        /// Row limit, zero meaning no limit.
        max_rows: i32,
    },
    /// `D`, describe a statement or portal.
    Describe {
        /// What is being described.
        target: Target,
        /// Its name.
        name: &'a str,
    },
    /// `C`, close a statement or portal.
    Close {
        /// What is being closed.
        target: Target,
        /// Its name.
        name: &'a str,
    },
    /// `S`, end an extended query sequence.
    ///
    /// The only frontend message that permits a connection release, and only
    /// once the matching `ReadyForQuery` has come back.
    Sync,
    /// `H`, flush without ending the sequence.
    Flush,
    /// `X`, terminate the session.
    Terminate,
    /// `p`, a password or SASL payload.
    ///
    /// The body is deliberately not exposed: it carries the JWT.
    Password,
    /// `F`, a fast-path function call.
    ///
    /// The payload is deliberately not exposed. There is no SQL to classify, so
    /// it always routes to the primary, and parsing an OID would mean keeping a
    /// function table in step with every extension a tenant installs. See
    /// ADR 0013.
    FunctionCall,
    /// `d`, a chunk of COPY data.
    CopyData,
    /// `c`, the end of a COPY stream.
    CopyDone,
    /// `f`, the client abandoning a COPY.
    CopyFail,
    /// Anything else, forwarded without being parsed.
    Opaque(Tag),
}

impl FrontendMessage<'_> {
    /// Whether this message begins or continues an extended query sequence.
    ///
    /// A connection inside a sequence must not be released even if the
    /// transaction status looks idle, because the client is mid-conversation.
    #[must_use]
    pub const fn starts_extended_sequence(&self) -> bool {
        matches!(
            self,
            Self::Parse { .. }
                | Self::Bind { .. }
                | Self::Execute { .. }
                | Self::Describe { .. }
                | Self::Close { .. }
        )
    }
}

/// Why a frontend message could not be decoded.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FrontendError {
    /// A field could not be read.
    #[error(transparent)]
    Field(#[from] FieldError),
    /// A `Describe` or `Close` named neither a statement nor a portal.
    #[error("unknown describe/close target byte {0:?}")]
    UnknownTarget(u8),
    /// [`bind_parameters`] was handed something that is not a `Bind`.
    #[error("expected a Bind frame, got tag {0:?}")]
    NotABind(u8),
}

/// The parameter values a `Bind` carries, read on demand.
///
/// # Why this is not a field on [`FrontendMessage::Bind`]
///
/// Every extended-protocol statement sends a `Bind`, and the relay decodes one
/// for each. Parameter values are variable in number and length, so holding
/// them in the variant means a `Vec` allocated on a path that currently
/// allocates nothing, on every node, whether or not anything wants them.
///
/// The one thing that wants them is the query cache, which is off by default
/// and opt-in per tenant. So the values are read by a caller that has already
/// decided to build a cache key, and the decode stays what it was.
///
/// # What the wire says
///
/// After the two names a `Bind` carries a count of parameter format codes and
/// that many `int16`s, then a count of parameter values and that many
/// length-prefixed byte strings, then the result format codes. A length of
/// `-1` is SQL `NULL`, which is not the same as a value of length zero and is
/// kept apart here for the same reason it is on the wire: two rows that differ
/// only in a null are two different rows.
///
/// # Lengths are not trusted
///
/// A count and a length both come from the client, and a decoder that
/// allocated on either is a decoder that a malformed message turns into an
/// allocation. Nothing here reserves ahead of what it has read, and every read
/// goes through [`Reader`], which refuses to move past the end of the frame.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct BindParameters<'a> {
    values: Vec<Option<&'a [u8]>>,
}

impl<'a> BindParameters<'a> {
    /// The values, in order. `None` is SQL `NULL`.
    #[must_use]
    pub fn values(&self) -> &[Option<&'a [u8]>] {
        &self.values
    }

    /// How many there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the statement was bound with no parameters at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// Reads the parameter values out of a `Bind` frame.
///
/// # Errors
///
/// [`FrontendError::NotABind`] if the frame is not one, and
/// [`FrontendError::Field`] if it is truncated or its counts do not match what
/// follows them.
pub fn bind_parameters<'a>(frame: &Frame<'a>) -> Result<BindParameters<'a>, FrontendError> {
    if frame.tag() != Tag::BIND {
        return Err(FrontendError::NotABind(frame.tag().0));
    }

    let mut r = Reader::new(frame.body());
    r.cstr("portal_name")?;
    r.cstr("statement_name")?;

    // The format codes are skipped rather than returned. They say how the
    // values are encoded, text or binary, and a caller keying a cache on the
    // bytes does not need to know: two `Bind`s that encoded the same value
    // differently produce different bytes and therefore different keys, which
    // costs an entry and cannot merge two questions into one.
    let formats = r.i16("parameter_format_count")?;
    for _ in 0..formats.max(0) {
        r.i16("parameter_format")?;
    }

    let count = r.i16("parameter_count")?;
    // Not `with_capacity`. The count is the client's and the values have not
    // been read yet, so reserving on it is a nine-byte message asking for
    // thirty-two thousand pointers.
    let mut values = Vec::new();
    for _ in 0..count.max(0) {
        let len = r.i32("parameter_length")?;
        if len < 0 {
            // SQL NULL. Any negative length is one: the protocol says -1, and
            // treating -2 as a length would be reading backwards.
            values.push(None);
            continue;
        }
        let len = usize::try_from(len).unwrap_or(0);
        values.push(Some(r.bytes(len, "parameter_value")?));
    }

    Ok(BindParameters { values })
}

/// Decodes a frontend frame.
///
/// # Errors
///
/// Fails when a message the proxy acts on is malformed.
pub fn decode<'a>(frame: &Frame<'a>) -> Result<FrontendMessage<'a>, FrontendError> {
    let mut r = Reader::new(frame.body());

    Ok(match frame.tag() {
        Tag::QUERY => FrontendMessage::Query {
            sql: r.cstr("sql")?,
        },
        Tag::PARSE => FrontendMessage::Parse {
            statement: r.cstr("statement_name")?,
            sql: r.cstr("sql")?,
        },
        Tag::BIND => FrontendMessage::Bind {
            portal: r.cstr("portal_name")?,
            statement: r.cstr("statement_name")?,
        },
        Tag::EXECUTE => FrontendMessage::Execute {
            portal: r.cstr("portal_name")?,
            max_rows: r.i32("max_rows")?,
        },
        Tag::DESCRIBE => FrontendMessage::Describe {
            target: Target::from_byte(r.u8("describe_target")?)?,
            name: r.cstr("name")?,
        },
        // Close and CommandComplete share the tag `C`. Direction disambiguates,
        // which is why decoding is never direction-agnostic.
        Tag::CLOSE => FrontendMessage::Close {
            target: Target::from_byte(r.u8("close_target")?)?,
            name: r.cstr("name")?,
        },
        Tag::SYNC => FrontendMessage::Sync,
        Tag::FLUSH => FrontendMessage::Flush,
        Tag::TERMINATE => FrontendMessage::Terminate,
        Tag::PASSWORD => FrontendMessage::Password,
        Tag::FUNCTION_CALL => FrontendMessage::FunctionCall,
        Tag::COPY_DATA => FrontendMessage::CopyData,
        Tag::COPY_DONE => FrontendMessage::CopyDone,
        Tag::COPY_FAIL => FrontendMessage::CopyFail,
        other => FrontendMessage::Opaque(other),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    #[test]
    fn a_binds_parameter_values_come_back_in_order() {
        // `M9.12`. Two bindings of one statement are two questions, and until
        // this existed they shared a cache key.
        let mut out = Vec::new();
        crate::encode_frontend::bind_with_parameters(
            &mut out,
            "",
            "s1",
            &[Some(b"alice"), None, Some(b""), Some(b"42")],
        );
        let frame = frame(Tag::BIND, &out[5..]);

        let params = bind_parameters(&frame).unwrap();
        assert_eq!(
            params.values(),
            &[Some(&b"alice"[..]), None, Some(&b""[..]), Some(&b"42"[..])]
        );
        assert_eq!(params.len(), 4);
        assert!(!params.is_empty());
    }

    #[test]
    fn a_null_and_an_empty_value_are_not_the_same_parameter() {
        // The distinction the wire draws with a length of -1, and the one a
        // cache key has to keep: `WHERE name = ''` and `WHERE name IS NULL`
        // are different questions with different answers.
        let mut null = Vec::new();
        crate::encode_frontend::bind_with_parameters(&mut null, "", "s", &[None]);
        let mut empty = Vec::new();
        crate::encode_frontend::bind_with_parameters(&mut empty, "", "s", &[Some(b"")]);

        let null = bind_parameters(&frame(Tag::BIND, &null[5..])).unwrap();
        let empty = bind_parameters(&frame(Tag::BIND, &empty[5..])).unwrap();

        assert_eq!(null.values(), &[None]);
        assert_eq!(empty.values(), &[Some(&b""[..])]);
        assert_ne!(null, empty);
    }

    #[test]
    fn a_bind_with_no_parameters_reads_as_none_rather_than_failing() {
        let mut out = Vec::new();
        crate::encode_frontend::bind(&mut out, "p", "s");
        let params = bind_parameters(&frame(Tag::BIND, &out[5..])).unwrap();
        assert!(params.is_empty());
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn format_codes_are_skipped_rather_than_read_as_values() {
        // A `Bind` may carry a format code per parameter before the values.
        // A reader that did not skip them would take the first code as a
        // length and read the values at an offset.
        let mut body = Vec::new();
        body.extend_from_slice(b"\0");
        body.extend_from_slice(b"s\0");
        body.extend_from_slice(&2_i16.to_be_bytes()); // two format codes
        body.extend_from_slice(&0_i16.to_be_bytes());
        body.extend_from_slice(&1_i16.to_be_bytes()); // binary
        body.extend_from_slice(&2_i16.to_be_bytes()); // two values
        body.extend_from_slice(&3_i32.to_be_bytes());
        body.extend_from_slice(b"abc");
        body.extend_from_slice(&1_i32.to_be_bytes());
        body.extend_from_slice(&[0xff]);
        body.extend_from_slice(&0_i16.to_be_bytes());

        let params = bind_parameters(&frame(Tag::BIND, &body)).unwrap();
        assert_eq!(params.values(), &[Some(&b"abc"[..]), Some(&[0xff_u8][..])]);
    }

    #[test]
    fn something_that_is_not_a_bind_is_refused_by_tag() {
        let mut out = Vec::new();
        crate::encode_frontend::query(&mut out, "SELECT 1");
        let err = bind_parameters(&frame(Tag::QUERY, &out[5..])).unwrap_err();
        assert!(matches!(err, FrontendError::NotABind(_)), "{err:?}");
    }

    #[test]
    fn a_length_longer_than_the_frame_is_refused_rather_than_allocated() {
        // The reason this is its own task. A count and a length both come from
        // the client, and a decoder that trusted either is how a nine-byte
        // message becomes an allocation.
        let mut body = Vec::new();
        body.extend_from_slice(b"\0");
        body.extend_from_slice(b"\0");
        body.extend_from_slice(&0_i16.to_be_bytes());
        body.extend_from_slice(&1_i16.to_be_bytes());
        body.extend_from_slice(&i32::MAX.to_be_bytes()); // two gigabytes, in a nine-byte frame
        assert!(bind_parameters(&frame(Tag::BIND, &body)).is_err());
    }

    #[test]
    fn a_count_larger_than_what_follows_it_is_refused() {
        // Thirty-two thousand parameters claimed and none supplied. Nothing
        // reserves on the count, so this is a short read rather than a
        // half-gigabyte `Vec`.
        let mut body = Vec::new();
        body.extend_from_slice(b"\0");
        body.extend_from_slice(b"\0");
        body.extend_from_slice(&0_i16.to_be_bytes());
        body.extend_from_slice(&i16::MAX.to_be_bytes());
        assert!(bind_parameters(&frame(Tag::BIND, &body)).is_err());
    }

    #[test]
    fn a_negative_count_reads_as_no_parameters() {
        // Not a length the protocol produces, so what matters is that it is
        // refused or read as nothing rather than turned into a loop bound.
        for count in [-1_i16, i16::MIN] {
            let mut body = Vec::new();
            body.extend_from_slice(b"\0");
            body.extend_from_slice(b"\0");
            body.extend_from_slice(&0_i16.to_be_bytes());
            body.extend_from_slice(&count.to_be_bytes());
            body.extend_from_slice(&0_i16.to_be_bytes());
            let params = bind_parameters(&frame(Tag::BIND, &body)).unwrap();
            assert!(params.is_empty(), "{count}");
        }
    }

    #[test]
    fn reading_parameters_never_panics_on_arbitrary_bytes() {
        // The fuzz target covers this properly; this is the cheap version that
        // runs in tier 1, where the fuzz target does not.
        for len in 0..40_usize {
            let body: Vec<u8> = (0..len)
                .map(|i| u8::try_from(i % 256).unwrap_or(0).wrapping_mul(37))
                .collect();
            let _ = bind_parameters(&frame(Tag::BIND, &body));
        }
    }

    use super::*;

    fn frame(tag: Tag, body: &[u8]) -> Frame<'_> {
        Frame::new(tag, body)
    }

    #[test]
    fn simple_query_decodes() {
        let decoded = decode(&frame(Tag::QUERY, b"SELECT 1\x00")).unwrap();
        assert_eq!(decoded, FrontendMessage::Query { sql: "SELECT 1" });
    }

    #[test]
    fn parse_yields_the_statement_name_and_sql() {
        // The names prepared statement mapping rewrites.
        let decoded = decode(&frame(Tag::PARSE, b"s1\x00SELECT $1\x00\x00\x00")).unwrap();
        assert_eq!(
            decoded,
            FrontendMessage::Parse {
                statement: "s1",
                sql: "SELECT $1",
            }
        );
    }

    #[test]
    fn the_unnamed_statement_is_the_empty_string() {
        // Drivers that do not name their prepares use this, and it must not be
        // confused with a missing field.
        let decoded = decode(&frame(Tag::PARSE, b"\x00SELECT 1\x00\x00\x00")).unwrap();
        assert_eq!(
            decoded,
            FrontendMessage::Parse {
                statement: "",
                sql: "SELECT 1",
            }
        );
    }

    #[test]
    fn bind_yields_both_names() {
        let decoded = decode(&frame(Tag::BIND, b"p1\x00s1\x00\x00\x00")).unwrap();
        assert_eq!(
            decoded,
            FrontendMessage::Bind {
                portal: "p1",
                statement: "s1",
            }
        );
    }

    #[test]
    fn execute_yields_its_row_limit() {
        let mut body = b"p1\x00".to_vec();
        body.extend_from_slice(&100_i32.to_be_bytes());
        assert_eq!(
            decode(&frame(Tag::EXECUTE, &body)).unwrap(),
            FrontendMessage::Execute {
                portal: "p1",
                max_rows: 100,
            }
        );
    }

    #[test]
    fn describe_and_close_distinguish_statements_from_portals() {
        for (tag, byte, target) in [
            (Tag::DESCRIBE, b'S', Target::Statement),
            (Tag::DESCRIBE, b'P', Target::Portal),
            (Tag::CLOSE, b'S', Target::Statement),
            (Tag::CLOSE, b'P', Target::Portal),
        ] {
            let mut body = vec![byte];
            body.extend_from_slice(b"name\x00");
            let decoded = decode(&frame(tag, &body)).unwrap();
            match decoded {
                FrontendMessage::Describe { target: t, name }
                | FrontendMessage::Close { target: t, name } => {
                    assert_eq!(t, target);
                    assert_eq!(name, "name");
                }
                other => unreachable!("{other:?}"),
            }
        }
    }

    #[test]
    fn an_unknown_describe_target_is_an_error() {
        let err = decode(&frame(Tag::DESCRIBE, b"Xname\x00")).unwrap_err();
        assert_eq!(err, FrontendError::UnknownTarget(b'X'));
    }

    #[test]
    fn empty_bodied_messages_decode() {
        for (tag, expected) in [
            (Tag::SYNC, FrontendMessage::Sync),
            (Tag::FLUSH, FrontendMessage::Flush),
            (Tag::TERMINATE, FrontendMessage::Terminate),
            (Tag::COPY_DONE, FrontendMessage::CopyDone),
        ] {
            assert_eq!(decode(&frame(tag, b"")).unwrap(), expected);
        }
    }

    #[test]
    fn a_password_message_never_exposes_its_body() {
        // It carries the JWT. The decoded form has nowhere to put it, which is
        // stronger than remembering not to log it.
        let decoded = decode(&frame(Tag::PASSWORD, b"eyJhbGciOiJSUzI1NiJ9.secret\x00")).unwrap();
        assert_eq!(decoded, FrontendMessage::Password);
        assert!(!format!("{decoded:?}").contains("eyJ"));
    }

    #[test]
    fn extended_sequence_membership_is_recognised() {
        // A connection inside a sequence must not be released even at an
        // apparently idle transaction status.
        let in_sequence = [
            FrontendMessage::Parse {
                statement: "",
                sql: "",
            },
            FrontendMessage::Bind {
                portal: "",
                statement: "",
            },
            FrontendMessage::Execute {
                portal: "",
                max_rows: 0,
            },
            FrontendMessage::Describe {
                target: Target::Statement,
                name: "",
            },
            FrontendMessage::Close {
                target: Target::Portal,
                name: "",
            },
        ];
        for msg in in_sequence {
            assert!(
                msg.starts_extended_sequence(),
                "{msg:?} should be in-sequence"
            );
        }

        for msg in [
            FrontendMessage::Sync,
            FrontendMessage::Query { sql: "" },
            FrontendMessage::Terminate,
            FrontendMessage::Flush,
        ] {
            assert!(!msg.starts_extended_sequence(), "{msg:?} should not be");
        }
    }

    #[test]
    fn a_function_call_is_recognised_without_being_parsed() {
        // Recognised so it can be routed deliberately; unparsed so no OID table
        // has to track a tenant's extensions. See ADR 0013.
        let decoded = decode(&frame(Tag::FUNCTION_CALL, b"\x00\x00\x04\xd2payload")).unwrap();
        assert_eq!(decoded, FrontendMessage::FunctionCall);
        assert!(!format!("{decoded:?}").contains("payload"));
    }

    #[test]
    fn a_function_call_is_not_an_extended_sequence() {
        // It is answered by a ReadyForQuery of its own, like a simple query,
        // rather than by a Sync.
        assert!(!FrontendMessage::FunctionCall.starts_extended_sequence());
    }

    #[test]
    fn copy_messages_decode() {
        assert_eq!(
            decode(&frame(Tag::COPY_DATA, b"1,2,3\n")).unwrap(),
            FrontendMessage::CopyData
        );
        assert_eq!(
            decode(&frame(Tag::COPY_FAIL, b"aborted\x00")).unwrap(),
            FrontendMessage::CopyFail
        );
    }

    #[test]
    fn truncated_messages_are_errors_not_panics() {
        for tag in [
            Tag::QUERY,
            Tag::PARSE,
            Tag::BIND,
            Tag::EXECUTE,
            Tag::DESCRIBE,
            Tag::CLOSE,
        ] {
            assert!(
                decode(&frame(tag, b"")).is_err(),
                "{tag} accepted an empty body"
            );
        }
    }

    #[test]
    fn unrecognised_tags_pass_through_opaquely() {
        assert_eq!(
            decode(&frame(Tag(b'?'), b"whatever")).unwrap(),
            FrontendMessage::Opaque(Tag(b'?'))
        );
    }

    #[test]
    fn decoding_never_panics_on_arbitrary_bodies() {
        let tags = [
            Tag::QUERY,
            Tag::PARSE,
            Tag::BIND,
            Tag::EXECUTE,
            Tag::DESCRIBE,
            Tag::CLOSE,
            Tag::SYNC,
            Tag::PASSWORD,
        ];

        let mut seed = 0x0123_4567_89AB_CDEF_u64;
        for _ in 0..20_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let len = usize::try_from(seed % 24).unwrap();
            let body: Vec<u8> = (0..len)
                .map(|i| u8::try_from((seed >> (i % 8 * 8)) & 0xFF).unwrap())
                .collect();
            let tag = tags[usize::try_from(seed % 8).unwrap()];

            let _ = decode(&frame(tag, &body));
        }
    }
}
