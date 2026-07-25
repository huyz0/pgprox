//! Whether to close an idle client so it reconnects elsewhere.
//!
//! ADR 0005 chose shedding as a supplement to reservations, not a replacement:
//! a client on a non-home node may be closed with SQLSTATE `57P01` so its
//! driver reconnects cleanly and gets another roll of the load balancer.
//!
//! # Shedding is the dangerous half of that ADR
//!
//! Reservations cost nothing when wrong: a node multiplexes harder. Shedding
//! closes a working connection. Get it wrong and the proxy generates churn,
//! which is worse than the fan-out it was trying to reduce.
//!
//! Everything here is therefore a reason **not** to shed. The default answer is
//! no, and each guard rail is a separate way of saying no, so a failure to
//! evaluate one cannot turn into a shed.

use std::time::Duration;

/// How shedding is tuned.
#[derive(Clone, Copy, Debug)]
pub struct ShedConfig {
    /// Master switch. False disables shedding entirely.
    pub enabled: bool,
    /// How long a client must have been idle before it may be shed.
    pub idle_threshold: Duration,
    /// How long after a membership change shedding stays paused.
    ///
    /// Rendezvous hashing rehomes tenants when membership moves, so shedding
    /// immediately would act on placement that is about to change again.
    pub settle_window: Duration,
    /// Most sheds per tenant per minute.
    pub max_per_tenant_per_minute: u32,
}

impl Default for ShedConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            idle_threshold: Duration::from_secs(30),
            settle_window: Duration::from_secs(30),
            max_per_tenant_per_minute: 60,
        }
    }
}

/// Everything the decision needs.
///
/// Five booleans, which clippy dislikes on principle. Grouping them into
/// bitflags or an enum would make the call site say less, not more: each is an
/// independent fact about a different subsystem, and the guard rails read as
/// prose precisely because they are named. Kept flat deliberately.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug)]
pub struct ShedCtx {
    /// How long this client has been idle at `ReadyForQuery('I')`.
    pub idle_for: Duration,
    /// Whether this node is the tenant's home.
    pub on_home_node: bool,
    /// Whether the tenant's home node has room for another connection.
    pub home_has_headroom: bool,
    /// Whether the home node is draining.
    pub home_draining: bool,
    /// Whether the session is pinned to an upstream connection.
    pub pinned: bool,
    /// Whether a transaction is open.
    pub in_transaction: bool,
    /// How long since membership last changed.
    pub since_membership_change: Duration,
    /// Sheds already performed for this tenant in the last minute.
    pub recent_sheds: u32,
}

/// Why a client was not shed.
///
/// Named rather than a bare `false`, so `pgprox_shed_total{reason}` and an
/// operator asking "why is this not rebalancing" get an answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum ShedRefusal {
    /// Shedding is switched off.
    Disabled,
    /// The session is mid-transaction.
    InTransaction,
    /// The session is pinned to its connection.
    Pinned,
    /// This node is the tenant's home, so moving the client achieves nothing.
    AlreadyHome,
    /// The client has not been idle long enough.
    NotIdleEnough,
    /// The home node has no room, so the client would come back here.
    NoHeadroomAtHome,
    /// The home node is draining, so sending clients there is backwards.
    HomeDraining,
    /// Membership changed recently and placement is still settling.
    Settling,
    /// This tenant has been shed too often lately.
    RateLimited,
}

/// The decision.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShedDecision {
    /// Close the client with `57P01`.
    Shed,
    /// Leave it alone, for this reason.
    Keep(ShedRefusal),
}

impl ShedDecision {
    /// Whether this decision means shedding.
    #[must_use]
    pub const fn is_shed(self) -> bool {
        matches!(self, Self::Shed)
    }
}

/// Decides whether to shed one client.
///
/// Guard rails are evaluated cheapest and most absolute first, so the reported
/// reason is the most fundamental one rather than whichever happened to be
/// checked last.
#[must_use]
pub fn decide(config: &ShedConfig, ctx: &ShedCtx) -> ShedDecision {
    use ShedRefusal as R;

    if !config.enabled {
        return ShedDecision::Keep(R::Disabled);
    }
    // Correctness before optimisation: closing these breaks a working session
    // rather than merely relocating it.
    if ctx.in_transaction {
        return ShedDecision::Keep(R::InTransaction);
    }
    if ctx.pinned {
        return ShedDecision::Keep(R::Pinned);
    }
    // Moving a client that is already home achieves nothing and costs a
    // reconnect.
    if ctx.on_home_node {
        return ShedDecision::Keep(R::AlreadyHome);
    }
    if ctx.idle_for < config.idle_threshold {
        return ShedDecision::Keep(R::NotIdleEnough);
    }
    // Sending clients to a draining node is the opposite of what drain means.
    if ctx.home_draining {
        return ShedDecision::Keep(R::HomeDraining);
    }
    if !ctx.home_has_headroom {
        return ShedDecision::Keep(R::NoHeadroomAtHome);
    }
    if ctx.since_membership_change < config.settle_window {
        return ShedDecision::Keep(R::Settling);
    }
    if ctx.recent_sheds >= config.max_per_tenant_per_minute {
        return ShedDecision::Keep(R::RateLimited);
    }

    ShedDecision::Shed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A context where every guard rail permits shedding.
    ///
    /// Each test then breaks exactly one, so a failure names the guard rail
    /// that stopped working.
    fn shedable() -> ShedCtx {
        ShedCtx {
            idle_for: Duration::from_secs(60),
            on_home_node: false,
            home_has_headroom: true,
            home_draining: false,
            pinned: false,
            in_transaction: false,
            since_membership_change: Duration::from_secs(300),
            recent_sheds: 0,
        }
    }

    #[test]
    fn a_clearly_shedable_client_is_shed() {
        // Without this the suite below would pass on a function that always
        // refuses.
        assert_eq!(
            decide(&ShedConfig::default(), &shedable()),
            ShedDecision::Shed
        );
        assert!(decide(&ShedConfig::default(), &shedable()).is_shed());
    }

    #[test]
    fn every_guard_rail_refuses_a_shed_that_would_otherwise_happen() {
        // One table, one break each, so a guard rail that stops working is
        // named rather than merely making some test red.
        /// A named way of breaking exactly one guard rail.
        type Case = (&'static str, fn(&mut ShedCtx), ShedRefusal);

        let cases: &[Case] = &[
            (
                "in a transaction",
                |c| c.in_transaction = true,
                ShedRefusal::InTransaction,
            ),
            ("pinned", |c| c.pinned = true, ShedRefusal::Pinned),
            (
                "already home",
                |c| c.on_home_node = true,
                ShedRefusal::AlreadyHome,
            ),
            (
                "not idle enough",
                |c| c.idle_for = Duration::from_secs(1),
                ShedRefusal::NotIdleEnough,
            ),
            (
                "home draining",
                |c| c.home_draining = true,
                ShedRefusal::HomeDraining,
            ),
            (
                "no headroom at home",
                |c| c.home_has_headroom = false,
                ShedRefusal::NoHeadroomAtHome,
            ),
            (
                "still settling",
                |c| c.since_membership_change = Duration::from_secs(1),
                ShedRefusal::Settling,
            ),
            (
                "rate limited",
                |c| c.recent_sheds = 60,
                ShedRefusal::RateLimited,
            ),
        ];

        for (name, break_it, expected) in cases {
            let mut ctx = shedable();
            break_it(&mut ctx);
            assert_eq!(
                decide(&ShedConfig::default(), &ctx),
                ShedDecision::Keep(*expected),
                "{name} did not prevent a shed"
            );
        }
    }

    #[test]
    fn the_kill_switch_overrides_everything() {
        let config = ShedConfig {
            enabled: false,
            ..ShedConfig::default()
        };
        assert_eq!(
            decide(&config, &shedable()),
            ShedDecision::Keep(ShedRefusal::Disabled)
        );
    }

    #[test]
    fn correctness_guards_are_reported_before_optimisation_ones() {
        // A client both mid-transaction and on its home node is refused for
        // being mid-transaction. Reporting the weaker reason would suggest the
        // stronger one does not apply.
        let mut ctx = shedable();
        ctx.in_transaction = true;
        ctx.on_home_node = true;
        ctx.idle_for = Duration::from_secs(0);

        assert_eq!(
            decide(&ShedConfig::default(), &ctx),
            ShedDecision::Keep(ShedRefusal::InTransaction)
        );
    }

    #[test]
    fn a_draining_home_node_is_reported_before_its_headroom() {
        // A draining node may well report headroom, since it is shedding its
        // own clients. Sending more there is backwards, and the reason should
        // say so rather than blaming capacity.
        let mut ctx = shedable();
        ctx.home_draining = true;
        ctx.home_has_headroom = true;

        assert_eq!(
            decide(&ShedConfig::default(), &ctx),
            ShedDecision::Keep(ShedRefusal::HomeDraining)
        );
    }

    #[test]
    fn the_idle_threshold_is_exclusive_at_the_boundary() {
        let config = ShedConfig::default();
        let mut ctx = shedable();

        ctx.idle_for = config.idle_threshold;
        assert!(decide(&config, &ctx).is_shed(), "exactly at the threshold");

        ctx.idle_for = config
            .idle_threshold
            .checked_sub(Duration::from_millis(1))
            .unwrap_or_default();
        assert!(!decide(&config, &ctx).is_shed(), "one millisecond short");
    }

    #[test]
    fn the_rate_limit_is_inclusive_at_the_boundary() {
        // At the limit means the limit is reached, not that one more is
        // allowed. An off-by-one here doubles the churn budget.
        let config = ShedConfig::default();
        let mut ctx = shedable();

        ctx.recent_sheds = config.max_per_tenant_per_minute - 1;
        assert!(decide(&config, &ctx).is_shed(), "one below the limit");

        ctx.recent_sheds = config.max_per_tenant_per_minute;
        assert!(!decide(&config, &ctx).is_shed(), "at the limit");
    }

    #[test]
    fn a_rate_limit_of_zero_prevents_all_shedding() {
        // A second kill switch, reachable by configuration rather than by the
        // enabled flag.
        let config = ShedConfig {
            max_per_tenant_per_minute: 0,
            ..ShedConfig::default()
        };
        assert_eq!(
            decide(&config, &shedable()),
            ShedDecision::Keep(ShedRefusal::RateLimited)
        );
    }

    #[test]
    fn the_settle_window_is_exclusive_at_the_boundary() {
        let config = ShedConfig::default();
        let mut ctx = shedable();

        ctx.since_membership_change = config.settle_window;
        assert!(decide(&config, &ctx).is_shed(), "exactly at the window");

        ctx.since_membership_change = Duration::ZERO;
        assert!(!decide(&config, &ctx).is_shed(), "membership just changed");
    }

    #[test]
    fn defaults_are_conservative() {
        // Shedding closes working connections. The defaults should make that
        // rare rather than eager.
        let config = ShedConfig::default();
        assert!(config.idle_threshold >= Duration::from_secs(10));
        assert!(config.settle_window >= Duration::from_secs(10));
        assert!(config.max_per_tenant_per_minute <= 120);
        assert!(
            config.enabled,
            "shedding off by default would be a surprise"
        );
    }

    #[test]
    fn every_refusal_reason_is_reachable() {
        // A reason nobody can produce is documentation pretending to be code,
        // and it would show up as a metric label that never appears.
        let all = [
            ShedRefusal::Disabled,
            ShedRefusal::InTransaction,
            ShedRefusal::Pinned,
            ShedRefusal::AlreadyHome,
            ShedRefusal::NotIdleEnough,
            ShedRefusal::NoHeadroomAtHome,
            ShedRefusal::HomeDraining,
            ShedRefusal::Settling,
            ShedRefusal::RateLimited,
        ];

        let mut produced = Vec::new();
        for (config, ctx) in shed_scenarios() {
            if let ShedDecision::Keep(reason) = decide(&config, &ctx) {
                produced.push(reason);
            }
        }
        for reason in all {
            assert!(
                produced.contains(&reason),
                "{reason:?} cannot be produced by any input"
            );
        }
    }

    /// One scenario per refusal reason.
    fn shed_scenarios() -> Vec<(ShedConfig, ShedCtx)> {
        let base = ShedConfig::default();
        let mut out = Vec::new();

        out.push((
            ShedConfig {
                enabled: false,
                ..base
            },
            shedable(),
        ));
        for break_it in [
            (|c: &mut ShedCtx| c.in_transaction = true) as fn(&mut ShedCtx),
            |c: &mut ShedCtx| c.pinned = true,
            |c: &mut ShedCtx| c.on_home_node = true,
            |c: &mut ShedCtx| c.idle_for = Duration::ZERO,
            |c: &mut ShedCtx| c.home_has_headroom = false,
            |c: &mut ShedCtx| c.home_draining = true,
            |c: &mut ShedCtx| c.since_membership_change = Duration::ZERO,
            |c: &mut ShedCtx| c.recent_sheds = 1_000,
        ] {
            let mut ctx = shedable();
            break_it(&mut ctx);
            out.push((base, ctx));
        }
        out
    }
}
