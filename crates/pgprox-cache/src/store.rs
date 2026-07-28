//! The store: what holds entries, expires them, and throws them out.
//!
//! # Bounded by bytes
//!
//! A cache bounded by entry count holds an unbounded amount of memory, because
//! nothing bounds the size of an entry: one `SELECT *` over a wide table is
//! worth ten thousand point lookups. This runs on a node whose whole design is
//! an argument about what a connection costs, so the budget is bytes and the
//! entry count is whatever fits.
//!
//! # One lock, for now, deliberately
//!
//! `M7.56` found 45% of this proxy's CPU in a single mutex, so a new one
//! deserves a sentence. The pool's lock is contended because callers *wait*
//! inside it: an acquire that finds the pool empty parks on a `Notify` while
//! holding the queue. Nothing here waits. The lock covers a hash lookup and a
//! map update, and every path through it is bounded by the work in this file.
//!
//! That is a different regime, not an exemption. If `M9.10`'s measurement finds
//! this lock in a profile, the answer is to shard by the hash of the key, which
//! is why nothing outside this module knows there is one lock rather than
//! sixteen.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use pgprox_core::cache::{CacheKey, CachedResult, QueryCache};
use pgprox_core::clock::Clock;
use pgprox_core::ids::TenantId;

/// What the cache has been doing, for metrics and for `SHOW CACHE`.
///
/// Counters rather than rates: the exporter divides, and a counter that was
/// reset to make a rate look right is a counter nobody can reason about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct CacheStats {
    /// Lookups that found a live entry.
    pub hits: u64,
    /// Lookups that found nothing.
    pub misses: u64,
    /// Lookups that found an entry past its TTL.
    ///
    /// Counted apart from a miss because they mean different things about the
    /// configuration: misses say the working set does not fit, expiries say
    /// the TTL is shorter than the reuse interval.
    pub expired: u64,
    /// Entries thrown out to stay inside the byte budget.
    pub evicted: u64,
    /// Entries dropped because a tenant wrote.
    pub invalidated: u64,
    /// Results too large to store at all.
    pub rejected: u64,
    /// Entries currently held.
    pub entries: u64,
    /// Bytes currently held, by the same accounting the budget uses.
    pub bytes: u64,
}

/// One stored result, with what the store needs to expire and rank it.
#[derive(Debug)]
struct Entry {
    value: CachedResult,
    /// When this stops being servable. ADR 0021's entire guarantee.
    expires_at: Instant,
    /// Where it sits in the recency order.
    seq: u64,
    /// What it costs, by the accounting in [`weigh`].
    bytes: usize,
}

/// The parts behind the lock.
#[derive(Debug, Default)]
struct Inner {
    entries: HashMap<CacheKey, Entry>,
    /// Recency order, least recent first.
    ///
    /// A `BTreeMap` keyed by a monotonic sequence rather than a scan for the
    /// minimum: eviction happens on the insert path, and a linear scan there
    /// would make the cost of a `put` depend on how full the cache is.
    lru: BTreeMap<u64, CacheKey>,
    next_seq: u64,
    bytes: usize,
    stats: CacheStats,
}

/// A query result cache bounded by bytes.
#[derive(Debug)]
pub struct Store {
    clock: Arc<dyn Clock>,
    max_bytes: usize,
    inner: Mutex<Inner>,
}

impl Store {
    /// Builds a cache holding at most `max_bytes`.
    #[must_use]
    pub fn new(clock: Arc<dyn Clock>, max_bytes: usize) -> Arc<Self> {
        Arc::new(Self {
            clock,
            max_bytes,
            inner: Mutex::new(Inner::default()),
        })
    }

    /// What it has been doing.
    #[must_use]
    pub fn stats(&self) -> CacheStats {
        let inner = self.lock();
        CacheStats {
            entries: inner.entries.len() as u64,
            bytes: inner.bytes as u64,
            ..inner.stats
        }
    }

    /// The byte budget it was built with.
    #[must_use]
    pub const fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Inner {
    /// Removes an entry and everything that pointed at it.
    ///
    /// The `lru` index and the byte total both have to move with the map or
    /// they drift, and a drifted byte total is a budget that stops meaning
    /// anything without ever failing a test that only checks the map.
    fn remove(&mut self, key: &CacheKey) -> Option<Entry> {
        let entry = self.entries.remove(key)?;
        self.lru.remove(&entry.seq);
        self.bytes -= entry.bytes;
        Some(entry)
    }

    /// Moves an entry to the front of the recency order.
    fn touch(&mut self, key: &CacheKey) {
        let seq = self.next_seq;
        self.next_seq += 1;
        if let Some(entry) = self.entries.get_mut(key) {
            self.lru.remove(&entry.seq);
            entry.seq = seq;
            self.lru.insert(seq, key.clone());
        }
    }

    /// Throws out least-recently-used entries until the budget is met.
    fn evict_to_fit(&mut self, max_bytes: usize) {
        while self.bytes > max_bytes {
            // The oldest key, or nothing left to throw out. The second is
            // unreachable while `bytes` and `entries` agree, and returning
            // rather than looping is what keeps a drift between them from
            // becoming a hang.
            let Some((_, key)) = self.lru.iter().next().map(|(s, k)| (*s, k.clone())) else {
                return;
            };
            self.remove(&key);
            self.stats.evicted += 1;
        }
    }
}

#[async_trait::async_trait]
impl QueryCache for Store {
    async fn get(&self, key: &CacheKey) -> Option<CachedResult> {
        let now = self.clock.now();
        let mut inner = self.lock();

        let Some(entry) = inner.entries.get(key) else {
            inner.stats.misses += 1;
            return None;
        };

        // Past its TTL, which is the one promise this cache makes. Removed
        // rather than left to the next eviction: an expired entry that stays
        // resident is memory spent on something that can never be served.
        if entry.expires_at <= now {
            inner.remove(key);
            inner.stats.expired += 1;
            return None;
        }

        let value = entry.value.clone();
        inner.touch(key);
        inner.stats.hits += 1;
        Some(value)
    }

    async fn put(&self, key: CacheKey, value: CachedResult) {
        let bytes = weigh(&key, &value);
        let now = self.clock.now();
        let mut inner = self.lock();

        // Bigger than the whole budget. Storing it would evict everything else
        // and then be evicted itself by the next insert, which is a cache that
        // does nothing but churn.
        if bytes > self.max_bytes {
            inner.stats.rejected += 1;
            return;
        }

        // A TTL far enough out to overflow the clock. Nothing sensible is
        // being asked for, and refusing is better than storing an entry that
        // never expires.
        let Some(expires_at) = now.checked_add(value.ttl) else {
            inner.stats.rejected += 1;
            return;
        };

        // Replacing rather than updating in place, so the byte total follows a
        // result that changed size.
        inner.remove(&key);

        let seq = inner.next_seq;
        inner.next_seq += 1;
        inner.bytes += bytes;
        inner.lru.insert(seq, key.clone());
        inner.entries.insert(
            key,
            Entry {
                value,
                expires_at,
                seq,
                bytes,
            },
        );

        inner.evict_to_fit(self.max_bytes);
    }

    async fn invalidate_tenant(&self, tenant: &TenantId) {
        let mut inner = self.lock();
        let doomed: Vec<CacheKey> = inner
            .entries
            .keys()
            .filter(|key| &key.tenant == tenant)
            .cloned()
            .collect();

        for key in doomed {
            inner.remove(&key);
            inner.stats.invalidated += 1;
        }
    }
}

/// What one entry costs, near enough to hold a budget to.
///
/// The heap behind the borrowed data, plus the two structs themselves. It does
/// not try to account for the hash map's own per-entry overhead or for an
/// `Arc`'s refcount header: the budget is a bound on the order of magnitude, so
/// being consistently a little low is better than being complicated. What it
/// must not do is ignore a field that a caller controls the size of, because
/// then a caller controls how much memory the budget fails to bound.
fn weigh(key: &CacheKey, value: &CachedResult) -> usize {
    let params: usize = key
        .params
        .iter()
        .map(|p| p.len() + size_of::<Vec<u8>>())
        .sum();

    size_of::<CacheKey>()
        + size_of::<CachedResult>()
        + key.tenant.as_str().len()
        + key.normalized_sql.len()
        + key.search_path.len()
        + params
        + value.frames.len()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::time::Duration;

    use pgprox_core::clock::FakeClock;

    use super::*;

    fn key(tenant: &str, sql: &str) -> CacheKey {
        CacheKey {
            tenant: TenantId::new(tenant),
            normalized_sql: Arc::from(sql),
            params: Vec::new(),
            search_path: Arc::from("public"),
        }
    }

    fn result(bytes: usize, ttl_ms: u64) -> CachedResult {
        CachedResult {
            frames: Arc::from(vec![0_u8; bytes].as_slice()),
            ttl: Duration::from_millis(ttl_ms),
        }
    }

    fn store(max_bytes: usize) -> (Arc<Store>, Arc<FakeClock>) {
        let clock = Arc::new(FakeClock::new());
        (Store::new(clock.clone(), max_bytes), clock)
    }

    #[tokio::test]
    async fn a_stored_result_comes_back() {
        let (cache, _clock) = store(64 * 1024);
        cache.put(key("acme", "SELECT 1"), result(16, 1000)).await;
        assert_eq!(
            cache.get(&key("acme", "SELECT 1")).await,
            Some(result(16, 1000))
        );
        assert_eq!(cache.stats().hits, 1);
    }

    #[tokio::test]
    async fn a_key_that_was_never_stored_is_a_miss_rather_than_an_expiry() {
        // The two are counted apart because they say different things: misses
        // mean the working set does not fit, expiries mean the TTL is shorter
        // than the reuse interval, and those have opposite fixes.
        let (cache, _clock) = store(64 * 1024);
        assert!(cache.get(&key("acme", "SELECT 1")).await.is_none());
        let stats = cache.stats();
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.expired, 0);
    }

    #[tokio::test]
    async fn an_entry_past_its_ttl_is_never_served() {
        // ADR 0021's whole guarantee, and the only one this cache makes.
        let (cache, clock) = store(64 * 1024);
        cache.put(key("acme", "SELECT 1"), result(16, 1000)).await;

        clock.advance(Duration::from_millis(999));
        assert!(
            cache.get(&key("acme", "SELECT 1")).await.is_some(),
            "expired a millisecond early"
        );

        clock.advance(Duration::from_millis(1));
        assert!(
            cache.get(&key("acme", "SELECT 1")).await.is_none(),
            "served an entry at exactly its TTL"
        );
        assert_eq!(cache.stats().expired, 1);
    }

    #[tokio::test]
    async fn an_expired_entry_stops_costing_memory_when_it_is_found() {
        // Left resident, it is memory spent on something that can never be
        // served, and the budget would evict live entries to make room for it.
        let (cache, clock) = store(64 * 1024);
        cache.put(key("acme", "SELECT 1"), result(1024, 10)).await;
        assert!(cache.stats().bytes >= 1024);

        clock.advance(Duration::from_millis(11));
        cache.get(&key("acme", "SELECT 1")).await;

        let stats = cache.stats();
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.bytes, 0, "an expired entry kept its bytes");
    }

    #[tokio::test]
    async fn the_budget_is_bytes_rather_than_entries() {
        // One large result costs what ten small ones do. A cache bounded by
        // count would hold either without noticing the difference.
        let (cache, _clock) = store(4096);
        cache.put(key("acme", "big"), result(3000, 1000)).await;
        assert_eq!(cache.stats().entries, 1);

        cache.put(key("acme", "also-big"), result(3000, 1000)).await;
        let stats = cache.stats();
        assert_eq!(stats.entries, 1, "two results over budget both stayed");
        assert!(stats.bytes <= 4096, "over budget: {} bytes", stats.bytes);
        assert_eq!(stats.evicted, 1);
    }

    #[tokio::test]
    async fn eviction_takes_the_least_recently_used() {
        let (cache, _clock) = store(4000);
        cache.put(key("acme", "a"), result(900, 1000)).await;
        cache.put(key("acme", "b"), result(900, 1000)).await;
        cache.put(key("acme", "c"), result(900, 1000)).await;

        // The setup, asserted rather than assumed. A budget that turned out to
        // fit two rather than three would make every conclusion below true for
        // the wrong reason, and the first version of this test did exactly
        // that: `weigh` adds about 115 bytes of struct and key to each 900-byte
        // result, so three of them did not fit in 3,000.
        assert_eq!(cache.stats().entries, 3, "the budget does not fit three");

        // Reading `a` makes `b` the oldest, so `b` is what the next insert
        // throws out. Without the touch on the read path this would evict `a`.
        assert!(cache.get(&key("acme", "a")).await.is_some());
        cache.put(key("acme", "d"), result(900, 1000)).await;

        assert!(
            cache.get(&key("acme", "a")).await.is_some(),
            "a was evicted"
        );
        assert!(cache.get(&key("acme", "b")).await.is_none(), "b survived");
        assert!(
            cache.get(&key("acme", "d")).await.is_some(),
            "d was not stored"
        );
    }

    #[tokio::test]
    async fn a_result_larger_than_the_whole_budget_is_refused() {
        // Storing it would evict everything and then be evicted by the next
        // insert, which is a cache that does nothing but churn.
        let (cache, _clock) = store(1024);
        cache.put(key("acme", "small"), result(64, 1000)).await;
        cache.put(key("acme", "huge"), result(8192, 1000)).await;

        let stats = cache.stats();
        assert_eq!(stats.rejected, 1);
        assert_eq!(stats.evicted, 0, "the refusal evicted something");
        assert!(
            cache.get(&key("acme", "small")).await.is_some(),
            "refusing a large result threw out a small one"
        );
    }

    #[tokio::test]
    async fn a_ttl_that_would_overflow_the_clock_is_refused() {
        // Nothing sensible is being asked for, and an entry that never expires
        // is the one thing ADR 0021 does not allow.
        let (cache, _clock) = store(64 * 1024);
        let forever = CachedResult {
            frames: Arc::from(vec![0_u8; 16].as_slice()),
            ttl: Duration::MAX,
        };
        cache.put(key("acme", "forever"), forever).await;

        assert_eq!(cache.stats().rejected, 1);
        assert!(cache.get(&key("acme", "forever")).await.is_none());
    }

    #[tokio::test]
    async fn storing_the_same_key_twice_replaces_rather_than_accumulates() {
        let (cache, _clock) = store(64 * 1024);
        cache.put(key("acme", "SELECT 1"), result(2048, 1000)).await;
        let after_first = cache.stats().bytes;

        cache.put(key("acme", "SELECT 1"), result(16, 1000)).await;
        let stats = cache.stats();

        assert_eq!(stats.entries, 1);
        assert!(
            stats.bytes < after_first,
            "a smaller replacement kept the old size: {} then {}",
            after_first,
            stats.bytes
        );
        assert_eq!(
            cache
                .get(&key("acme", "SELECT 1"))
                .await
                .unwrap()
                .frames
                .len(),
            16
        );
    }

    #[tokio::test]
    async fn invalidating_a_tenant_leaves_every_other_tenant_alone() {
        let (cache, _clock) = store(64 * 1024);
        cache.put(key("acme", "SELECT 1"), result(16, 1000)).await;
        cache.put(key("acme", "SELECT 2"), result(16, 1000)).await;
        cache.put(key("other", "SELECT 1"), result(16, 1000)).await;

        cache.invalidate_tenant(&TenantId::new("acme")).await;

        assert!(cache.get(&key("acme", "SELECT 1")).await.is_none());
        assert!(cache.get(&key("acme", "SELECT 2")).await.is_none());
        assert!(
            cache.get(&key("other", "SELECT 1")).await.is_some(),
            "invalidating one tenant dropped another's entries"
        );
        assert_eq!(cache.stats().invalidated, 2);
    }

    #[tokio::test]
    async fn invalidating_a_tenant_gives_its_bytes_back() {
        let (cache, _clock) = store(64 * 1024);
        cache.put(key("acme", "SELECT 1"), result(2048, 1000)).await;
        cache.invalidate_tenant(&TenantId::new("acme")).await;

        let stats = cache.stats();
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.bytes, 0, "invalidation leaked the byte total");
    }

    #[tokio::test]
    async fn invalidating_a_tenant_with_nothing_stored_does_nothing() {
        let (cache, _clock) = store(64 * 1024);
        cache.invalidate_tenant(&TenantId::new("nobody")).await;
        assert_eq!(cache.stats().invalidated, 0);
    }

    #[tokio::test]
    async fn two_tenants_running_the_same_sql_do_not_share_an_entry() {
        // The reason the tenant is in the key. Sharing here is one tenant
        // reading another's rows, which is the worst thing this crate could do.
        let (cache, _clock) = store(64 * 1024);
        cache.put(key("acme", "SELECT 1"), result(16, 1000)).await;
        assert!(
            cache.get(&key("other", "SELECT 1")).await.is_none(),
            "a tenant was served another tenant's entry"
        );
    }

    #[tokio::test]
    async fn the_same_sql_under_a_different_search_path_is_a_different_entry() {
        // The same reason, one level subtler: `SELECT * FROM t` names a
        // different table under a different path, so the text alone is not the
        // question the server was asked.
        let (cache, _clock) = store(64 * 1024);
        let mut other_path = key("acme", "SELECT * FROM t");
        other_path.search_path = Arc::from("tenant_acme");

        cache
            .put(key("acme", "SELECT * FROM t"), result(16, 1000))
            .await;
        assert!(
            cache.get(&other_path).await.is_none(),
            "search_path was not part of the key"
        );
    }

    #[tokio::test]
    async fn different_parameters_are_different_entries() {
        let (cache, _clock) = store(64 * 1024);
        let mut bound = key("acme", "SELECT $1");
        bound.params = vec![b"1".to_vec()];
        let mut other = key("acme", "SELECT $1");
        other.params = vec![b"2".to_vec()];

        cache.put(bound.clone(), result(16, 1000)).await;
        assert!(cache.get(&bound).await.is_some());
        assert!(
            cache.get(&other).await.is_none(),
            "two parameter values shared an entry"
        );
    }

    #[test]
    fn the_byte_total_counts_what_a_caller_controls() {
        // Whatever the accounting misses, it must not miss a field whose size
        // a caller chooses, or a caller chooses how much memory the budget
        // fails to bound. No store here: this is about `weigh` alone.
        let small = weigh(&key("acme", "a"), &result(16, 1000));

        let mut long_sql = key("acme", &"x".repeat(4096));
        long_sql.params = vec![vec![0; 4096]];
        long_sql.search_path = Arc::from("y".repeat(4096));
        let large = weigh(&long_sql, &result(4096, 1000));

        assert!(
            large > small + 16_000,
            "the accounting missed a caller-controlled field: {small} then {large}"
        );
    }

    #[tokio::test]
    async fn stats_report_what_is_held_rather_than_what_was_ever_stored() {
        let (cache, _clock) = store(64 * 1024);
        cache.put(key("acme", "a"), result(16, 1000)).await;
        cache.put(key("acme", "b"), result(16, 1000)).await;
        cache.invalidate_tenant(&TenantId::new("acme")).await;

        let stats = cache.stats();
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.invalidated, 2);
    }

    #[tokio::test]
    async fn the_budget_it_was_built_with_is_readable() {
        let (cache, _clock) = store(4096);
        assert_eq!(cache.max_bytes(), 4096);
    }
}
