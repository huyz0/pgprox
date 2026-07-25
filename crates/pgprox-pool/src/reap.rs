//! Closing idle upstream connections.
//!
//! # Why this is aggressive rather than conservative
//!
//! Most pools keep a floor of warm connections because reconnecting is
//! expensive. This one keeps none, and that is the point.
//!
//! A tenant's clients land on whichever node the load balancer picks, so over
//! time every node ends up holding connections for every tenant it has ever
//! seen. With a floor, that state is permanent: five nodes each hold a minimum
//! for five thousand tenants, and the upstream cap is spent on connections
//! nobody is using. Reaping idle connections is what lets that fan-out collapse
//! on its own, so a tenant's connections gather on the nodes actually serving
//! it. See ADR 0005 and the crate's `AGENTS.md`.
//!
//! The cost is a reconnect after a quiet period, paid by the first client to
//! come back. The alternative is paying for every tenant on every node forever.
//!
//! # What is never reaped
//!
//! A connection in use. The reaper only ever sees idle ones, and a connection
//! checked out to a session is not in that list. That is a property of where
//! the reaper looks rather than a check it performs, which is the kind of
//! safety worth arranging deliberately.

use std::time::{Duration, Instant};

use pgprox_core::pool::UpstreamId;

use crate::pool::Pool;

/// How the reaper is tuned.
#[derive(Clone, Copy, Debug)]
pub struct ReapConfig {
    /// How long a connection may sit idle before it is closed.
    pub idle_timeout: Duration,
    /// How many connections to keep whatever their idle time.
    ///
    /// Zero, and see the module docs. Present as a field because an operator
    /// with one enormous tenant per node has the opposite problem and should be
    /// able to say so, not because the default is in question.
    pub keep_warm: u32,
    /// The longest a connection may live at all, idle or not.
    ///
    /// Bounds the damage from a connection that has accumulated state nobody
    /// noticed, and gives a rolling restart of the database a way to actually
    /// finish. Zero means no limit.
    pub max_lifetime: Duration,
}

impl Default for ReapConfig {
    fn default() -> Self {
        Self {
            idle_timeout: Duration::from_secs(30),
            keep_warm: 0,
            max_lifetime: Duration::from_secs(3_600),
        }
    }
}

/// Which connections should be closed, and why.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reaping {
    /// Connections to close, oldest idle first.
    pub close: Vec<UpstreamId>,
}

impl Reaping {
    /// Whether anything is to be closed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.close.is_empty()
    }

    /// How many connections are to be closed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.close.len()
    }
}

/// Decides which of a pool's idle connections to close.
///
/// Sans-I/O, like the pool it reads: it names connections, and the caller
/// closes the sockets and tells the pool they are gone.
///
/// `keep_warm` keeps the *most recently* used connections, since those are the
/// ones a returning client is most likely to be served by. Keeping the oldest
/// would hold connections that have already proven nobody wants them.
#[must_use]
pub fn reap(pool: &Pool, config: &ReapConfig, now: Instant) -> Reaping {
    let mut idle: Vec<(Instant, UpstreamId)> = pool
        .idle()
        .filter_map(|connection| {
            connection
                .idle_since()
                .map(|since| (since, connection.id()))
        })
        .collect();

    // Oldest first, ties broken by id so two nodes reaping the same pool state
    // choose the same connections.
    idle.sort_by(|(a_since, a_id), (b_since, b_id)| {
        a_since.cmp(b_since).then_with(|| a_id.cmp(b_id))
    });

    let keep = usize::try_from(config.keep_warm).unwrap_or(usize::MAX);
    let candidates = idle.len().saturating_sub(keep);

    let close = idle
        .into_iter()
        .take(candidates)
        // `saturating_duration_since` because a connection released by a clock
        // that has since jumped backwards reads as fresh, not as impossibly
        // old. Reaping the whole pool on a clock adjustment would be a
        // self-inflicted outage.
        .filter(|(since, _)| now.saturating_duration_since(*since) >= config.idle_timeout)
        .map(|(_, id)| id)
        .collect();

    Reaping { close }
}

/// Whether a connection has outlived `max_lifetime`.
///
/// Separate from [`reap`] because it applies to connections in use as well, and
/// a connection in use must be retired at its next release rather than closed
/// underneath a running transaction.
#[must_use]
pub fn is_expired(opened_at: Instant, config: &ReapConfig, now: Instant) -> bool {
    !config.max_lifetime.is_zero()
        && now.saturating_duration_since(opened_at) >= config.max_lifetime
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::pool::{Acquired, PoolConfig};
    use pgprox_core::ids::{PoolKey, ServerId};
    use pgprox_core::pool::ReleaseOutcome;

    fn key() -> PoolKey {
        PoolKey::new(ServerId::new("db-1", 5432), "tenant_acme", "acme_app")
    }

    fn pool() -> Pool {
        Pool::new(
            key(),
            PoolConfig {
                max_size: 10,
                ..PoolConfig::default()
            },
        )
    }

    /// Opens `n` connections without releasing any, so they are distinct.
    ///
    /// Opening and releasing one at a time would reuse the first, since that is
    /// exactly what the pool is for.
    fn open_n(pool: &mut Pool, n: usize) -> Vec<UpstreamId> {
        (0..n)
            .map(|_| {
                assert_eq!(pool.acquire(), Acquired::OpenNew);
                pool.opened()
            })
            .collect()
    }

    /// Opens one connection and releases it cleanly at `at`.
    fn idle_at(pool: &mut Pool, at: Instant) -> UpstreamId {
        let id = open_n(pool, 1)[0];
        pool.release(id, ReleaseOutcome::Reusable, at);
        id
    }

    fn config() -> ReapConfig {
        ReapConfig::default()
    }

    #[test]
    fn a_connection_idle_past_the_timeout_is_closed() {
        let config = config();
        let start = Instant::now();
        let mut pool = pool();
        let id = idle_at(&mut pool, start);

        assert!(
            reap(&pool, &config, start).is_empty(),
            "a freshly idle connection was reaped"
        );
        assert_eq!(
            reap(&pool, &config, start + config.idle_timeout).close,
            vec![id],
            "the timeout is inclusive at its boundary"
        );
    }

    #[test]
    fn a_quiet_pool_drops_to_zero_without_anyone_asking() {
        // The property that makes fan-out collapse. With a floor, five nodes
        // each hold a minimum for five thousand tenants and the upstream cap is
        // spent on connections nobody is using.
        let config = config();
        let start = Instant::now();
        let mut pool = pool();
        for id in open_n(&mut pool, 5) {
            pool.release(id, ReleaseOutcome::Reusable, start);
        }
        assert_eq!(pool.stats().idle, 5);

        let reaping = reap(&pool, &config, start + config.idle_timeout);
        assert_eq!(reaping.len(), 5);
        assert!(!reaping.is_empty());

        for id in reaping.close {
            assert!(pool.close_idle(id));
        }
        assert_eq!(pool.total(), 0, "a quiet pool kept connections open");
    }

    #[test]
    fn a_connection_in_use_is_never_reaped() {
        // A property of where the reaper looks rather than a check it performs.
        // The idle list simply does not contain checked-out connections.
        let config = config();
        let start = Instant::now();
        let mut pool = pool();
        assert_eq!(pool.acquire(), Acquired::OpenNew);
        let held = pool.opened();

        let far_future = start + config.idle_timeout * 100;
        assert!(
            reap(&pool, &config, far_future).is_empty(),
            "a connection in use was reaped"
        );
        assert_eq!(pool.checked_out(held).unwrap().id(), held);
    }

    #[test]
    fn keep_warm_keeps_the_most_recently_used() {
        // The ones a returning client is most likely to be served by. Keeping
        // the oldest would hold connections that have already proven nobody
        // wants them.
        let start = Instant::now();
        let mut pool = pool();
        let ids = open_n(&mut pool, 3);
        let (oldest, middle, newest) = (ids[0], ids[1], ids[2]);
        pool.release(oldest, ReleaseOutcome::Reusable, start);
        pool.release(
            middle,
            ReleaseOutcome::Reusable,
            start + Duration::from_secs(1),
        );
        pool.release(
            newest,
            ReleaseOutcome::Reusable,
            start + Duration::from_secs(2),
        );

        let config = ReapConfig {
            keep_warm: 1,
            ..config()
        };
        let reaping = reap(&pool, &config, start + Duration::from_secs(300));
        assert_eq!(reaping.close, vec![oldest, middle]);
        assert!(
            !reaping.close.contains(&newest),
            "the warmest connection was reaped"
        );
    }

    #[test]
    fn keep_warm_larger_than_the_pool_reaps_nothing() {
        let start = Instant::now();
        let mut pool = pool();
        idle_at(&mut pool, start);

        let config = ReapConfig {
            keep_warm: 100,
            ..config()
        };
        assert!(reap(&pool, &config, start + Duration::from_secs(300)).is_empty());
    }

    #[test]
    fn the_oldest_idle_connection_is_closed_first() {
        let start = Instant::now();
        let mut pool = pool();
        let ids = open_n(&mut pool, 2);
        let (second, first) = (ids[0], ids[1]);
        pool.release(
            second,
            ReleaseOutcome::Reusable,
            start + Duration::from_secs(1),
        );
        pool.release(first, ReleaseOutcome::Reusable, start);

        let reaping = reap(&pool, &config(), start + Duration::from_secs(300));
        assert_eq!(
            reaping.close,
            vec![first, second],
            "connections were not closed oldest first"
        );
    }

    #[test]
    fn reaping_is_deterministic_when_idle_times_tie() {
        // Two nodes given the same pool state must choose the same
        // connections, or a bug reproduces on one and not the other.
        let start = Instant::now();
        let mut a = pool();
        let mut b = pool();
        for id in open_n(&mut a, 4) {
            a.release(id, ReleaseOutcome::Reusable, start);
        }
        for id in open_n(&mut b, 4) {
            b.release(id, ReleaseOutcome::Reusable, start);
        }

        let now = start + Duration::from_secs(300);
        assert_eq!(reap(&a, &config(), now), reap(&b, &config(), now));
    }

    #[test]
    fn a_clock_that_jumped_backwards_does_not_empty_the_pool() {
        // Reaping everything on a clock adjustment would be a self-inflicted
        // outage, and the reading is only ever wrong in one direction.
        let start = Instant::now();
        let mut pool = pool();
        idle_at(&mut pool, start + Duration::from_secs(60));

        assert!(
            reap(&pool, &config(), start).is_empty(),
            "a backwards clock reaped the pool"
        );
    }

    #[test]
    fn a_client_arriving_between_the_decision_and_the_close_keeps_its_connection() {
        // The one race the reaper can lose. It names a connection, a client
        // acquires it, and only then does the caller act on the decision.
        // close_idle touches only idle connections, so the answer is a refusal
        // rather than a connection removed from under a running transaction.
        let config = config();
        let start = Instant::now();
        let mut pool = pool();
        let id = idle_at(&mut pool, start);

        let reaping = reap(&pool, &config, start + config.idle_timeout);
        assert_eq!(reaping.close, vec![id]);

        // The client gets there first.
        assert_eq!(pool.acquire(), Acquired::Reused(id));
        assert!(
            !pool.close_idle(id),
            "a connection was closed from under a client that had just taken it"
        );
        assert_eq!(pool.stats().active, 1);
    }

    #[test]
    fn an_empty_pool_reaps_nothing() {
        let pool = pool();
        assert!(reap(&pool, &config(), Instant::now()).is_empty());
    }

    #[test]
    fn a_zero_idle_timeout_closes_a_connection_as_soon_as_it_is_idle() {
        // Pathological configuration, and it must be merely wasteful rather
        // than wrong: every transaction pays a reconnect, nothing breaks.
        let start = Instant::now();
        let mut pool = pool();
        let id = idle_at(&mut pool, start);

        let config = ReapConfig {
            idle_timeout: Duration::ZERO,
            ..config()
        };
        assert_eq!(reap(&pool, &config, start).close, vec![id]);
    }

    #[test]
    fn a_connection_past_its_lifetime_is_expired() {
        // Bounds the damage from a connection that accumulated state nobody
        // noticed, and lets a rolling restart of the database actually finish.
        let config = config();
        let start = Instant::now();

        assert!(!is_expired(start, &config, start));
        let just_short = config
            .max_lifetime
            .checked_sub(Duration::from_secs(1))
            .unwrap();
        assert!(!is_expired(start, &config, start + just_short));
        assert!(is_expired(start, &config, start + config.max_lifetime));
    }

    #[test]
    fn a_zero_lifetime_means_no_limit_rather_than_immediate_expiry() {
        // Otherwise the disabling value would be the most destructive one.
        let start = Instant::now();
        let config = ReapConfig {
            max_lifetime: Duration::ZERO,
            ..config()
        };
        assert!(!is_expired(
            start,
            &config,
            start + Duration::from_secs(86_400)
        ));
    }

    #[test]
    fn lifetime_expiry_does_not_depend_on_being_idle() {
        // It applies to connections in use too, which is why it is a separate
        // question from reaping: one is retired at its next release, not
        // closed underneath a running transaction.
        let config = config();
        let start = Instant::now();
        let mut pool = pool();
        assert_eq!(pool.acquire(), Acquired::OpenNew);
        pool.opened();

        assert!(is_expired(start, &config, start + config.max_lifetime));
        assert!(
            reap(&pool, &config, start + config.max_lifetime).is_empty(),
            "an in-use connection was closed underneath its transaction"
        );
    }

    #[test]
    fn the_defaults_reap_but_do_not_thrash() {
        let config = ReapConfig::default();
        assert_eq!(config.keep_warm, 0, "the default kept a floor");
        assert!(config.idle_timeout >= Duration::from_secs(5));
        assert!(config.max_lifetime > config.idle_timeout);
    }
}
