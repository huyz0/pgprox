//! Holding back an extended-query sequence so the cache can answer it.
//!
//! # Why anything is held back at all
//!
//! A `Parse` or a `Bind` is forwarded as it arrives, and forwarding acquires a
//! pooled connection. So by the time an `Execute` is decoded the session is
//! already holding one, and a cache hit there saves the database's execution
//! and none of the proxy's work. `M7.56` measured 45% of this proxy's CPU in
//! the pool's lock, which is what the cache was moved up the roadmap to avoid.
//!
//! Nothing about the protocol requires those frames to go out when they arrive.
//! A frontend must send a `Sync` or a `Flush` before it examines the results of
//! an extended-query command, so a proxy may hold a sequence until the client
//! asks for an answer. This is the machine that holds one.
//!
//! # What it does not decide
//!
//! Whether the statement may be cached at all, and whether the session is in a
//! state a sequence may be withheld from. Both arrive as [`Facts`], the way
//! `pgprox_cache::SessionFacts` arrives at the cacheability rule, because this
//! crate may not depend on `pgprox-cache` and because the caller is the one
//! holding the pool, the transaction status and the tenant's configuration.
//!
//! # Safe way round
//!
//! The machine lists what it can hold and everything else is
//! [`Held::Send`]. A message nobody has thought about goes upstream rather than
//! being swallowed, which is the direction that costs a miss instead of a
//! desynchronised protocol.

use std::sync::Arc;

use pgprox_proto::frame::{Frame, Tag};
use pgprox_proto::frontend::{FrontendMessage, Target};

/// The most a session will hold on the client's behalf.
///
/// A sequence larger than this goes upstream. The bytes were the client's own
/// so holding them is not an amplification, but a session is a per-connection
/// cost in a process designed around 100k of them, and a bound nobody can
/// exceed is worth more here than the hit rate it costs. A `Bind` carrying more
/// than this is a statement whose answer the store would refuse anyway.
pub const MAX_HELD_BYTES: usize = 64 * 1024;

/// What the shell should do with the frame it just fed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Held {
    /// Kept. Nothing goes upstream, and the client is owed nothing yet.
    Withheld,
    /// The sequence is complete and can be looked up.
    ///
    /// The frames are still held: a miss has to replay them, and a hit has to
    /// know what the client sent to answer it.
    Complete,
    /// Send whatever is held, in the order it arrived, then this frame as
    /// usual.
    ///
    /// Returned both when a sequence gives up and when there was never one, so
    /// a caller has one path: drain [`HeldSequence::replay`], which yields
    /// nothing when nothing is held.
    Send,
}

/// What the caller knows about the frame it is feeding in.
///
/// Both fields are the caller's judgement rather than something readable off
/// the wire, which is why they are passed rather than reached for.
#[derive(Clone, Copy, Debug, Default)]
pub struct Facts<'a> {
    /// The SQL of the statement this frame concerns, where the caller knows it.
    ///
    /// A `Parse` carries its own and a `Bind` names a statement the session
    /// prepared earlier, and the caller has both. It is passed rather than read
    /// out of the message so the machine keys on exactly the text the caller
    /// checked for cacheability: two different answers to "which statement is
    /// this" is how a cache serves one question's rows for another.
    pub sql: Option<&'a str>,
    /// Whether a sequence may begin at this frame.
    ///
    /// The caller's whole entry condition: an opted-in tenant, a statement the
    /// cacheability rule accepts, no connection held and no transaction open.
    /// Consulted only when nothing is held yet, because a sequence that has
    /// already begun cannot be re-judged: some of it may be upstream.
    pub may_begin: bool,
}

/// An extended-query sequence held back from the upstream.
///
/// Empty until something is withheld, and empty again once the caller has dealt
/// with a [`Held::Complete`] or a [`Held::Send`]. Reset rather than dropped, so
/// a session that uses this keeps one buffer rather than allocating per
/// statement.
#[derive(Debug, Default)]
pub struct HeldSequence {
    /// The frames as they will go upstream: tag, length, body.
    ///
    /// After the statement-name rewrite rather than before it, because a replay
    /// has to send what would have been sent.
    frames: Vec<u8>,
    /// The statement's SQL, from whichever frame told the caller about it.
    sql: Option<Arc<str>>,
    /// The parameter run out of the `Bind`, as the wire carried it.
    params: Arc<[u8]>,
    /// The portal the `Bind` named, which the `Execute` has to agree with.
    portal: String,
    /// How far through the one shape this can answer the sequence has got.
    step: Step,
    /// Whether the client asked for a `RowDescription`.
    ///
    /// Apart from [`Step`] because it is the one thing the caller still needs to
    /// know after the `Execute` has moved the step past it.
    described: bool,
}

/// How far a held sequence has got.
///
/// Ordered, and the order is the shape: a `Parse`, then a `Bind`, then at most
/// one `Describe` of the portal it bound, then the `Execute`. Anything arriving
/// out of that order is a sequence this cannot reason about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Step {
    #[default]
    Nothing,
    Parsed,
    Bound,
    Executed,
}

impl HeldSequence {
    /// A machine holding nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether anything is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// The statement the sequence runs, once a `Bind` has named one.
    #[must_use]
    pub fn sql(&self) -> Option<&str> {
        self.sql.as_deref()
    }

    /// The parameter run to key on, empty when nothing was bound.
    #[must_use]
    pub fn params(&self) -> &Arc<[u8]> {
        &self.params
    }

    /// Whether the client asked for a `RowDescription`.
    ///
    /// What it is owed on a hit, and the reason the stored payload keeps a
    /// description apart from the rows: a client that sent no `Describe` must
    /// not be handed one.
    #[must_use]
    pub const fn wants_row_description(&self) -> bool {
        self.described
    }

    /// The held frames, in the order they arrived.
    #[must_use]
    pub fn replay(&self) -> Frames<'_> {
        Frames { rest: &self.frames }
    }

    /// Writes the answer this sequence is owed, out of a stored payload.
    ///
    /// The frames the client is owed rather than the frames the server sent
    /// last time: a `ParseComplete` for its `Parse`, a `BindComplete` for its
    /// `Bind`, the payload's description for its `Describe`, the rows and the
    /// completion for its `Execute`, and a `ReadyForQuery('I')` for the `Sync`
    /// that got here.
    ///
    /// That is what makes one payload serve two drivers whose framing differs.
    /// The proxy already synthesises a `ParseComplete` when a pooled connection
    /// turns out to hold the statement a `Parse` names, which is this move at
    /// one frame rather than five.
    ///
    /// The `'I'` is an assertion rather than something relayed, and it is true
    /// because of where withholding is allowed to start: a session with no
    /// connection held and no transaction open. See ADR 0022.
    ///
    /// # Errors
    ///
    /// [`Unservable`] when the payload cannot answer this sequence, which the
    /// caller treats as a miss. Nothing is written to `out` in that case, so a
    /// refusal cannot leave half an answer in the client's buffer.
    pub fn assemble(&self, payload: &[u8], out: &mut Vec<u8>) -> Result<(), Unservable> {
        if self.step != Step::Executed {
            return Err(Unservable::NothingRan);
        }
        let (description, rows) = split(payload)?;
        if self.described && description.is_none() {
            return Err(Unservable::NoRowDescription);
        }

        for (tag, _) in self.replay() {
            // Frontend tags, so `D` is a `Describe` rather than a `DataRow` and
            // `E` is an `Execute` rather than an `ErrorResponse`. The two
            // directions share bytes and this loop only ever walks one of them.
            match tag {
                Tag::PARSE => out.extend_from_slice(&PARSE_COMPLETE),
                Tag::BIND => out.extend_from_slice(&BIND_COMPLETE),
                Tag::DESCRIBE => out.extend_from_slice(description.unwrap_or_default()),
                Tag::EXECUTE => out.extend_from_slice(rows),
                // Unreachable: nothing else is ever held. Ignored rather than
                // asserted, because a panic here would be on the path of a node
                // serving 100k other connections.
                _ => {}
            }
        }
        pgprox_proto::encode::ready_for_query(out, pgprox_proto::backend::TxStatus::Idle);
        Ok(())
    }

    /// Forgets everything, keeping the buffer.
    ///
    /// Called on every exit, which is what makes forgetting a portal automatic
    /// rather than a rule about names: a held sequence ends at the frame that
    /// ends it and there is only ever one.
    pub fn clear(&mut self) {
        self.frames.clear();
        self.sql = None;
        self.params = Arc::default();
        self.portal.clear();
        self.step = Step::Nothing;
        self.described = false;
    }

    /// Feeds in one client frame.
    ///
    /// `body` is what would go upstream, so the statement name is already this
    /// proxy's own. `tag` is its tag, kept alongside because a replay writes
    /// both.
    pub fn feed(
        &mut self,
        message: &FrontendMessage<'_>,
        tag: Tag,
        body: &[u8],
        facts: Facts<'_>,
    ) -> Held {
        if self.frames.is_empty() && !(facts.may_begin && begins(message)) {
            return Held::Send;
        }
        if !self.frames.is_empty() && matches!(message, FrontendMessage::Sync) {
            // The only frame that is neither held nor sent: its answer is the
            // `ReadyForQuery` a hit generates. A miss sends it after the
            // replay, which is the caller falling through to its usual path.
            return if self.step == Step::Executed {
                Held::Complete
            } else {
                Held::Send
            };
        }
        if !self.may_hold(message, &facts) {
            return Held::Send;
        }
        if self.frames.len() + body.len() + 5 > MAX_HELD_BYTES {
            return Held::Send;
        }

        self.record(message, &facts, body);
        self.frames.push(tag.get());
        let len = u32::try_from(body.len() + 4).unwrap_or(u32::MAX);
        self.frames.extend_from_slice(&len.to_be_bytes());
        self.frames.extend_from_slice(body);
        Held::Withheld
    }

    /// Whether this frame fits the one shape that can be answered locally.
    ///
    /// One `Parse` first, then one `Bind`, then at most one `Describe` of the
    /// portal it bound, then one `Execute` of that same portal with no row
    /// limit. Anything else, in any other order, is a sequence this cannot
    /// reason about.
    fn may_hold(&self, message: &FrontendMessage<'_>, facts: &Facts<'_>) -> bool {
        match message {
            // A `Parse` may only open one. Later in a sequence it is a second
            // statement, which is a pipeline rather than a query.
            FrontendMessage::Parse { .. } => {
                self.frames.is_empty() && self.step == Step::Nothing && facts.sql.is_some()
            }
            // The SQL has to agree with whatever a `Parse` in this sequence
            // said, or the sequence spans two statements: `Parse s1` then
            // `Bind s2` is two questions and one key.
            FrontendMessage::Bind { .. } => {
                matches!(self.step, Step::Nothing | Step::Parsed)
                    && facts.sql.is_some()
                    && self
                        .sql
                        .as_deref()
                        .is_none_or(|held| facts.sql == Some(held))
            }
            // A statement `Describe` answers with a `ParameterDescription`
            // that the stored payload does not carry, so it is not held.
            FrontendMessage::Describe { target, name } => {
                matches!(target, Target::Portal)
                    && self.step == Step::Bound
                    && !self.described
                    && *name == self.portal
            }
            // A row limit suspends the portal instead of completing it, which
            // is not an answer to store or to serve.
            FrontendMessage::Execute { portal, max_rows } => {
                self.step == Step::Bound && *max_rows == 0 && *portal == self.portal
            }
            _ => false,
        }
    }

    /// Remembers what this frame contributes, once it is known to be held.
    fn record(&mut self, message: &FrontendMessage<'_>, facts: &Facts<'_>, body: &[u8]) {
        match message {
            FrontendMessage::Parse { .. } => {
                self.step = Step::Parsed;
                self.sql = facts.sql.map(Arc::from);
            }
            FrontendMessage::Bind { portal, .. } => {
                self.step = Step::Bound;
                self.sql = facts.sql.map(Arc::from);
                self.portal.clear();
                self.portal.push_str(portal);
                // A `Bind` this proxy could not read the parameters of keys on
                // an empty run, which would be the same key as a statement with
                // nothing bound. It cannot happen, because the frame decoded a
                // moment ago and this reads the same bytes, and if it ever does
                // the sequence gives up rather than guessing.
                self.params = pgprox_proto::frontend::bind_parameters(&Frame::new(Tag::BIND, body))
                    .map_or_else(|_| Arc::default(), |read| Arc::from(read.raw()));
            }
            FrontendMessage::Describe { .. } => self.described = true,
            FrontendMessage::Execute { .. } => self.step = Step::Executed,
            _ => {}
        }
    }
}

/// `ParseComplete`, which the client is owed for a `Parse` nothing ran.
const PARSE_COMPLETE: [u8; 5] = [b'1', 0, 0, 0, 4];

/// `BindComplete`, likewise.
const BIND_COMPLETE: [u8; 5] = [b'2', 0, 0, 0, 4];

/// Why a stored payload cannot answer a held sequence.
///
/// Every variant is a miss rather than a failure. The caller replays the
/// sequence upstream and the client is never told any of this happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Unservable {
    /// The client asked for a description the payload does not carry.
    ///
    /// Reachable across an upgrade rather than in steady state: an entry stored
    /// by a node that recorded a different payload shape.
    #[error("the payload carries no row description")]
    NoRowDescription,
    /// The payload is not a description, rows and a completion.
    #[error("the payload is not a stored statement answer")]
    Malformed,
    /// Asked to answer a sequence that has not run a statement.
    #[error("the sequence has not executed anything")]
    NothingRan,
}

/// Whether a backend frame belongs in a stored payload.
///
/// The one list, so the recorder and the assembler cannot disagree about what a
/// payload is. Two lists with overlapping entries drift, and the one nobody
/// remembers to fix is always the second.
///
/// A `ParseComplete`, a `BindComplete` and a `ReadyForQuery` are deliberately
/// out: they answer the client's framing rather than its question, and keeping
/// them is what would make an entry serve only the driver that filled it.
///
/// A `NoticeResponse` is out for a different reason. It is a message to the
/// session that ran the statement, and replaying one to the next session is a
/// warning about something that did not happen to it. Notices are asynchronous,
/// so dropping one desynchronises nothing.
#[must_use]
pub fn belongs_in_payload(tag: Tag) -> bool {
    matches!(
        tag,
        Tag::ROW_DESCRIPTION | Tag::DATA_ROW | Tag::COMMAND_COMPLETE | Tag::EMPTY_QUERY_RESPONSE
    )
}

/// Splits a payload into its description, if any, and everything after it.
///
/// Validated rather than trusted even though this proxy wrote it. The bytes do
/// not outlive the node that stored them, but they do outlive the code that
/// stored them: an entry written by a node running a different version has the
/// same shape as a corrupt one, and both have to be a miss rather than a
/// desynchronised client.
fn split(payload: &[u8]) -> Result<(Option<&[u8]>, &[u8]), Unservable> {
    let mut at = 0;
    let mut description = None;
    let mut rows_from = 0;
    let mut completed = false;

    while at < payload.len() {
        // Anything after the completion, including a second one.
        if completed || payload.len() - at < 5 {
            return Err(Unservable::Malformed);
        }
        let tag = Tag(payload[at]);
        let len = u32::from_be_bytes([
            payload[at + 1],
            payload[at + 2],
            payload[at + 3],
            payload[at + 4],
        ]) as usize;
        let end = at + 5 + len.checked_sub(4).ok_or(Unservable::Malformed)?;
        if end > payload.len() || !belongs_in_payload(tag) {
            return Err(Unservable::Malformed);
        }

        match tag {
            Tag::ROW_DESCRIPTION => {
                if at != 0 {
                    // A description after a row describes nothing the assembler
                    // could put anywhere.
                    return Err(Unservable::Malformed);
                }
                description = Some(&payload[..end]);
                rows_from = end;
            }
            Tag::COMMAND_COMPLETE | Tag::EMPTY_QUERY_RESPONSE => completed = true,
            _ => {}
        }
        at = end;
    }

    if completed {
        Ok((description, &payload[rows_from..]))
    } else {
        // Including an empty payload. A statement's answer ends in a completion,
        // and one that does not is half of one.
        Err(Unservable::Malformed)
    }
}

/// Whether a sequence may begin at this frame.
///
/// A `Bind` as well as a `Parse`, because a driver with a statement cache sends
/// `Bind`, `Execute` and `Sync` and nothing else: the `Parse` happened in a
/// round trip of its own, possibly on another connection entirely.
fn begins(message: &FrontendMessage<'_>) -> bool {
    matches!(
        message,
        FrontendMessage::Parse { .. } | FrontendMessage::Bind { .. }
    )
}

/// The held frames, in arrival order.
///
/// Not `Copy`, deliberately: a copy of a half-consumed walk that kept walking
/// would replay frames the caller had already sent.
#[derive(Clone, Debug)]
pub struct Frames<'a> {
    rest: &'a [u8],
}

impl<'a> Iterator for Frames<'a> {
    type Item = (Tag, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        // Bounds checked even though this walks a buffer this module wrote. A
        // reader that trusts a length is a reader that panics the day something
        // else writes one, and the cost is two comparisons per frame.
        if self.rest.len() < 5 {
            return None;
        }
        let tag = Tag(self.rest[0]);
        let len = u32::from_be_bytes([self.rest[1], self.rest[2], self.rest[3], self.rest[4]]);
        let end = 5 + (len as usize).checked_sub(4)?;
        if self.rest.len() < end {
            return None;
        }
        let body = &self.rest[5..end];
        self.rest = &self.rest[end..];
        Some((tag, body))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use pgprox_proto::encode_frontend;

    /// The frame a client would send, split into what `feed` takes.
    fn framed(encoded: &[u8]) -> (Tag, Vec<u8>) {
        (Tag(encoded[0]), encoded[5..].to_vec())
    }

    fn parse(statement: &str, sql: &str) -> (Tag, Vec<u8>) {
        let mut out = Vec::new();
        encode_frontend::parse(&mut out, statement, sql);
        framed(&out)
    }

    fn bind(portal: &str, statement: &str) -> (Tag, Vec<u8>) {
        let mut out = Vec::new();
        encode_frontend::bind(&mut out, portal, statement);
        framed(&out)
    }

    fn bound(portal: &str, statement: &str, values: &[Option<&[u8]>]) -> (Tag, Vec<u8>) {
        let mut out = Vec::new();
        encode_frontend::bind_with_parameters(&mut out, portal, statement, values);
        framed(&out)
    }

    /// Built by hand: `encode_frontend::execute` sends no row limit, and the
    /// limit is one of the things this machine refuses on.
    fn execute(portal: &str, max_rows: i32) -> (Tag, Vec<u8>) {
        let mut body = portal.as_bytes().to_vec();
        body.push(0);
        body.extend_from_slice(&max_rows.to_be_bytes());
        (Tag::EXECUTE, body)
    }

    /// `S` for a statement and `P` for a portal, per the protocol.
    fn describe(target: u8, name: &str) -> (Tag, Vec<u8>) {
        let mut body = vec![target];
        body.extend_from_slice(name.as_bytes());
        body.push(0);
        (Tag::DESCRIBE, body)
    }

    fn describe_portal(name: &str) -> (Tag, Vec<u8>) {
        describe(b'P', name)
    }

    /// Feeds one frame in, decoding it the way the relay does.
    fn feed(held: &mut HeldSequence, frame: &(Tag, Vec<u8>), facts: Facts<'_>) -> Held {
        let decoded = pgprox_proto::frontend::decode(&Frame::new(frame.0, &frame.1)).unwrap();
        held.feed(&decoded, frame.0, &frame.1, facts)
    }

    /// The facts a caller offers for a statement it is happy to cache.
    fn ok(sql: &str) -> Facts<'_> {
        Facts {
            sql: Some(sql),
            may_begin: true,
        }
    }

    /// The facts for a frame that carries no statement of its own.
    const NONE: Facts<'static> = Facts {
        sql: None,
        may_begin: true,
    };

    #[test]
    fn a_whole_sequence_is_held_and_completes_at_the_sync() {
        let mut held = HeldSequence::new();
        let sql = "SELECT $1";

        assert_eq!(
            feed(&mut held, &parse("s1", sql), ok(sql)),
            Held::Withheld,
            "the parse went upstream"
        );
        assert_eq!(
            feed(&mut held, &bound("", "s1", &[Some(b"7")]), ok(sql)),
            Held::Withheld
        );
        assert_eq!(feed(&mut held, &describe_portal(""), NONE), Held::Withheld);
        assert_eq!(feed(&mut held, &execute("", 0), NONE), Held::Withheld);

        let sync = (Tag::SYNC, Vec::new());
        assert_eq!(feed(&mut held, &sync, NONE), Held::Complete);

        assert_eq!(held.sql(), Some(sql));
        assert_eq!(held.params().as_ref(), &[0, 0, 0, 1, b'7']);
        assert!(held.wants_row_description());

        // Four frames held, and the `Sync` is not one of them: a miss sends it
        // through the caller's usual path after the replay.
        let tags: Vec<Tag> = held.replay().map(|(tag, _)| tag).collect();
        assert_eq!(
            tags,
            vec![Tag::PARSE, Tag::BIND, Tag::DESCRIBE, Tag::EXECUTE]
        );
    }

    #[test]
    fn two_bindings_of_one_statement_are_two_keys() {
        // The acceptance criterion `M9.17` was opened for. Until the parameters
        // reached the key, `SELECT $1` with 1 and with 2 shared an entry.
        let sql = "SELECT $1";
        let mut first = HeldSequence::new();
        feed(&mut first, &bound("", "s1", &[Some(b"1")]), ok(sql));
        let mut second = HeldSequence::new();
        feed(&mut second, &bound("", "s1", &[Some(b"2")]), ok(sql));

        assert_ne!(first.params(), second.params());
    }

    #[test]
    fn a_null_and_an_empty_binding_are_two_keys() {
        let sql = "SELECT $1";
        let mut null = HeldSequence::new();
        feed(&mut null, &bound("", "s1", &[None]), ok(sql));
        let mut empty = HeldSequence::new();
        feed(&mut empty, &bound("", "s1", &[Some(b"")]), ok(sql));

        assert_ne!(null.params(), empty.params());
    }

    #[test]
    fn a_bind_alone_opens_a_sequence() {
        // What every driver with a statement cache sends: the `Parse` was a
        // round trip of its own, possibly against another connection.
        let mut held = HeldSequence::new();
        assert_eq!(
            feed(&mut held, &bind("", "s1"), ok("SELECT 1")),
            Held::Withheld
        );
        assert_eq!(feed(&mut held, &execute("", 0), NONE), Held::Withheld);
        assert_eq!(
            feed(&mut held, &(Tag::SYNC, Vec::new()), NONE),
            Held::Complete
        );
        assert!(
            !held.wants_row_description(),
            "a client that sent no describe is owed one"
        );
    }

    #[test]
    fn nothing_is_held_from_a_session_the_caller_will_not_have_it_from() {
        // The entry condition, which is entirely the caller's: an opted-in
        // tenant, a cacheable statement, no connection held, no transaction
        // open.
        let mut held = HeldSequence::new();
        let facts = Facts {
            sql: Some("SELECT 1"),
            may_begin: false,
        };
        assert_eq!(feed(&mut held, &parse("s1", "SELECT 1"), facts), Held::Send);
        assert!(held.is_empty());
    }

    #[test]
    fn a_frame_with_no_statement_behind_it_opens_nothing() {
        // A `Bind` for something this session never parsed reaches the caller
        // with no SQL to offer, and a sequence keyed on nothing would be keyed
        // on the last statement to pass through.
        let mut held = HeldSequence::new();
        assert_eq!(feed(&mut held, &bind("", "s1"), NONE), Held::Send);
        assert!(held.is_empty());
    }

    #[test]
    fn a_flush_gives_the_sequence_up() {
        // The client is waiting for an answer now, which is the whole reason
        // the message exists. Holding on is the deadlock M8 found.
        let mut held = HeldSequence::new();
        feed(&mut held, &bind("", "s1"), ok("SELECT 1"));
        feed(&mut held, &execute("", 0), NONE);

        assert_eq!(feed(&mut held, &(Tag::FLUSH, Vec::new()), NONE), Held::Send);
        assert_eq!(held.replay().count(), 2, "the held frames were dropped");
    }

    #[test]
    fn a_row_limit_gives_the_sequence_up() {
        // An `Execute` with a limit is answered with `PortalSuspended` and
        // leaves the portal open, which is not an answer to store or to serve.
        let mut held = HeldSequence::new();
        feed(&mut held, &bind("", "s1"), ok("SELECT 1"));

        assert_eq!(feed(&mut held, &execute("", 10), NONE), Held::Send);
    }

    #[test]
    fn a_statement_describe_gives_the_sequence_up() {
        // Its answer carries a `ParameterDescription`, which the stored payload
        // does not hold. No mainstream driver sends one in the same sequence as
        // an `Execute`.
        let mut held = HeldSequence::new();
        feed(&mut held, &bind("", "s1"), ok("SELECT 1"));

        assert_eq!(feed(&mut held, &describe(b'S', "s1"), NONE), Held::Send);
    }

    #[test]
    fn a_describe_of_another_portal_gives_the_sequence_up() {
        let mut held = HeldSequence::new();
        feed(&mut held, &bind("p1", "s1"), ok("SELECT 1"));

        assert_eq!(feed(&mut held, &describe_portal("p2"), NONE), Held::Send);
    }

    #[test]
    fn an_execute_of_another_portal_gives_the_sequence_up() {
        // A portal bound in an earlier round trip, which this sequence holds
        // neither the SQL nor the parameters of.
        let mut held = HeldSequence::new();
        feed(&mut held, &bind("p1", "s1"), ok("SELECT 1"));

        assert_eq!(feed(&mut held, &execute("p2", 0), NONE), Held::Send);
    }

    #[test]
    fn a_second_bind_or_execute_gives_the_sequence_up() {
        // A pipeline of two statements. Its answer is two result sets, and this
        // holds one key.
        let mut twice_bound = HeldSequence::new();
        feed(&mut twice_bound, &bind("", "s1"), ok("SELECT 1"));
        assert_eq!(
            feed(&mut twice_bound, &bind("", "s1"), ok("SELECT 1")),
            Held::Send
        );

        let mut twice_run = HeldSequence::new();
        feed(&mut twice_run, &bind("", "s1"), ok("SELECT 1"));
        feed(&mut twice_run, &execute("", 0), NONE);
        assert_eq!(feed(&mut twice_run, &execute("", 0), NONE), Held::Send);
    }

    #[test]
    fn a_parse_after_a_bind_gives_the_sequence_up() {
        let mut held = HeldSequence::new();
        feed(&mut held, &bind("", "s1"), ok("SELECT 1"));

        assert_eq!(
            feed(&mut held, &parse("s2", "SELECT 2"), ok("SELECT 2")),
            Held::Send
        );
    }

    #[test]
    fn a_bind_of_a_different_statement_gives_the_sequence_up() {
        // `Parse s1` then `Bind s2` is two questions, and this holds one key.
        let mut held = HeldSequence::new();
        feed(&mut held, &parse("s1", "SELECT 1"), ok("SELECT 1"));

        assert_eq!(feed(&mut held, &bind("", "s2"), ok("SELECT 2")), Held::Send);
    }

    #[test]
    fn a_describe_before_a_bind_gives_the_sequence_up() {
        let mut held = HeldSequence::new();
        feed(&mut held, &parse("s1", "SELECT 1"), ok("SELECT 1"));

        assert_eq!(feed(&mut held, &describe_portal(""), NONE), Held::Send);
    }

    #[test]
    fn a_describe_after_the_execute_gives_the_sequence_up() {
        // Legal, and it means the client wants the description of a portal
        // that has already run. The stored payload's description belongs
        // before the rows.
        let mut held = HeldSequence::new();
        feed(&mut held, &bind("", "s1"), ok("SELECT 1"));
        feed(&mut held, &execute("", 0), NONE);

        assert_eq!(feed(&mut held, &describe_portal(""), NONE), Held::Send);
    }

    #[test]
    fn a_sync_with_nothing_run_gives_the_sequence_up() {
        // The prepare round trip every driver with a statement cache makes:
        // `Parse`, then a `Sync` to hear that it worked. There is no answer to
        // serve, so the `Parse` goes upstream where it was always going.
        let mut held = HeldSequence::new();
        feed(&mut held, &parse("s1", "SELECT 1"), ok("SELECT 1"));

        assert_eq!(feed(&mut held, &(Tag::SYNC, Vec::new()), NONE), Held::Send);
        assert_eq!(held.replay().count(), 1);
    }

    #[test]
    fn a_simple_query_mid_sequence_gives_it_up() {
        // Legal from a client that changed its mind, and nothing this machine
        // can reason about.
        let mut held = HeldSequence::new();
        feed(&mut held, &bind("", "s1"), ok("SELECT 1"));

        let mut out = Vec::new();
        encode_frontend::query(&mut out, "SELECT 2");
        assert_eq!(feed(&mut held, &framed(&out), ok("SELECT 2")), Held::Send);
    }

    #[test]
    fn a_terminate_mid_sequence_gives_it_up() {
        let mut held = HeldSequence::new();
        feed(&mut held, &bind("", "s1"), ok("SELECT 1"));

        assert_eq!(
            feed(&mut held, &(Tag::TERMINATE, Vec::new()), NONE),
            Held::Send
        );
    }

    #[test]
    fn a_sequence_too_large_to_hold_goes_upstream() {
        // The bound on what one session will hold. The bytes were the client's
        // own, so this is not an amplification, but a session is a
        // per-connection cost in a process designed around 100k of them.
        let mut held = HeldSequence::new();
        let big = vec![b'x'; MAX_HELD_BYTES];
        assert_eq!(
            feed(&mut held, &bound("", "s1", &[Some(&big)]), ok("SELECT $1")),
            Held::Send
        );
        assert!(held.is_empty());
    }

    #[test]
    fn clearing_forgets_the_portal_and_keeps_the_buffer() {
        // Forgetting is what happens at the end of every sequence rather than
        // a rule about which portal names outlive which message. A map that
        // only grew would be a leak for the length of a session, which here is
        // measured in days.
        let mut held = HeldSequence::new();
        feed(&mut held, &parse("s1", "SELECT $1"), ok("SELECT $1"));
        feed(
            &mut held,
            &bound("p1", "s1", &[Some(b"1")]),
            ok("SELECT $1"),
        );
        feed(&mut held, &describe_portal("p1"), NONE);
        feed(&mut held, &execute("p1", 0), NONE);

        held.clear();

        assert!(held.is_empty());
        assert_eq!(held.sql(), None);
        assert!(held.params().is_empty());
        assert!(!held.wants_row_description());
        assert_eq!(held.replay().count(), 0);

        // And the same machine takes the next sequence, which is why it is
        // cleared rather than dropped.
        assert_eq!(
            feed(&mut held, &bind("", "s1"), ok("SELECT 1")),
            Held::Withheld
        );
    }

    /// One frame, built the way a payload's frames are.
    fn backend(tag: Tag, body: &[u8]) -> Vec<u8> {
        let mut out = vec![tag.get()];
        out.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_be_bytes());
        out.extend_from_slice(body);
        out
    }

    /// A stored payload: a description, one row, and a completion.
    fn payload(with_description: bool) -> Vec<u8> {
        let mut out = Vec::new();
        if with_description {
            out.extend_from_slice(&backend(Tag::ROW_DESCRIPTION, b"desc"));
        }
        out.extend_from_slice(&backend(Tag::DATA_ROW, b"row"));
        out.extend_from_slice(&backend(Tag::COMMAND_COMPLETE, b"SELECT 1\0"));
        out
    }

    /// A sequence the whole way through, optionally asking for a description.
    fn ran(describing: bool) -> HeldSequence {
        let mut held = HeldSequence::new();
        let sql = "SELECT $1";
        feed(&mut held, &parse("s1", sql), ok(sql));
        feed(&mut held, &bound("", "s1", &[Some(b"7")]), ok(sql));
        if describing {
            feed(&mut held, &describe_portal(""), NONE);
        }
        feed(&mut held, &execute("", 0), NONE);
        held
    }

    #[test]
    fn a_hit_is_assembled_from_the_frames_the_client_sent() {
        // Byte for byte, because this is the whole of what the client sees and
        // an ordering mistake here is a driver that desynchronises rather than
        // one that reports an error.
        let mut out = Vec::new();
        ran(true).assemble(&payload(true), &mut out).unwrap();

        let mut want = Vec::new();
        want.extend_from_slice(&PARSE_COMPLETE);
        want.extend_from_slice(&BIND_COMPLETE);
        want.extend_from_slice(&backend(Tag::ROW_DESCRIPTION, b"desc"));
        want.extend_from_slice(&backend(Tag::DATA_ROW, b"row"));
        want.extend_from_slice(&backend(Tag::COMMAND_COMPLETE, b"SELECT 1\0"));
        want.extend_from_slice(&[b'Z', 0, 0, 0, 5, b'I']);

        assert_eq!(out, want);
    }

    #[test]
    fn a_client_that_asked_for_no_description_is_not_sent_one() {
        // The reason the payload keeps the description apart from the rows. One
        // entry answers a driver that describes its portal and one that does
        // not, and handing a `RowDescription` to the second is a frame it has
        // no state to receive.
        let mut out = Vec::new();
        let mut held = HeldSequence::new();
        feed(&mut held, &bind("", "s1"), ok("SELECT 1"));
        feed(&mut held, &execute("", 0), NONE);
        held.assemble(&payload(true), &mut out).unwrap();

        let mut want = Vec::new();
        want.extend_from_slice(&BIND_COMPLETE);
        want.extend_from_slice(&backend(Tag::DATA_ROW, b"row"));
        want.extend_from_slice(&backend(Tag::COMMAND_COMPLETE, b"SELECT 1\0"));
        want.extend_from_slice(&[b'Z', 0, 0, 0, 5, b'I']);

        assert_eq!(out, want);
    }

    #[test]
    fn a_payload_with_no_description_cannot_answer_a_client_that_asked() {
        // Reachable across an upgrade rather than in steady state, and a miss
        // rather than an assembled answer with a frame missing from the middle.
        let mut out = Vec::new();
        assert_eq!(
            ran(true).assemble(&payload(false), &mut out),
            Err(Unservable::NoRowDescription)
        );
        assert!(
            out.is_empty(),
            "a refusal left half an answer in the buffer"
        );
    }

    #[test]
    fn a_sequence_that_ran_nothing_has_no_answer_to_assemble() {
        let mut out = Vec::new();
        let mut held = HeldSequence::new();
        feed(&mut held, &bind("", "s1"), ok("SELECT 1"));

        assert_eq!(
            held.assemble(&payload(true), &mut out),
            Err(Unservable::NothingRan)
        );
    }

    #[test]
    fn a_payload_that_is_not_an_answer_is_refused_rather_than_replayed() {
        let held = ran(false);
        let mut out = Vec::new();

        for (case, bytes) in [
            ("empty", Vec::new()),
            ("no completion", backend(Tag::DATA_ROW, b"row")),
            ("a short header", vec![b'D', 0, 0]),
            ("a length under its own header", vec![b'D', 0, 0, 0, 3]),
            ("a length past the end", vec![b'D', 0, 0, 0, 99]),
            ("a frame that is not part of an answer", {
                let mut out = backend(Tag::PARSE_COMPLETE, b"");
                out.extend_from_slice(&backend(Tag::COMMAND_COMPLETE, b"SELECT 1\0"));
                out
            }),
            ("a description after the rows", {
                let mut out = backend(Tag::DATA_ROW, b"row");
                out.extend_from_slice(&backend(Tag::ROW_DESCRIPTION, b"desc"));
                out.extend_from_slice(&backend(Tag::COMMAND_COMPLETE, b"SELECT 1\0"));
                out
            }),
            ("anything after the completion", {
                let mut out = payload(true);
                out.extend_from_slice(&backend(Tag::DATA_ROW, b"late"));
                out
            }),
        ] {
            assert_eq!(
                held.assemble(&bytes, &mut out),
                Err(Unservable::Malformed),
                "{case} was accepted as a stored answer"
            );
        }
    }

    #[test]
    fn an_empty_query_response_ends_a_payload_as_a_completion_does() {
        let mut out = Vec::new();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&backend(Tag::EMPTY_QUERY_RESPONSE, b""));
        ran(false).assemble(&bytes, &mut out).unwrap();

        assert!(out.ends_with(&[b'Z', 0, 0, 0, 5, b'I']));
    }

    #[test]
    fn a_truncated_buffer_ends_the_walk_rather_than_panicking() {
        // This module writes the buffer, so a short frame in it is a bug rather
        // than an input. It still must not be a panic on a node serving 100k
        // other connections.
        let mut held = HeldSequence::new();
        feed(&mut held, &bind("", "s1"), ok("SELECT 1"));
        held.frames.truncate(3);
        assert_eq!(held.replay().count(), 0);

        held.frames.clear();
        held.frames.extend_from_slice(&[b'B', 0, 0, 0, 200]);
        assert_eq!(held.replay().count(), 0, "a length past the end was read");

        held.frames.clear();
        held.frames.extend_from_slice(&[b'B', 0, 0, 0, 0]);
        assert_eq!(
            held.replay().count(),
            0,
            "a length under the header was read"
        );
    }
}
