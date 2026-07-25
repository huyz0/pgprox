//! Upstream connection pool contract.
//!
//! # Why the guard defaults to discarding
//!
//! An upstream connection released mid-transaction must be closed, not returned
//! to the pool. Returning it would hand another client a connection sitting
//! inside someone else's transaction.
//!
//! So [`UpstreamGuard`] discards by default, and reuse requires an explicit
//! [`UpstreamGuard::release_clean`] at a point the caller has established is
//! safe. A guard dropped by a cancelled future, an early return, or a panic is
//! therefore discarded rather than recycled. Fail-safe rather than fail-open,
//! which matters because the failure mode is silent cross-client state leakage.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::ClientError;
use crate::ids::{PoolKey, ServerId};

/// Identifies one pooled upstream connection.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct UpstreamId(pub u64);

/// What should happen to a connection when its guard is dropped.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[non_exhaustive]
pub enum ReleaseOutcome {
    /// Close it. The default, because it is the safe answer when the caller
    /// did not say otherwise.
    #[default]
    Discard,
    /// Return it to the pool for reuse.
    Reusable,
}

/// Receives connections back from their guards.
///
/// Implemented by the real pool in `pgprox-pool`.
pub trait ConnectionRelease: Send + Sync + fmt::Debug {
    /// Called exactly once per guard, when it drops.
    fn release(&self, id: UpstreamId, key: &PoolKey, outcome: ReleaseOutcome);
}

/// A borrowed upstream connection.
///
/// Dropping it returns the connection according to [`ReleaseOutcome`], which
/// is [`ReleaseOutcome::Discard`] unless [`UpstreamGuard::release_clean`] was
/// called.
#[derive(Debug)]
pub struct UpstreamGuard {
    id: UpstreamId,
    key: PoolKey,
    outcome: ReleaseOutcome,
    releaser: Arc<dyn ConnectionRelease>,
}

impl UpstreamGuard {
    /// Builds a guard. Called by pool implementations.
    #[must_use]
    pub fn new(id: UpstreamId, key: PoolKey, releaser: Arc<dyn ConnectionRelease>) -> Self {
        Self {
            id,
            key,
            outcome: ReleaseOutcome::Discard,
            releaser,
        }
    }

    /// Which connection this is.
    #[must_use]
    pub const fn id(&self) -> UpstreamId {
        self.id
    }

    /// Which pool it came from.
    #[must_use]
    pub const fn key(&self) -> &PoolKey {
        &self.key
    }

    /// What will happen when this guard drops.
    #[must_use]
    pub const fn outcome(&self) -> ReleaseOutcome {
        self.outcome
    }

    /// Marks the connection safe to reuse.
    ///
    /// Call only at a genuine transaction boundary: `ReadyForQuery` with status
    /// `I`, no extended-query sequence outstanding, and the session unpinned.
    pub const fn release_clean(&mut self) {
        self.outcome = ReleaseOutcome::Reusable;
    }

    /// Marks the connection unusable, undoing a previous
    /// [`UpstreamGuard::release_clean`].
    pub const fn poison(&mut self) {
        self.outcome = ReleaseOutcome::Discard;
    }
}

impl Drop for UpstreamGuard {
    fn drop(&mut self) {
        self.releaser.release(self.id, &self.key, self.outcome);
    }
}

/// A point-in-time view of one pool.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PoolStats {
    /// Connections currently checked out.
    pub active: u32,
    /// Connections open but not in use.
    pub idle: u32,
    /// Callers waiting for a connection.
    pub waiting: u32,
    /// The cap this pool is currently allowed to reach.
    pub limit: u32,
}

impl PoolStats {
    /// Connections open, in use or not.
    #[must_use]
    pub const fn total(&self) -> u32 {
        self.active + self.idle
    }

    /// Whether the pool can open another connection.
    #[must_use]
    pub const fn has_headroom(&self) -> bool {
        self.total() < self.limit
    }
}

/// Why a connection could not be acquired.
#[derive(Clone, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PoolError {
    /// The upstream server is at its configured cap and no connection freed up.
    #[error("upstream {server} is at its connection cap of {cap}")]
    AtCap {
        /// The server that is full.
        server: ServerId,
        /// The cap in force.
        cap: u32,
    },
    /// No connection became available before the deadline.
    #[error("timed out after {waited:?} waiting for a connection to {server}")]
    Timeout {
        /// The server being waited on.
        server: ServerId,
        /// How long the caller waited.
        waited: Duration,
    },
    /// The upstream refused or dropped the connection attempt.
    #[error("could not connect to {server}: {reason}")]
    ConnectFailed {
        /// The server that could not be reached.
        server: ServerId,
        /// Operator-facing detail. Never shown to a client.
        reason: String,
    },
}

impl From<PoolError> for ClientError {
    fn from(err: PoolError) -> Self {
        match err {
            PoolError::AtCap { server, cap } => Self::UpstreamAtCap { server, cap },
            PoolError::Timeout { server, waited } => Self::AcquireTimeout { server, waited },
            // From the client's side an unreachable upstream is the same as a
            // full one: retry and it may work.
            PoolError::ConnectFailed { server, .. } => Self::UpstreamAtCap { server, cap: 0 },
        }
    }
}

/// A pool of upstream connections.
#[async_trait::async_trait]
pub trait UpstreamPool: Send + Sync + fmt::Debug {
    /// Takes a connection, waiting until `deadline` if necessary.
    async fn acquire(&self, key: &PoolKey, deadline: Instant) -> Result<UpstreamGuard, PoolError>;

    /// A snapshot of one pool's state.
    fn stats(&self, key: &PoolKey) -> PoolStats;
}

#[async_trait::async_trait]
impl<T: UpstreamPool + ?Sized> UpstreamPool for Arc<T> {
    async fn acquire(&self, key: &PoolKey, deadline: Instant) -> Result<UpstreamGuard, PoolError> {
        (**self).acquire(key, deadline).await
    }

    fn stats(&self, key: &PoolKey) -> PoolStats {
        (**self).stats(key)
    }
}

#[cfg(any(test, feature = "test-fakes"))]
pub use fake::FakeUpstreamPool;

#[cfg(any(test, feature = "test-fakes"))]
mod fake {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Mutex, PoisonError};

    use super::{
        Arc, ConnectionRelease, Instant, PoolError, PoolKey, PoolStats, ReleaseOutcome,
        UpstreamGuard, UpstreamId, UpstreamPool,
    };

    #[derive(Debug, Default)]
    struct PoolState {
        active: u32,
        idle: u32,
        /// Connections that were discarded rather than reused, which is what a
        /// mid-transaction release looks like from the pool's side.
        discarded: u32,
    }

    /// An in-memory [`UpstreamPool`] for tests.
    ///
    /// Actually tracks acquisitions and actually refuses past its cap, rather
    /// than recording calls. A caller that leaks guards will see this fake stop
    /// handing out connections, which is the bug worth catching early.
    #[derive(Debug)]
    pub struct FakeUpstreamPool {
        cap: u32,
        state: Arc<Mutex<HashMap<PoolKey, PoolState>>>,
        next_id: AtomicU64,
    }

    impl FakeUpstreamPool {
        /// A pool allowing `cap` concurrent connections per key.
        #[must_use]
        pub fn new(cap: u32) -> Arc<Self> {
            Arc::new(Self {
                cap,
                state: Arc::new(Mutex::new(HashMap::new())),
                next_id: AtomicU64::new(1),
            })
        }

        /// How many connections were discarded rather than reused.
        #[must_use]
        pub fn discarded(&self, key: &PoolKey) -> u32 {
            self.lock().get(key).map_or(0, |s| s.discarded)
        }

        fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<PoolKey, PoolState>> {
            self.state.lock().unwrap_or_else(PoisonError::into_inner)
        }
    }

    /// Returns connections to the fake pool. Separate from the pool itself so
    /// the guard can hold it without a reference cycle.
    #[derive(Debug)]
    struct FakeRelease {
        state: Arc<Mutex<HashMap<PoolKey, PoolState>>>,
    }

    impl ConnectionRelease for FakeRelease {
        fn release(&self, _id: UpstreamId, key: &PoolKey, outcome: ReleaseOutcome) {
            let mut guard = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            let entry = guard.entry(key.clone()).or_default();
            entry.active = entry.active.saturating_sub(1);
            match outcome {
                ReleaseOutcome::Reusable => entry.idle += 1,
                ReleaseOutcome::Discard => entry.discarded += 1,
            }
        }
    }

    #[async_trait::async_trait]
    impl UpstreamPool for FakeUpstreamPool {
        async fn acquire(
            &self,
            key: &PoolKey,
            _deadline: Instant,
        ) -> Result<UpstreamGuard, PoolError> {
            {
                let mut guard = self.lock();
                let entry = guard.entry(key.clone()).or_default();
                if entry.active >= self.cap {
                    return Err(PoolError::AtCap {
                        server: key.server.clone(),
                        cap: self.cap,
                    });
                }
                entry.active += 1;
                entry.idle = entry.idle.saturating_sub(1);
            }

            Ok(UpstreamGuard::new(
                UpstreamId(self.next_id.fetch_add(1, Ordering::SeqCst)),
                key.clone(),
                Arc::new(FakeRelease {
                    state: Arc::clone(&self.state),
                }),
            ))
        }

        fn stats(&self, key: &PoolKey) -> PoolStats {
            let guard = self.lock();
            let entry = guard.get(key);
            PoolStats {
                active: entry.map_or(0, |s| s.active),
                idle: entry.map_or(0, |s| s.idle),
                waiting: 0,
                limit: self.cap,
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::ids::ServerId;

    fn key() -> PoolKey {
        PoolKey::new(ServerId::new("db-1", 5432), "tenant_acme", "acme_app")
    }

    /// A deadline far enough out that nothing treats it as expired.
    fn never() -> Instant {
        Instant::now() + Duration::from_secs(3600)
    }

    #[tokio::test]
    async fn a_guard_discards_by_default() {
        // The property that makes cancellation safe: a guard dropped without an
        // explicit clean release must not recycle the connection.
        let pool = FakeUpstreamPool::new(4);
        let guard = pool.acquire(&key(), never()).await.unwrap();
        assert_eq!(guard.outcome(), ReleaseOutcome::Discard);
        drop(guard);

        assert_eq!(pool.discarded(&key()), 1, "connection was recycled");
        assert_eq!(pool.stats(&key()).idle, 0);
    }

    #[tokio::test]
    async fn an_explicitly_clean_release_is_reused() {
        let pool = FakeUpstreamPool::new(4);
        let mut guard = pool.acquire(&key(), never()).await.unwrap();
        guard.release_clean();
        drop(guard);

        assert_eq!(
            pool.stats(&key()).idle,
            1,
            "clean connection was thrown away"
        );
        assert_eq!(pool.discarded(&key()), 0);
    }

    #[tokio::test]
    async fn poison_undoes_a_clean_release() {
        // A session that reaches a transaction boundary and then hits an error
        // must be able to take the reuse back.
        let pool = FakeUpstreamPool::new(4);
        let mut guard = pool.acquire(&key(), never()).await.unwrap();
        guard.release_clean();
        guard.poison();
        drop(guard);

        assert_eq!(pool.discarded(&key()), 1);
        assert_eq!(pool.stats(&key()).idle, 0);
    }

    #[tokio::test]
    async fn an_early_return_discards_rather_than_recycling() {
        // Models a cancelled future: the guard goes out of scope without ever
        // reaching the release point.
        async fn borrow_then_fail(pool: &Arc<FakeUpstreamPool>) -> Result<(), &'static str> {
            let _guard = pool.acquire(&key(), never()).await.unwrap();
            Err("something went wrong mid-transaction")
        }

        let pool = FakeUpstreamPool::new(4);
        assert!(borrow_then_fail(&pool).await.is_err());
        assert_eq!(
            pool.discarded(&key()),
            1,
            "an abandoned connection was recycled"
        );
    }

    #[tokio::test]
    async fn the_fake_actually_refuses_past_its_cap() {
        // A fake that records calls instead of enforcing the cap would let a
        // caller's backpressure path go untested.
        let pool = FakeUpstreamPool::new(2);
        let a = pool.acquire(&key(), never()).await.unwrap();
        let b = pool.acquire(&key(), never()).await.unwrap();

        let err = pool.acquire(&key(), never()).await.unwrap_err();
        assert!(
            matches!(err, PoolError::AtCap { cap: 2, .. }),
            "got {err:?}"
        );

        drop(a);
        assert!(
            pool.acquire(&key(), never()).await.is_ok(),
            "slot not freed"
        );
        drop(b);
    }

    #[tokio::test]
    async fn pools_are_tracked_per_key() {
        // Two roles on the same server are different pools, since a pooled
        // connection cannot be handed to a client authenticating differently.
        let pool = FakeUpstreamPool::new(1);
        let server = ServerId::new("db-1", 5432);
        let role_a = PoolKey::new(server.clone(), "d", "role_a");
        let role_b = PoolKey::new(server, "d", "role_b");

        let _a = pool.acquire(&role_a, never()).await.unwrap();
        assert!(
            pool.acquire(&role_b, never()).await.is_ok(),
            "one role's cap blocked another's"
        );
    }

    #[test]
    fn stats_report_headroom() {
        let stats = PoolStats {
            active: 3,
            idle: 1,
            waiting: 0,
            limit: 5,
        };
        assert_eq!(stats.total(), 4);
        assert!(stats.has_headroom());

        let full = PoolStats {
            active: 5,
            idle: 0,
            waiting: 2,
            limit: 5,
        };
        assert!(!full.has_headroom());
        assert_eq!(PoolStats::default().total(), 0);
    }

    #[test]
    fn pool_errors_map_to_client_errors() {
        let server = ServerId::new("db-1", 5432);
        let at_cap: ClientError = PoolError::AtCap {
            server: server.clone(),
            cap: 10,
        }
        .into();
        assert_eq!(at_cap.sqlstate().as_str(), "53300");

        let timeout: ClientError = PoolError::Timeout {
            server: server.clone(),
            waited: Duration::from_secs(1),
        }
        .into();
        assert_eq!(timeout.sqlstate().as_str(), "57014");

        let failed: ClientError = PoolError::ConnectFailed {
            server,
            reason: "connection refused by 10.0.0.9".into(),
        }
        .into();
        assert!(failed.is_retryable());
        assert!(
            !failed.client_message().contains("10.0.0.9"),
            "internal address leaked to the client"
        );
    }

    #[tokio::test]
    async fn pool_works_through_an_arc_dyn() {
        let pool: Arc<dyn UpstreamPool> = FakeUpstreamPool::new(1);
        let guard = pool.acquire(&key(), never()).await.unwrap();
        assert_eq!(guard.key(), &key());
        assert_eq!(guard.id(), UpstreamId(1));
        assert_eq!(pool.stats(&key()).active, 1);
    }
}
