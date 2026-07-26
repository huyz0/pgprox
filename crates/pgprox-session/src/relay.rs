//! One frame in, one decision out.
//!
//! This is the join the rest of the project was built for. Four machines meet
//! here and none of them knew about the others:
//!
//! - `pgprox_proto::SessionState` says whether *this moment* is safe to
//!   release at: transaction status, extended-query sequence, COPY.
//! - `pgprox_pool::PinState` says whether *any* moment is, which is a
//!   different question and never resolves itself.
//! - `pgprox_route::SessionRouter` says where a statement goes.
//! - The connection this session is holding, if it is holding one.
//!
//! # The release rule
//!
//! Release only at `ReadyForQuery('I')`, with no extended-query sequence
//! outstanding, with no COPY in flight, and only when the session is not
//! pinned. Never from the SQL text. `COMMIT` is not the signal; the status
//! byte the server sent back is, because a `COMMIT` inside a failed
//! transaction does not commit and a text scan cannot tell.
//!
//! # What this module does not do
//!
//! It does not acquire, release, read, or write. It says which of those the
//! shell should do. The shell reports back what happened, through
//! [`Relay::acquired`] and [`Relay::released`], because an acquire can fail
//! and a machine that assumed otherwise would believe it holds a connection it
//! does not.

use std::time::Instant;

use pgprox_core::route::RouteTarget;
use pgprox_pool::pin::{PinReason, PinState, REPLAYABLE_PARAMETERS};
use pgprox_proto::backend::{BackendMessage, TxStatus};
use pgprox_proto::frontend::FrontendMessage;
use pgprox_proto::session::SessionState;
use pgprox_route::replica::Replicas;
use pgprox_route::router::{Routed, SessionRouter};

/// What the shell should do with a client frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientAction {
    /// Send it upstream, acquiring a connection to `target` first if `acquire`.
    Send {
        /// Where it goes.
        target: RouteTarget,
        /// Whether a connection has to be acquired before it can.
        acquire: bool,
    },
    /// Answer the client here. The server never sees this statement.
    ///
    /// A `SET pgprox.route` is about the session rather than about the
    /// database, and forwarding it would make Postgres reject it.
    Answer(Routed),
    /// The client asked to end the session.
    Close,
}

/// The result of one client frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientOutcome {
    /// What to do.
    pub action: ClientAction,
    /// Set when this frame is the one that pinned the session, so
    /// `pgprox_pin_total{reason}` is incremented exactly once per session.
    pub pinned: Option<PinReason>,
}

/// The result of one server frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerOutcome {
    /// Whether the upstream connection may now go back to the pool.
    pub release: bool,
    /// Set when this frame pinned the session, which a `NotificationResponse`
    /// can do without the client ever issuing `LISTEN`.
    pub pinned: Option<PinReason>,
}

/// The per-session relay decision machine.
#[derive(Debug, Default)]
pub struct Relay {
    session: SessionState,
    pin: PinState,
    router: SessionRouter,
    holding: bool,
}

impl Relay {
    /// A relay for a session that has just authenticated.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the session holds an upstream connection.
    #[must_use]
    pub const fn is_holding(&self) -> bool {
        self.holding
    }

    /// Whether the session is pinned, and why.
    #[must_use]
    pub const fn pin_reason(&self) -> Option<PinReason> {
        self.pin.reason()
    }

    /// The protocol state, for anything that needs to ask why a connection is
    /// being held.
    #[must_use]
    pub const fn session(&self) -> &SessionState {
        &self.session
    }

    /// Whether a write has been routed whose position is not yet known.
    ///
    /// The shell asks the primary where it landed and feeds the answer back
    /// through [`Self::record_write`]. Until it does, the session's reads keep
    /// going to the primary, which is the safe direction.
    #[must_use]
    pub const fn wrote(&self) -> bool {
        self.router.wrote()
    }

    /// Records where the session's writes have reached.
    pub fn record_write(&mut self, lsn: pgprox_core::ids::Lsn) {
        self.router.record_write(lsn);
    }

    /// Records that the shell acquired the connection it was told to.
    pub const fn acquired(&mut self) {
        self.holding = true;
    }

    /// Records that the shell returned the connection to the pool.
    pub const fn released(&mut self) {
        self.holding = false;
    }

    /// Feeds in one client frame.
    pub fn on_client(
        &mut self,
        message: &FrontendMessage<'_>,
        replicas: &Replicas,
        now: Instant,
    ) -> ClientOutcome {
        if matches!(message, FrontendMessage::Terminate) {
            return ClientOutcome {
                action: ClientAction::Close,
                pinned: None,
            };
        }

        self.session.on_frontend(message);

        let sql = match message {
            FrontendMessage::Query { sql } | FrontendMessage::Parse { sql, .. } => Some(*sql),
            _ => None,
        };

        let Some(sql) = sql else {
            // Bind, Execute, Describe, Close, Sync, Flush, CopyData and the
            // rest carry no SQL to classify. They belong to the statement that
            // came before them, so they go wherever it went.
            return self.forward_without_routing();
        };

        let pinned = self.pin.observe_statement(sql, REPLAYABLE_PARAMETERS);
        if pinned.is_some() {
            // The router has to know: a pinned session is bound to one
            // connection, and that connection is on the primary.
            self.router.set_pinned(true);
        }

        let in_transaction = matches!(self.session.tx_status(), TxStatus::InTransaction);
        match self.router.route(sql, in_transaction, replicas, now) {
            Routed::To(target) => ClientOutcome {
                action: ClientAction::Send {
                    target,
                    acquire: !self.holding,
                },
                pinned,
            },
            answered => ClientOutcome {
                action: ClientAction::Answer(answered),
                pinned,
            },
        }
    }

    /// Where a frame with no SQL of its own goes.
    fn forward_without_routing(&mut self) -> ClientOutcome {
        ClientOutcome {
            action: ClientAction::Send {
                // The primary is the conservative answer for a session that
                // somehow has no connection and no statement to classify. A
                // frame arriving here with nothing held is a driver doing
                // something unusual rather than a case worth guessing at.
                target: RouteTarget::Primary,
                acquire: !self.holding,
            },
            pinned: None,
        }
    }

    /// Feeds in one server frame.
    pub fn on_server(&mut self, message: &BackendMessage<'_>) -> ServerOutcome {
        self.session.on_backend(message);

        let pinned = match message {
            // A session can receive notifications it never asked for, from a
            // trigger or another session's NOTIFY. It is still bound to the
            // backend that delivered them.
            BackendMessage::NotificationResponse { .. } => self.pin.observe_notification(),
            _ => None,
        };
        if pinned.is_some() {
            self.router.set_pinned(true);
        }

        if let BackendMessage::ReadyForQuery(status) = message
            && matches!(status, TxStatus::Idle)
        {
            self.router.end_transaction();
        }

        ServerOutcome {
            // is_releasable answers the "is this moment safe" half. The pin
            // answers the "is any moment safe" half. Both, or the connection
            // stays where it is.
            release: self.holding && self.session.is_releasable() && !self.pin.is_pinned(),
            pinned,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use pgprox_proto::backend::TxStatus;
    use pgprox_route::replica::ReplicaConfig;

    fn replicas() -> Replicas {
        // No replicas: this module's subject is the release rule, and every
        // route resolves to the primary so the assertions are about holding
        // rather than about routing, which pgprox-route already tests.
        Replicas::new(0, ReplicaConfig::default())
    }

    fn query(sql: &str) -> FrontendMessage<'_> {
        FrontendMessage::Query { sql }
    }

    fn ready(status: TxStatus) -> BackendMessage<'static> {
        BackendMessage::ReadyForQuery(status)
    }

    /// Runs a statement to its `ReadyForQuery`, returning whether it released.
    fn round_trip(relay: &mut Relay, sql: &str, ends_at: TxStatus) -> bool {
        let outcome = relay.on_client(&query(sql), &replicas(), Instant::now());
        if let ClientAction::Send { acquire: true, .. } = outcome.action {
            relay.acquired();
        }
        let released = relay.on_server(&ready(ends_at)).release;
        if released {
            relay.released();
        }
        released
    }

    #[test]
    fn a_first_statement_acquires_and_an_idle_ready_releases() {
        let mut relay = Relay::new();
        let outcome = relay.on_client(&query("SELECT 1"), &replicas(), Instant::now());

        assert_eq!(
            outcome.action,
            ClientAction::Send {
                target: RouteTarget::Primary,
                acquire: true,
            }
        );
        relay.acquired();
        assert!(relay.on_server(&ready(TxStatus::Idle)).release);
    }

    #[test]
    fn a_connection_is_never_released_mid_transaction() {
        // The rule the whole design rests on. Released here and the next
        // client gets a connection with an open transaction on it.
        let mut relay = Relay::new();
        assert!(!round_trip(&mut relay, "BEGIN", TxStatus::InTransaction));
        assert!(relay.is_holding());
        assert!(!round_trip(
            &mut relay,
            "UPDATE t SET a = 1",
            TxStatus::InTransaction
        ));
        assert!(relay.is_holding());
        assert!(round_trip(&mut relay, "COMMIT", TxStatus::Idle));
    }

    #[test]
    fn a_failed_transaction_holds_until_it_is_rolled_back() {
        // 'E' is not 'I'. A connection released here carries an aborted
        // transaction into somebody else's session.
        let mut relay = Relay::new();
        round_trip(&mut relay, "BEGIN", TxStatus::InTransaction);
        assert!(!round_trip(&mut relay, "SELECT nope", TxStatus::Failed));
        assert!(relay.is_holding());
        assert!(round_trip(&mut relay, "ROLLBACK", TxStatus::Idle));
    }

    #[test]
    fn a_commit_that_did_not_commit_does_not_release() {
        // The reason the status byte is authoritative and the SQL text is not:
        // COMMIT inside a failed transaction rolls back, and the text says
        // nothing about that.
        let mut relay = Relay::new();
        round_trip(&mut relay, "BEGIN", TxStatus::InTransaction);
        round_trip(&mut relay, "SELECT nope", TxStatus::Failed);
        assert!(
            !round_trip(&mut relay, "COMMIT", TxStatus::Failed),
            "released on the word COMMIT rather than on the status byte"
        );
    }

    #[test]
    fn an_extended_sequence_holds_until_sync_comes_back() {
        let mut relay = Relay::new();
        let outcome = relay.on_client(
            &FrontendMessage::Parse {
                statement: "s1",
                sql: "SELECT 1",
            },
            &replicas(),
            Instant::now(),
        );
        assert!(matches!(
            outcome.action,
            ClientAction::Send { acquire: true, .. }
        ));
        relay.acquired();

        for message in [
            FrontendMessage::Bind {
                portal: "",
                statement: "s1",
            },
            FrontendMessage::Execute {
                portal: "",
                max_rows: 0,
            },
        ] {
            relay.on_client(&message, &replicas(), Instant::now());
        }

        // A Flush is not the end of a sequence, and the server's answer to it
        // is not a ReadyForQuery. Nothing may be released here.
        relay.on_client(&FrontendMessage::Flush, &replicas(), Instant::now());
        assert!(!relay.on_server(&BackendMessage::EmptyQueryResponse).release);

        relay.on_client(&FrontendMessage::Sync, &replicas(), Instant::now());
        assert!(relay.on_server(&ready(TxStatus::Idle)).release);
    }

    #[test]
    fn a_pinned_session_never_releases_however_idle_it_looks() {
        let mut relay = Relay::new();
        let outcome = relay.on_client(&query("LISTEN channel"), &replicas(), Instant::now());

        assert_eq!(outcome.pinned, Some(PinReason::Listen));
        relay.acquired();
        assert!(!relay.on_server(&ready(TxStatus::Idle)).release);
        assert!(!round_trip(&mut relay, "SELECT 1", TxStatus::Idle));
        assert!(relay.is_holding());
    }

    #[test]
    fn a_notification_pins_a_session_that_never_asked_for_one() {
        // A trigger, or another session's NOTIFY. The session is bound to the
        // backend that delivered it whether or not it issued LISTEN.
        let mut relay = Relay::new();
        relay.on_client(&query("SELECT 1"), &replicas(), Instant::now());
        relay.acquired();

        let outcome = relay.on_server(&BackendMessage::NotificationResponse {
            process_id: 42,
            channel: "c",
            payload: "",
        });
        assert_eq!(outcome.pinned, Some(PinReason::Listen));
        assert!(!relay.on_server(&ready(TxStatus::Idle)).release);
    }

    #[test]
    fn a_session_is_reported_as_pinned_exactly_once() {
        // The value is a metric increment. Reporting it per statement would
        // make pgprox_pin_total count statements rather than sessions.
        let mut relay = Relay::new();
        let first = relay.on_client(&query("LISTEN c"), &replicas(), Instant::now());
        let second = relay.on_client(&query("LISTEN d"), &replicas(), Instant::now());

        assert_eq!(first.pinned, Some(PinReason::Listen));
        assert_eq!(second.pinned, None);
        assert_eq!(relay.pin_reason(), Some(PinReason::Listen));
    }

    #[test]
    fn a_copy_holds_the_connection_and_gives_it_back_at_the_end() {
        // The distinction M6.5 removed from the pool: a COPY holds while it
        // runs and releases when it ends. A pin would keep the connection for
        // the session's life.
        let mut relay = Relay::new();
        relay.on_client(&query("COPY t FROM STDIN"), &replicas(), Instant::now());
        relay.acquired();

        assert!(!relay.on_server(&BackendMessage::CopyInResponse).release);
        assert_eq!(relay.pin_reason(), None, "a COPY pinned the session");

        relay.on_client(&FrontendMessage::CopyDone, &replicas(), Instant::now());
        assert!(relay.on_server(&ready(TxStatus::Idle)).release);
    }

    #[test]
    fn a_route_hint_is_answered_here_rather_than_forwarded() {
        // Postgres would reject SET pgprox.route as an unknown parameter, so
        // forwarding it would turn a supported feature into an error.
        let mut relay = Relay::new();
        let outcome = relay.on_client(
            &query("SET pgprox.route = 'replica'"),
            &replicas(),
            Instant::now(),
        );

        assert_eq!(outcome.action, ClientAction::Answer(Routed::HintAccepted));
        assert!(
            !relay.is_holding(),
            "a statement the server never sees acquired a connection"
        );
    }

    #[test]
    fn a_typo_in_a_route_hint_is_reported_rather_than_ignored() {
        let mut relay = Relay::new();
        let outcome = relay.on_client(
            &query("SET pgprox.route = 'replicas'"),
            &replicas(),
            Instant::now(),
        );
        assert_eq!(outcome.action, ClientAction::Answer(Routed::HintRejected));
    }

    #[test]
    fn a_second_statement_on_a_held_connection_does_not_acquire_again() {
        let mut relay = Relay::new();
        round_trip(&mut relay, "BEGIN", TxStatus::InTransaction);

        let outcome = relay.on_client(&query("SELECT 1"), &replicas(), Instant::now());
        assert_eq!(
            outcome.action,
            ClientAction::Send {
                target: RouteTarget::Primary,
                acquire: false,
            }
        );
    }

    #[test]
    fn terminate_ends_the_session_without_touching_the_upstream() {
        let mut relay = Relay::new();
        round_trip(&mut relay, "SELECT 1", TxStatus::Idle);
        let outcome = relay.on_client(&FrontendMessage::Terminate, &replicas(), Instant::now());
        assert_eq!(outcome.action, ClientAction::Close);
    }

    #[test]
    fn a_ready_for_query_on_a_session_holding_nothing_releases_nothing() {
        // Reachable from a shell that lost track of its own state. Releasing
        // here would return a connection twice, which is worse than the bug
        // that got here.
        let mut relay = Relay::new();
        assert!(!relay.on_server(&ready(TxStatus::Idle)).release);
    }
}
