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
use pgprox_core::hash::IssuedIds;
use pgprox_core::ids::PoolKey;
use pgprox_core::pool::{
    ConnectionRelease, PoolError, PoolStats, ReleaseOutcome, UpstreamGuard, UpstreamId,
    UpstreamPool,
};
use tokio::sync::Notify;

use crate::pool::{Acquired, Pool, PoolConfig};
use crate::reap::{ReapConfig, reap};

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

/// So a caller can hold its own handle on the connector while the pool holds
/// one too.
///
/// The composition root needs both: the pool opens connections through it, and
/// the grant path teaches it which backend a key means. Without this the two
/// would need two connectors and only one of them would know anything.
#[async_trait::async_trait]
impl<C: Connector> Connector for Arc<C> {
    type Connection = C::Connection;

    async fn connect(&self, key: &PoolKey) -> Result<Self::Connection, PoolError> {
        (**self).connect(key).await
    }
}

/// Per-key state: the decisions and the payloads.
struct Keyed<C> {
    pool: Pool,
    /// Open connections by id. The pool tracks that they exist; this holds
    /// them.
    ///
    /// Keyed on an id this node issues, so it takes `pgprox_core::hash`'s
    /// hasher for the reason given there. `M30.3`.
    connections: HashMap<UpstreamId, C, IssuedIds>,
}

impl<C> Keyed<C> {
    /// Whether this key holds nothing and nobody is in it.
    ///
    /// Both halves, because the two maps can disagree: a payload the reaper
    /// took out leaves the pool's own count at zero a moment before the
    /// connection map is empty.
    fn is_unused(&self) -> bool {
        self.pool.is_unused() && self.connections.is_empty()
    }
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
    /// Waiters woken that found nothing and parked again.
    ///
    /// The number that says whether the pool is doing wasted work, and the
    /// only way to see the difference between waking one waiter per released
    /// connection and waking all of them: both leave the same number of
    /// callers queued afterwards, and only this says how many were disturbed
    /// getting there. See `M7.58`.
    futile: std::sync::atomic::AtomicU64,
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
            futile: std::sync::atomic::AtomicU64::new(0),
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

    /// Waiters this pool has woken that found nothing and parked again.
    ///
    /// Zero is a pool waking exactly the callers it has connections for. It
    /// climbs when a release wakes more waiters than it freed connections, and
    /// it is the measurement `M7.58` turns on: the queue length afterwards is
    /// identical either way.
    #[must_use]
    pub fn futile_wakeups(&self) -> u64 {
        self.futile.load(std::sync::atomic::Ordering::Relaxed)
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
            connections: HashMap::default(),
        });
        f(&mut entry.pool)
    }

    /// Every pool this node holds, with its counts.
    ///
    /// For the admin surfaces, which have no other way to learn which pools
    /// exist: a pool is created by the first client of a database, so the list
    /// is not derivable from configuration.
    #[must_use]
    pub fn all_stats(&self) -> Vec<(PoolKey, PoolStats)> {
        let mut all: Vec<(PoolKey, PoolStats)> = self
            .lock()
            .iter()
            .map(|(key, keyed)| (key.clone(), keyed.pool.stats()))
            .collect();
        // Sorted so two pods rendering the same node agree on the order, and so
        // a dashboard's rows do not shuffle between refreshes.
        all.sort_by(|a, b| a.0.cmp(&b.0));
        all
    }

    /// Closes idle connections that have outstayed their welcome.
    ///
    /// Called from a background task on a timer. Returns the payloads it took
    /// out, so `pgprox_upstream_conns` moves by their count.
    ///
    /// Handing them back rather than dropping them here is `M20.4`: a socket
    /// deserves a `Terminate` before it goes, and writing one is an await this
    /// function cannot do. It holds a `std::sync::Mutex`, and the rule at the
    /// top of this file is that the lock is never held across an await. So the
    /// reaper decides under the lock and the caller says goodbye outside it.
    ///
    /// Dropping the payload is still the point: the pool forgets a connection
    /// the moment it is named, but until the payload goes the socket is open
    /// and the upstream is counting it against its cap. A caller that ignores
    /// the return value drops them immediately, which is the old behaviour.
    #[must_use]
    pub fn reap_idle(&self, config: &ReapConfig) -> Vec<K::Connection> {
        let now = self.clock.now();
        let mut keyed = self.lock();
        let mut closed = Vec::new();

        for entry in keyed.values_mut() {
            for id in reap(&entry.pool, config, now).close {
                // `close_idle` refuses if a client acquired it between the
                // decision and here, which is the one race the reaper can lose.
                if entry.pool.close_idle(id)
                    && let Some(payload) = entry.connections.remove(&id)
                {
                    closed.push(payload);
                }
            }
        }

        // And the pools themselves. A `Keyed` was created by the first client
        // of a key and dropped never, so a node that served a tenant which no
        // longer exists held its pool until the process ended: small per key
        // and unbounded in the number of keys. `M24.8`.
        //
        // Safe because `with_pool` creates on demand, so forgetting a key costs
        // the next client of it one map insert and nothing else.
        keyed.retain(|_, entry| !entry.is_unused());
        drop(keyed);

        self.forget_unheld_doorbells();
        closed
    }

    /// Drops the doorbells nobody is holding.
    ///
    /// A separate pass rather than part of the one above, because the hazard is
    /// not the same one. A waiter parks on the `Notify` it took out of this map;
    /// if the map drops it, the next release creates a fresh one and rings that,
    /// and the waiter sleeps until its own deadline instead.
    ///
    /// `strong_count == 1` is the exact question, and asking it under this lock
    /// is what makes it exact: every other way to get a handle takes the same
    /// lock, so a count of one means nobody else can be about to wait either.
    /// Reading the pool's `waiting` count instead would have a window between a
    /// caller taking the doorbell and registering as a waiter.
    fn forget_unheld_doorbells(&self) {
        let mut bells = self
            .doorbells
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        bells.retain(|_, bell| Arc::strong_count(bell) > 1);
    }

    /// How many doorbells this pool is holding.
    ///
    /// For the tests that the two maps shrink together, since a key forgotten
    /// from one and kept in the other is half a fix.
    #[must_use]
    pub fn doorbells_held(&self) -> usize {
        self.doorbells
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
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

    /// Takes the connection behind a guard out of the pool.
    ///
    /// [`Self::with_connection`] runs its closure while the pool's lock is
    /// held, which is fine for a peek and impossible for a relay: a relay
    /// awaits on the socket it borrowed, and awaiting under a `std` mutex is
    /// the deadlock this project's async standard forbids.
    ///
    /// So a session takes the connection out, uses it for as long as it holds
    /// the guard, and gives it back with [`Self::return_connection`]. While it
    /// is out the pool still counts the slot as checked out, so nothing else
    /// can be handed the same socket.
    ///
    /// Returns [`None`] if the guard has been released or the connection was
    /// already taken.
    pub fn take_connection(&self, key: &PoolKey, id: UpstreamId) -> Option<K::Connection> {
        let mut keyed = self.lock();
        keyed.get_mut(key)?.connections.remove(&id)
    }

    /// Gives a taken connection back.
    ///
    /// Called before the guard is dropped. A connection that is not returned is
    /// closed rather than leaked: the guard's release frees the slot either
    /// way, and a socket nobody holds is one the upstream is still counting
    /// until it notices.
    pub fn return_connection(&self, key: &PoolKey, id: UpstreamId, connection: K::Connection) {
        let mut keyed = self.lock();
        if let Some(entry) = keyed.get_mut(key) {
            entry.connections.insert(id, connection);
        }
    }

    /// Acquires a connection, waiting until `deadline`.
    async fn acquire_inner(
        &self,
        key: &PoolKey,
        deadline: Instant,
    ) -> Result<UpstreamGuard, PoolError> {
        let started = self.clock.now();
        let doorbell = self.doorbell(key);
        // Whether this caller has already been woken once. A first pass that
        // has to wait is the queue doing its job; a later one is a wakeup that
        // bought nothing.
        let mut woken = false;

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
                Acquired::Wait => {
                    if woken {
                        // Relaxed: a counter read by a test and a scrape, and
                        // nothing decides anything on its ordering.
                        self.futile
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }
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
            woken = true;
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
            connections: HashMap::default(),
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
            // the waiters have no other way to learn that. One slot, one
            // waiter: see `release` for why this is `notify_one`.
            self.pool.doorbell(self.key).notify_one();
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
        //
        // One waiter, not all of them. This released one connection, and there
        // is one caller that can have it. `notify_waiters` here was a
        // thundering herd: at five hundred clients against sixty upstream
        // connections it woke roughly four hundred and forty tasks per
        // release, and each one took this mutex to be told to wait, then took
        // it twice more building and dropping a `WaitGuard` on its way back to
        // sleep. `M9.10`'s profile is a picture of that and nothing else.
        //
        // Safe because `acquire_inner` registers its interest before it checks
        // the pool, so a caller that is going to wait is already a waiter when
        // this runs, and because `tokio::Notify` hands a notification on to
        // another waiter when the one it woke is dropped before polling.
        self.doorbell(key).notify_one();
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

    /// `M14.22`. Two mutants survived in this file.
    #[tokio::test]
    async fn the_debug_rendering_says_what_it_holds() {
        // The `Debug` impl is hand-written so that a socket is never printable
        // and no payload can reach a log by construction, and it could be
        // replaced wholesale by `Ok(Default::default())`: an empty rendering,
        // every time. Nothing asked, so `LivePool` would have printed as
        // nothing in every diagnostic that ever formatted it.
        let (pool, clock) = pool(4);

        let empty = format!("{pool:?}");
        assert!(
            empty.contains("LivePool"),
            "the type name is missing: {empty}"
        );
        assert!(
            empty.contains("pools"),
            "the pool count is missing: {empty}"
        );
        assert!(
            empty.contains('0'),
            "an empty pool did not report zero: {empty}"
        );

        // Once a key exists the count follows it, so the rendering is derived
        // rather than a fixed string that happens to contain the right words.
        let _guard = pool.acquire(&key(), never(&clock)).await.unwrap();
        let one = format!("{pool:?}");
        assert!(
            one.contains('1'),
            "a pool holding one key did not report it: {one}"
        );
        assert_ne!(empty, one, "the rendering did not change with the contents");
    }

    #[tokio::test]
    async fn a_waiter_woken_with_nothing_to_take_is_counted() {
        // `futile_wakeups` could return `0` unconditionally, and the existing
        // test asserts it *is* zero, which the mutant satisfies exactly. That
        // test is the measurement `M7.58` rests on: waking one waiter per
        // released connection rather than all of them. A counter frozen at zero
        // makes that measurement unfalsifiable, which is worse than not having
        // it, because the assertion still reads as evidence.
        //
        // A futile wakeup is a waiter that wakes and finds nothing, so ringing
        // the doorbell without releasing anything produces one deliberately.
        let (pool, clock) = pool(1);
        let held = pool.acquire(&key(), never(&clock)).await.unwrap();

        let waiting = {
            let pool = pool.clone();
            let deadline = never(&clock);
            tokio::spawn(async move { pool.acquire(&key(), deadline).await })
        };
        while pool.stats(&key()).waiting == 0 {
            tokio::task::yield_now().await;
        }
        assert_eq!(pool.futile_wakeups(), 0, "nothing has been woken yet");

        // Wake it with no connection available. It must find nothing, count the
        // wakeup, and park again.
        pool.doorbell(&key()).notify_one();
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            pool.futile_wakeups(),
            1,
            "a waiter woken with no connection available was not counted"
        );
        assert_eq!(pool.stats(&key()).waiting, 1, "it should have parked again");

        // A second futile wakeup counts again, so the counter accumulates
        // rather than latching at one.
        pool.doorbell(&key()).notify_one();
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        assert_eq!(pool.futile_wakeups(), 2);

        // And a real release still moves the waiter on.
        drop(held);
        assert!(waiting.await.unwrap().is_ok());
    }

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
    async fn one_release_wakes_one_waiter_rather_than_all_of_them() {
        // `M7.58`. `notify_waiters` woke every waiter for every release, and
        // at five hundred clients against sixty connections that was four
        // hundred and forty tasks woken to hand out one. Each took the pool's
        // mutex to be told to wait, then twice more building and dropping a
        // `WaitGuard`, which is what `M9.10`'s profile is a picture of.
        //
        // The count is what this checks, because "it still works" was already
        // true of the herd.
        let (pool, clock) = pool(1);
        let mut held = pool.acquire(&key(), never(&clock)).await.unwrap();
        held.release_clean();

        let mut waiters = Vec::new();
        for _ in 0..8 {
            let pool = Arc::clone(&pool);
            let deadline = never(&clock);
            waiters.push(tokio::spawn(
                async move { pool.acquire(&key(), deadline).await },
            ));
        }
        // Let all eight reach the point of parking.
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        assert_eq!(pool.stats(&key()).waiting, 8);

        // One connection back. Exactly one waiter may leave the queue, and the
        // other seven must still be parked rather than having woken, taken the
        // lock, and gone back to sleep.
        drop(held);
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        assert_eq!(pool.stats(&key()).waiting, 7);

        // The queue length is 7 either way, which is why it cannot be the
        // assertion: with the herd, all eight woke, one won, and seven took
        // the lock to be told to wait again. Only the wakeup count sees that.
        assert_eq!(
            pool.futile_wakeups(),
            0,
            "a release woke waiters it had no connection for"
        );

        // And the rest are not stranded: each release moves exactly one on.
        for expected in (0..7).rev() {
            let mut next = None;
            for waiter in &mut waiters {
                if waiter.is_finished() {
                    next = Some(waiter);
                    break;
                }
            }
            let mut guard = next
                .expect("no waiter finished")
                .await
                .unwrap()
                .expect("a woken waiter failed to acquire");
            guard.release_clean();
            drop(guard);
            waiters.retain(|w| !w.is_finished());
            for _ in 0..16 {
                tokio::task::yield_now().await;
            }
            assert_eq!(pool.stats(&key()).waiting, expected);
        }
    }

    #[tokio::test]
    async fn a_newcomer_taking_the_connection_does_not_strand_the_waiter() {
        // The fairness question `notify_one` raises. With the herd, a waiter
        // that kept losing lost to the crowd; with one wakeup it can lose to a
        // caller that never queued at all. What must not happen is that the
        // notification is spent and the waiter sleeps to its deadline with
        // connections going in and out around it.
        let (pool, clock) = pool(1);
        let mut held = pool.acquire(&key(), never(&clock)).await.unwrap();
        held.release_clean();

        let waiter = {
            let pool = Arc::clone(&pool);
            let deadline = never(&clock);
            tokio::spawn(async move { pool.acquire(&key(), deadline).await })
        };
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert_eq!(pool.stats(&key()).waiting, 1);

        // The release wakes the waiter, and this thread takes the connection
        // before the waiter is polled. The waiter finds nothing and parks
        // again, having spent its notification on a connection it did not get.
        drop(held);
        let mut barger = pool.acquire(&key(), never(&clock)).await.unwrap();
        barger.release_clean();
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert!(
            !waiter.is_finished(),
            "the waiter got the barged connection"
        );

        // The next release has to reach it. If it did not, this hangs, which
        // the test timeout reports as the stranding it is.
        drop(barger);
        let acquired = tokio::time::timeout(Duration::from_secs(5), waiter)
            .await
            .expect("the waiter was stranded after losing a wakeup to a newcomer")
            .unwrap();
        assert!(acquired.is_ok());
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
    async fn a_quiet_pool_reaps_itself_and_lets_go_of_its_sockets() {
        // The reaper names connections and the pool forgets them, but until the
        // payload goes the socket is still open and the upstream is still
        // counting it against its cap.
        let (pool, clock) = pool(4);
        let reaping = ReapConfig::default();

        let mut guard = pool.acquire(&key(), never(&clock)).await.unwrap();
        let id = guard.id();
        guard.release_clean();
        drop(guard);
        assert_eq!(pool.stats(&key()).idle, 1);
        assert_eq!(pool.with_connection(&key(), id, |c| *c), Some(1));

        assert_eq!(
            pool.reap_idle(&reaping).len(),
            0,
            "a freshly idle connection was reaped"
        );

        clock.advance(reaping.idle_timeout);
        assert_eq!(pool.reap_idle(&reaping).len(), 1);
        assert_eq!(pool.stats(&key()).total(), 0, "a quiet pool stayed open");
        assert_eq!(
            pool.with_connection(&key(), id, |c| *c),
            None,
            "a reaped connection's socket was kept open"
        );
    }

    #[tokio::test]
    async fn a_pool_nobody_is_using_is_forgotten_rather_than_kept_forever() {
        // `M24.8`. A `Keyed` was created by the first client of a key and
        // dropped never: `reap_idle` closed the connections and left the
        // `Pool`, and the doorbell map only ever grew. A node that served a
        // tenant which no longer exists held its pool until the process ended,
        // which is small per key and unbounded in the number of keys.
        let (pool, clock) = pool(4);
        let reaping = ReapConfig::default();

        let mut guard = pool.acquire(&key(), never(&clock)).await.unwrap();
        guard.release_clean();
        drop(guard);
        assert_eq!(pool.all_stats().len(), 1);

        clock.advance(reaping.idle_timeout);
        assert_eq!(pool.reap_idle(&reaping).len(), 1);

        assert!(
            pool.all_stats().is_empty(),
            "a pool with nothing open, nobody waiting and nothing checked out \
             was kept: {:?}",
            pool.all_stats()
        );
        assert_eq!(pool.doorbells_held(), 0, "its doorbell outlived it");

        // And it comes back on demand, which is what makes forgetting safe:
        // a pool is created by the first client of a key either way.
        let guard = pool.acquire(&key(), never(&clock)).await.unwrap();
        assert_eq!(pool.all_stats().len(), 1);
        drop(guard);
    }

    #[tokio::test]
    async fn a_pool_still_holding_something_is_not_forgotten() {
        // The other half. Forgetting a key with a connection checked out would
        // lose the connection; forgetting one with an idle connection that is
        // not yet old enough would close it early. Neither is reachable through
        // `reap_idle`, and this is what says so.
        let (pool, clock) = pool(4);
        let reaping = ReapConfig::default();

        // Checked out.
        let mut guard = pool.acquire(&key(), never(&clock)).await.unwrap();
        clock.advance(reaping.idle_timeout * 100);
        assert_eq!(pool.reap_idle(&reaping).len(), 0);
        assert_eq!(pool.all_stats().len(), 1, "a pool in use was forgotten");

        // Idle, and younger than the timeout.
        guard.release_clean();
        drop(guard);
        assert_eq!(pool.reap_idle(&reaping).len(), 0);
        assert_eq!(
            pool.all_stats().len(),
            1,
            "a pool holding a warm connection was forgotten"
        );
    }

    #[tokio::test]
    async fn a_doorbell_somebody_is_holding_is_not_dropped() {
        // The hazard in forgetting a key. A waiter parks on the `Notify` it
        // took out of the map; if the map then drops it, the next release
        // creates a fresh one and rings that, and the waiter sleeps until its
        // own deadline instead.
        //
        // A zero limit is the way to reach it: `acquire` answers `Wait` with
        // nothing open at all, so the pool looks unused while somebody is in it.
        let (pool, clock) = pool(4);
        pool.set_limit(&key(), 0);

        let waiter = {
            let pool = Arc::clone(&pool);
            let deadline = clock.now() + Duration::from_secs(30);
            tokio::spawn(async move { pool.acquire(&key(), deadline).await })
        };
        while pool.stats(&key()).waiting == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }

        clock.advance(ReapConfig::default().idle_timeout * 100);
        assert!(pool.reap_idle(&ReapConfig::default()).is_empty());

        assert_eq!(
            pool.all_stats().len(),
            1,
            "a pool with a caller waiting in it was forgotten"
        );
        assert_eq!(
            pool.doorbells_held(),
            1,
            "the waiter's doorbell was dropped"
        );

        // And it is still the doorbell that wakes them.
        pool.set_limit(&key(), 4);
        let guard = tokio::time::timeout(Duration::from_secs(5), waiter)
            .await
            .expect("the waiter was never woken")
            .unwrap()
            .unwrap();
        drop(guard);
    }

    #[tokio::test]
    async fn the_reaper_leaves_a_connection_in_use_alone() {
        let (pool, clock) = pool(4);
        let reaping = ReapConfig::default();
        let guard = pool.acquire(&key(), never(&clock)).await.unwrap();

        clock.advance(reaping.idle_timeout * 100);
        assert_eq!(
            pool.reap_idle(&reaping).len(),
            0,
            "a connection in use was closed underneath its transaction"
        );
        assert_eq!(pool.stats(&key()).active, 1);
        drop(guard);
    }

    #[tokio::test]
    async fn reaping_frees_room_for_a_caller_at_the_cap() {
        // The reaper is not only housekeeping. A pool full of idle connections
        // for tenants nobody is using is a pool that refuses the tenant who is.
        let (pool, clock) = pool(1);
        let reaping = ReapConfig::default();

        let server = ServerId::new("db-1", 5432);
        let quiet = PoolKey::new(server.clone(), "d", "quiet_role");
        let mut guard = pool.acquire(&quiet, never(&clock)).await.unwrap();
        guard.release_clean();
        drop(guard);

        clock.advance(reaping.idle_timeout);
        assert_eq!(pool.reap_idle(&reaping).len(), 1);
        assert_eq!(pool.stats(&quiet).total(), 0);
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

    #[tokio::test]
    async fn every_pool_the_node_holds_is_reportable() {
        // The admin surfaces have no other way to learn which pools exist: one
        // is created by the first client of a database, not by configuration.
        let (pool, clock) = pool(4);
        let other = PoolKey::new(ServerId::new("db-1", 5432), "tenant_globex", "globex_app");

        let _first = pool.acquire(&key(), never(&clock)).await.unwrap();
        let _second = pool.acquire(&other, never(&clock)).await.unwrap();

        let all = pool.all_stats();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].0, key(), "the pools came back in an unstable order");
        assert_eq!(all[0].1.active, 1);
    }

    #[tokio::test]
    async fn a_session_can_take_its_connection_out_and_give_it_back() {
        // The relay awaits on the socket it borrowed, so it cannot borrow it
        // under the pool's lock. Taking it out is what makes that possible.
        let (pool, clock) = pool(4);
        let guard = pool.acquire(&key(), never(&clock)).await.unwrap();

        let taken = pool.take_connection(&key(), guard.id()).expect("taken");
        assert!(
            pool.take_connection(&key(), guard.id()).is_none(),
            "the same connection was handed out twice"
        );

        pool.return_connection(&key(), guard.id(), taken);
        assert!(pool.take_connection(&key(), guard.id()).is_some());
    }

    #[tokio::test]
    async fn a_connection_out_on_loan_is_still_counted_as_checked_out() {
        // Otherwise the pool would open a second connection to serve the next
        // caller while the first was still using the one it took, and the cap
        // would mean nothing.
        let (pool, clock) = pool(1);
        let guard = pool.acquire(&key(), never(&clock)).await.unwrap();
        let _taken = pool.take_connection(&key(), guard.id()).expect("taken");

        assert_eq!(pool.stats(&key()).active, 1);
        assert!(
            pool.acquire(&key(), clock.now() + Duration::from_millis(1))
                .await
                .is_err(),
            "a pool of one handed out a second connection"
        );
    }

    #[tokio::test]
    async fn returning_a_connection_to_a_pool_that_has_gone_drops_it() {
        // Rather than resurrecting the pool. A key with no pool means the
        // node stopped serving that database, and re-creating it here would
        // put a connection somewhere nothing will ever reap it from.
        let (pool, clock) = pool(4);
        let guard = pool.acquire(&key(), never(&clock)).await.unwrap();
        let taken = pool.take_connection(&key(), guard.id()).expect("taken");

        let elsewhere = PoolKey::new(ServerId::new("db-9", 5432), "nope", "nobody");
        pool.return_connection(&elsewhere, guard.id(), taken);

        assert!(pool.take_connection(&elsewhere, guard.id()).is_none());
    }
}
