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
        Tag::COPY_DATA => FrontendMessage::CopyData,
        Tag::COPY_DONE => FrontendMessage::CopyDone,
        Tag::COPY_FAIL => FrontendMessage::CopyFail,
        other => FrontendMessage::Opaque(other),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
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
