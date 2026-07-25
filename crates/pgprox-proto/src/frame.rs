//! Frame boundaries.
//!
//! A Postgres message is a one-byte type tag, a four-byte big-endian length,
//! and a body. The length counts itself but not the tag, which is the detail
//! everyone gets wrong once.
//!
//! Startup-phase messages (`StartupMessage`, `SSLRequest`, `CancelRequest`)
//! have no tag: they are a length followed by a body. They are decoded by
//! [`decode_untagged`].
//!
//! # Nothing here allocates
//!
//! Decoding borrows from the caller's buffer. A client declaring a 2 GB message
//! is refused by a length check before anything is reserved, so the classic
//! failure of trusting a declared length and allocating it cannot happen.

use std::fmt;

/// Bytes a message length prefix occupies, and the minimum legal value of that
/// prefix since it counts itself.
pub const LEN_PREFIX: usize = 4;

/// Largest message the proxy will relay, 1 GiB.
///
/// This matches Postgres's own limit, because it applies to bytes passing
/// through rather than bytes held. A `DataRow` carrying a 500 MB `bytea` is a
/// legitimate answer to a legitimate query, and an earlier 64 MiB value here
/// meant this proxy refused queries real Postgres answers fine.
pub const DEFAULT_MAX_FRAME: usize = 1024 * 1024 * 1024;

/// Largest message body the proxy will buffer in order to read it.
///
/// Distinct from [`DEFAULT_MAX_FRAME`], and the distinction is the whole point.
/// Bytes relayed are never held, so their limit can be generous. Bytes parsed
/// are held per connection, so at 100k connections their limit must be small.
///
/// Everything the proxy inspects is small by nature: `ReadyForQuery` is one
/// byte, an `ErrorResponse` is a few hundred, and the names in `Parse` and
/// `Bind` sit at the front of the body. 1 MiB is generous for all of it.
pub const DEFAULT_MAX_INSPECT: usize = 1024 * 1024;

/// The inspect cap must stay below the relay cap. Checked at compile time, so
/// swapping the two values cannot ship.
const _: () = assert!(DEFAULT_MAX_INSPECT < DEFAULT_MAX_FRAME);

/// A message type tag.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Tag(pub u8);

impl Tag {
    // Frontend, client to server.
    /// `Q`, simple query.
    pub const QUERY: Self = Self(b'Q');
    /// `P`, parse a statement.
    pub const PARSE: Self = Self(b'P');
    /// `B`, bind a portal.
    pub const BIND: Self = Self(b'B');
    /// `E`, execute a portal.
    pub const EXECUTE: Self = Self(b'E');
    /// `D`, describe a statement or portal.
    pub const DESCRIBE: Self = Self(b'D');
    /// `C`, close a statement or portal.
    pub const CLOSE: Self = Self(b'C');
    /// `S`, sync, ending an extended query sequence.
    pub const SYNC: Self = Self(b'S');
    /// `X`, terminate.
    pub const TERMINATE: Self = Self(b'X');
    /// `p`, password or SASL payload.
    pub const PASSWORD: Self = Self(b'p');
    /// `H`, flush.
    pub const FLUSH: Self = Self(b'H');
    /// `f`, the frontend failing a COPY.
    pub const COPY_FAIL: Self = Self(b'f');
    /// `F`, a fast-path function call. Relayed, never parsed. See ADR 0013.
    pub const FUNCTION_CALL: Self = Self(b'F');

    // Backend, server to client. Several tags are reused across directions,
    // which is why decoding is always direction-aware.
    /// `R`, an authentication request.
    pub const AUTHENTICATION: Self = Self(b'R');
    /// `K`, backend key data, used for cancellation.
    pub const BACKEND_KEY_DATA: Self = Self(b'K');
    /// `Z`, ready for query, carrying the transaction status.
    pub const READY_FOR_QUERY: Self = Self(b'Z');
    /// `T`, row description.
    pub const ROW_DESCRIPTION: Self = Self(b'T');
    /// `D`, a data row. Never parsed; forwarded as opaque bytes.
    pub const DATA_ROW: Self = Self(b'D');
    /// `C`, command complete.
    pub const COMMAND_COMPLETE: Self = Self(b'C');
    /// `I`, the statement was empty.
    ///
    /// Sent instead of `CommandComplete`, not in addition to it, so anything
    /// counting completions must recognise both or it miscounts every empty
    /// statement a driver sends.
    pub const EMPTY_QUERY_RESPONSE: Self = Self(b'I');
    /// `E`, an error response.
    pub const ERROR_RESPONSE: Self = Self(b'E');
    /// `N`, a notice response.
    pub const NOTICE_RESPONSE: Self = Self(b'N');
    /// `S`, a runtime parameter status.
    pub const PARAMETER_STATUS: Self = Self(b'S');
    /// `A`, an asynchronous notification.
    pub const NOTIFICATION_RESPONSE: Self = Self(b'A');
    /// `v`, protocol version negotiation.
    pub const NEGOTIATE_PROTOCOL_VERSION: Self = Self(b'v');
    /// `1`, parse complete.
    pub const PARSE_COMPLETE: Self = Self(b'1');
    /// `2`, bind complete.
    pub const BIND_COMPLETE: Self = Self(b'2');
    /// `3`, close complete.
    pub const CLOSE_COMPLETE: Self = Self(b'3');
    /// `t`, the parameter types of a described statement.
    ///
    /// Needed to rewrite a `Describe` response under statement mapping: a
    /// client asks how many parameters its statement takes and refuses to bind
    /// a different number. See ADR 0011.
    pub const PARAMETER_DESCRIPTION: Self = Self(b't');
    /// `V`, the result of a fast-path function call.
    pub const FUNCTION_CALL_RESPONSE: Self = Self(b'V');
    /// `n`, no data.
    pub const NO_DATA: Self = Self(b'n');
    /// `s`, portal suspended.
    pub const PORTAL_SUSPENDED: Self = Self(b's');
    /// `G`, the server is ready to receive COPY data.
    pub const COPY_IN_RESPONSE: Self = Self(b'G');
    /// `H`, the server is about to send COPY data.
    pub const COPY_OUT_RESPONSE: Self = Self(b'H');
    /// `W`, COPY both ways, used by replication.
    pub const COPY_BOTH_RESPONSE: Self = Self(b'W');
    /// `d`, a chunk of COPY data. Flows in both directions.
    pub const COPY_DATA: Self = Self(b'd');
    /// `c`, the end of a COPY stream. Flows in both directions.
    pub const COPY_DONE: Self = Self(b'c');

    /// The raw byte.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl fmt::Debug for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_ascii_graphic() {
            write!(f, "Tag({})", self.0 as char)
        } else {
            write!(f, "Tag(0x{:02x})", self.0)
        }
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_ascii_graphic() {
            write!(f, "{}", self.0 as char)
        } else {
            write!(f, "0x{:02x}", self.0)
        }
    }
}

/// A decoded message, borrowing its body from the caller's buffer.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Frame<'a> {
    tag: Tag,
    body: &'a [u8],
}

impl<'a> Frame<'a> {
    /// Builds a frame from a tag and body.
    #[must_use]
    pub const fn new(tag: Tag, body: &'a [u8]) -> Self {
        Self { tag, body }
    }

    /// The message type.
    #[must_use]
    pub const fn tag(&self) -> Tag {
        self.tag
    }

    /// The body, excluding the tag and length prefix.
    #[must_use]
    pub const fn body(&self) -> &'a [u8] {
        self.body
    }

    /// Bytes this frame occupies on the wire, including tag and length.
    #[must_use]
    pub const fn wire_len(&self) -> usize {
        1 + LEN_PREFIX + self.body.len()
    }
}

impl fmt::Debug for Frame<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Body length only, never contents: frames carry client traffic, which
        // routinely holds customer data in SQL literals.
        f.debug_struct("Frame")
            .field("tag", &self.tag)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// Why a frame could not be decoded.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DecodeError {
    /// The declared length is smaller than the length prefix itself, so the
    /// message cannot be well formed.
    #[error("declared length {declared} is below the {LEN_PREFIX}-byte minimum")]
    LengthTooSmall {
        /// What the message claimed.
        declared: u32,
    },
    /// The declared length exceeds the configured maximum.
    ///
    /// Returned before anything is reserved. A client claiming a huge message
    /// gets an error, never an allocation.
    #[error("declared length {declared} exceeds the maximum of {max}")]
    LengthTooLarge {
        /// What the message claimed.
        declared: u32,
        /// The configured limit.
        max: usize,
    },
}

/// A message header: everything knowable from the first five bytes.
///
/// Reading this without the body is what lets the relay start forwarding before
/// a message has finished arriving.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameHeader {
    /// The message type.
    pub tag: Tag,
    /// Body length, excluding the tag and the length prefix.
    pub body_len: usize,
}

impl FrameHeader {
    /// Bytes this message occupies on the wire in total.
    #[must_use]
    pub const fn wire_len(&self) -> usize {
        1 + LEN_PREFIX + self.body_len
    }
}

/// Reads a tagged message header from the first five bytes of `buf`.
///
/// Returns [`None`] when fewer than five bytes have arrived. The body need not
/// be present, which is the difference from [`decode`].
///
/// # Errors
///
/// Fails when the declared length is impossible or exceeds `max_frame`.
pub fn decode_header(buf: &[u8], max_frame: usize) -> Result<Option<FrameHeader>, DecodeError> {
    if buf.len() < 1 + LEN_PREFIX {
        return Ok(None);
    }
    let declared = read_u32(&buf[1..]);
    Ok(Some(FrameHeader {
        tag: Tag(buf[0]),
        body_len: check_length(declared, max_frame)?,
    }))
}

/// Which side of the connection a message came from.
///
/// Several tags mean different things per direction: `C` is `Close` from a
/// client and `CommandComplete` from a server, `D` is `Describe` or `DataRow`,
/// `S` is `Sync` or `ParameterStatus`. A policy that ignored direction would
/// buffer result rows in order to read them as `Describe` messages.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// Client to server.
    Frontend,
    /// Server to client.
    Backend,
}

/// How much of a message the proxy needs to read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Inspect {
    /// Relay it without reading it. The only option that is free.
    None,
    /// Read this many leading bytes, relaying the rest untouched.
    ///
    /// Used where the interesting fields sit at the front of an otherwise
    /// unbounded body: `Bind` names precede parameter values that can be
    /// hundreds of megabytes.
    Prefix(usize),
    /// Read the whole body, so it must be buffered.
    ///
    /// Only for messages that are small by construction.
    Whole,
}

/// How much of a message this proxy needs to read, by direction and tag.
///
/// The default is [`Inspect::None`]. Anything not named here is relayed
/// untouched, which is what keeps `DataRow` and `CopyData` off the buffering
/// path entirely.
#[must_use]
pub fn inspect_policy(direction: Direction, tag: Tag) -> Inspect {
    match direction {
        Direction::Backend => match tag {
            // Small by construction, and all carry state the proxy acts on.
            Tag::READY_FOR_QUERY
            | Tag::AUTHENTICATION
            | Tag::PARAMETER_STATUS
            | Tag::BACKEND_KEY_DATA
            | Tag::COMMAND_COMPLETE
            | Tag::EMPTY_QUERY_RESPONSE
            | Tag::PARAMETER_DESCRIPTION
            | Tag::NEGOTIATE_PROTOCOL_VERSION
            | Tag::COPY_IN_RESPONSE
            | Tag::COPY_OUT_RESPONSE
            | Tag::COPY_BOTH_RESPONSE
            | Tag::COPY_DONE => Inspect::Whole,

            // All three have the fields we want at the front and an
            // unbounded tail: a server can attach a long detail or hint to an
            // error, and a NOTIFY payload is client-controlled.
            Tag::ERROR_RESPONSE | Tag::NOTICE_RESPONSE | Tag::NOTIFICATION_RESPONSE => {
                Inspect::Prefix(8 * 1024)
            }

            // DataRow lands here, which is the point.
            _ => Inspect::None,
        },
        Direction::Frontend => match tag {
            Tag::SYNC | Tag::FLUSH | Tag::TERMINATE | Tag::COPY_DONE => Inspect::Whole,

            // The SQL is needed for classification and for statement mapping,
            // but a generated statement can be enormous. Past the prefix it is
            // treated as unclassifiable, which routes to the primary.
            Tag::QUERY | Tag::PARSE => Inspect::Prefix(64 * 1024),

            // Names first, then parameter values that can be huge.
            Tag::BIND | Tag::EXECUTE | Tag::DESCRIBE | Tag::CLOSE => Inspect::Prefix(4 * 1024),

            // CopyData, CopyFail, and Password land here. Password is
            // deliberately among them: it carries the JWT, and a proxy that
            // never copies it cannot leak it.
            _ => Inspect::None,
        },
    }
}

/// What a decode attempt produced.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decoded<'a> {
    /// A complete frame, and how many bytes of the buffer it consumed.
    Frame(Frame<'a>, usize),
    /// Not enough bytes yet. Carries how many are needed in total for the
    /// frame currently at the front of the buffer, so a caller can size its
    /// next read instead of guessing.
    Incomplete {
        /// Total bytes needed, when known from the length prefix.
        needed: Option<usize>,
    },
}

/// Decodes one tagged message from the front of `buf`.
///
/// Returns [`Decoded::Incomplete`] when more bytes are needed. The buffer is
/// never modified; the caller advances it by the returned length.
///
/// # Errors
///
/// Fails when the declared length is impossible or exceeds `max_frame`.
pub fn decode(buf: &[u8], max_frame: usize) -> Result<Decoded<'_>, DecodeError> {
    if buf.len() < 1 + LEN_PREFIX {
        return Ok(Decoded::Incomplete { needed: None });
    }

    let tag = Tag(buf[0]);
    let declared = read_u32(&buf[1..]);
    let body_len = check_length(declared, max_frame)?;

    let total = 1 + LEN_PREFIX + body_len;
    if buf.len() < total {
        return Ok(Decoded::Incomplete {
            needed: Some(total),
        });
    }

    Ok(Decoded::Frame(
        Frame::new(tag, &buf[1 + LEN_PREFIX..total]),
        total,
    ))
}

/// Decodes one untagged message: a length prefix followed by a body.
///
/// Used for `StartupMessage`, `SSLRequest`, `GSSENCRequest`, and
/// `CancelRequest`, none of which carry a type tag.
///
/// # Errors
///
/// Fails when the declared length is impossible or exceeds `max_frame`.
pub fn decode_untagged(buf: &[u8], max_frame: usize) -> Result<Decoded<'_>, DecodeError> {
    if buf.len() < LEN_PREFIX {
        return Ok(Decoded::Incomplete { needed: None });
    }

    let declared = read_u32(buf);
    let body_len = check_length(declared, max_frame)?;

    let total = LEN_PREFIX + body_len;
    if buf.len() < total {
        return Ok(Decoded::Incomplete {
            needed: Some(total),
        });
    }

    // Tag 0 is not a real message type; untagged frames are distinguished by
    // the caller having chosen this function, not by inspecting the tag.
    Ok(Decoded::Frame(
        Frame::new(Tag(0), &buf[LEN_PREFIX..total]),
        total,
    ))
}

/// Validates a declared length and returns the body length it implies.
fn check_length(declared: u32, max_frame: usize) -> Result<usize, DecodeError> {
    let declared_usize = declared as usize;

    if declared_usize < LEN_PREFIX {
        return Err(DecodeError::LengthTooSmall { declared });
    }
    // Checked before any buffer is sized. This is the allocation guard.
    if declared_usize > max_frame {
        return Err(DecodeError::LengthTooLarge {
            declared,
            max: max_frame,
        });
    }

    Ok(declared_usize - LEN_PREFIX)
}

/// Reads a big-endian `u32` from the first four bytes of `buf`.
fn read_u32(buf: &[u8]) -> u32 {
    // Callers check the length first; this indexes a fixed window so there is
    // no slice-length arithmetic to get wrong.
    u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]])
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Builds a tagged message on the wire.
    #[allow(clippy::cast_possible_truncation)]
    fn wire(tag: Tag, body: &[u8]) -> Vec<u8> {
        let mut out = vec![tag.get()];
        let len = u32::try_from(body.len() + LEN_PREFIX).unwrap();
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn decodes_a_complete_frame() {
        let bytes = wire(Tag::QUERY, b"SELECT 1\0");
        let Decoded::Frame(frame, consumed) = decode(&bytes, DEFAULT_MAX_FRAME).unwrap() else {
            unreachable!("should have decoded");
        };
        assert_eq!(frame.tag(), Tag::QUERY);
        assert_eq!(frame.body(), b"SELECT 1\0");
        assert_eq!(consumed, bytes.len());
        assert_eq!(frame.wire_len(), bytes.len());
    }

    #[test]
    fn a_frame_split_at_every_boundary_reassembles() {
        // TCP delivers arbitrary chunks. Decoding must be a pure function of
        // how many bytes have arrived, never of how they were split.
        let bytes = wire(Tag::QUERY, b"SELECT pg_sleep(0)\0");

        for split in 0..bytes.len() {
            let partial = &bytes[..split];
            match decode(partial, DEFAULT_MAX_FRAME).unwrap() {
                Decoded::Incomplete { needed } => {
                    if split > LEN_PREFIX {
                        assert_eq!(
                            needed,
                            Some(bytes.len()),
                            "should know the full size once the prefix has arrived"
                        );
                    } else {
                        assert_eq!(needed, None, "cannot know the size before the prefix");
                    }
                }
                Decoded::Frame(..) => unreachable!("decoded from {split} of {} bytes", bytes.len()),
            }
        }

        assert!(matches!(
            decode(&bytes, DEFAULT_MAX_FRAME).unwrap(),
            Decoded::Frame(..)
        ));
    }

    #[test]
    fn extra_bytes_after_a_frame_are_left_alone() {
        // Pipelined messages arrive in one read. Decoding one must not consume
        // the next.
        let mut bytes = wire(Tag::SYNC, b"");
        let first_len = bytes.len();
        bytes.extend_from_slice(&wire(Tag::QUERY, b"SELECT 1\0"));

        let Decoded::Frame(frame, consumed) = decode(&bytes, DEFAULT_MAX_FRAME).unwrap() else {
            unreachable!()
        };
        assert_eq!(frame.tag(), Tag::SYNC);
        assert_eq!(consumed, first_len);

        let Decoded::Frame(second, _) = decode(&bytes[consumed..], DEFAULT_MAX_FRAME).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(second.tag(), Tag::QUERY);
    }

    #[test]
    fn an_oversized_length_is_refused_without_allocating() {
        // The classic failure: trusting a declared length and reserving it. The
        // check happens on a 5-byte buffer, so there is nothing to allocate.
        let mut bytes = vec![Tag::QUERY.get()];
        bytes.extend_from_slice(&u32::MAX.to_be_bytes());

        let err = decode(&bytes, DEFAULT_MAX_FRAME).unwrap_err();
        assert_eq!(
            err,
            DecodeError::LengthTooLarge {
                declared: u32::MAX,
                max: DEFAULT_MAX_FRAME,
            }
        );
        assert!(err.to_string().contains("exceeds the maximum"));
    }

    #[test]
    fn the_maximum_is_configurable_and_enforced_exactly() {
        let body = vec![0_u8; 100];
        let bytes = wire(Tag::COPY_DATA, &body);
        let declared = body.len() + LEN_PREFIX;

        // Exactly at the limit is allowed.
        assert!(matches!(
            decode(&bytes, declared).unwrap(),
            Decoded::Frame(..)
        ));
        // One below is not.
        assert!(decode(&bytes, declared - 1).is_err());
    }

    #[test]
    fn a_length_below_the_prefix_size_is_malformed() {
        for declared in 0..u32::try_from(LEN_PREFIX).unwrap() {
            let mut bytes = vec![Tag::QUERY.get()];
            bytes.extend_from_slice(&declared.to_be_bytes());
            bytes.extend_from_slice(b"junk");

            let err = decode(&bytes, DEFAULT_MAX_FRAME).unwrap_err();
            assert_eq!(err, DecodeError::LengthTooSmall { declared });
        }
    }

    #[test]
    fn an_empty_body_is_legal() {
        // Sync, Flush, Terminate, CopyDone and several others carry nothing.
        let bytes = wire(Tag::SYNC, b"");
        let Decoded::Frame(frame, consumed) = decode(&bytes, DEFAULT_MAX_FRAME).unwrap() else {
            unreachable!()
        };
        assert!(frame.body().is_empty());
        assert_eq!(consumed, 1 + LEN_PREFIX);
    }

    #[test]
    fn an_empty_buffer_is_incomplete_not_an_error() {
        assert_eq!(
            decode(&[], DEFAULT_MAX_FRAME).unwrap(),
            Decoded::Incomplete { needed: None }
        );
        assert_eq!(
            decode_untagged(&[], DEFAULT_MAX_FRAME).unwrap(),
            Decoded::Incomplete { needed: None }
        );
    }

    #[test]
    fn untagged_frames_decode_without_a_tag() {
        // SSLRequest: length 8, then the magic 80877103.
        let mut bytes = 8_u32.to_be_bytes().to_vec();
        bytes.extend_from_slice(&80_877_103_u32.to_be_bytes());

        let Decoded::Frame(frame, consumed) = decode_untagged(&bytes, DEFAULT_MAX_FRAME).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(consumed, 8);
        assert_eq!(frame.body(), &80_877_103_u32.to_be_bytes());
    }

    #[test]
    fn an_untagged_frame_split_at_every_boundary_reassembles() {
        let mut bytes = 12_u32.to_be_bytes().to_vec();
        bytes.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);

        for split in 0..bytes.len() {
            assert!(
                matches!(
                    decode_untagged(&bytes[..split], DEFAULT_MAX_FRAME).unwrap(),
                    Decoded::Incomplete { .. }
                ),
                "decoded early at {split}"
            );
        }
        assert!(matches!(
            decode_untagged(&bytes, DEFAULT_MAX_FRAME).unwrap(),
            Decoded::Frame(..)
        ));
    }

    #[test]
    fn untagged_decoding_enforces_the_same_limits() {
        let huge = u32::MAX.to_be_bytes().to_vec();
        assert!(decode_untagged(&huge, DEFAULT_MAX_FRAME).is_err());

        let tiny = 2_u32.to_be_bytes().to_vec();
        assert_eq!(
            decode_untagged(&tiny, DEFAULT_MAX_FRAME).unwrap_err(),
            DecodeError::LengthTooSmall { declared: 2 }
        );
    }

    #[test]
    fn decoding_never_panics_on_arbitrary_input() {
        // The property fuzzing will explore. A malformed frame from the network
        // must not take down a node serving 100k other connections.
        let mut seed = 0x2545_F491_4F6C_DD1D_u64;
        for _ in 0..20_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;

            let len = (seed % 32) as usize;
            let bytes: Vec<u8> = (0..len)
                .map(|i| u8::try_from((seed >> (i % 8 * 8)) & 0xFF).unwrap())
                .collect();

            let _ = decode(&bytes, DEFAULT_MAX_FRAME);
            let _ = decode_untagged(&bytes, DEFAULT_MAX_FRAME);
        }
    }

    #[test]
    fn tags_render_readably_for_logs_and_traces() {
        assert_eq!(format!("{:?}", Tag::QUERY), "Tag(Q)");
        assert_eq!(Tag::QUERY.to_string(), "Q");
        assert_eq!(format!("{:?}", Tag(0)), "Tag(0x00)");
        assert_eq!(Tag(0x01).to_string(), "0x01");
        assert_eq!(Tag::QUERY.get(), b'Q');
    }

    #[test]
    fn frame_debug_never_prints_the_body() {
        // Frames carry client traffic, which holds customer data in literals.
        let frame = Frame::new(Tag::QUERY, b"SELECT ssn FROM people");
        let rendered = format!("{frame:?}");
        assert!(!rendered.contains("ssn"), "leaked in {rendered}");
        assert!(rendered.contains("body_len"));
    }

    #[test]
    fn a_header_decodes_from_exactly_five_bytes() {
        // The property the relay depends on: the body need not have arrived.
        let mut bytes = vec![Tag::DATA_ROW.get()];
        bytes.extend_from_slice(&1_000_004_u32.to_be_bytes());

        let header = decode_header(&bytes, DEFAULT_MAX_FRAME).unwrap().unwrap();
        assert_eq!(header.tag, Tag::DATA_ROW);
        assert_eq!(header.body_len, 1_000_000);
        assert_eq!(header.wire_len(), 1_000_005);

        // decode, by contrast, still wants the whole thing.
        assert!(matches!(
            decode(&bytes, DEFAULT_MAX_FRAME).unwrap(),
            Decoded::Incomplete { .. }
        ));
    }

    #[test]
    fn a_header_needs_all_five_bytes() {
        let bytes = wire(Tag::QUERY, b"x");
        for split in 0..=LEN_PREFIX {
            assert_eq!(
                decode_header(&bytes[..split], DEFAULT_MAX_FRAME).unwrap(),
                None,
                "reported a header from {split} bytes"
            );
        }
        assert!(
            decode_header(&bytes[..=LEN_PREFIX], DEFAULT_MAX_FRAME)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn a_header_enforces_the_same_length_rules() {
        let mut huge = vec![Tag::DATA_ROW.get()];
        huge.extend_from_slice(&u32::MAX.to_be_bytes());
        assert!(decode_header(&huge, DEFAULT_MAX_FRAME).is_err());

        let mut tiny = vec![Tag::QUERY.get()];
        tiny.extend_from_slice(&2_u32.to_be_bytes());
        assert_eq!(
            decode_header(&tiny, DEFAULT_MAX_FRAME).unwrap_err(),
            DecodeError::LengthTooSmall { declared: 2 }
        );
    }

    #[test]
    fn the_relay_limit_matches_postgres_rather_than_a_buffer_size() {
        // The bug this replaced: a 64 MiB cap here refused a legitimate SELECT
        // of a 100 MB bytea, which real Postgres answers fine.
        assert_eq!(DEFAULT_MAX_FRAME, 1024 * 1024 * 1024);

        let mut hundred_mb = vec![Tag::DATA_ROW.get()];
        hundred_mb.extend_from_slice(&(100 * 1024 * 1024_u32 + 4).to_be_bytes());
        assert!(
            decode_header(&hundred_mb, DEFAULT_MAX_FRAME).is_ok(),
            "a 100 MB result row was refused"
        );
    }

    #[test]
    fn bulk_data_is_never_inspected() {
        // The rule that keeps buffering off the hot path.
        assert_eq!(
            inspect_policy(Direction::Backend, Tag::DATA_ROW),
            Inspect::None
        );
        assert_eq!(
            inspect_policy(Direction::Backend, Tag::COPY_DATA),
            Inspect::None
        );
        assert_eq!(
            inspect_policy(Direction::Frontend, Tag::COPY_DATA),
            Inspect::None
        );
        assert_eq!(
            inspect_policy(Direction::Backend, Tag::ROW_DESCRIPTION),
            Inspect::None
        );
    }

    #[test]
    fn a_password_message_is_never_copied() {
        // It carries the JWT. A proxy that never copies it cannot leak it.
        assert_eq!(
            inspect_policy(Direction::Frontend, Tag::PASSWORD),
            Inspect::None
        );
    }

    #[test]
    fn direction_disambiguates_reused_tags() {
        // C is Close from a client and CommandComplete from a server; D is
        // Describe or DataRow; S is Sync or ParameterStatus. A policy ignoring
        // direction would buffer result rows to read them as Describe.
        assert_eq!(
            inspect_policy(Direction::Backend, Tag::DATA_ROW),
            Inspect::None
        );
        assert!(matches!(
            inspect_policy(Direction::Frontend, Tag::DESCRIBE),
            Inspect::Prefix(_)
        ));

        assert_eq!(
            inspect_policy(Direction::Backend, Tag::COMMAND_COMPLETE),
            Inspect::Whole
        );
        assert!(matches!(
            inspect_policy(Direction::Frontend, Tag::CLOSE),
            Inspect::Prefix(_)
        ));

        assert_eq!(
            inspect_policy(Direction::Backend, Tag::PARAMETER_STATUS),
            Inspect::Whole
        );
        assert_eq!(
            inspect_policy(Direction::Frontend, Tag::SYNC),
            Inspect::Whole
        );
    }

    #[test]
    fn messages_with_unbounded_bodies_are_only_prefix_inspected() {
        // Bind carries parameter values that can be hundreds of megabytes; the
        // names are at the front. Whole would put that on the buffering path.
        for tag in [Tag::BIND, Tag::EXECUTE, Tag::DESCRIBE, Tag::CLOSE] {
            match inspect_policy(Direction::Frontend, tag) {
                Inspect::Prefix(n) => assert!(n <= DEFAULT_MAX_INSPECT, "{tag} prefix {n} too big"),
                other => unreachable!("{tag} is {other:?}, not a prefix"),
            }
        }
        for tag in [Tag::QUERY, Tag::PARSE] {
            assert!(matches!(
                inspect_policy(Direction::Frontend, tag),
                Inspect::Prefix(_)
            ));
        }
    }

    #[test]
    fn every_whole_inspected_message_is_small_by_construction() {
        // Whole means buffered, and buffered at 100k connections means the
        // message had better be bounded by the protocol itself.
        let whole: Vec<Tag> = [
            Tag::READY_FOR_QUERY,
            Tag::AUTHENTICATION,
            Tag::PARAMETER_STATUS,
            Tag::BACKEND_KEY_DATA,
            Tag::COMMAND_COMPLETE,
            Tag::NEGOTIATE_PROTOCOL_VERSION,
            Tag::COPY_IN_RESPONSE,
            Tag::COPY_OUT_RESPONSE,
            Tag::COPY_BOTH_RESPONSE,
            Tag::EMPTY_QUERY_RESPONSE,
            Tag::PARAMETER_DESCRIPTION,
        ]
        .into_iter()
        .filter(|t| inspect_policy(Direction::Backend, *t) == Inspect::Whole)
        .collect();
        assert_eq!(whole.len(), 11, "a message changed policy without review");
        assert_eq!(
            inspect_policy(Direction::Backend, Tag::EMPTY_QUERY_RESPONSE),
            Inspect::Whole,
            "an empty query response must be seen, not relayed blindly"
        );
    }

    #[test]
    fn header_decoding_never_panics_on_arbitrary_input() {
        let mut seed = 0xA5A5_1234_DEAD_C0DE_u64;
        for _ in 0..20_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let len = usize::try_from(seed % 12).unwrap();
            let bytes: Vec<u8> = (0..len)
                .map(|i| u8::try_from((seed >> (i % 8 * 8)) & 0xFF).unwrap())
                .collect();
            let _ = decode_header(&bytes, DEFAULT_MAX_FRAME);
            let _ = inspect_policy(Direction::Backend, Tag(bytes.first().copied().unwrap_or(0)));
            let _ = inspect_policy(
                Direction::Frontend,
                Tag(bytes.first().copied().unwrap_or(0)),
            );
        }
    }

    /// Every message code in the Postgres 3.0 protocol, with the direction it
    /// flows and the name Postgres gives it.
    ///
    /// Transcribed from the protocol appendix rather than from our own
    /// constants, so this list is an independent statement of what exists. A
    /// test derived from our own code could only ever agree with itself.
    const PROTOCOL_MESSAGES: &[(Direction, u8, &str)] = &[
        // Frontend, client to server.
        (Direction::Frontend, b'B', "Bind"),
        (Direction::Frontend, b'C', "Close"),
        (Direction::Frontend, b'd', "CopyData"),
        (Direction::Frontend, b'c', "CopyDone"),
        (Direction::Frontend, b'f', "CopyFail"),
        (Direction::Frontend, b'D', "Describe"),
        (Direction::Frontend, b'E', "Execute"),
        (Direction::Frontend, b'H', "Flush"),
        (Direction::Frontend, b'F', "FunctionCall"),
        (Direction::Frontend, b'P', "Parse"),
        (Direction::Frontend, b'p', "PasswordMessage/SASL"),
        (Direction::Frontend, b'Q', "Query"),
        (Direction::Frontend, b'S', "Sync"),
        (Direction::Frontend, b'X', "Terminate"),
        // Backend, server to client.
        (Direction::Backend, b'R', "Authentication"),
        (Direction::Backend, b'K', "BackendKeyData"),
        (Direction::Backend, b'2', "BindComplete"),
        (Direction::Backend, b'3', "CloseComplete"),
        (Direction::Backend, b'C', "CommandComplete"),
        (Direction::Backend, b'd', "CopyData"),
        (Direction::Backend, b'c', "CopyDone"),
        (Direction::Backend, b'G', "CopyInResponse"),
        (Direction::Backend, b'H', "CopyOutResponse"),
        (Direction::Backend, b'W', "CopyBothResponse"),
        (Direction::Backend, b'D', "DataRow"),
        (Direction::Backend, b'I', "EmptyQueryResponse"),
        (Direction::Backend, b'E', "ErrorResponse"),
        (Direction::Backend, b'V', "FunctionCallResponse"),
        (Direction::Backend, b'v', "NegotiateProtocolVersion"),
        (Direction::Backend, b'n', "NoData"),
        (Direction::Backend, b'N', "NoticeResponse"),
        (Direction::Backend, b'A', "NotificationResponse"),
        (Direction::Backend, b't', "ParameterDescription"),
        (Direction::Backend, b'S', "ParameterStatus"),
        (Direction::Backend, b'1', "ParseComplete"),
        (Direction::Backend, b's', "PortalSuspended"),
        (Direction::Backend, b'Z', "ReadyForQuery"),
        (Direction::Backend, b'T', "RowDescription"),
    ];

    #[test]
    fn every_protocol_message_has_a_named_constant() {
        // A message with no constant is indistinguishable from an unknown one,
        // so nothing can route it deliberately. This is the audit that stops a
        // future addition being silently unhandled.
        let defined: std::collections::HashSet<u8> = [
            Tag::QUERY,
            Tag::PARSE,
            Tag::BIND,
            Tag::EXECUTE,
            Tag::DESCRIBE,
            Tag::CLOSE,
            Tag::SYNC,
            Tag::TERMINATE,
            Tag::PASSWORD,
            Tag::FLUSH,
            Tag::COPY_FAIL,
            Tag::FUNCTION_CALL,
            Tag::AUTHENTICATION,
            Tag::BACKEND_KEY_DATA,
            Tag::READY_FOR_QUERY,
            Tag::ROW_DESCRIPTION,
            Tag::DATA_ROW,
            Tag::COMMAND_COMPLETE,
            Tag::EMPTY_QUERY_RESPONSE,
            Tag::ERROR_RESPONSE,
            Tag::NOTICE_RESPONSE,
            Tag::PARAMETER_STATUS,
            Tag::NOTIFICATION_RESPONSE,
            Tag::NEGOTIATE_PROTOCOL_VERSION,
            Tag::PARSE_COMPLETE,
            Tag::BIND_COMPLETE,
            Tag::CLOSE_COMPLETE,
            Tag::NO_DATA,
            Tag::PORTAL_SUSPENDED,
            Tag::PARAMETER_DESCRIPTION,
            Tag::FUNCTION_CALL_RESPONSE,
            Tag::COPY_IN_RESPONSE,
            Tag::COPY_OUT_RESPONSE,
            Tag::COPY_BOTH_RESPONSE,
            Tag::COPY_DATA,
            Tag::COPY_DONE,
        ]
        .into_iter()
        .map(Tag::get)
        .collect();

        let missing: Vec<&str> = PROTOCOL_MESSAGES
            .iter()
            .filter(|(_, code, _)| !defined.contains(code))
            .map(|(_, _, name)| *name)
            .collect();

        assert!(
            missing.is_empty(),
            "protocol messages with no constant: {missing:?}"
        );
    }

    #[test]
    fn every_protocol_message_has_a_stated_inspect_policy() {
        // inspect_policy is total, so this cannot fail by omission. It exists to
        // record the answers, so a change to any of them shows up as a diff on
        // this list rather than as behaviour nobody noticed.
        let mut inspected = Vec::new();
        for (direction, code, name) in PROTOCOL_MESSAGES {
            if inspect_policy(*direction, Tag(*code)) != Inspect::None {
                inspected.push(*name);
            }
        }
        inspected.sort_unstable();

        assert_eq!(
            inspected,
            vec![
                "Authentication",
                "BackendKeyData",
                "Bind",
                "Close",
                "CommandComplete",
                "CopyBothResponse",
                "CopyDone",
                "CopyDone",
                "CopyInResponse",
                "CopyOutResponse",
                "Describe",
                "EmptyQueryResponse",
                "ErrorResponse",
                "Execute",
                "Flush",
                "NegotiateProtocolVersion",
                "NoticeResponse",
                "NotificationResponse",
                "ParameterDescription",
                "ParameterStatus",
                "Parse",
                "Query",
                "ReadyForQuery",
                "Sync",
                "Terminate",
            ],
            "an inspect policy changed; confirm the memory cost is still bounded"
        );
    }

    #[test]
    fn bulk_messages_are_absent_from_the_inspected_set() {
        // The ones that must never be buffered, stated positively so the list
        // above cannot quietly grow to include them.
        for (direction, code, name) in [
            (Direction::Backend, b'D', "DataRow"),
            (Direction::Backend, b'd', "CopyData"),
            (Direction::Frontend, b'd', "CopyData"),
            (Direction::Backend, b'T', "RowDescription"),
            (Direction::Frontend, b'p', "PasswordMessage"),
            (Direction::Frontend, b'F', "FunctionCall"),
            (Direction::Backend, b'V', "FunctionCallResponse"),
        ] {
            assert_eq!(
                inspect_policy(direction, Tag(code)),
                Inspect::None,
                "{name} is being buffered"
            );
        }
    }
}
