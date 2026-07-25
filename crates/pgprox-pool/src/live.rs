//! The async pool: waiting, deadlines, and the [`UpstreamPool`]
//! implementation.
//!
//! [`crate::pool::Pool`] decides; this waits. Keeping them apart is what lets
//! every rule about caps and releases be tested without a runtime, and leaves
//! this layer small enough to reason about on its own.
//!
//! # Woken, not polled
//!
//! A waiter parks on a [`Notify`] and is woken when a connection comes back.
//! Polling would trade latency against wasted wakeups at exactly the moment the
//! node is busiest, and at a hundred thousand client connections a poll
//! interval is not a small number of wakeups.
//!
//! # The lock is never held across an await
//!
//! It is a `std::sync::Mutex`, taken to make a decision and released before
//! anything suspends. That is not just a deadlock argument: holding it across a
//! connect would serialise every acquire on the node behind one slow upstream.
//!
//! # Cancellation
//!
//! A client that disconnects mid-wait drops the acquire future. Everything a
//! waiter touches is therefore restored by a guard rather than by code after
//! the await, because that code does not run. The waiter count is the visible
//! case, and a reserved connection slot is the expensive one.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use pgprox_core::clock::Clock;
use pgprox_core::ids::PoolKey;
use pgprox_core::pool::{
    ConnectionRelease, PoolError, PoolStats, ReleaseOutcome, UpstreamGuard, UpstreamId,
    UpstreamPool,
};
use tokio::sync::Notify;

use crate::pool::{Acquired, Pool, PoolConfig};

/// Opens upstream connections.
///
/// The one piece of real I/O, kept behind a trait so the pool's own behaviour
/// is testable against a fake that fails, hangs or succeeds on demand.
#[async_trait::async_trait]
pub trait Connector: Send + Sync + fmt::Debug {
    /// What an open connection is. A socket in production.
    type Connection: Send + 'static;

    /// Opens one.
    ///
    /// # Errors
    ///
    /// Fails when the upstream refuses, is unreachable, or rejects the
    /// credentials.
    async fn connect(&self, key: &PoolKey) -> Result<Self::Connection, PoolError>;
}

/// Per-key state: the decisions and the payloads.
struct Keyed<C> {
    pool: Pool,
    /// Open connections by id. The pool tracks that they exist; this holds
    /// them.
    connections: HashMap<UpstreamId, C>,
}

/// An [`UpstreamPool`] over a [`Connector`].
pub struct LivePool<K: Connector> {
    /// A handle to this pool's own `Arc`.
    ///
    /// An [`UpstreamGuard`] holds an `Arc<dyn ConnectionRelease>` so it can
    /// return its connection when dropped, and the trait's `acquire` takes
    /// `&self`. Without this the implementation could not build a guard at all,
    /// and the choice would be between changing a `pgprox-core` contract that
    /// five crates depend on, or panicking in the one method callers actually
    /// use. `Arc::new_cyclic` makes it a non-question.
    me: std::sync::Weak<Self>,
    connector: K,
    clock: Arc<dyn Clock>,
    config: PoolConfig,
    keyed: Mutex<HashMap<PoolKey, Keyed<K::Connection>>>,
    /// One doorbell per key, kept outside the lock so a waiter can hold an
    /// `Arc` to it without holding the map.
    doorbells: Mutex<HashMap<PoolKey, Arc<Notify>>>,
}

/// Hand-written rather than derived, because deriving would require the
/// caller's connection type to be `Debug` and a socket has no business being
/// printable. It also keeps a payload out of any log line by construction.
impl<K: Connector + 'static> fmt::Debug for LivePool<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let keys = self.lock().len();
        f.debug_struct("LivePool")
            .field("connector", &self.connector)
            .field("pools", &keys)
            .finish_non_exhaustive()
    }
}

impl<K: Connector + 'static> LivePool<K> {
    /// A pool over `connector`.
    #[must_use]
    pub fn new(connector: K, clock: Arc<dyn Clock>, config: PoolConfig) -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
            me: me.clone(),
            connector,
            clock,
            config,
            keyed: Mutex::new(HashMap::new()),
            doorbells: Mutex::new(HashMap::new()),
        })
    }

    /// Locks the key map, recovering from a poisoned lock.
    ///
    /// A panic while holding it must not take every other connection on the
    /// node down with it. The invariants inside are restored by the guards in
    /// this module rather than by unwinding, so the state a panicking thread
    /// leaves behind is consistent.
    fn lock(&self) -> MutexGuard<'_, HashMap<PoolKey, Keyed<K::Connection>>> {
        self.keyed.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The doorbell for a key, creating it if this is the first caller.
    fn doorbell(&self, key: &PoolKey) -> Arc<Notify> {
        let mut bells = self
            .doorbells
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        Arc::clone(bells.entry(key.clone()).or_default())
    }

    /// Sets the cap for a key, as the cluster layer's allowance changes.
    ///
    /// Wakes the waiters, since raising the limit may have made room and
    /// nothing else would tell them.
    pub fn set_limit(&self, key: &PoolKey, limit: u32) {
        self.with_pool(key, |pool| pool.set_limit(limit));
        self.doorbell(key).notify_waiters();
    }

    /// Runs `f` against one key's pool, creating it if needed.
    fn with_pool<T>(&self, key: &PoolKey, f: impl FnOnce(&mut Pool) -> T) -> T {
        let mut keyed = self.lock();
        let entry = keyed.entry(key.clone()).or_insert_with(|| Keyed {
            pool: Pool::new(key.clone(), self.config),
            connections: HashMap::new(),
        });
        f(&mut entry.pool)
    }

    /// The open connection behind a guard, for the caller to actually use.
    ///
    /// Returns [`None`] once the guard has been dropped, which is what makes
    /// using a released connection a compile-or-runtime miss rather than a
    /// silent share.
    pub fn with_connection<T>(
        &self,
        key: &PoolKey,
        id: UpstreamId,
        f: impl FnOnce(&mut K::Connection) -> T,
    ) -> Option<T> {
        let mut keyed = self.lock();
        let entry = keyed.get_mut(key)?;
        entry.connections.get_mut(&id).map(f)
    }

    /// Acquires a connection, waiting until `deadline`.
    async fn acquire_inner(
        &self,
        key: &PoolKey,
        deadline: Instant,
    ) -> Result<UpstreamGuard, PoolError> {
        let started = self.clock.now();
        let doorbell = self.doorbell(key);

        loop {
            // Registering interest *before* checking is what closes the race
            // where a connection is released between the check and the wait,
            // which would otherwise park a waiter that nothing will wake.
            let notified = doorbell.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            match self.with_pool(key, Pool::acquire) {
                Acquired::Reused(id) => return self.guard(key, id),
                Acquired::OpenNew => return self.open(key).await,
                Acquired::Wait => {}
            }

            let now = self.clock.now();
            if now >= deadline {
                return Err(self.with_pool(key, |pool| pool.give_up(now - started)));
            }

            // The counter is restored by a guard, not by code after the await:
            // a client that disconnects mid-wait drops this future, and code
            // after the await does not run.
            let _waiting = WaitGuard::new(self, key);
            if tokio::time::timeout_at(deadline.into(), notified)
                .await
                .is_err()
            {
                let waited = self.clock.now().saturating_duration_since(started);
                return Err(self.with_pool(key, |pool| pool.give_up(waited)));
            }
        }
    }

    /// Opens a connection against a slot the pool has already reserved.
    async fn open(&self, key: &PoolKey) -> Result<UpstreamGuard, PoolError> {
        // Holds the reserved slot, and gives it back if this future is dropped
        // mid-connect. Without it a cancelled acquire would leak a slot, and a
        // pool that leaked its whole cap would refuse every future caller.
        let slot = SlotGuard::new(self, key);

        let connection = match self.connector.connect(key).await {
            Ok(connection) => connection,
            Err(err) => return Err(err),
        };

        slot.consume();
        let mut keyed = self.lock();
        let entry = keyed.entry(key.clone()).or_insert_with(|| Keyed {
            pool: Pool::new(key.clone(), self.config),
            connections: HashMap::new(),
        });
        let id = entry.pool.opened();
        entry.connections.insert(id, connection);
        drop(keyed);

        self.guard(key, id)
    }

    /// Builds the guard handed to the caller.
    ///
    /// Fails only if the pool is being dropped, which a caller holding a
    /// reference to it cannot observe. Reported as an error rather than a
    /// panic all the same: this is on the path every client connection takes.
    fn guard(&self, key: &PoolKey, id: UpstreamId) -> Result<UpstreamGuard, PoolError> {
        let Some(me) = self.me.upgrade() else {
            return Err(PoolError::ConnectFailed {
                server: key.server.clone(),
                reason: "the pool is shutting down".into(),
            });
        };
        Ok(UpstreamGuard::new(
            id,
            key.clone(),
            me as Arc<dyn ConnectionRelease>,
        ))
    }
}

/// Holds a waiter's place in the count until it stops waiting.
struct WaitGuard<'a, K: Connector + 'static> {
    pool: &'a LivePool<K>,
    key: &'a PoolKey,
}

impl<'a, K: Connector + 'static> WaitGuard<'a, K> {
    fn new(pool: &'a LivePool<K>, key: &'a PoolKey) -> Self {
        pool.with_pool(key, Pool::begin_wait);
        Self { pool, key }
    }
}

impl<K: Connector + 'static> Drop for WaitGuard<'_, K> {
    fn drop(&mut self) {
        self.pool.with_pool(self.key, Pool::end_wait);
    }
}

/// Holds a reserved connection slot until it is either used or given back.
struct SlotGuard<'a, K: Connector + 'static> {
    pool: &'a LivePool<K>,
    key: &'a PoolKey,
    consumed: bool,
}

impl<'a, K: Connector + 'static> SlotGuard<'a, K> {
    const fn new(pool: &'a LivePool<K>, key: &'a PoolKey) -> Self {
        Self {
            pool,
            key,
            consumed: false,
        }
    }

    /// The connection opened, so the slot is now a real connection.
    fn consume(mut self) {
        self.consumed = true;
    }
}

impl<K: Connector + 'static> Drop for SlotGuard<'_, K> {
    fn drop(&mut self) {
        if !self.consumed {
            self.pool.with_pool(self.key, Pool::open_failed);
            // A slot given back is room that did not exist a moment ago, and
            // the waiters have no other way to learn that.
            self.pool.doorbell(self.key).notify_waiters();
        }
    }
}

impl<K: Connector + 'static> ConnectionRelease for LivePool<K> {
    fn release(&self, id: UpstreamId, key: &PoolKey, outcome: ReleaseOutcome) {
        let now = self.clock.now();
        {
            let mut keyed = self.lock();
            let Some(entry) = keyed.get_mut(key) else {
                return;
            };
            if !entry.pool.release(id, outcome, now) {
                // Discarded, or never ours. Either way the payload goes with
                // it, which is what closes the socket.
                entry.connections.remove(&id);
            }
        }
        // Outside the lock. A waiter woken while this thread still holds it
        // would immediately block on it.
        self.doorbell(key).notify_waiters();
    }
}

#[async_trait::async_trait]
impl<K: Connector + 'static> UpstreamPool for LivePool<K> {
    async fn acquire(&self, key: &PoolKey, deadline: Instant) -> Result<UpstreamGuard, PoolError> {
        self.acquire_inner(key, deadline).await
    }

    fn stats(&self, key: &PoolKey) -> PoolStats {
        self.lock()
            .get(key)
            .map_or_else(PoolStats::default, |entry| entry.pool.stats())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use pgprox_core::clock::FakeClock;
    use pgprox_core::ids::ServerId;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    fn key() -> PoolKey {
        PoolKey::new(ServerId::new("db-1", 5432), "tenant_acme", "acme_app")
    }

    /// A connector that counts opens and can be told to fail or to hang.
    #[derive(Debug, Default)]
    struct FakeConnector {
        opens: AtomicU32,
        fail: AtomicU32,
    }

    impl FakeConnector {
        fn opens(&self) -> u32 {
            self.opens.load(Ordering::SeqCst)
        }

        /// Makes the next `n` connection attempts fail.
        fn fail_next(&self, n: u32) {
            self.fail.store(n, Ordering::SeqCst);
        }
    }

    #[async_trait::async_trait]
    impl Connector for FakeConnector {
        type Connection = u32;

        async fn connect(&self, key: &PoolKey) -> Result<u32, PoolError> {
            if self
                .fail
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok()
            {
                return Err(PoolError::ConnectFailed {
                    server: key.server.clone(),
                    reason: "refused by the fake".into(),
                });
            }
            Ok(self.opens.fetch_add(1, Ordering::SeqCst) + 1)
        }
    }

    fn pool(max: u32) -> (Arc<LivePool<FakeConnector>>, FakeClock) {
        let clock = FakeClock::new();
        let pool = LivePool::new(
            FakeConnector::default(),
            Arc::new(clock.clone()),
            PoolConfig {
                max_size: max,
                ..PoolConfig::default()
            },
        );
        (pool, clock)
    }

    fn never(clock: &FakeClock) -> Instant {
        clock.now() + Duration::from_secs(3_600)
    }

    #[tokio::test]
    async fn acquiring_from_an_empty_pool_opens_a_connection() {
        let (pool, clock) = pool(4);
        let guard = pool.acquire(&key(), never(&clock)).await.unwrap();

        assert_eq!(pool.stats(&key()).active, 1);
        assert_eq!(
            pool.with_connection(&key(), guard.id(), |c| *c),
            Some(1),
            "the guard does not reach its connection"
        );
    }

    #[tokio::test]
    async fn a_clean_release_is_reused_and_a_dirty_one_is_closed() {
        let (pool, clock) = pool(4);

        let mut guard = pool.acquire(&key(), never(&clock)).await.unwrap();
        let first = guard.id();
        guard.release_clean();
        drop(guard);
        assert_eq!(pool.stats(&key()).idle, 1);

        let guard = pool.acquire(&key(), never(&clock)).await.unwrap();
        assert_eq!(guard.id(), first, "a warm connection was not reused");
        assert_eq!(pool.connector.opens(), 1);

        // Dropped without a clean release, as a cancelled transaction would be.
        drop(guard);
        assert_eq!(pool.stats(&key()).idle, 0);
        assert_eq!(pool.stats(&key()).active, 0);
        assert_eq!(
            pool.with_connection(&key(), first, |c| *c),
            None,
            "a discarded connection's socket was kept"
        );
    }

    #[tokio::test]
    async fn a_waiter_is_woken_by_a_release_rather_than_by_polling() {
        // At a hundred thousand client connections a poll interval is not a
        // small number of wakeups, and the latency it trades against is the
        // one an operator notices first.
        let (pool, clock) = pool(1);
        let mut held = pool.acquire(&key(), never(&clock)).await.unwrap();
        held.release_clean();

        let waiter = {
            let pool = Arc::clone(&pool);
            let deadline = never(&clock);
            tokio::spawn(async move { pool.acquire(&key(), deadline).await })
        };

        // Let the waiter reach the point of parking.
        tokio::task::yield_now().await;
        assert_eq!(pool.stats(&key()).waiting, 1);

        drop(held);
        let acquired = waiter.await.unwrap();
        assert!(acquired.is_ok(), "the waiter was never woken");
        assert_eq!(pool.stats(&key()).waiting, 0);
    }

    #[tokio::test]
    async fn a_waiter_that_misses_its_deadline_gives_up() {
        let clock = FakeClock::new();
        let pool = LivePool::new(
            FakeConnector::default(),
            Arc::new(clock.clone()),
            PoolConfig {
                max_size: 1,
                ..PoolConfig::default()
            },
        );

        let _held = pool.acquire(&key(), never(&clock)).await.unwrap();
        let deadline = Instant::now() + Duration::from_millis(50);

        let err = pool.acquire(&key(), deadline).await.unwrap_err();
        assert!(
            matches!(err, PoolError::AtCap { cap: 1, .. }),
            "a full pool reported the wrong error: {err:?}"
        );
        assert_eq!(
            pool.stats(&key()).waiting,
            0,
            "a waiter that gave up still occupied a slot"
        );
    }

    #[tokio::test]
    async fn a_deadline_already_past_does_not_wait_at_all() {
        let (pool, clock) = pool(1);
        let _held = pool.acquire(&key(), never(&clock)).await.unwrap();

        let err = pool
            .acquire(
                &key(),
                clock.now().checked_sub(Duration::from_secs(1)).unwrap(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, PoolError::AtCap { .. }), "{err:?}");
        assert_eq!(pool.stats(&key()).waiting, 0);
    }

    #[tokio::test]
    async fn a_cancelled_waiter_stops_occupying_a_slot() {
        // A client that disconnects mid-wait drops the future, so code after
        // the await never runs. The count has to be restored by a guard.
        let (pool, clock) = pool(1);
        let _held = pool.acquire(&key(), never(&clock)).await.unwrap();

        let waiter = {
            let pool = Arc::clone(&pool);
            let deadline = never(&clock);
            tokio::spawn(async move { pool.acquire(&key(), deadline).await })
        };
        tokio::task::yield_now().await;
        assert_eq!(pool.stats(&key()).waiting, 1);

        waiter.abort();
        let _ = waiter.await;
        assert_eq!(
            pool.stats(&key()).waiting,
            0,
            "a cancelled waiter leaked its place in the count"
        );
    }

    #[tokio::test]
    async fn a_failed_connect_gives_the_slot_back() {
        // Without this a failing upstream would permanently shrink the pool,
        // and a pool that leaked its whole cap would refuse every caller after.
        let (pool, clock) = pool(2);
        pool.connector.fail_next(1);

        let err = pool.acquire(&key(), never(&clock)).await.unwrap_err();
        assert!(matches!(err, PoolError::ConnectFailed { .. }), "{err:?}");
        assert_eq!(pool.stats(&key()).total(), 0, "a failed open kept its slot");

        assert!(
            pool.acquire(&key(), never(&clock)).await.is_ok(),
            "the pool never recovered from a failed connect"
        );
    }

    #[tokio::test]
    async fn a_failed_connect_wakes_the_waiters() {
        // The slot it gives back is room that did not exist a moment ago, and
        // nothing else would tell them.
        let (pool, clock) = pool(1);
        pool.connector.fail_next(1);

        let waiter = {
            let pool = Arc::clone(&pool);
            let deadline = never(&clock);
            tokio::spawn(async move { pool.acquire(&key(), deadline).await })
        };
        let first = pool.acquire(&key(), never(&clock)).await;
        assert!(first.is_err());

        assert!(
            waiter.await.unwrap().is_ok(),
            "a waiter slept through a slot being freed"
        );
    }

    #[tokio::test]
    async fn raising_the_limit_wakes_the_waiters() {
        let (pool, clock) = pool(4);
        pool.set_limit(&key(), 1);
        let _held = pool.acquire(&key(), never(&clock)).await.unwrap();

        let waiter = {
            let pool = Arc::clone(&pool);
            let deadline = never(&clock);
            tokio::spawn(async move { pool.acquire(&key(), deadline).await })
        };
        tokio::task::yield_now().await;
        assert_eq!(pool.stats(&key()).waiting, 1);

        pool.set_limit(&key(), 2);
        assert!(
            waiter.await.unwrap().is_ok(),
            "a waiter slept through the cap being raised"
        );
    }

    #[tokio::test]
    async fn several_waiters_are_all_served_as_connections_return() {
        let (pool, clock) = pool(1);
        let mut held = pool.acquire(&key(), never(&clock)).await.unwrap();
        held.release_clean();

        let waiters: Vec<_> = (0..3)
            .map(|_| {
                let pool = Arc::clone(&pool);
                let deadline = never(&clock);
                tokio::spawn(async move {
                    let mut guard = pool.acquire(&key(), deadline).await?;
                    guard.release_clean();
                    Ok::<(), PoolError>(())
                })
            })
            .collect();

        tokio::task::yield_now().await;
        assert_eq!(pool.stats(&key()).waiting, 3);
        drop(held);

        for waiter in waiters {
            assert!(waiter.await.unwrap().is_ok(), "a waiter was never served");
        }
        assert_eq!(pool.stats(&key()).waiting, 0);
        assert_eq!(
            pool.connector.opens(),
            1,
            "the cap was exceeded while serving waiters"
        );
    }

    #[tokio::test]
    async fn pools_are_kept_apart_by_key() {
        // Two roles on one server are different pools: a connection
        // authenticated as one cannot be handed to the other.
        let (pool, clock) = pool(1);
        let server = ServerId::new("db-1", 5432);
        let a = PoolKey::new(server.clone(), "d", "role_a");
        let b = PoolKey::new(server, "d", "role_b");

        let _first = pool.acquire(&a, never(&clock)).await.unwrap();
        // Bound, not dropped: `is_ok()` would release it before the assertion
        // below could see it.
        let _second = pool
            .acquire(&b, never(&clock))
            .await
            .expect("one role's cap blocked another's");
        assert_eq!(pool.stats(&a).active, 1);
        assert_eq!(pool.stats(&b).active, 1);
    }

    #[tokio::test]
    async fn stats_for_an_unknown_key_are_empty_rather_than_absent() {
        // The admin surface asks about pools that may not exist yet, and an
        // error there would be noise rather than information.
        let (pool, _clock) = pool(1);
        let stats = pool.stats(&key());
        assert_eq!(stats.total(), 0);
        assert_eq!(stats.limit, 0);
    }

    #[tokio::test]
    async fn the_pool_works_through_the_trait_object() {
        // How M6 will hold it. The guard needs an Arc to this pool in order to
        // return its connection, and the trait's acquire takes &self, so the
        // pool keeps a Weak to itself via Arc::new_cyclic. Getting this wrong
        // would have meant either changing a pgprox-core contract that five
        // crates depend on, or panicking in the one method callers use.
        let (pool, clock) = pool(2);
        let dynamic: Arc<dyn UpstreamPool> = pool;

        let mut guard = dynamic.acquire(&key(), never(&clock)).await.unwrap();
        assert_eq!(dynamic.stats(&key()).active, 1);

        guard.release_clean();
        drop(guard);
        assert_eq!(
            dynamic.stats(&key()).idle,
            1,
            "a guard from the trait object did not return its connection"
        );
    }

    #[tokio::test]
    async fn releasing_a_connection_for_an_unknown_key_is_harmless() {
        let (pool, _clock) = pool(1);
        pool.release(UpstreamId(1), &key(), ReleaseOutcome::Reusable);
        assert_eq!(pool.stats(&key()).total(), 0);
    }
}
