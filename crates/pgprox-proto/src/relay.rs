//! Streaming relay.
//!
//! # The problem this solves
//!
//! [`crate::frame::decode`] needs a whole message before it returns one, so a
//! relay built on it must accumulate an entire body before forwarding a byte. A
//! single large `DataRow` would then hold up to a gigabyte, and ADR 0008's whole
//! premise is that an idle connection costs roughly 200 bytes.
//!
//! [`FrameRelay`] instead reads the five-byte header, asks
//! [`crate::frame::inspect_policy`] how much of the body it needs, and forwards
//! the rest as it arrives. Memory is bounded by the inspect cap rather than by
//! the largest message a peer might send.
//!
//! Sans-I/O: bytes go in, instructions come out. Nothing here reads a socket.

use crate::frame::{
    DEFAULT_MAX_FRAME, DecodeError, Direction, FrameHeader, Inspect, LEN_PREFIX, decode_header,
    inspect_policy,
};

/// A message boundary the relay just crossed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub struct Completed {
    /// The header that started the message.
    pub header: FrameHeader,
    /// Whether the inspected portion is the whole body.
    ///
    /// False when the body outran the policy's prefix, so a parser must treat
    /// what it has as truncated rather than as a short message.
    pub complete: bool,
}

/// What one [`FrameRelay::push`] produced.
///
/// # Every consumed byte is forwarded
///
/// `consumed` is not "bytes to maybe forward", it is "bytes that belong to the
/// stream and must go onward", including header bytes the relay held while
/// waiting for the rest of a header. An earlier design signalled forwarding
/// through the step type, which silently dropped header bytes whenever a header
/// arrived split across reads. The boundary-split test found it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub struct RelayOutcome {
    /// Leading input bytes the relay took. Forward all of them.
    pub consumed: usize,
    /// Set when this push finished a message.
    pub completed: Option<Completed>,
}

/// Where a relay is in a message.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    /// Waiting for the five header bytes.
    Header,
    /// Inside a body, with this many bytes still to come.
    Body { remaining: usize },
}

/// Streams messages through, buffering only what must be read.
///
/// One per direction: a proxy holds two, since the inspect policy differs.
#[derive(Clone, Debug)]
pub struct FrameRelay {
    direction: Direction,
    max_frame: usize,
    phase: Phase,
    /// The header of the message in flight.
    header: Option<FrameHeader>,
    /// Bytes still wanted for inspection, counting down.
    want_inspect: usize,
    /// The inspected prefix. Bounded by the policy, never by the body.
    buffer: Vec<u8>,
    /// Partial header bytes, at most four.
    header_buf: Vec<u8>,
}

impl FrameRelay {
    /// A relay for one direction, with the default relay cap.
    #[must_use]
    pub fn new(direction: Direction) -> Self {
        Self::with_max_frame(direction, DEFAULT_MAX_FRAME)
    }

    /// A relay with an explicit cap on relayed message size.
    #[must_use]
    pub fn with_max_frame(direction: Direction, max_frame: usize) -> Self {
        Self {
            direction,
            max_frame,
            phase: Phase::Header,
            header: None,
            want_inspect: 0,
            buffer: Vec::new(),
            header_buf: Vec::new(),
        }
    }

    /// The inspected portion of the message that just completed.
    ///
    /// Empty when the policy was [`Inspect::None`]. Valid until the next
    /// [`FrameRelay::push`].
    #[must_use]
    pub fn inspected(&self) -> &[u8] {
        &self.buffer
    }

    /// The header of the message in flight, if one has been read.
    #[must_use]
    pub const fn header(&self) -> Option<FrameHeader> {
        self.header
    }

    /// Bytes currently held. Bounded by the inspect policy, never by body size.
    #[must_use]
    pub fn buffered(&self) -> usize {
        self.buffer.len() + self.header_buf.len()
    }

    /// Offers bytes to the relay.
    ///
    /// Consumes as much as it can and reports how much. Call again with the
    /// remainder until it consumes nothing.
    ///
    /// # Errors
    ///
    /// Fails when a declared length is impossible or exceeds the relay cap.
    pub fn push(&mut self, input: &[u8]) -> Result<RelayOutcome, DecodeError> {
        if input.is_empty() {
            return Ok(RelayOutcome {
                consumed: 0,
                completed: None,
            });
        }
        match self.phase {
            Phase::Header => self.push_header(input),
            Phase::Body { remaining } => Ok(self.push_body(input, remaining)),
        }
    }

    fn push_header(&mut self, input: &[u8]) -> Result<RelayOutcome, DecodeError> {
        let need = 1 + LEN_PREFIX - self.header_buf.len();
        let take = need.min(input.len());
        self.header_buf.extend_from_slice(&input[..take]);

        // Still short of a header. The bytes are consumed and must be
        // forwarded even though nothing is known about them yet.
        if self.header_buf.len() < 1 + LEN_PREFIX {
            return Ok(RelayOutcome {
                consumed: take,
                completed: None,
            });
        }

        let Some(header) = decode_header(&self.header_buf, self.max_frame)? else {
            return Ok(RelayOutcome {
                consumed: take,
                completed: None,
            });
        };

        self.header = Some(header);
        self.header_buf.clear();
        self.buffer.clear();

        self.want_inspect = match inspect_policy(self.direction, header.tag) {
            Inspect::None => 0,
            Inspect::Prefix(n) => n.min(header.body_len),
            Inspect::Whole => header.body_len,
        };

        // A body-less message is finished the moment its header is.
        if header.body_len == 0 {
            self.phase = Phase::Header;
            return Ok(RelayOutcome {
                consumed: take,
                completed: Some(Completed {
                    header,
                    complete: true,
                }),
            });
        }

        self.phase = Phase::Body {
            remaining: header.body_len,
        };
        Ok(RelayOutcome {
            consumed: take,
            completed: None,
        })
    }

    fn push_body(&mut self, input: &[u8], remaining: usize) -> RelayOutcome {
        let take = remaining.min(input.len());

        // Copy only what the policy asked for. This is the line that keeps a
        // gigabyte body from becoming a gigabyte of memory.
        if self.want_inspect > 0 {
            let copy = self.want_inspect.min(take);
            self.buffer.extend_from_slice(&input[..copy]);
            self.want_inspect -= copy;
        }

        let left = remaining - take;
        if left > 0 {
            self.phase = Phase::Body { remaining: left };
            return RelayOutcome {
                consumed: take,
                completed: None,
            };
        }

        self.phase = Phase::Header;
        let header = self.header.unwrap_or(FrameHeader {
            tag: crate::frame::Tag(0),
            body_len: 0,
        });
        RelayOutcome {
            consumed: take,
            completed: Some(Completed {
                header,
                complete: self.buffer.len() == header.body_len,
            }),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::frame::{DEFAULT_MAX_INSPECT, Tag};

    /// Builds a tagged message on the wire.
    fn wire(tag: Tag, body: &[u8]) -> Vec<u8> {
        let mut out = vec![tag.get()];
        out.extend_from_slice(
            &u32::try_from(body.len() + LEN_PREFIX)
                .unwrap()
                .to_be_bytes(),
        );
        out.extend_from_slice(body);
        out
    }

    /// Drives a relay over `bytes` in chunks of `chunk`, returning the message
    /// boundaries crossed and the total forwarded.
    ///
    /// Every consumed byte is counted as forwarded, which is what a real relay
    /// does: `consumed` means "these belong to the stream", not "these are
    /// interesting".
    fn drive(relay: &mut FrameRelay, bytes: &[u8], chunk: usize) -> (Vec<Completed>, usize) {
        let mut done = Vec::new();
        let mut forwarded = 0;
        let mut offset = 0;

        while offset < bytes.len() {
            let end = (offset + chunk).min(bytes.len());
            let mut window = &bytes[offset..end];

            while !window.is_empty() {
                let outcome = relay.push(window).unwrap();
                if outcome.consumed == 0 {
                    break;
                }
                forwarded += outcome.consumed;
                if let Some(completed) = outcome.completed {
                    done.push(completed);
                }
                window = &window[outcome.consumed..];
            }
            offset = end;
        }
        (done, forwarded)
    }

    #[test]
    fn a_large_body_relays_with_bounded_memory() {
        // The property the whole revision exists for. Ten megabytes through a
        // relay that never holds more than the inspect cap.
        let body = vec![b'x'; 10 * 1024 * 1024];
        let bytes = wire(Tag::DATA_ROW, &body);

        let mut relay = FrameRelay::new(Direction::Backend);
        let mut peak = 0;
        let mut offset = 0;

        while offset < bytes.len() {
            let end = (offset + 8192).min(bytes.len());
            let mut window = &bytes[offset..end];
            while !window.is_empty() {
                let outcome = relay.push(window).unwrap();
                peak = peak.max(relay.buffered());
                if outcome.consumed == 0 {
                    break;
                }
                window = &window[outcome.consumed..];
            }
            offset = end;
        }

        assert!(
            peak <= 1 + LEN_PREFIX,
            "held {peak} bytes relaying a 10 MiB row; decode would have held all of it"
        );
    }

    #[test]
    fn bytes_are_forwarded_before_the_body_ends() {
        // Streaming, not buffering: the first Forward must arrive long before
        // the last byte does.
        let body = vec![b'x'; 1024 * 1024];
        let bytes = wire(Tag::DATA_ROW, &body);

        let mut relay = FrameRelay::new(Direction::Backend);
        let first = relay.push(&bytes[..64]).unwrap();
        assert!(
            first.consumed > 0 && first.completed.is_none(),
            "nothing forwarded from the first 64 bytes of a 1 MiB row: {first:?}"
        );
    }

    #[test]
    fn what_is_buffered_is_counted_exactly() {
        // `buffered` is what an operator reads to know a session is not holding
        // a gigabyte, and `M10.7` found that returning 0, returning 1, or
        // subtracting the two halves instead of adding them survived every test
        // here. A number nobody asserts is a number that can be anything.
        let mut relay = FrameRelay::new(Direction::Backend);
        assert_eq!(relay.buffered(), 0);

        // Three bytes of a header: short of the five a header needs, so they
        // sit in the header buffer and nothing is in the body buffer.
        let bytes = wire(Tag::READY_FOR_QUERY, b"I");
        relay.push(&bytes[..3]).unwrap();
        assert_eq!(relay.buffered(), 3);

        // And back to nothing once the message is through, because what was
        // held has been handed over.
        relay.push(&bytes[3..]).unwrap();
        assert_eq!(relay.buffered(), 0);
    }

    #[test]
    fn four_bytes_are_not_yet_a_header() {
        // A header is a tag and a four-byte length. `M10.7` found that
        // `1 + LEN_PREFIX` could become `1 * LEN_PREFIX`, which is four, and
        // nothing noticed: a four-byte prefix would then be read as a complete
        // header and its length taken from bytes that are not all there.
        let mut relay = FrameRelay::new(Direction::Backend);
        let bytes = wire(Tag::READY_FOR_QUERY, b"I");

        let outcome = relay.push(&bytes[..4]).unwrap();
        assert_eq!(outcome.consumed, 4);
        assert!(
            outcome.completed.is_none(),
            "four bytes were treated as a header: {outcome:?}"
        );
        assert_eq!(relay.buffered(), 4);
    }

    #[test]
    fn a_whole_inspected_message_yields_its_body() {
        let mut relay = FrameRelay::new(Direction::Backend);
        let bytes = wire(Tag::READY_FOR_QUERY, b"I");

        let (done, forwarded) = drive(&mut relay, &bytes, bytes.len());
        assert_eq!(forwarded, bytes.len(), "not every byte was accounted for");
        assert_eq!(done.len(), 1);
        assert!(done[0].complete);
        assert_eq!(relay.inspected(), b"I");
    }

    #[test]
    fn a_prefix_inspected_message_yields_only_its_prefix() {
        // Bind's names are at the front; its parameter values can be enormous.
        let Inspect::Prefix(limit) = inspect_policy(Direction::Frontend, Tag::BIND) else {
            unreachable!("Bind should be prefix-inspected")
        };

        let mut body = b"portal\0statement\0".to_vec();
        body.extend_from_slice(&vec![b'v'; limit * 4]);
        let bytes = wire(Tag::BIND, &body);

        let mut relay = FrameRelay::new(Direction::Frontend);
        let (done, forwarded) = drive(&mut relay, &bytes, 4096);

        assert_eq!(forwarded, bytes.len());
        assert_eq!(relay.inspected().len(), limit, "buffered past the prefix");
        assert!(
            relay.inspected().starts_with(b"portal\0statement\0"),
            "the names were not captured"
        );
        assert_eq!(done.len(), 1);
        assert!(!done[0].complete, "a truncated inspection must say so");
    }

    #[test]
    fn an_uninspected_message_buffers_nothing() {
        let body = vec![b'x'; 100_000];
        let bytes = wire(Tag::DATA_ROW, &body);

        let mut relay = FrameRelay::new(Direction::Backend);
        drive(&mut relay, &bytes, 4096);
        assert!(relay.inspected().is_empty(), "a DataRow was copied");
    }

    #[test]
    fn a_body_less_message_completes_from_its_header() {
        for tag in [Tag::SYNC, Tag::TERMINATE, Tag::FLUSH] {
            let mut relay = FrameRelay::new(Direction::Frontend);
            let bytes = wire(tag, b"");
            let (done, forwarded) = drive(&mut relay, &bytes, bytes.len());

            assert_eq!(forwarded, bytes.len(), "{tag} lost bytes");
            assert_eq!(done.len(), 1, "{tag} did not complete exactly once");
            assert!(done[0].complete);
        }
    }

    #[test]
    fn a_message_split_at_every_boundary_relays_identically() {
        // TCP chunks arbitrarily. The relay must be a function of how many
        // bytes arrived, never of how they were split.
        let bytes = wire(Tag::READY_FOR_QUERY, b"T");

        for chunk in 1..=bytes.len() {
            let mut relay = FrameRelay::new(Direction::Backend);
            let (done, forwarded) = drive(&mut relay, &bytes, chunk);

            assert_eq!(forwarded, bytes.len(), "chunk {chunk} lost bytes");
            assert_eq!(relay.inspected(), b"T", "chunk {chunk} lost the status");
            assert_eq!(done.len(), 1, "chunk {chunk} did not complete once");
            assert!(done[0].complete, "chunk {chunk} reported truncation");
        }
    }

    #[test]
    fn several_messages_in_one_chunk_relay_in_order() {
        // Pipelined messages arrive together.
        let mut bytes = wire(Tag::PARSE_COMPLETE, b"");
        bytes.extend_from_slice(&wire(Tag::READY_FOR_QUERY, b"I"));
        bytes.extend_from_slice(&wire(Tag::DATA_ROW, b"row-bytes"));

        let mut relay = FrameRelay::new(Direction::Backend);
        let (done, forwarded) = drive(&mut relay, &bytes, bytes.len());

        assert_eq!(forwarded, bytes.len());
        let tags: Vec<Tag> = done.iter().map(|c| c.header.tag).collect();
        assert_eq!(
            tags,
            vec![Tag::PARSE_COMPLETE, Tag::READY_FOR_QUERY, Tag::DATA_ROW]
        );
    }

    #[test]
    fn the_relay_reports_the_header_before_the_body_arrives() {
        let bytes = wire(Tag::DATA_ROW, &vec![b'x'; 5000]);
        let mut relay = FrameRelay::new(Direction::Backend);

        assert_eq!(relay.header(), None);
        relay.push(&bytes[..5]).unwrap();

        let header = relay.header().unwrap();
        assert_eq!(header.tag, Tag::DATA_ROW);
        assert_eq!(header.body_len, 5000);
    }

    #[test]
    fn an_oversized_declared_length_is_refused() {
        let mut bytes = vec![Tag::DATA_ROW.get()];
        bytes.extend_from_slice(&u32::MAX.to_be_bytes());

        let mut relay = FrameRelay::with_max_frame(Direction::Backend, 1024);
        assert!(relay.push(&bytes).is_err());
    }

    #[test]
    fn a_hundred_megabyte_row_is_permitted_by_the_default_cap() {
        // The bug this revision fixes: the old 64 MiB cap refused this.
        let mut bytes = vec![Tag::DATA_ROW.get()];
        bytes.extend_from_slice(&(100 * 1024 * 1024_u32 + 4).to_be_bytes());

        let mut relay = FrameRelay::new(Direction::Backend);
        assert!(
            relay.push(&bytes).is_ok(),
            "a 100 MB result row was refused"
        );
        assert_eq!(relay.header().unwrap().body_len, 100 * 1024 * 1024);
    }

    #[test]
    fn inspection_never_exceeds_the_configured_cap() {
        // Whatever the policy says, buffering stays within the documented
        // bound, because that bound times 100k connections is the memory story.
        for (direction, tag) in [
            (Direction::Frontend, Tag::QUERY),
            (Direction::Frontend, Tag::PARSE),
            (Direction::Frontend, Tag::BIND),
            (Direction::Backend, Tag::ERROR_RESPONSE),
            (Direction::Backend, Tag::NOTIFICATION_RESPONSE),
        ] {
            let body = vec![b'z'; 512 * 1024];
            let bytes = wire(tag, &body);
            let mut relay = FrameRelay::new(direction);
            drive(&mut relay, &bytes, 8192);

            assert!(
                relay.inspected().len() <= DEFAULT_MAX_INSPECT,
                "{tag} buffered {} bytes",
                relay.inspected().len()
            );
        }
    }

    #[test]
    fn an_empty_push_consumes_nothing_rather_than_looping() {
        let mut relay = FrameRelay::new(Direction::Backend);
        assert_eq!(relay.push(&[]).unwrap().consumed, 0);

        let bytes = wire(Tag::DATA_ROW, b"body");
        relay.push(&bytes[..5]).unwrap();
        assert_eq!(relay.push(&[]).unwrap().consumed, 0);
    }

    #[test]
    fn a_relay_is_reusable_across_messages() {
        // State from one message must not leak into the next.
        let mut relay = FrameRelay::new(Direction::Backend);

        let first = wire(Tag::READY_FOR_QUERY, b"T");
        drive(&mut relay, &first, first.len());
        assert_eq!(relay.inspected(), b"T");

        let second = wire(Tag::DATA_ROW, b"opaque");
        drive(&mut relay, &second, second.len());
        assert!(
            relay.inspected().is_empty(),
            "the previous message's inspection survived"
        );
    }

    #[test]
    fn relaying_never_panics_on_arbitrary_input() {
        let mut seed = 0x1357_9BDF_2468_ACE0_u64;
        for _ in 0..20_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let len = usize::try_from(seed % 40).unwrap();
            let bytes: Vec<u8> = (0..len)
                .map(|i| u8::try_from((seed >> (i % 8 * 8)) & 0xFF).unwrap())
                .collect();

            for direction in [Direction::Frontend, Direction::Backend] {
                let mut relay = FrameRelay::new(direction);
                let mut window = bytes.as_slice();
                for _ in 0..8 {
                    match relay.push(window) {
                        Err(_) => break,
                        Ok(outcome) => {
                            if outcome.consumed == 0 || outcome.consumed > window.len() {
                                break;
                            }
                            window = &window[outcome.consumed..];
                        }
                    }
                }
            }
        }
    }
}
