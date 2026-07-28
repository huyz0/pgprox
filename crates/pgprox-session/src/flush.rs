//! What the server still owes the client, so a `Flush` can be answered.
//!
//! # Why this exists
//!
//! The extended query protocol has two ways to say "answer me now". A `Sync`
//! ends the sequence and the server replies with a `ReadyForQuery`, which is a
//! terminator anything can wait for. A `Flush` does not: the server answers
//! everything outstanding and then says nothing more, because the client is
//! expected to keep going. There is no message that means "that was all".
//!
//! A proxy that reads until `ReadyForQuery` therefore deadlocks on a `Flush`.
//! The server has answered, the client is waiting for the answer, and the
//! proxy is waiting for a message that will never be sent. asyncpg prepares
//! every statement with `Parse`, `Describe`, `Flush`, so on a proxy with this
//! bug it hangs on its first parameterised query and nothing else does. That
//! is how it survived to M8: the e2e run drives psql and pgbench, and both use
//! `Sync`.
//!
//! # How the answer is known
//!
//! By counting. Every extended-query frame the proxy forwards makes the server
//! owe exactly one completion, and the tags that discharge each kind are
//! fixed. When nothing is outstanding, a `Flush` has been fully answered and
//! the proxy can go back to reading the client.
//!
//! Counting rather than a timeout, because a timeout would either be too short
//! for a slow statement or long enough to be its own hang.

use std::collections::VecDeque;

use pgprox_proto::frame::Tag;
use pgprox_proto::frontend::FrontendMessage;

/// One thing the server owes, named by what discharges it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Owed {
    /// `Parse` is answered by `ParseComplete`.
    Parse,
    /// `Bind` is answered by `BindComplete`.
    Bind,
    /// `Close` is answered by `CloseComplete`.
    Close,
    /// `Describe` is answered by `RowDescription` or `NoData`.
    ///
    /// A `Describe` of a statement is answered by `ParameterDescription`
    /// first, which is not the end of it: the description of the *result* is
    /// what closes the exchange, and a statement returning no rows says so
    /// with `NoData`.
    Describe,
    /// `Execute` is answered by `CommandComplete`, `PortalSuspended` or
    /// `EmptyQueryResponse`.
    ///
    /// Any number of `DataRow`s come first and none of them ends it. A portal
    /// stopped at its row limit says `PortalSuspended`, which is an ending as
    /// much as `CommandComplete` is: the client asked for that many rows and
    /// has them.
    Execute,
}

/// The queue of completions the server has not sent yet.
///
/// Empty means a `Flush` at this moment has been answered in full.
#[derive(Debug, Default, Clone)]
pub struct Outstanding {
    owed: VecDeque<Owed>,
}

impl Outstanding {
    /// Nothing outstanding.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a client frame the proxy has forwarded upstream.
    ///
    /// Forwarded, not received. A `Parse` the proxy answers itself because the
    /// connection already holds that statement never reaches the server, so
    /// the server owes nothing for it, and counting it would leave a `Flush`
    /// waiting forever for a completion nobody will send.
    pub fn sent(&mut self, message: &FrontendMessage<'_>) {
        let owed = match message {
            FrontendMessage::Parse { .. } => Owed::Parse,
            FrontendMessage::Bind { .. } => Owed::Bind,
            FrontendMessage::Close { .. } => Owed::Close,
            FrontendMessage::Describe { .. } => Owed::Describe,
            FrontendMessage::Execute { .. } => Owed::Execute,
            // `Sync` and `Query` end the sequence with a `ReadyForQuery`,
            // which clears this outright rather than discharging one entry.
            // `Flush` asks for what is already owed and adds nothing.
            _ => return,
        };
        self.owed.push_back(owed);
    }

    /// Records a server frame the proxy has forwarded to the client.
    ///
    /// Forwarded, again: the completions for a `Parse` or a `Close` the proxy
    /// issued on the client's behalf are swallowed before they get here, and
    /// they discharge nothing the client is waiting for.
    ///
    /// By tag rather than by decoded message, because every completion this
    /// cares about is a body-less frame the decoder leaves opaque, and asking
    /// for the decode would be asking for a parse the caller already skipped.
    pub fn received(&mut self, tag: Tag) {
        // The server abandons the rest of the sequence on an error and will
        // not answer any of it: it discards messages until a `Sync`. The
        // client sends one, and that produces the `ReadyForQuery` beside it
        // here, but the abandonment has to be recorded when the error arrives
        // or the proxy waits for completions nobody will send.
        if tag == Tag::ERROR_RESPONSE || tag == Tag::READY_FOR_QUERY {
            self.owed.clear();
            return;
        }
        self.discharge(tag);
    }

    /// Whether every frame forwarded since the last `Sync` has been answered.
    #[must_use]
    pub fn settled(&self) -> bool {
        self.owed.is_empty()
    }

    /// Pops the head if this tag is what the head was waiting for.
    ///
    /// Strict about order, because the server answers in order and a tag that
    /// does not match the head is one that discharges nothing: a `DataRow`, a
    /// `ParameterDescription`, a `NoticeResponse`. Guessing which entry an
    /// out-of-order tag belonged to would be a way to lose count silently.
    fn discharge(&mut self, tag: Tag) {
        let Some(head) = self.owed.front().copied() else {
            return;
        };
        let matches = match head {
            Owed::Parse => tag == Tag::PARSE_COMPLETE,
            Owed::Bind => tag == Tag::BIND_COMPLETE,
            Owed::Close => tag == Tag::CLOSE_COMPLETE,
            Owed::Describe => tag == Tag::ROW_DESCRIPTION || tag == Tag::NO_DATA,
            Owed::Execute => {
                tag == Tag::COMMAND_COMPLETE
                    || tag == Tag::PORTAL_SUSPENDED
                    || tag == Tag::EMPTY_QUERY_RESPONSE
            }
        };
        if matches {
            self.owed.pop_front();
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn parse() -> FrontendMessage<'static> {
        FrontendMessage::Parse {
            statement: "s1",
            sql: "SELECT $1",
        }
    }

    fn describe() -> FrontendMessage<'static> {
        FrontendMessage::Describe {
            target: pgprox_proto::frontend::Target::Statement,
            name: "s1",
        }
    }

    fn bind() -> FrontendMessage<'static> {
        FrontendMessage::Bind {
            portal: "",
            statement: "s1",
        }
    }

    fn execute() -> FrontendMessage<'static> {
        FrontendMessage::Execute {
            portal: "",
            max_rows: 0,
        }
    }

    #[test]
    fn nothing_sent_is_already_settled() {
        assert!(Outstanding::new().settled());
    }

    #[test]
    fn the_asyncpg_prepare_sequence_settles_when_the_description_arrives() {
        // Parse, Describe, Flush. This is the exact sequence that hung: the
        // server answers with ParseComplete, ParameterDescription and
        // RowDescription and then goes quiet, and there is no ReadyForQuery.
        let mut owed = Outstanding::new();
        owed.sent(&parse());
        owed.sent(&describe());
        owed.sent(&FrontendMessage::Flush);
        assert!(!owed.settled(), "a Flush on its own settles nothing");

        owed.received(Tag(b'1'));
        assert!(!owed.settled(), "the description is still owed");
        owed.received(Tag(b't'));
        assert!(
            !owed.settled(),
            "ParameterDescription is not the end of a Describe"
        );
        owed.received(Tag(b'T'));
        assert!(owed.settled(), "RowDescription ends it");
    }

    #[test]
    fn a_statement_returning_no_rows_ends_its_describe_with_no_data() {
        let mut owed = Outstanding::new();
        owed.sent(&describe());
        owed.received(Tag(b'n'));
        assert!(owed.settled());
    }

    #[test]
    fn rows_do_not_discharge_the_execute_they_belong_to() {
        let mut owed = Outstanding::new();
        owed.sent(&bind());
        owed.sent(&execute());
        owed.received(Tag(b'2'));
        for _ in 0..100 {
            owed.received(Tag(b'D'));
            assert!(!owed.settled(), "a DataRow ended an Execute");
        }
        owed.received(Tag(b'C'));
        assert!(owed.settled());
    }

    #[test]
    fn a_suspended_portal_ends_its_execute() {
        // The client asked for a row limit and got it. It is waiting on
        // exactly this and nothing further is coming until it asks again.
        let mut owed = Outstanding::new();
        owed.sent(&execute());
        owed.received(Tag(b's'));
        assert!(owed.settled());
    }

    #[test]
    fn an_empty_query_ends_its_execute() {
        let mut owed = Outstanding::new();
        owed.sent(&execute());
        owed.received(Tag(b'I'));
        assert!(owed.settled());
    }

    #[test]
    fn a_close_is_answered_by_its_own_completion() {
        let mut owed = Outstanding::new();
        owed.sent(&FrontendMessage::Close {
            target: pgprox_proto::frontend::Target::Statement,
            name: "s1",
        });
        owed.received(Tag(b'3'));
        assert!(owed.settled());
    }

    #[test]
    fn an_error_abandons_everything_still_owed() {
        // The server skips the rest of the sequence and waits for a Sync. A
        // proxy that kept waiting for the completions it was promised would
        // hang on the error path instead of the happy one.
        let mut owed = Outstanding::new();
        owed.sent(&parse());
        owed.sent(&describe());
        owed.sent(&execute());
        owed.received(Tag::ERROR_RESPONSE);
        assert!(owed.settled());
    }

    #[test]
    fn ready_for_query_clears_whatever_is_left() {
        let mut owed = Outstanding::new();
        owed.sent(&parse());
        owed.received(Tag::READY_FOR_QUERY);
        assert!(owed.settled());
    }

    #[test]
    fn a_completion_out_of_order_discharges_nothing() {
        // The server answers in order. A tag that does not match the head
        // belongs to something else, and popping on it would lose count in a
        // way that shows up as a hang three statements later.
        let mut owed = Outstanding::new();
        owed.sent(&parse());
        owed.received(Tag(b'2'));
        assert!(!owed.settled(), "a BindComplete discharged a Parse");
        owed.received(Tag(b'1'));
        assert!(owed.settled());
    }

    #[test]
    fn a_frame_that_owes_nothing_adds_nothing() {
        let mut owed = Outstanding::new();
        owed.sent(&FrontendMessage::Sync);
        owed.sent(&FrontendMessage::Flush);
        owed.sent(&FrontendMessage::Query { sql: "SELECT 1" });
        assert!(owed.settled());
    }

    #[test]
    fn a_completion_with_nothing_owed_is_ignored() {
        let mut owed = Outstanding::new();
        owed.received(Tag(b'1'));
        assert!(owed.settled());
    }
}
