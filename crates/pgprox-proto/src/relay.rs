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
    DEFAULT_MAX_FRAME, DEFAULT_MAX_INSPECT, DecodeError, Direction, FrameHeader, Inspect,
    LEN_PREFIX, decode_header, inspect_policy,
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

/// How many bytes of a message's body must be buffered to read it.
///
/// The rule, in one place, because two consumers apply it: [`FrameRelay`] here,
/// and the proxy's own relay loop, which does its framing against a socket and
/// cannot use a push-based state machine without copying bytes it currently
/// hands out as borrowed slices.
///
/// `M16.10` is why this exists. The two had the rule written out separately,
/// which is one edit away from a proxy that buffers more than the component
/// that documents the bound. Neither implementation was wrong; having two was.
///
/// `max_inspect` is a parameter rather than the constant because a relay may be
/// built with a tighter one, and because a caller that passed the wrong thing
/// should have to say so.
#[must_use]
pub fn inspect_budget(
    direction: Direction,
    tag: crate::frame::Tag,
    body_len: usize,
    max_inspect: usize,
) -> usize {
    match inspect_policy(direction, tag) {
        Inspect::None => 0,
        Inspect::Prefix(n) => n.min(body_len),
        Inspect::Whole => body_len,
    }
    .min(max_inspect)
}

/// Inspection capacity a relay keeps between messages.
///
/// Capping the peak is only half of bounding memory. `Vec::clear` keeps its
/// allocation, so without this a connection that inspected one large message
/// holds that capacity for the rest of its life, and the cost of making it do
/// so is a single frame. The cap alone would turn "a gigabyte per connection,
/// while the message is in flight" into "a megabyte per connection, for good",
/// which at 100k connections is the same problem in a smaller font.
///
/// Above this, the buffer is released once the message that needed it is over.
/// Below it, nothing happens: shrinking on every message would trade a bounded
/// amount of memory for an allocation on the hot path, and
/// `tests/budgets.rs` is what says that path allocates nothing.
///
/// 8 KiB because it is the largest prefix any frequently inspected message
/// asks for, `ErrorResponse`. `ReadyForQuery` is one byte and
/// `CommandComplete` a few dozen, so the steady state sits far below it.
const RETAINED_INSPECT: usize = 8 * 1024;

/// Retaining as much as the cap allows would retain everything and mean
/// nothing. Checked at compile time, the way `frame.rs` checks the pair above
/// it, so the relationship cannot be broken by a value nobody re-read.
const _: () = assert!(RETAINED_INSPECT < DEFAULT_MAX_INSPECT);

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
    /// The ceiling on what one message may put in `buffer`.
    ///
    /// Distinct from `max_frame`, and the distinction is the reason both
    /// exist: relayed bytes are never held, so their limit can be generous,
    /// while inspected bytes are held per connection and are multiplied by the
    /// connection count. `Inspect::Whole` asks for the whole body, and a body
    /// length is a number the peer chose.
    max_inspect: usize,
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
        Self::with_limits(direction, max_frame, DEFAULT_MAX_INSPECT)
    }

    /// A relay with both caps set explicitly.
    ///
    /// `max_frame` bounds what may pass through; `max_inspect` bounds what may
    /// be held in order to read it. Nothing requires the second to be smaller,
    /// but a `max_inspect` above `max_frame` cannot bind, since no message that
    /// large would be relayed in the first place.
    #[must_use]
    pub fn with_limits(direction: Direction, max_frame: usize, max_inspect: usize) -> Self {
        Self {
            direction,
            max_frame,
            max_inspect,
            phase: Phase::Header,
            header: None,
            want_inspect: 0,
            buffer: Vec::new(),
            header_buf: Vec::new(),
        }
    }

    /// The ceiling on bytes held for inspection.
    #[must_use]
    pub const fn max_inspect(&self) -> usize {
        self.max_inspect
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
        const HEADER: usize = 1 + LEN_PREFIX;

        // A header is five bytes and a read is thousands, so the overwhelmingly
        // common case is that a whole header is already contiguous in the
        // caller's slice and there is nothing to reassemble. `header_buf` is
        // for the boundary case, and it was being paid for on every message.
        let take = if self.header_buf.is_empty() && input.len() >= HEADER {
            HEADER
        } else {
            let need = HEADER - self.header_buf.len();
            let take = need.min(input.len());
            self.header_buf.extend_from_slice(&input[..take]);

            // Still short of a header. The bytes are consumed and must be
            // forwarded even though nothing is known about them yet.
            if self.header_buf.len() < HEADER {
                return Ok(RelayOutcome {
                    consumed: take,
                    completed: None,
                });
            }
            take
        };

        // Whichever branch ran, a full header is now in exactly one of two
        // places, and which one is decidable: the fast path leaves `header_buf`
        // untouched and therefore empty, and the slow path only reaches here
        // having filled it.
        let source: &[u8] = if self.header_buf.is_empty() {
            input
        } else {
            &self.header_buf
        };

        let Some(header) = decode_header(source, self.max_frame)? else {
            return Ok(RelayOutcome {
                consumed: take,
                completed: None,
            });
        };

        self.header = Some(header);
        self.header_buf.clear();

        // The cap applies to every policy, not just `Whole`. A prefix is a
        // constant chosen here and is already small, but `Whole` means "as much
        // as the body claims to be", and the body length is the peer's number.
        // Without this line a `Sync` declaring a gigabyte is a gigabyte held,
        // and `Sync` is one of four frontend tags a client can send before it
        // has authenticated.
        self.want_inspect = inspect_budget(
            self.direction,
            header.tag,
            header.body_len,
            self.max_inspect,
        );

        // Cleared here rather than at the end of the previous message, because
        // `inspected()` is documented as valid until the next push and a caller
        // reads it after the completion that produced it.
        self.buffer.clear();
        if self.buffer.capacity() > RETAINED_INSPECT.max(self.want_inspect) {
            self.buffer
                .shrink_to(RETAINED_INSPECT.max(self.want_inspect));
        }

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
    fn a_contiguous_header_is_read_where_it_lies() {
        // The property the fast path is. `header_buf` exists for a header split
        // across two reads, and a read is thousands of bytes while a header is
        // five, so the split is the rare case and it was being paid for on
        // every message. Asserted rather than described, because nothing else
        // here can tell the two paths apart: they produce identical output.
        let mut relay = FrameRelay::new(Direction::Backend);
        let bytes = wire(Tag::READY_FOR_QUERY, b"I");

        relay.push(&bytes).unwrap();
        assert!(
            relay.header_buf.capacity() == 0,
            "a contiguous header was copied into the reassembly buffer"
        );

        // And the buffer is still there for the case it exists for. Driven in
        // two-byte chunks so the header genuinely straddles a read, and through
        // `drive` because one push consumes at most one message's worth of
        // header or body, which is the contract `push` documents.
        let mut split = FrameRelay::new(Direction::Backend);
        split.push(&bytes[..2]).unwrap();
        assert_eq!(split.header_buf.len(), 2, "the split path was not taken");

        let mut split = FrameRelay::new(Direction::Backend);
        let (done, forwarded) = drive(&mut split, &bytes, 2);
        assert_eq!(forwarded, bytes.len());
        assert_eq!(done.len(), 1);
        assert_eq!(split.inspected(), b"I");
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
    fn the_budget_is_the_policy_capped_by_the_ceiling() {
        // The rule both consumers apply, stated once. `M16.10` found it written
        // out twice, here and in the proxy's own relay loop, which is one edit
        // away from a proxy that buffers more than the component documenting
        // the bound.
        let big = 64 * 1024 * 1024;

        // Uninspected reads nothing at all, whatever the body claims.
        assert_eq!(
            inspect_budget(Direction::Backend, Tag::DATA_ROW, big, DEFAULT_MAX_INSPECT),
            0
        );
        // A prefix is the smaller of the policy's number and the body's.
        assert_eq!(
            inspect_budget(
                Direction::Backend,
                Tag::ERROR_RESPONSE,
                big,
                DEFAULT_MAX_INSPECT
            ),
            8192
        );
        assert_eq!(
            inspect_budget(
                Direction::Backend,
                Tag::ERROR_RESPONSE,
                40,
                DEFAULT_MAX_INSPECT
            ),
            40
        );
        // Whole means the body, and the body is a number the peer chose, so the
        // ceiling is what stops it.
        assert_eq!(
            inspect_budget(Direction::Frontend, Tag::SYNC, big, DEFAULT_MAX_INSPECT),
            DEFAULT_MAX_INSPECT
        );
        assert_eq!(
            inspect_budget(
                Direction::Backend,
                Tag::READY_FOR_QUERY,
                1,
                DEFAULT_MAX_INSPECT
            ),
            1
        );
        // A tighter ceiling binds where the policy would not.
        assert_eq!(
            inspect_budget(Direction::Backend, Tag::ERROR_RESPONSE, big, 64),
            64
        );
    }

    #[test]
    fn the_relay_budgets_itself_with_the_shared_rule() {
        // The half that says the extraction actually took: what the relay holds
        // is what the rule says, not merely something under the cap. Asserted
        // through the relay rather than by reading it, so a relay that stopped
        // calling the rule would fail here.
        let body = vec![b'v'; 40 * 1024];
        let bytes = wire(Tag::ERROR_RESPONSE, &body);

        let mut relay = FrameRelay::new(Direction::Backend);
        drive(&mut relay, &bytes, 4096);

        assert_eq!(
            relay.inspected().len(),
            inspect_budget(
                Direction::Backend,
                Tag::ERROR_RESPONSE,
                body.len(),
                DEFAULT_MAX_INSPECT
            ),
            "the relay and the rule disagree about how much to hold"
        );
    }

    #[test]
    fn a_whole_inspected_message_cannot_buffer_past_the_inspect_cap() {
        // The bug this task exists for. `Sync` is `Inspect::Whole` and its body
        // must be empty, but the relay takes the declared length on trust. Four
        // frontend tags are `Whole`, so this needs no authentication and no
        // cooperating server: it is one frame from anyone who can reach the
        // listener.
        //
        // Eight megabytes here to keep the test quick. The declared length is a
        // u32 checked only against `max_frame`, so the same frame claiming a
        // gigabyte held a gigabyte.
        let body_len = 8 * 1024 * 1024;
        let mut bytes = vec![Tag::SYNC.get()];
        bytes.extend_from_slice(&u32::try_from(body_len + LEN_PREFIX).unwrap().to_be_bytes());
        bytes.extend_from_slice(&vec![b'x'; body_len]);

        let mut relay = FrameRelay::new(Direction::Frontend);
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
            peak <= DEFAULT_MAX_INSPECT,
            "a client made the relay hold {peak} bytes against a {DEFAULT_MAX_INSPECT} cap"
        );
    }

    #[test]
    fn the_buffer_a_large_message_needed_does_not_outlive_it() {
        // The other half, and the half that makes the cap worth having.
        // `Vec::clear` keeps its allocation, so a cap on the peak alone leaves
        // the capacity in place for the life of the connection, and one frame
        // per connection is all it costs an attacker to put it there.
        let body_len = 512 * 1024;
        let mut bytes = vec![Tag::SYNC.get()];
        bytes.extend_from_slice(&u32::try_from(body_len + LEN_PREFIX).unwrap().to_be_bytes());
        bytes.extend_from_slice(&vec![b'x'; body_len]);

        let mut relay = FrameRelay::new(Direction::Frontend);
        drive(&mut relay, &bytes, 8192);
        assert!(
            relay.inspected().len() > RETAINED_INSPECT,
            "the setup did not put a large buffer in place"
        );

        // Any following message releases it. A `Terminate` is the smallest
        // thing a client sends, and the one it would send next.
        let small = wire(Tag::TERMINATE, b"");
        drive(&mut relay, &small, small.len());

        assert!(
            relay.buffer.capacity() <= RETAINED_INSPECT,
            "the relay kept {} bytes for a session that is not using them",
            relay.buffer.capacity()
        );
    }

    #[test]
    fn the_retained_size_is_the_size_it_says() {
        // The assertion above compares against `RETAINED_INSPECT`, which is the
        // constant that produced the number, so it holds for any value of it. A
        // mutation run made the point by turning `8 * 1024` into `8 + 1024` and
        // watching the whole file stay green.
        //
        // That is the shape `M14` catalogued and this milestone quoted before
        // committing an instance of it. So the number is pinned, and it is
        // pinned against the thing that chose it rather than against itself:
        // the retained buffer has to cover the largest prefix a frequently
        // inspected message asks for, or a session seeing ordinary errors would
        // give the capacity back and take it again on every one.
        assert_eq!(RETAINED_INSPECT, 8192);

        let Inspect::Prefix(error_prefix) = inspect_policy(Direction::Backend, Tag::ERROR_RESPONSE)
        else {
            unreachable!("ErrorResponse should be prefix-inspected")
        };
        assert!(
            RETAINED_INSPECT >= error_prefix,
            "retaining {RETAINED_INSPECT} would churn on every {error_prefix}-byte error"
        );

        // The other end of the range, that a retention as large as the cap
        // would retain everything, is a `const` assertion beside the constant
        // rather than a test: it cannot compile rather than cannot pass.
    }

    #[test]
    fn an_ordinary_session_never_shrinks_its_buffer() {
        // The cost side. Shrinking on every message would trade bounded memory
        // for an allocation per frame on the hot path, which is what
        // `tests/budgets.rs` exists to refuse. Everything a busy session
        // inspects sits under the retention bound, so nothing here reallocates.
        let mut relay = FrameRelay::new(Direction::Backend);
        let ready = wire(Tag::READY_FOR_QUERY, b"I");
        let complete = wire(Tag::COMMAND_COMPLETE, b"SELECT 1\0");

        drive(&mut relay, &ready, ready.len());
        let settled = relay.buffer.capacity();

        for _ in 0..64 {
            drive(&mut relay, &complete, complete.len());
            drive(&mut relay, &ready, ready.len());
        }
        assert!(
            relay.buffer.capacity() >= settled,
            "an ordinary session gave capacity back and had to take it again"
        );
    }

    #[test]
    fn a_capped_inspection_reports_itself_truncated() {
        // The signal a parser needs. What it has is the front of the body, not
        // a short message, and treating one as the other is how a truncated
        // `ParameterStatus` becomes a parameter nobody set.
        let body = vec![b'v'; 4096];
        let bytes = wire(Tag::PARAMETER_STATUS, &body);

        let mut relay = FrameRelay::with_limits(Direction::Backend, DEFAULT_MAX_FRAME, 64);
        let (done, forwarded) = drive(&mut relay, &bytes, 512);

        assert_eq!(forwarded, bytes.len(), "capping inspection dropped bytes");
        assert_eq!(relay.inspected().len(), 64);
        assert_eq!(done.len(), 1);
        assert!(!done[0].complete, "a capped inspection claimed to be whole");
    }

    #[test]
    fn a_whole_message_under_the_cap_is_still_read_whole() {
        // The cap must not cost anything in the case it exists for. Everything
        // the policy marks `Whole` is small by construction, so the ordinary
        // path has to be unaffected.
        let mut relay = FrameRelay::new(Direction::Backend);
        assert_eq!(relay.max_inspect(), DEFAULT_MAX_INSPECT);

        let bytes = wire(Tag::READY_FOR_QUERY, b"I");
        let (done, _) = drive(&mut relay, &bytes, bytes.len());
        assert_eq!(relay.inspected(), b"I");
        assert!(done[0].complete);
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

    #[test]
    fn push_has_a_real_fuzz_target() {
        // `M88.17`. `relaying_never_panics_on_arbitrary_input` above is a
        // fixed-seed PRNG loop: the same 20,000 inputs every run, no coverage
        // guidance, no corpus, no crash minimization. `FrameRelay::push`
        // reassembles a partially-read frame from an untrusted peer, exactly
        // what `M15` fuzzed elsewhere in this crate with `cargo-fuzz` for the
        // same reason, and had none of that. `include_str!` on the target
        // itself makes its absence a compile failure, not just a missing
        // row — which is what "before this fix" actually was.
        const _TARGET: &str = include_str!("../../../fuzz/fuzz_targets/frame_relay.rs");
        const FUZZ_CARGO_TOML: &str = include_str!("../../../fuzz/Cargo.toml");
        const FUZZ_SH: &str = include_str!("../../../scripts/fuzz.sh");

        assert!(
            FUZZ_CARGO_TOML.contains("name = \"frame_relay\""),
            "fuzz/Cargo.toml does not register a frame_relay fuzz target"
        );
        assert!(
            FUZZ_SH.contains("frame_relay"),
            "scripts/fuzz.sh does not run the frame_relay target"
        );
    }
}
