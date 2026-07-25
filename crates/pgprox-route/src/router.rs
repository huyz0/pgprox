//! Where a statement goes, and the session state that decides it.
//!
//! # One decision per transaction
//!
//! The target is chosen at the transaction's first statement and holds until
//! the transaction ends. A transaction spanning two servers has no coherent
//! semantics: its statements would see different snapshots, its locks would be
//! taken in one place and released in another, and a rollback would undo half
//! of it.
//!
//! So [`SessionRouter`] is a state machine, not a function. The decision it
//! makes for the first statement is the one it repeats, and the classifier's
//! opinion of later statements is not consulted. A `SELECT` inside a write
//! transaction stays on the primary.
//!
//! # Where the rule lives
//!
//! Not here. [`pgprox_core::route::decide`] is the routing rule, shared by this
//! router and every fake, so there is exactly one implementation of a
//! consistency decision. This module supplies its inputs and remembers its
//! answer.

use std::time::Instant;

use pgprox_core::ids::Lsn;
use pgprox_core::route::{ReplicaState, RouteCtx, RouteHint, RouteTarget, StmtClass, decide};

use crate::classify::{begins_read_only_transaction, classify};
use crate::hints::{RouteAssignment, parse_route_assignment, statement_hint};
use crate::replica::{Replicas, Watermark};

/// What happened to a statement offered to the router.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Routed {
    /// Send it to this target.
    To(RouteTarget),
    /// It was a `SET pgprox.route`, consumed here rather than forwarded.
    ///
    /// The caller answers the client itself, since the server never sees it.
    HintAccepted,
    /// It was a `SET pgprox.route` with a value that meant nothing.
    ///
    /// The caller reports this to the client rather than ignoring it, so a
    /// typo does not leave them believing their reads are on replicas.
    HintRejected,
}

/// Routing state for one client session.
///
/// Holds the session-scoped hint, the read-your-writes floor, and the target
/// fixed by an open transaction.
#[derive(Clone, Debug, Default)]
pub struct SessionRouter {
    hint: RouteHint,
    watermark: Watermark,
    /// The target this transaction was pinned to at its first statement.
    fixed: Option<RouteTarget>,
    /// Whether the open transaction promised the server it would not write.
    read_only_transaction: bool,
    pinned: bool,
}

impl SessionRouter {
    /// A fresh session: no hint, no writes yet, no transaction open.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The session-scoped hint currently in force.
    #[must_use]
    pub const fn hint(&self) -> RouteHint {
        self.hint
    }

    /// The read-your-writes floor.
    #[must_use]
    pub const fn watermark(&self) -> Watermark {
        self.watermark
    }

    /// The target this transaction is fixed to, if one is open.
    #[must_use]
    pub const fn fixed_target(&self) -> Option<RouteTarget> {
        self.fixed
    }

    /// Whether the session is pinned to one upstream connection.
    #[must_use]
    pub const fn is_pinned(&self) -> bool {
        self.pinned
    }

    /// Records that the session has been pinned, which forces the primary.
    ///
    /// Pinning is the pool's decision, not this module's. Once it happens the
    /// session is bound to one upstream connection, and that connection is on
    /// the primary.
    pub const fn set_pinned(&mut self, pinned: bool) {
        self.pinned = pinned;
    }

    /// Records where a write committed, so later reads see it.
    pub fn record_write(&mut self, lsn: Lsn) {
        self.watermark.advance(lsn);
    }

    /// Ends the session's routing state, as a new client on a reused
    /// connection needs.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Records that a transaction has ended, freeing the fixed target.
    ///
    /// Called on `ReadyForQuery` with status `I`, the same signal that releases
    /// the upstream connection. The watermark deliberately survives: it belongs
    /// to the session, not to the transaction that set it.
    pub const fn end_transaction(&mut self) {
        self.fixed = None;
        self.read_only_transaction = false;
    }

    /// Routes a statement.
    ///
    /// `in_transaction` is the transaction status the protocol layer already
    /// tracks, passed in rather than inferred from SQL text: `BEGIN` is not the
    /// only way a transaction opens, and the status byte is authoritative where
    /// a text scan is a guess.
    pub fn route(
        &mut self,
        sql: &str,
        in_transaction: bool,
        replicas: &Replicas,
        now: Instant,
    ) -> Routed {
        // A session-scoped hint is a statement about the session, not one to be
        // routed. Handled first so it never reaches the server.
        if let Some(assignment) = parse_route_assignment(sql) {
            return match assignment {
                RouteAssignment::Set(hint) => {
                    self.hint = hint;
                    Routed::HintAccepted
                }
                RouteAssignment::Reset => {
                    self.hint = RouteHint::Auto;
                    Routed::HintAccepted
                }
                RouteAssignment::Invalid => Routed::HintRejected,
            };
        }

        // An open transaction goes where its first statement went, whatever
        // this statement is.
        if let Some(target) = self.fixed {
            return Routed::To(target);
        }

        if begins_read_only_transaction(sql) {
            // The session has told the server to refuse writes for the whole
            // transaction, which is a stronger promise than the classifier can
            // make about any single statement.
            self.read_only_transaction = true;
        }

        let class = if self.read_only_transaction {
            StmtClass::ReadOnly
        } else {
            classify(sql)
        };

        let ctx = RouteCtx {
            class,
            watermark: self.watermark.get(),
            // Deliberately false: this call *is* the first statement, so the
            // decision is being made now rather than inherited. The fixing
            // happens below.
            in_transaction: false,
            pinned: self.pinned,
            // A per-statement comment outranks the session setting, since it is
            // the more specific of the two.
            hint: statement_hint(sql).unwrap_or(self.hint),
        };

        let target = decide(&ctx, &replicas.states(now));

        // Fix the target if this statement opened a transaction, or if one was
        // already open by the time the status byte said so.
        if in_transaction || begins_transaction(sql) {
            self.fixed = Some(target);
        }

        Routed::To(target)
    }
}

/// Whether a statement opens a transaction.
///
/// Only the explicit forms. An implicit single-statement transaction ends at
/// the same `ReadyForQuery` that would release the connection, so there is
/// nothing to fix a target for.
fn begins_transaction(sql: &str) -> bool {
    let mut words = sql.split_whitespace();
    matches!(
        words.next().map(str::to_ascii_lowercase).as_deref(),
        Some("begin" | "start")
    )
}

/// A [`pgprox_core::route::Router`] over the shared rule, for callers that
/// route one statement at a time with no session state of their own.
///
/// Deliberately not configurable to return arbitrary answers, for the same
/// reason `FakeRouter` is not: a router that could be told to send a write to a
/// replica would let a caller's tests pass against behaviour the real one never
/// produces.
#[derive(Debug, Default, Clone, Copy)]
pub struct StatelessRouter;

impl pgprox_core::route::Router for StatelessRouter {
    fn route(&self, ctx: &RouteCtx, replicas: &[ReplicaState]) -> RouteTarget {
        decide(ctx, replicas)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::replica::ReplicaConfig;

    /// One replica, caught up and healthy.
    fn replicas(replayed: u64) -> (Replicas, Instant) {
        let now = Instant::now();
        let mut replicas = Replicas::new(1, ReplicaConfig::default());
        replicas.observe(0, Lsn::new(replayed), true, now);
        (replicas, now)
    }

    fn route(router: &mut SessionRouter, sql: &str, in_transaction: bool) -> Routed {
        let (replicas, now) = replicas(1_000);
        router.route(sql, in_transaction, &replicas, now)
    }

    #[test]
    fn a_read_goes_to_a_replica_and_a_write_to_the_primary() {
        let mut router = SessionRouter::new();
        assert_eq!(
            route(&mut router, "SELECT * FROM t", false),
            Routed::To(RouteTarget::Replica(0))
        );
        assert_eq!(
            route(&mut router, "DELETE FROM t", false),
            Routed::To(RouteTarget::Primary)
        );
    }

    #[test]
    fn a_transaction_keeps_the_target_its_first_statement_chose() {
        // A transaction spanning two servers would see different snapshots and
        // take locks in one place while releasing them in another.
        let mut router = SessionRouter::new();
        assert_eq!(
            route(&mut router, "BEGIN", false),
            Routed::To(RouteTarget::Primary),
            "BEGIN is not a read, so it opens on the primary"
        );
        assert_eq!(router.fixed_target(), Some(RouteTarget::Primary));

        // A plain SELECT inside it stays put, even though on its own it would
        // reach a replica.
        assert_eq!(
            route(&mut router, "SELECT * FROM t", true),
            Routed::To(RouteTarget::Primary),
            "a read inside a write transaction escaped to a replica"
        );

        router.end_transaction();
        assert_eq!(router.fixed_target(), None);
        assert_eq!(
            route(&mut router, "SELECT * FROM t", false),
            Routed::To(RouteTarget::Replica(0)),
            "the target stayed fixed after the transaction ended"
        );
    }

    #[test]
    fn a_read_only_transaction_reaches_a_replica_and_holds_there() {
        // The session has told the server to refuse writes for the whole
        // transaction, which is a stronger promise than the classifier can make
        // about any one statement.
        let mut router = SessionRouter::new();
        assert_eq!(
            route(&mut router, "BEGIN READ ONLY", false),
            Routed::To(RouteTarget::Replica(0))
        );
        assert_eq!(router.fixed_target(), Some(RouteTarget::Replica(0)));
        assert_eq!(
            route(&mut router, "SELECT * FROM t", true),
            Routed::To(RouteTarget::Replica(0))
        );
    }

    #[test]
    fn a_read_only_transaction_admits_a_statement_the_classifier_would_not() {
        // The promise is to the server, which will reject a write outright, so
        // it covers constructs the lexical scan cannot vouch for.
        let mut router = SessionRouter::new();
        route(&mut router, "START TRANSACTION READ ONLY", false);
        router.end_transaction();
        assert_eq!(router.fixed_target(), None);

        let mut router = SessionRouter::new();
        assert_eq!(
            route(&mut router, "BEGIN READ ONLY", false),
            Routed::To(RouteTarget::Replica(0))
        );
        assert_eq!(
            route(&mut router, "SELECT pg_catalog.nextval('s')", true),
            Routed::To(RouteTarget::Replica(0)),
            "the transaction-wide promise was not honoured"
        );
    }

    #[test]
    fn a_session_hint_is_consumed_rather_than_forwarded() {
        // The server never sees it. Forwarding it would store a value nothing
        // reads, in a second place that can disagree with this one.
        let mut router = SessionRouter::new();
        assert_eq!(
            route(&mut router, "SET pgprox.route = 'primary'", false),
            Routed::HintAccepted
        );
        assert_eq!(router.hint(), RouteHint::Primary);
        assert_eq!(
            route(&mut router, "SELECT * FROM t", false),
            Routed::To(RouteTarget::Primary),
            "the session hint was not applied"
        );

        assert_eq!(
            route(&mut router, "RESET pgprox.route", false),
            Routed::HintAccepted
        );
        assert_eq!(router.hint(), RouteHint::Auto);
        assert_eq!(
            route(&mut router, "SELECT * FROM t", false),
            Routed::To(RouteTarget::Replica(0))
        );
    }

    #[test]
    fn a_bad_hint_value_is_rejected_rather_than_swallowed() {
        let mut router = SessionRouter::new();
        assert_eq!(
            route(&mut router, "SET pgprox.route = 'relpica'", false),
            Routed::HintRejected
        );
        assert_eq!(
            router.hint(),
            RouteHint::Auto,
            "a rejected hint changed the session anyway"
        );
    }

    #[test]
    fn a_statement_comment_outranks_the_session_setting() {
        // The more specific of the two wins, which is what lets an ORM override
        // a connection-wide default for one query.
        let mut router = SessionRouter::new();
        route(&mut router, "SET pgprox.route = 'primary'", false);
        assert_eq!(
            route(&mut router, "/* pgprox:replica */ SELECT * FROM t", false),
            Routed::To(RouteTarget::Replica(0))
        );
        assert_eq!(
            router.hint(),
            RouteHint::Primary,
            "a per-statement comment changed the session"
        );
    }

    #[test]
    fn a_hint_still_cannot_send_a_write_to_a_replica() {
        let mut router = SessionRouter::new();
        route(&mut router, "SET pgprox.route = 'replica'", false);
        assert_eq!(
            route(&mut router, "DELETE FROM t", false),
            Routed::To(RouteTarget::Primary)
        );
        assert_eq!(
            route(
                &mut router,
                "/* pgprox:replica */ UPDATE t SET a = 1",
                false
            ),
            Routed::To(RouteTarget::Primary)
        );
    }

    #[test]
    fn a_write_keeps_later_reads_off_replicas_that_have_not_caught_up() {
        // Read-your-writes across statements, which is what the watermark is
        // for and why it outlives the transaction that set it.
        let now = Instant::now();
        let mut lagging = Replicas::new(1, ReplicaConfig::default());
        lagging.observe(0, Lsn::new(499), true, now);

        let mut router = SessionRouter::new();
        router.record_write(Lsn::new(500));
        assert_eq!(
            router.route("SELECT * FROM t", false, &lagging, now),
            Routed::To(RouteTarget::Primary),
            "a session read from a replica behind its own write"
        );

        lagging.observe(0, Lsn::new(500), true, now);
        assert_eq!(
            router.route("SELECT * FROM t", false, &lagging, now),
            Routed::To(RouteTarget::Replica(0))
        );
    }

    #[test]
    fn the_watermark_survives_the_transaction_that_set_it() {
        let mut router = SessionRouter::new();
        router.record_write(Lsn::new(500));
        router.end_transaction();
        assert_eq!(
            router.watermark().get(),
            Some(Lsn::new(500)),
            "the floor was cleared with the transaction"
        );
    }

    #[test]
    fn a_pinned_session_stays_on_the_primary() {
        let mut router = SessionRouter::new();
        router.set_pinned(true);
        assert!(router.is_pinned());
        assert_eq!(
            route(&mut router, "SELECT * FROM t", false),
            Routed::To(RouteTarget::Primary)
        );

        // Even with a hint asking otherwise.
        route(&mut router, "SET pgprox.route = 'replica'", false);
        assert_eq!(
            route(&mut router, "SELECT * FROM t", false),
            Routed::To(RouteTarget::Primary)
        );
    }

    #[test]
    fn no_healthy_replica_means_the_primary() {
        let now = Instant::now();
        let empty = Replicas::new(0, ReplicaConfig::default());
        let mut router = SessionRouter::new();
        assert_eq!(
            router.route("SELECT * FROM t", false, &empty, now),
            Routed::To(RouteTarget::Primary)
        );
    }

    #[test]
    fn reset_returns_the_session_to_a_fresh_one() {
        // A pooled client connection handed to a new client must not inherit
        // the last one's watermark or hint.
        let mut router = SessionRouter::new();
        router.record_write(Lsn::new(500));
        route(&mut router, "SET pgprox.route = 'primary'", false);
        router.set_pinned(true);

        router.reset();
        assert_eq!(router.watermark().get(), None);
        assert_eq!(router.hint(), RouteHint::Auto);
        assert!(!router.is_pinned());
        assert_eq!(router.fixed_target(), None);
    }

    #[test]
    fn a_transaction_opened_without_begin_still_fixes_its_target() {
        // The status byte is authoritative. A transaction can open in ways a
        // text scan does not see, and the first statement routed under it is
        // still the one that decides.
        let mut router = SessionRouter::new();
        assert_eq!(
            route(&mut router, "SELECT * FROM t", true),
            Routed::To(RouteTarget::Replica(0))
        );
        assert_eq!(router.fixed_target(), Some(RouteTarget::Replica(0)));
        assert_eq!(
            route(&mut router, "SELECT * FROM other", true),
            Routed::To(RouteTarget::Replica(0))
        );
    }

    #[test]
    fn a_single_statement_outside_a_transaction_fixes_nothing() {
        // Its implicit transaction ends at the same ReadyForQuery that releases
        // the connection, so there is nothing to remember.
        let mut router = SessionRouter::new();
        route(&mut router, "SELECT * FROM t", false);
        assert_eq!(router.fixed_target(), None);
    }

    #[test]
    fn the_stateless_router_applies_the_shared_rule() {
        use pgprox_core::route::Router as _;

        let now = Instant::now();
        let (replicas, _) = replicas(1_000);
        let states = replicas.states(now);

        let router = StatelessRouter;
        let read = RouteCtx {
            class: StmtClass::ReadOnly,
            ..RouteCtx::default()
        };
        assert_eq!(router.route(&read, &states), RouteTarget::Replica(0));

        let write = RouteCtx {
            class: StmtClass::Write,
            ..RouteCtx::default()
        };
        assert_eq!(router.route(&write, &states), RouteTarget::Primary);
    }
}
