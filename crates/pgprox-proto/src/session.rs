//! When an upstream connection may be released.
//!
//! This is the most correctness-critical logic in the crate. Releasing too
//! eagerly hands another client a connection that is mid-transaction,
//! mid-sequence, or mid-COPY, and the resulting cross-client state leakage is
//! invisible until someone sees data from the wrong session.
//!
//! Three conditions must all hold. The transaction status must be `I`, no
//! extended query sequence may be outstanding, and the session must not be in
//! COPY. Any one of them failing holds the connection.
//!
//! Sans-I/O: this is a pure function of the frames that have passed, so a byte
//! sequence captured from a trace drives it directly.

use crate::backend::{BackendMessage, TxStatus};
use crate::frontend::FrontendMessage;

/// Which way a COPY stream is flowing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum CopyDirection {
    /// Client to server.
    In,
    /// Server to client.
    Out,
    /// Both, used by replication.
    Both,
}

/// Why a connection is being held.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum HoldReason {
    /// A transaction is open.
    InTransaction,
    /// A transaction failed and is awaiting rollback.
    FailedTransaction,
    /// An extended query sequence has not been ended by `Sync`.
    ExtendedSequence,
    /// A COPY stream is in progress.
    Copy(CopyDirection),
    /// Authentication has not finished.
    Authenticating,
}

/// Tracks whether releasing the upstream connection is safe.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SessionState {
    tx_status: TxStatus,
    /// True from the first extended-query message until the `ReadyForQuery`
    /// that answers its `Sync`.
    in_sequence: bool,
    copy: Option<CopyDirection>,
    ready_seen: bool,
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionState {
    /// A session that has not yet finished authenticating.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            tx_status: TxStatus::Idle,
            in_sequence: false,
            copy: None,
            ready_seen: false,
        }
    }

    /// The current transaction status.
    #[must_use]
    pub const fn tx_status(&self) -> TxStatus {
        self.tx_status
    }

    /// Whether an extended query sequence is outstanding.
    #[must_use]
    pub const fn in_sequence(&self) -> bool {
        self.in_sequence
    }

    /// The COPY stream in progress, if any.
    #[must_use]
    pub const fn copy(&self) -> Option<CopyDirection> {
        self.copy
    }

    /// Why the connection is being held, or [`None`] if it may be released.
    ///
    /// Checked in the order an operator would want to see reported: the
    /// transaction first, since that is the usual answer.
    #[must_use]
    pub const fn hold_reason(&self) -> Option<HoldReason> {
        if !self.ready_seen {
            return Some(HoldReason::Authenticating);
        }
        match self.tx_status {
            TxStatus::InTransaction => return Some(HoldReason::InTransaction),
            TxStatus::Failed => return Some(HoldReason::FailedTransaction),
            TxStatus::Idle => {}
        }
        if let Some(direction) = self.copy {
            return Some(HoldReason::Copy(direction));
        }
        if self.in_sequence {
            return Some(HoldReason::ExtendedSequence);
        }
        None
    }

    /// Whether the upstream connection may be returned to the pool.
    #[must_use]
    pub const fn is_releasable(&self) -> bool {
        self.hold_reason().is_none()
    }

    /// Records a message from the client.
    pub fn on_frontend(&mut self, msg: &FrontendMessage<'_>) {
        match msg {
            // A COPY IN stream ends when the client says so, either way.
            FrontendMessage::CopyDone | FrontendMessage::CopyFail => {
                if self.copy == Some(CopyDirection::In) {
                    self.copy = None;
                }
            }
            // Sync does not end the sequence by itself. The sequence is over
            // when the server answers with ReadyForQuery, and releasing on the
            // client's Sync would race the frames still in flight.
            FrontendMessage::Sync => {}
            other if other.starts_extended_sequence() => self.in_sequence = true,
            _ => {}
        }
    }

    /// Records a message from the server.
    pub fn on_backend(&mut self, msg: &BackendMessage<'_>) {
        match msg {
            BackendMessage::ReadyForQuery(status) => {
                self.ready_seen = true;
                self.tx_status = *status;
                // ReadyForQuery is the end of any sequence, and Postgres sends
                // one after an error too, which is how a failed sequence
                // recovers rather than holding the connection forever.
                self.in_sequence = false;
            }
            BackendMessage::CopyInResponse => self.copy = Some(CopyDirection::In),
            BackendMessage::CopyOutResponse => self.copy = Some(CopyDirection::Out),
            BackendMessage::CopyBothResponse => self.copy = Some(CopyDirection::Both),
            BackendMessage::CopyDone => self.copy = None,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::Target;

    /// A session that has authenticated and is sitting idle.
    fn ready() -> SessionState {
        let mut s = SessionState::new();
        s.on_backend(&BackendMessage::ReadyForQuery(TxStatus::Idle));
        s
    }

    fn parse() -> FrontendMessage<'static> {
        FrontendMessage::Parse {
            statement: "s1",
            sql: "SELECT 1",
        }
    }

    #[test]
    fn a_new_session_is_held_until_authentication_finishes() {
        let s = SessionState::new();
        assert!(!s.is_releasable());
        assert_eq!(s.hold_reason(), Some(HoldReason::Authenticating));
    }

    #[test]
    fn an_idle_authenticated_session_is_releasable() {
        assert!(ready().is_releasable());
        assert_eq!(ready().hold_reason(), None);
    }

    #[test]
    fn an_open_transaction_holds_the_connection() {
        let mut s = ready();
        s.on_backend(&BackendMessage::ReadyForQuery(TxStatus::InTransaction));
        assert!(!s.is_releasable());
        assert_eq!(s.hold_reason(), Some(HoldReason::InTransaction));
    }

    #[test]
    fn a_failed_transaction_holds_the_connection() {
        // The intuitive but wrong reading is that a failed transaction is over.
        // It is not: the session rejects every statement until it sees a
        // rollback, so handing it to another client hands them a broken session.
        let mut s = ready();
        s.on_backend(&BackendMessage::ReadyForQuery(TxStatus::Failed));
        assert!(!s.is_releasable());
        assert_eq!(s.hold_reason(), Some(HoldReason::FailedTransaction));
    }

    #[test]
    fn an_extended_sequence_holds_the_connection() {
        let mut s = ready();
        assert!(s.is_releasable());

        s.on_frontend(&parse());
        assert!(s.in_sequence());
        assert!(!s.is_releasable(), "released mid-sequence");
        assert_eq!(s.hold_reason(), Some(HoldReason::ExtendedSequence));
    }

    #[test]
    fn a_sync_alone_does_not_permit_release() {
        // The subtle one. Sync is the client saying it has finished sending,
        // not the server saying it has finished answering. Releasing here would
        // race the frames still in flight.
        let mut s = ready();
        s.on_frontend(&parse());
        s.on_frontend(&FrontendMessage::Sync);

        assert!(
            !s.is_releasable(),
            "released on the client's Sync, before the server answered"
        );
        assert_eq!(s.hold_reason(), Some(HoldReason::ExtendedSequence));
    }

    #[test]
    fn the_sequence_ends_at_the_ready_for_query_that_answers_it() {
        let mut s = ready();
        s.on_frontend(&parse());
        s.on_frontend(&FrontendMessage::Sync);
        s.on_backend(&BackendMessage::ReadyForQuery(TxStatus::Idle));

        assert!(!s.in_sequence());
        assert!(s.is_releasable());
    }

    #[test]
    fn a_missing_sync_holds_the_connection_indefinitely() {
        // The pipelining bug this exists to catch: a client that never syncs
        // must never have its connection recycled underneath it.
        let mut s = ready();
        s.on_frontend(&parse());
        s.on_frontend(&FrontendMessage::Bind {
            portal: "",
            statement: "s1",
        });
        s.on_frontend(&FrontendMessage::Execute {
            portal: "",
            max_rows: 0,
        });

        assert!(!s.is_releasable(), "released with no Sync ever sent");
    }

    #[test]
    fn every_extended_message_starts_a_sequence() {
        for msg in [
            parse(),
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
        ] {
            let mut s = ready();
            s.on_frontend(&msg);
            assert!(!s.is_releasable(), "{msg:?} did not hold the connection");
        }
    }

    #[test]
    fn a_failed_sequence_recovers_rather_than_holding_forever() {
        // Postgres sends ReadyForQuery after an error too. Without that ending
        // the sequence, one failed extended query would leak the connection.
        let mut s = ready();
        s.on_frontend(&parse());
        s.on_backend(&BackendMessage::ErrorResponse(
            crate::backend::ErrorFields::default(),
        ));
        assert!(!s.is_releasable(), "error alone should not release");

        s.on_backend(&BackendMessage::ReadyForQuery(TxStatus::Idle));
        assert!(
            s.is_releasable(),
            "connection leaked after a failed sequence"
        );
    }

    #[test]
    fn a_copy_stream_holds_the_connection_until_it_ends() {
        for (start, direction) in [
            (BackendMessage::CopyInResponse, CopyDirection::In),
            (BackendMessage::CopyOutResponse, CopyDirection::Out),
            (BackendMessage::CopyBothResponse, CopyDirection::Both),
        ] {
            let mut s = ready();
            s.on_backend(&start);

            assert_eq!(s.copy(), Some(direction));
            assert!(!s.is_releasable(), "released during COPY {direction:?}");
            assert_eq!(s.hold_reason(), Some(HoldReason::Copy(direction)));

            s.on_backend(&BackendMessage::CopyDone);
            assert!(s.is_releasable(), "COPY {direction:?} never ended");
        }
    }

    #[test]
    fn a_client_ending_a_copy_in_stream_releases_the_hold() {
        // COPY IN ends from the client's side, which is the direction the
        // server-side CopyDone does not cover.
        for ending in [FrontendMessage::CopyDone, FrontendMessage::CopyFail] {
            let mut s = ready();
            s.on_backend(&BackendMessage::CopyInResponse);
            assert!(!s.is_releasable());

            s.on_frontend(&ending);
            assert!(s.is_releasable(), "{ending:?} did not end COPY IN");
        }
    }

    #[test]
    fn a_client_copy_done_does_not_end_a_copy_out_stream() {
        // The server is still sending. Ending the hold here would recycle a
        // connection with rows still in flight.
        let mut s = ready();
        s.on_backend(&BackendMessage::CopyOutResponse);
        s.on_frontend(&FrontendMessage::CopyDone);

        assert!(!s.is_releasable(), "COPY OUT ended by the client");
        assert_eq!(s.hold_reason(), Some(HoldReason::Copy(CopyDirection::Out)));
    }

    #[test]
    fn copy_inside_a_transaction_reports_the_transaction_first() {
        // Both hold. The transaction is the more useful thing to tell an
        // operator, and it is the one that outlives the COPY.
        let mut s = ready();
        s.on_backend(&BackendMessage::ReadyForQuery(TxStatus::InTransaction));
        s.on_backend(&BackendMessage::CopyInResponse);
        assert_eq!(s.hold_reason(), Some(HoldReason::InTransaction));
        assert!(!s.is_releasable());
    }

    #[test]
    fn ordinary_traffic_does_not_change_the_verdict() {
        // DataRow, RowDescription and friends flow through without affecting
        // releasability, which is what lets them be forwarded unparsed.
        let mut s = ready();
        for msg in [
            BackendMessage::Opaque(crate::Tag::DATA_ROW),
            BackendMessage::Opaque(crate::Tag::ROW_DESCRIPTION),
            BackendMessage::CommandComplete { tag: "SELECT 1" },
            BackendMessage::ParameterStatus {
                name: "x",
                value: "y",
            },
        ] {
            s.on_backend(&msg);
            assert!(s.is_releasable(), "{msg:?} changed releasability");
        }
    }

    #[test]
    fn a_simple_query_does_not_start_a_sequence() {
        let mut s = ready();
        s.on_frontend(&FrontendMessage::Query { sql: "SELECT 1" });
        assert!(!s.in_sequence());
        assert!(s.is_releasable());
    }

    #[test]
    fn the_default_state_matches_a_new_one() {
        assert_eq!(SessionState::default(), SessionState::new());
        assert_eq!(SessionState::new().tx_status(), TxStatus::Idle);
    }
}
