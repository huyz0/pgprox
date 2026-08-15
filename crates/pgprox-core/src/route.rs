//! Routing contract.
//!
//! # The rule
//!
//! When the classifier is not confident, route to the primary. A false negative
//! costs a little throughput. A false positive is a stale read, which is a data
//! correctness bug from the tenant's perspective and worse than the slowness it
//! was meant to fix.

use std::fmt;
use std::sync::Arc;

use crate::ids::Lsn;

/// What a statement does, as far as the classifier can tell.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[non_exhaustive]
pub enum StmtClass {
    /// Provably reads nothing but committed data and takes no locks.
    ReadOnly,
    /// Writes, takes locks, or calls something volatile.
    Write,
    /// Could not be classified with confidence. The default, so a new
    /// construct the classifier has not learned yet is treated as a write.
    #[default]
    Unknown,
}

impl StmtClass {
    /// Whether a statement of this class may go to a replica.
    ///
    /// Only [`StmtClass::ReadOnly`] may. [`StmtClass::Unknown`] may not, which
    /// is the whole point of having three classes rather than two.
    #[must_use]
    pub const fn replica_eligible(self) -> bool {
        matches!(self, Self::ReadOnly)
    }
}

/// An explicit routing instruction from the client.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[non_exhaustive]
pub enum RouteHint {
    /// Decide from the statement class.
    #[default]
    Auto,
    /// Force the primary.
    Primary,
    /// Prefer a replica, if one is eligible. Never overrides consistency:
    /// a replica behind the session watermark is still refused.
    Replica,
}

/// Where a statement should go.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum RouteTarget {
    /// The primary.
    Primary,
    /// The replica at this index in the grant's replica list.
    Replica(usize),
}

/// What a replica poller last observed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReplicaState {
    /// How far this replica has replayed.
    pub replayed: Lsn,
    /// Whether it answered the last poll and is still in recovery.
    pub healthy: bool,
}

impl ReplicaState {
    /// Whether this replica can serve a read at `watermark`.
    ///
    /// A session with no watermark has never written, so any healthy replica
    /// will do.
    #[must_use]
    pub fn can_serve(&self, watermark: Option<Lsn>) -> bool {
        self.healthy && watermark.is_none_or(|floor| self.replayed >= floor)
    }
}

/// Everything the router needs to decide.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RouteCtx {
    /// What the statement does.
    pub class: StmtClass,
    /// The session's write floor: a replica must have replayed at least this
    /// far. `None` means the session has never written.
    pub watermark: Option<Lsn>,
    /// Whether a transaction is already open, in which case the target was
    /// fixed at its first statement.
    pub in_transaction: bool,
    /// Whether the session is pinned to an upstream connection.
    pub pinned: bool,
    /// Whether an earlier write in this session committed but its LSN is not
    /// yet known.
    ///
    /// True between a write committing and the caller learning where it
    /// landed (a probe that can itself fail). `watermark` cannot be trusted
    /// to reflect that write until then, so this must fix the primary the
    /// same way `pinned` does rather than let a stale or absent watermark
    /// wave a replica through.
    pub wrote: bool,
    /// An explicit instruction from the client.
    pub hint: RouteHint,
}

/// Chooses where a statement goes.
pub trait Router: Send + Sync + fmt::Debug {
    /// Picks a target.
    ///
    /// Implementations must return [`RouteTarget::Primary`] whenever they
    /// cannot prove a replica is safe.
    fn route(&self, ctx: &RouteCtx, replicas: &[ReplicaState]) -> RouteTarget;
}

impl<T: Router + ?Sized> Router for Arc<T> {
    fn route(&self, ctx: &RouteCtx, replicas: &[ReplicaState]) -> RouteTarget {
        (**self).route(ctx, replicas)
    }
}

/// The routing rule, as a pure function.
///
/// Lives here rather than in `pgprox-route` so the real router and every fake
/// share one implementation. Two implementations of a consistency rule is two
/// chances to get it wrong.
#[must_use]
pub fn decide(ctx: &RouteCtx, replicas: &[ReplicaState]) -> RouteTarget {
    // A pinned session, an open transaction, an unconfirmed earlier write, or
    // an explicit Primary hint all fix the answer regardless of what the
    // statement does.
    if ctx.pinned || ctx.in_transaction || ctx.wrote || ctx.hint == RouteHint::Primary {
        return RouteTarget::Primary;
    }

    // A hint asks for a replica; it does not assert the statement is safe on
    // one. Consistency still decides.
    let eligible = match ctx.hint {
        RouteHint::Replica => ctx.class != StmtClass::Write,
        _ => ctx.class.replica_eligible(),
    };
    if !eligible {
        return RouteTarget::Primary;
    }

    replicas
        .iter()
        .position(|replica| replica.can_serve(ctx.watermark))
        .map_or(RouteTarget::Primary, RouteTarget::Replica)
}

#[cfg(any(test, feature = "test-fakes"))]
pub use fake::FakeRouter;

#[cfg(any(test, feature = "test-fakes"))]
mod fake {
    use super::{Arc, ReplicaState, RouteCtx, RouteTarget, Router, decide};

    /// A [`Router`] applying the real rule.
    ///
    /// Deliberately not configurable to return arbitrary answers. A fake router
    /// that could be told to send a write to a replica would let a caller's
    /// tests pass against behaviour the real router will never produce.
    #[derive(Debug, Default, Clone, Copy)]
    pub struct FakeRouter;

    impl FakeRouter {
        /// Builds one.
        #[must_use]
        pub fn new() -> Arc<Self> {
            Arc::new(Self)
        }
    }

    impl Router for FakeRouter {
        fn route(&self, ctx: &RouteCtx, replicas: &[ReplicaState]) -> RouteTarget {
            decide(ctx, replicas)
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn replica(lsn: u64) -> ReplicaState {
        ReplicaState {
            replayed: Lsn::new(lsn),
            healthy: true,
        }
    }

    fn read_only() -> RouteCtx {
        RouteCtx {
            class: StmtClass::ReadOnly,
            ..RouteCtx::default()
        }
    }

    #[test]
    fn an_unknown_statement_goes_to_the_primary() {
        // The default class is Unknown, so a construct the classifier has not
        // learned yet is treated as a write rather than guessed at.
        assert_eq!(StmtClass::default(), StmtClass::Unknown);
        assert!(!StmtClass::Unknown.replica_eligible());
        assert_eq!(
            decide(&RouteCtx::default(), &[replica(100)]),
            RouteTarget::Primary
        );
    }

    #[test]
    fn a_write_goes_to_the_primary() {
        let ctx = RouteCtx {
            class: StmtClass::Write,
            ..RouteCtx::default()
        };
        assert_eq!(decide(&ctx, &[replica(100)]), RouteTarget::Primary);
    }

    #[test]
    fn a_read_only_statement_may_use_a_replica() {
        assert_eq!(
            decide(&read_only(), &[replica(100)]),
            RouteTarget::Replica(0)
        );
    }

    #[test]
    fn a_replica_behind_the_watermark_is_not_eligible() {
        // Read-your-writes. This is the test that stops a session seeing its own
        // write vanish.
        let ctx = RouteCtx {
            watermark: Some(Lsn::new(500)),
            ..read_only()
        };
        assert_eq!(decide(&ctx, &[replica(499)]), RouteTarget::Primary);
        assert_eq!(decide(&ctx, &[replica(500)]), RouteTarget::Replica(0));
        assert_eq!(decide(&ctx, &[replica(501)]), RouteTarget::Replica(0));
    }

    #[test]
    fn the_first_caught_up_replica_is_chosen() {
        let ctx = RouteCtx {
            watermark: Some(Lsn::new(500)),
            ..read_only()
        };
        let replicas = [replica(100), replica(499), replica(600), replica(700)];
        assert_eq!(decide(&ctx, &replicas), RouteTarget::Replica(2));
    }

    #[test]
    fn an_unhealthy_replica_is_skipped_however_far_ahead_it_is() {
        let unhealthy = ReplicaState {
            replayed: Lsn::new(9_999),
            healthy: false,
        };
        assert_eq!(decide(&read_only(), &[unhealthy]), RouteTarget::Primary);
        assert!(!unhealthy.can_serve(None));
    }

    #[test]
    fn a_session_that_has_never_written_accepts_any_healthy_replica() {
        assert!(replica(0).can_serve(None));
        assert_eq!(decide(&read_only(), &[replica(0)]), RouteTarget::Replica(0));
    }

    #[test]
    fn no_replicas_means_the_primary() {
        assert_eq!(decide(&read_only(), &[]), RouteTarget::Primary);
    }

    #[test]
    fn an_open_transaction_stays_on_its_target() {
        // A transaction spanning two servers has no coherent semantics, so the
        // decision made at its first statement is final.
        let ctx = RouteCtx {
            in_transaction: true,
            ..read_only()
        };
        assert_eq!(decide(&ctx, &[replica(100)]), RouteTarget::Primary);
    }

    #[test]
    fn a_pinned_session_stays_on_the_primary() {
        let ctx = RouteCtx {
            pinned: true,
            ..read_only()
        };
        assert_eq!(decide(&ctx, &[replica(100)]), RouteTarget::Primary);
    }

    #[test]
    fn an_unconfirmed_write_stays_on_the_primary_even_with_no_watermark() {
        // `wrote` is exactly the case a watermark cannot express: the write
        // committed but the caller never learned its LSN (the probe that
        // would have set it failed), so `watermark` is still `None` — the
        // same value a session that never wrote anything has. Without this
        // flag the two are indistinguishable and the first one reads its own
        // write off a replica that never saw it.
        let ctx = RouteCtx {
            wrote: true,
            ..read_only()
        };
        assert_eq!(decide(&ctx, &[replica(100)]), RouteTarget::Primary);
    }

    #[test]
    fn a_primary_hint_overrides_a_read_only_statement() {
        let ctx = RouteCtx {
            hint: RouteHint::Primary,
            ..read_only()
        };
        assert_eq!(decide(&ctx, &[replica(100)]), RouteTarget::Primary);
    }

    #[test]
    fn a_replica_hint_never_overrides_consistency() {
        // A hint asks for a replica. It does not assert the read is safe there,
        // and it must not be able to.
        let stale = RouteCtx {
            hint: RouteHint::Replica,
            watermark: Some(Lsn::new(500)),
            ..read_only()
        };
        assert_eq!(decide(&stale, &[replica(499)]), RouteTarget::Primary);

        let write = RouteCtx {
            hint: RouteHint::Replica,
            class: StmtClass::Write,
            ..RouteCtx::default()
        };
        assert_eq!(
            decide(&write, &[replica(9_999)]),
            RouteTarget::Primary,
            "a hint must never send a write to a replica"
        );
    }

    #[test]
    fn a_replica_hint_admits_an_unknown_statement() {
        // The one thing the hint does buy: the client asserting it knows better
        // than the classifier, which is only allowed for Unknown, never Write.
        let ctx = RouteCtx {
            hint: RouteHint::Replica,
            class: StmtClass::Unknown,
            ..RouteCtx::default()
        };
        assert_eq!(decide(&ctx, &[replica(100)]), RouteTarget::Replica(0));
    }

    #[test]
    fn the_fake_applies_the_real_rule() {
        // A fake that could be told to send a write to a replica would let a
        // caller's tests pass against behaviour the real router never produces.
        let router: Arc<dyn Router> = FakeRouter::new();
        assert_eq!(
            router.route(&read_only(), &[replica(100)]),
            RouteTarget::Replica(0)
        );
        let write = RouteCtx {
            class: StmtClass::Write,
            ..RouteCtx::default()
        };
        assert_eq!(router.route(&write, &[replica(100)]), RouteTarget::Primary);
    }
}
