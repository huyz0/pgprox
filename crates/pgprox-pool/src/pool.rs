//! The pool: which connections exist, who holds them, and when they come back.
//!
//! # The release rule
//!
//! A connection is returned to the pool only at a genuine transaction
//! boundary: `ReadyForQuery` with status `I`, no extended query sequence
//! outstanding, and the session unpinned. Never on SQL text, never on a
//! heuristic.
//!
//! Anything else is closed rather than returned. That is not tidiness: handing
//! a connection sitting inside someone else's transaction to a second client
//! gives them a session already holding locks, mid-way through a unit of work
//! they know nothing about, which is the worst failure this crate can produce
//! because nothing about it looks like an error.
//!
//! [`pgprox_core::pool::UpstreamGuard`] enforces the direction by defaulting to
//! discard, so a guard dropped by a cancelled future, an early return or a
//! panic closes its connection. Reuse takes an explicit call at a point the
//! caller has established is safe.
//!
//! # Sans-I/O
//!
//! This module opens no sockets. It decides which connection a caller should
//! use, whether a new one may be opened, and who waits; the caller does the
//! connecting. That is what lets the release rule and the cap arithmetic be
//! tested exhaustively without a Postgres anywhere.
//!
//! The async [`pgprox_core::pool::UpstreamPool`] implementation wraps this and
//! adds the waiting, in `crate::live`.

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use pgprox_core::ids::{PoolKey, ServerId};
use pgprox_core::pool::{PoolError, PoolStats, ReleaseOutcome, UpstreamId};

use crate::statements::{ConnectionStatements, StatementConfig};

/// How a pool is tuned.
#[derive(Clone, Copy, Debug)]
pub struct PoolConfig {
    /// Connections this pool may hold at once.
    pub max_size: u32,
    /// Connections kept open when idle.
    ///
    /// Zero, and deliberately so. Idle upstream connections are what stops a
    /// tenant's fan-out across nodes from collapsing on its own: a node that
    /// held a floor of connections for every tenant it ever saw would hold the
    /// whole fleet's worth. See the crate's `AGENTS.md`.
    pub min_size: u32,
    /// Statement map tuning for connections this pool opens.
    pub statements: StatementConfig,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_size: 20,
            min_size: 0,
            statements: StatementConfig::default(),
        }
    }
}

/// One upstream connection the pool knows about.
#[derive(Clone, Debug)]
pub struct Connection {
    id: UpstreamId,
    /// Prepared statements this connection holds.
    pub statements: ConnectionStatements,
    /// When it last went idle, for the reaper.
    idle_since: Option<Instant>,
}

impl Connection {
    /// Which connection this is.
    #[must_use]
    pub const fn id(&self) -> UpstreamId {
        self.id
    }

    /// When it went idle, or [`None`] if it is checked out.
    #[must_use]
    pub const fn idle_since(&self) -> Option<Instant> {
        self.idle_since
    }
}

/// What a caller should do to satisfy an acquire.
#[derive(Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Acquired {
    /// Use this connection. It is already open and now checked out.
    Reused(UpstreamId),
    /// Open a new connection and call [`Pool::opened`] with it.
    ///
    /// The slot is reserved before the caller starts connecting, so two
    /// callers racing at the cap cannot both be told to open. A caller that
    /// fails to connect must call [`Pool::open_failed`] to give the slot back.
    OpenNew,
    /// The pool is full and every connection is busy. Wait, or give up at the
    /// deadline.
    Wait,
}

/// A pool of upstream connections for one key.
///
/// Sans-I/O: it decides, the caller connects.
#[derive(Debug)]
pub struct Pool {
    config: PoolConfig,
    key: PoolKey,
    /// Connections not currently checked out, most recently used last.
    ///
    /// Reused from the back, so a busy pool keeps a small set of connections
    /// warm and the rest age out. Reusing from the front would touch every
    /// connection in turn and keep them all just barely alive.
    idle: VecDeque<Connection>,
    /// Connections currently checked out.
    checked_out: HashMap<UpstreamId, Connection>,
    /// Slots reserved for connections being opened right now.
    opening: u32,
    /// Callers waiting for a connection.
    waiting: u32,
    /// The cap in force, which the cluster layer may lower below `max_size`.
    limit: u32,
    next_id: u64,
}

impl Pool {
    /// An empty pool for one key.
    #[must_use]
    pub fn new(key: PoolKey, config: PoolConfig) -> Self {
        Self {
            config,
            key,
            idle: VecDeque::new(),
            checked_out: HashMap::new(),
            opening: 0,
            waiting: 0,
            limit: config.max_size,
            next_id: 1,
        }
    }

    /// Which pool this is.
    #[must_use]
    pub const fn key(&self) -> &PoolKey {
        &self.key
    }

    /// The server this pool connects to.
    #[must_use]
    pub const fn server(&self) -> &ServerId {
        &self.key.server
    }

    /// The cap currently in force.
    #[must_use]
    pub const fn limit(&self) -> u32 {
        self.limit
    }

    /// Lowers or raises the cap, within `max_size`.
    ///
    /// This is how the cluster layer's allowance reaches the pool. Lowering it
    /// below what is already open does not close anything: connections in use
    /// are finishing real transactions, and killing them would turn a quota
    /// change into a client-visible error. The pool simply stops opening more
    /// until it has drained under the new cap.
    pub fn set_limit(&mut self, limit: u32) {
        self.limit = limit.min(self.config.max_size);
    }

    /// Connections open or being opened.
    #[must_use]
    pub fn total(&self) -> u32 {
        let idle = u32::try_from(self.idle.len()).unwrap_or(u32::MAX);
        let busy = u32::try_from(self.checked_out.len()).unwrap_or(u32::MAX);
        idle.saturating_add(busy).saturating_add(self.opening)
    }

    /// A snapshot for the admin surface.
    #[must_use]
    pub fn stats(&self) -> PoolStats {
        PoolStats {
            active: u32::try_from(self.checked_out.len()).unwrap_or(u32::MAX),
            idle: u32::try_from(self.idle.len()).unwrap_or(u32::MAX),
            waiting: self.waiting,
            limit: self.limit,
        }
    }

    /// Decides how to satisfy an acquire.
    ///
    /// Prefers a warm connection, then opening a new one, then waiting.
    pub fn acquire(&mut self) -> Acquired {
        if let Some(mut connection) = self.idle.pop_back() {
            connection.idle_since = None;
            let id = connection.id;
            self.checked_out.insert(id, connection);
            return Acquired::Reused(id);
        }

        if self.total() < self.limit {
            // Reserved before the caller starts connecting. Two callers racing
            // at the cap must not both be told to open, or the pool briefly
            // exceeds a limit the cluster layer promised the server.
            self.opening += 1;
            return Acquired::OpenNew;
        }

        Acquired::Wait
    }

    /// Records that a connection the caller was told to open is now open.
    ///
    /// Returns its id, already checked out to the caller that opened it.
    pub fn opened(&mut self) -> UpstreamId {
        self.opening = self.opening.saturating_sub(1);
        let id = UpstreamId(self.next_id);
        self.next_id += 1;
        self.checked_out.insert(
            id,
            Connection {
                id,
                statements: ConnectionStatements::new(self.config.statements),
                idle_since: None,
            },
        );
        id
    }

    /// Records that opening failed, giving the reserved slot back.
    ///
    /// Without this a failing upstream would permanently shrink the pool: every
    /// attempt would hold a slot no connection ever occupies.
    pub fn open_failed(&mut self) {
        self.opening = self.opening.saturating_sub(1);
    }

    /// Returns a connection according to the outcome its guard carried.
    ///
    /// [`ReleaseOutcome::Discard`] drops it, which is what a release outside a
    /// transaction boundary must do. Returns whether the connection went back
    /// into the pool.
    pub fn release(&mut self, id: UpstreamId, outcome: ReleaseOutcome, now: Instant) -> bool {
        let Some(mut connection) = self.checked_out.remove(&id) else {
            // Not ours, or already released. Releasing twice must not
            // resurrect a connection or corrupt the count.
            return false;
        };

        match outcome {
            ReleaseOutcome::Reusable => {
                connection.idle_since = Some(now);
                self.idle.push_back(connection);
                true
            }
            // Dropped. The socket is the caller's to close. `ReleaseOutcome`
            // is `#[non_exhaustive]`, so a variant added later lands here and
            // discards, which is the safe direction: a new outcome nobody has
            // taught this pool about must not recycle a connection.
            _ => false,
        }
    }

    /// Records a caller starting to wait.
    pub fn begin_wait(&mut self) {
        self.waiting = self.waiting.saturating_add(1);
    }

    /// Records a caller finishing its wait, successfully or not.
    pub fn end_wait(&mut self) {
        self.waiting = self.waiting.saturating_sub(1);
    }

    /// The error a caller that has given up should report.
    #[must_use]
    pub fn give_up(&self, waited: std::time::Duration) -> PoolError {
        // At the cap and timed out are different things to an operator: one
        // says the server is full, the other says this node is. Reporting the
        // cap when the pool has headroom would send them to the wrong place.
        if self.total() >= self.limit {
            PoolError::AtCap {
                server: self.key.server.clone(),
                cap: self.limit,
            }
        } else {
            PoolError::Timeout {
                server: self.key.server.clone(),
                waited,
            }
        }
    }

    /// A checked-out connection, for its statement map.
    #[must_use]
    pub fn checked_out(&self, id: UpstreamId) -> Option<&Connection> {
        self.checked_out.get(&id)
    }

    /// A checked-out connection, mutably.
    pub fn checked_out_mut(&mut self, id: UpstreamId) -> Option<&mut Connection> {
        self.checked_out.get_mut(&id)
    }

    /// Idle connections, oldest first.
    pub fn idle(&self) -> impl Iterator<Item = &Connection> {
        self.idle.iter()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn key() -> PoolKey {
        PoolKey::new(ServerId::new("db-1", 5432), "tenant_acme", "acme_app")
    }

    fn pool(max: u32) -> Pool {
        Pool::new(
            key(),
            PoolConfig {
                max_size: max,
                ..PoolConfig::default()
            },
        )
    }

    /// Acquires and completes an open, as a caller would.
    fn open(pool: &mut Pool) -> UpstreamId {
        assert_eq!(pool.acquire(), Acquired::OpenNew);
        pool.opened()
    }

    #[test]
    fn an_empty_pool_opens_rather_than_waiting() {
        let mut pool = pool(4);
        assert_eq!(pool.acquire(), Acquired::OpenNew);
        let id = pool.opened();
        assert_eq!(pool.stats().active, 1);
        assert_eq!(pool.stats().idle, 0);
        assert_eq!(pool.checked_out(id).unwrap().id(), id);
    }

    #[test]
    fn a_connection_is_never_released_mid_transaction() {
        // The rule this milestone is named for. A connection released outside a
        // transaction boundary is closed, not returned: handing one sitting
        // inside someone else's transaction to a second client would give them
        // a session already holding locks, part-way through work they know
        // nothing about, and nothing about it would look like an error.
        let now = Instant::now();
        let mut pool = pool(4);
        let id = open(&mut pool);

        assert!(
            !pool.release(id, ReleaseOutcome::Discard, now),
            "a connection released mid-transaction went back into the pool"
        );
        assert_eq!(pool.stats().idle, 0);
        assert_eq!(pool.stats().active, 0);
        assert_eq!(pool.total(), 0);

        // And the clean case, so the test is not passing by refusing every
        // release.
        let id = open(&mut pool);
        assert!(pool.release(id, ReleaseOutcome::Reusable, now));
        assert_eq!(pool.stats().idle, 1);
    }

    #[test]
    fn a_released_connection_is_reused_rather_than_reopened() {
        let now = Instant::now();
        let mut pool = pool(4);
        let first = open(&mut pool);
        pool.release(first, ReleaseOutcome::Reusable, now);

        assert_eq!(
            pool.acquire(),
            Acquired::Reused(first),
            "a warm connection was left idle while a new one was opened"
        );
        assert_eq!(pool.total(), 1);
    }

    #[test]
    fn the_most_recently_used_connection_is_reused_first() {
        // A busy pool keeps a small set warm and lets the rest age out.
        // Reusing the oldest would touch every connection in turn and keep them
        // all just barely alive, which is the opposite of what the reaper
        // wants.
        let now = Instant::now();
        let mut pool = pool(4);
        let a = open(&mut pool);
        let b = open(&mut pool);
        pool.release(a, ReleaseOutcome::Reusable, now);
        pool.release(b, ReleaseOutcome::Reusable, now);

        assert_eq!(pool.acquire(), Acquired::Reused(b));
        assert_eq!(pool.acquire(), Acquired::Reused(a));
    }

    #[test]
    fn the_pool_waits_rather_than_opening_past_its_limit() {
        let mut pool = pool(2);
        open(&mut pool);
        open(&mut pool);

        assert_eq!(pool.acquire(), Acquired::Wait);
        assert_eq!(pool.total(), 2);
    }

    #[test]
    fn a_slot_is_reserved_before_the_caller_connects() {
        // Two callers racing at the cap must not both be told to open, or the
        // pool briefly exceeds a limit the cluster layer promised the server.
        let mut pool = pool(1);
        assert_eq!(pool.acquire(), Acquired::OpenNew);
        assert_eq!(
            pool.acquire(),
            Acquired::Wait,
            "two callers were told to open a single remaining slot"
        );
        assert_eq!(pool.total(), 1, "a reserved slot did not count");
    }

    #[test]
    fn a_failed_open_gives_the_slot_back() {
        // Without this a failing upstream permanently shrinks the pool: every
        // attempt holds a slot no connection ever occupies.
        let mut pool = pool(1);
        assert_eq!(pool.acquire(), Acquired::OpenNew);
        pool.open_failed();

        assert_eq!(pool.total(), 0);
        assert_eq!(
            pool.acquire(),
            Acquired::OpenNew,
            "a failed connection attempt cost the pool a slot forever"
        );
    }

    #[test]
    fn releasing_an_unknown_connection_changes_nothing() {
        // Releasing twice must not resurrect a connection or corrupt the count.
        let now = Instant::now();
        let mut pool = pool(4);
        let id = open(&mut pool);
        assert!(pool.release(id, ReleaseOutcome::Reusable, now));

        assert!(!pool.release(id, ReleaseOutcome::Reusable, now));
        assert_eq!(pool.stats().idle, 1, "a double release duplicated it");
        assert!(!pool.release(UpstreamId(999), ReleaseOutcome::Reusable, now));
        assert_eq!(pool.stats().idle, 1);
    }

    #[test]
    fn the_cluster_layers_allowance_lowers_the_limit() {
        let mut pool = pool(10);
        assert_eq!(pool.limit(), 10);

        pool.set_limit(3);
        assert_eq!(pool.limit(), 3);
        for _ in 0..3 {
            open(&mut pool);
        }
        assert_eq!(pool.acquire(), Acquired::Wait);
    }

    #[test]
    fn the_limit_never_rises_above_max_size() {
        // max_size is the operator's configuration. A cluster allowance may
        // narrow it and must not widen it.
        let mut pool = pool(4);
        pool.set_limit(100);
        assert_eq!(pool.limit(), 4);
    }

    #[test]
    fn lowering_the_limit_does_not_close_connections_in_use() {
        // They are finishing real transactions. Killing them would turn a
        // quota change into a client-visible error.
        let now = Instant::now();
        let mut pool = pool(10);
        let a = open(&mut pool);
        let b = open(&mut pool);
        let c = open(&mut pool);

        pool.set_limit(1);
        assert_eq!(pool.stats().active, 3, "in-flight work was killed");
        assert_eq!(pool.acquire(), Acquired::Wait);

        // It drains under the new cap as work finishes.
        pool.release(a, ReleaseOutcome::Reusable, now);
        pool.release(b, ReleaseOutcome::Discard, now);
        pool.release(c, ReleaseOutcome::Discard, now);
        assert_eq!(pool.total(), 1);
    }

    #[test]
    fn stats_report_what_the_admin_surface_needs() {
        let now = Instant::now();
        let mut pool = pool(5);
        let a = open(&mut pool);
        open(&mut pool);
        pool.release(a, ReleaseOutcome::Reusable, now);
        pool.begin_wait();
        pool.begin_wait();

        let stats = pool.stats();
        assert_eq!(stats.active, 1);
        assert_eq!(stats.idle, 1);
        assert_eq!(stats.waiting, 2);
        assert_eq!(stats.limit, 5);
        assert_eq!(stats.total(), 2);
        assert!(stats.has_headroom());

        pool.end_wait();
        assert_eq!(pool.stats().waiting, 1);
    }

    #[test]
    fn the_waiter_count_does_not_go_negative() {
        // It is a metric an operator reads. Wrapping to four billion waiters
        // would look like an incident.
        let mut pool = pool(1);
        pool.end_wait();
        assert_eq!(pool.stats().waiting, 0);
    }

    #[test]
    fn giving_up_at_the_cap_and_on_a_timeout_are_different_errors() {
        // One says the server is full, the other says this node is slow.
        // Reporting the cap when the pool has headroom sends an operator to
        // the wrong place entirely.
        let mut pool = pool(1);
        open(&mut pool);
        assert!(matches!(
            pool.give_up(Duration::from_secs(1)),
            PoolError::AtCap { cap: 1, .. }
        ));

        let mut roomy = Pool::new(
            key(),
            PoolConfig {
                max_size: 4,
                ..PoolConfig::default()
            },
        );
        open(&mut roomy);
        assert!(matches!(
            roomy.give_up(Duration::from_secs(1)),
            PoolError::Timeout { .. }
        ));
    }

    #[test]
    fn a_connection_carries_its_own_statement_map() {
        // Which is what makes replay-on-acquire possible: the pool hands back a
        // connection that remembers what it has prepared.
        use crate::statements::{GlobalName, Preparation};

        let now = Instant::now();
        let mut pool = pool(4);
        let id = open(&mut pool);
        let global = GlobalName::for_sql("SELECT $1");

        let connection = pool.checked_out_mut(id).unwrap();
        assert_eq!(
            connection.statements.prepare_for(&global),
            Preparation::Replay { evict: Vec::new() }
        );

        pool.release(id, ReleaseOutcome::Reusable, now);
        assert_eq!(pool.acquire(), Acquired::Reused(id));
        assert_eq!(
            pool.checked_out_mut(id)
                .unwrap()
                .statements
                .prepare_for(&global),
            Preparation::AlreadyHeld,
            "a reused connection forgot what it had prepared"
        );
    }

    #[test]
    fn an_idle_connection_records_when_it_went_idle() {
        let now = Instant::now();
        let mut pool = pool(4);
        let id = open(&mut pool);
        assert_eq!(pool.checked_out(id).unwrap().idle_since(), None);

        pool.release(id, ReleaseOutcome::Reusable, now);
        assert_eq!(pool.idle().next().unwrap().idle_since(), Some(now));

        // Checking it out again clears the mark, so the reaper cannot close a
        // connection that is in use.
        pool.acquire();
        assert_eq!(pool.checked_out(id).unwrap().idle_since(), None);
    }

    #[test]
    fn the_minimum_pool_size_is_zero() {
        // Idle upstream connections are what stops a tenant's fan-out across
        // nodes collapsing on its own. A node holding a floor for every tenant
        // it ever saw would hold the whole fleet's worth.
        assert_eq!(PoolConfig::default().min_size, 0);
    }

    #[test]
    fn a_pool_reports_its_key_and_server() {
        let pool = pool(1);
        assert_eq!(pool.key(), &key());
        assert_eq!(pool.server(), &ServerId::new("db-1", 5432));
    }

    #[test]
    fn a_zero_limit_pool_only_waits() {
        // What a node with no cluster allowance looks like. It must refuse
        // rather than open, since the allowance is the cap promise.
        let mut pool = pool(4);
        pool.set_limit(0);
        assert_eq!(pool.acquire(), Acquired::Wait);
        assert_eq!(pool.total(), 0);
    }
}
