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

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::Instant;

use pgprox_core::cache::{CacheKey, CachedResult, QueryCache};
use pgprox_core::clock::Clock;
use pgprox_core::config::QueryCacheConfig;
use pgprox_core::hash::IssuedIds;
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

/// One slot's place in the recency order, and whose entry it is.
///
/// Beside the entry map rather than inside it, and that is the whole reason
/// this is fast. Links held in the `Entry` mean every edit to a neighbour is a
/// lookup by key, so one hit hashes a six-field key up to four times; held
/// here, an edit is an index into a `Vec`.
#[derive(Clone, Debug, Default)]
struct Placed {
    /// The key whose entry occupies this slot, or [`None`] if it is free.
    key: Option<Arc<CacheKey>>,
    /// The slot used just before this one, or [`None`] at the least recently
    /// used end.
    older: Option<Slot>,
    /// The slot used just after this one, or [`None`] at the most recently
    /// used end.
    newer: Option<Slot>,
}

/// Where an entry sits, as an index this file issues.
///
/// A newtype so a slot cannot be passed where a byte count or a sequence was
/// wanted, which is the mistake a bare `u32` invites in a file that already
/// counts three different things.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
struct Slot(u32);

/// One stored result, with what the store needs to expire and rank it.
#[derive(Debug)]
struct Entry {
    value: CachedResult,
    /// When this stops being servable. ADR 0021's entire guarantee.
    expires_at: Instant,
    /// Where this entry sits, so removing it does not mean finding it again.
    slot: Slot,
    /// What it costs, by the accounting in [`weigh`].
    bytes: usize,
}

/// The parts behind the lock.
#[derive(Debug, Default)]
struct Inner {
    /// Keyed by a shared `Arc` rather than by the key itself, so the three
    /// places an entry is referenced from hold one allocation between them.
    /// A hit then costs one atomic increment where it used to cost six, and
    /// the map stores a pointer where it used to store six words. `M26.2`.
    entries: HashMap<Arc<CacheKey>, Entry>,
    /// The least and most recently used entries, or [`None`] when empty.
    ///
    /// # The recency order is a list, not a tree
    ///
    /// It was a `BTreeMap` keyed by a monotonic sequence, and a hit removed
    /// the entry's sequence and inserted a higher one. Two O(log n) traversals
    /// and, because the live range slid upward forever, nodes merging at one
    /// end and splitting at the other: roughly one heap block per seven hits.
    ///
    /// That was 894 instructions, 38% of a hit, measured by making `touch` a
    /// no-op and taking the difference. A list the entries hold between them
    /// costs four writes and allocates nothing. `M26.4`.
    ///
    /// Slots rather than keys, so unlinking a neighbour does not mean hashing
    /// its key to find it. That is the mistake the obvious version makes: an
    /// `Option<Arc<CacheKey>>` for each link turns one hit into three hash
    /// lookups, and holding the links inside the entries makes it two, because
    /// reaching an entry means going back through the map.
    least: Option<Slot>,
    most: Option<Slot>,
    /// Which keys each tenant holds.
    ///
    /// A second index, and it exists because of one number. Invalidation ran
    /// `entries.keys().filter(|k| &k.tenant == tenant)`, which walks every
    /// entry on the node and compares an `Arc<str>` by its contents, so a
    /// write cost 198,283 instructions at 4,096 entries across 64 tenants
    /// against 4,144 for a hit. Linear in the whole node rather than in the
    /// tenant, on the path `M9.10` counted 10,700 of against 20,000 lookups.
    /// `M26.1`.
    ///
    /// Sequence numbers rather than keys, and that is the whole cost of this
    /// index. Holding `CacheKey`s meant hashing all six of its fields a second
    /// time on every `put`, which the entry map had just done, and it showed:
    /// `put` went from 4,269 instructions to 6,951. A sequence is a `u64` the
    /// entry already has, so the set costs a `u64` hash, and invalidation
    /// reads the key back out of `lru`, where it already is.
    ///
    /// A tenant with nothing left is removed rather than left holding an empty
    /// set, so a node that has served five thousand tenants and now serves one
    /// does not keep five thousand entries here.
    ///
    /// The outer key keeps the default hasher and the inner set does not.
    /// `TenantId` arrives in a client's token, so a map keyed on it is exactly
    /// what `SipHash`'s per-process seed defends; a `Slot` is an index this file
    /// issues and there is nobody to defend against. `pgprox_core::hash` states
    /// the rule and this is both halves of it in one declaration. `M30.3`.
    by_tenant: HashMap<TenantId, HashSet<Slot, IssuedIds>>,
    /// The recency links and the key, by slot.
    ///
    /// Eviction and invalidation both start from a slot and need the key to
    /// take the entry out of `entries`, so the two directions have to be
    /// walkable. A `Vec` indexed by slot, with a free list, rather than a map:
    /// a slot is an index this file issues, so nothing needs hashing, and
    /// every link edit is an array write.
    slots: Vec<Placed>,
    /// Slots whose entries have gone, ready to be issued again.
    free: Vec<Slot>,
    bytes: usize,
    stats: CacheStats,
}

/// A query result cache bounded by bytes.
///
/// # Settings are live, not constructor arguments
///
/// A node is built once and runs for weeks, and its configuration is pulled
/// rather than pushed: an operator adding a tenant to a `ConfigMap` expects the
/// running node to start caching for it, not to need a restart. So the budget,
/// the TTL cap and the tenant list all live behind a lock the tick loop
/// replaces through [`Store::reconfigure`].
///
/// They are in a second lock rather than in `inner` because they are read on
/// every statement and written once a second, which is what an `RwLock` is for,
/// and because [`Store::serves`] has to answer without touching the entry map
/// at all.
#[derive(Debug)]
pub struct Store {
    clock: Arc<dyn Clock>,
    settings: RwLock<QueryCacheConfig>,
    inner: Mutex<Inner>,
}

impl Store {
    /// Builds a cache that serves nobody.
    ///
    /// Off, because that is what a node with no `query_cache` section is and
    /// the composition root builds this before it has read one.
    #[must_use]
    pub fn new(clock: Arc<dyn Clock>) -> Arc<Self> {
        Arc::new(Self {
            clock,
            settings: RwLock::new(QueryCacheConfig::default()),
            inner: Mutex::new(Inner::default()),
        })
    }

    /// Applies a new configuration.
    ///
    /// Three things happen, and the order matters: the settings are replaced,
    /// then every entry for a tenant that is no longer served is dropped, then
    /// what is left is evicted down to the new budget. A tenant taken out of
    /// the document has had its cache turned off, and leaving its results
    /// resident would mean an operator who revoked the opt-in still had a node
    /// holding that tenant's rows.
    pub fn reconfigure(&self, config: &QueryCacheConfig) {
        {
            let mut settings = self
                .settings
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            if *settings == *config {
                // The common case by far: a tick that changed nothing. Nothing
                // below would do anything either, and taking the entry lock
                // once a second for that is a lock a profile does not need to
                // show.
                return;
            }
            settings.clone_from(config);
        }

        let mut inner = self.lock();
        let doomed: Vec<Arc<CacheKey>> = inner
            .entries
            .keys()
            .filter(|key| !config.serves(&key.tenant))
            .cloned()
            .collect();
        for key in doomed {
            inner.remove(&key);
            inner.stats.invalidated += 1;
        }
        inner.evict_to_fit(config.max_bytes);
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

    /// The byte budget it is currently holding to.
    #[must_use]
    pub fn max_bytes(&self) -> usize {
        self.with_settings(|settings| settings.max_bytes)
    }

    /// Reads something out of the settings.
    ///
    /// A closure returning a small value rather than a method handing out the
    /// settings themselves: the tenant list is a map, and cloning it on every
    /// statement to answer one question about it is the kind of cost that only
    /// shows up under load.
    fn with_settings<T>(&self, read: impl FnOnce(&QueryCacheConfig) -> T) -> T {
        read(&self.settings.read().unwrap_or_else(PoisonError::into_inner))
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Inner {
    /// Removes an entry and everything that pointed at it.
    ///
    /// The recency order, the tenant index and the byte total all have to move
    /// with the map or they drift, and a drifted byte total is a budget that
    /// stops meaning anything without ever failing a test that only checks the
    /// map.
    fn remove(&mut self, key: &CacheKey) -> Option<Entry> {
        let entry = self.entries.remove(key)?;
        let slot = entry.slot;

        self.unlink(slot);

        // The tenant index, and an empty set goes with it: a node that has
        // served five thousand tenants and now serves one must not keep five
        // thousand entries here.
        if let Some(slots) = self.by_tenant.get_mut(&key.tenant) {
            slots.remove(&slot);
            if slots.is_empty() {
                self.by_tenant.remove(&key.tenant);
            }
        }

        self.slots[slot.0 as usize] = Placed::default();
        self.free.push(slot);
        self.bytes -= entry.bytes;
        Some(entry)
    }

    /// Issues a slot for a key, reusing one if any have been given back.
    fn claim(&mut self, key: &Arc<CacheKey>) -> Slot {
        let placed = Placed {
            key: Some(Arc::clone(key)),
            older: None,
            newer: None,
        };
        if let Some(slot) = self.free.pop() {
            self.slots[slot.0 as usize] = placed;
            return slot;
        }
        let slot = Slot(u32::try_from(self.slots.len()).unwrap_or(u32::MAX));
        self.slots.push(placed);
        slot
    }

    /// Files an entry's slot under its tenant.
    ///
    /// `entry` rather than a `get_mut` that falls back to it. The fallback
    /// looks cheaper, since a tenant is new to this map once and stores
    /// answers thousands of times, and it was measured and is not: two hash
    /// lookups on the cold path cost more than the `Arc` increment it saves,
    /// and `cache_put` went from 5,246 instructions to 5,284.
    fn index(&mut self, tenant: &TenantId, slot: Slot) {
        self.by_tenant
            .entry(tenant.clone())
            .or_default()
            .insert(slot);
    }

    /// Takes a slot out of the recency order.
    ///
    /// Leaves its own links alone: every caller either fills them in again or
    /// frees the slot, and clearing them here would be a write nobody reads.
    fn unlink(&mut self, slot: Slot) {
        let (older, newer) = {
            let placed = &self.slots[slot.0 as usize];
            (placed.older, placed.newer)
        };

        match older {
            Some(before) => self.slots[before.0 as usize].newer = newer,
            None => self.least = newer,
        }
        match newer {
            Some(after) => self.slots[after.0 as usize].older = older,
            None => self.most = older,
        }
    }

    /// Puts a slot at the most-recently-used end.
    fn link_newest(&mut self, slot: Slot) {
        let previous = self.most;
        if let Some(previous) = previous {
            self.slots[previous.0 as usize].newer = Some(slot);
        }

        let placed = &mut self.slots[slot.0 as usize];
        placed.older = previous;
        placed.newer = None;

        self.most = Some(slot);
        if self.least.is_none() {
            self.least = Some(slot);
        }
    }

    /// Moves an entry the caller has already found to the most recently used
    /// end.
    ///
    /// Takes the slot rather than the key, because the caller has just looked
    /// the entry up and a second lookup here means hashing all six of the
    /// key's fields again. That was 2,538 instructions of the difference
    /// between a hit and a miss. `M26.2`.
    fn touch(&mut self, slot: Slot) {
        // Already the most recent, which is the common case for a hot key and
        // the one worth not paying for at all.
        if self.most == Some(slot) {
            return;
        }
        self.unlink(slot);
        self.link_newest(slot);
    }

    /// Throws out least-recently-used entries until the budget is met.
    fn evict_to_fit(&mut self, max_bytes: usize) {
        while self.bytes > max_bytes {
            // The oldest slot, or nothing left to throw out. The second is
            // unreachable while `bytes` and `entries` agree, and returning
            // rather than looping is what keeps a drift between them from
            // becoming a hang.
            let Some(key) = self
                .least
                .and_then(|slot| self.slots[slot.0 as usize].key.clone())
            else {
                return;
            };
            self.remove(&key);
            self.stats.evicted += 1;
        }
    }

    /// The slots in recency order, least recently used first.
    ///
    /// For the tests that the order is an order. Nothing on the serving path
    /// walks it, which is the point of it being a list.
    #[cfg(test)]
    fn recency(&self) -> Vec<Slot> {
        let mut walked = Vec::new();
        let mut at = self.least;
        while let Some(slot) = at {
            walked.push(slot);
            at = self.slots[slot.0 as usize].newer;
            assert!(
                walked.len() <= self.entries.len(),
                "the recency order has a cycle in it"
            );
        }
        walked
    }
}

impl QueryCache for Store {
    fn serves(&self, tenant: &TenantId) -> bool {
        self.with_settings(|settings| settings.serves(tenant))
    }

    fn get(&self, key: &CacheKey) -> Option<CachedResult> {
        // Not counted as a miss. A miss says the working set does not fit;
        // this says the tenant is not using the cache, and mixing them makes
        // the hit rate of the tenants that did opt in unreadable.
        if !self.serves(&key.tenant) {
            return None;
        }

        let now = self.clock.now();
        let mut inner = self.lock();

        // One lookup, and everything a hit needs taken out of it before the
        // borrow ends. `get` then `touch(key)` hashed all six of the key's
        // fields twice per hit, which was most of why a hit cost two and a
        // half times a miss on a structure whose whole argument is that a hit
        // is the cheap path. `M26.2`.
        let Some(entry) = inner.entries.get_mut(key) else {
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

        // Everything a hit needs, read while the entry is still borrowed,
        // since `touch` needs the map back.
        let value = entry.value.clone();
        let slot = entry.slot;

        inner.touch(slot);
        inner.stats.hits += 1;
        Some(value)
    }

    fn put(&self, key: CacheKey, value: CachedResult) {
        // What the tenant is configured for, and nothing stored for a tenant
        // that is not. This is where ADR 0021's bound is actually applied:
        // whatever the caller asked for, the entry expires no later than the
        // configured TTL, which is itself already bounded by the operator's
        // cap.
        let (Some(granted), max_bytes) =
            self.with_settings(|settings| (settings.ttl_for(&key.tenant), settings.max_bytes))
        else {
            return;
        };
        // Rewritten rather than only used to compute `expires_at`, so what a
        // hit reports as its TTL is what the entry actually got.
        let value = CachedResult {
            ttl: value.ttl.min(granted),
            ..value
        };

        let bytes = weigh(&key, &value);
        let now = self.clock.now();
        let mut inner = self.lock();

        // Bigger than the whole budget. Storing it would evict everything else
        // and then be evicted itself by the next insert, which is a cache that
        // does nothing but churn.
        if bytes > max_bytes {
            inner.stats.rejected += 1;
            return;
        }

        // A TTL far enough out to overflow the clock. Unreachable through a
        // configuration a document produced, because `parse_duration` refuses
        // one that long, and kept because `reconfigure` takes a `Config` and
        // this file is not the place that gets to assume where it came from.
        let Some(expires_at) = now.checked_add(value.ttl) else {
            inner.stats.rejected += 1;
            return;
        };

        // Replacing rather than updating in place, so the byte total follows a
        // result that changed size.
        inner.remove(&key);

        // One allocation, shared by the entry map, the recency order and the
        // entry's own handle on it. See `Entry::key`.
        let key = Arc::new(key);
        let slot = inner.claim(&key);
        inner.bytes += bytes;
        inner.index(&key.tenant, slot);
        inner.entries.insert(
            Arc::clone(&key),
            Entry {
                value,
                expires_at,
                slot,
                bytes,
            },
        );
        // After the entry is in the map, because linking edits its neighbours
        // through it.
        inner.link_newest(slot);

        inner.evict_to_fit(max_bytes);
    }

    fn invalidate_tenant(&self, tenant: &TenantId) {
        let mut inner = self.lock();

        // The tenant's own keys, taken whole. This walked every entry on the
        // node and compared a string per entry until `M26.1`, which is a cost
        // paid by every write on behalf of every other tenant.
        //
        // Taken out rather than borrowed, because `remove` needs the map and
        // would otherwise be borrowing it twice. It also leaves the index
        // correct if this returns early: the entry for a tenant being emptied
        // is exactly the one `remove` would have deleted anyway.
        let Some(doomed) = inner.by_tenant.remove(tenant) else {
            return;
        };

        for slot in doomed {
            // The slot names the key, which is how a second copy is avoided. A
            // slot naming nothing would be the two indexes disagreeing, which
            // `the_tenant_index_holds_exactly_what_the_entry_map_holds` is
            // what stops.
            let Some(key) = inner.slots[slot.0 as usize].key.clone() else {
                continue;
            };
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
    size_of::<CacheKey>()
        + size_of::<CachedResult>()
        + key.tenant.as_str().len()
        + key.database.len()
        + key.user.len()
        + key.normalized_sql.len()
        + key.search_path.len()
        + key.params.len()
        + value.frames.len()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::time::Duration;

    use pgprox_core::clock::FakeClock;
    use pgprox_core::config::TenantCache;

    use super::*;

    fn key(tenant: &str, sql: &str) -> CacheKey {
        CacheKey {
            tenant: TenantId::new(tenant),
            database: Arc::from("tenant_db"),
            user: Arc::from("app"),
            normalized_sql: Arc::from(sql),
            params: Arc::from(&[][..]),
            search_path: Arc::from("public"),
        }
    }

    fn result(bytes: usize, ttl_ms: u64) -> CachedResult {
        CachedResult {
            frames: Arc::from(vec![0_u8; bytes].as_slice()),
            ttl: Duration::from_millis(ttl_ms),
        }
    }

    /// A configuration serving `tenants`, with room for anything a test asks.
    ///
    /// An hour, so the cap is never what a test below is measuring. The tests
    /// that are about the cap set their own.
    fn config(max_bytes: usize, tenants: &[&str]) -> QueryCacheConfig {
        QueryCacheConfig {
            max_bytes,
            max_entry_bytes: QueryCacheConfig::default().max_entry_bytes,
            ttl_cap: Duration::from_secs(3600),
            tenants: tenants
                .iter()
                .map(|name| {
                    (
                        TenantId::new(name),
                        TenantCache {
                            ttl: Duration::from_secs(3600),
                        },
                    )
                })
                .collect(),
        }
    }

    /// A store serving the two tenants the tests below use.
    fn store(max_bytes: usize) -> (Arc<Store>, Arc<FakeClock>) {
        let clock = Arc::new(FakeClock::new());
        let store = Store::new(clock.clone());
        store.reconfigure(&config(max_bytes, &["acme", "other"]));
        (store, clock)
    }

    #[test]
    fn a_stored_result_comes_back() {
        let (cache, _clock) = store(64 * 1024);
        cache.put(key("acme", "SELECT 1"), result(16, 1000));
        assert_eq!(cache.get(&key("acme", "SELECT 1")), Some(result(16, 1000)));
        assert_eq!(cache.stats().hits, 1);
    }

    #[test]
    fn a_key_that_was_never_stored_is_a_miss_rather_than_an_expiry() {
        // The two are counted apart because they say different things: misses
        // mean the working set does not fit, expiries mean the TTL is shorter
        // than the reuse interval, and those have opposite fixes.
        let (cache, _clock) = store(64 * 1024);
        assert!(cache.get(&key("acme", "SELECT 1")).is_none());
        let stats = cache.stats();
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.expired, 0);
    }

    #[test]
    fn an_entry_past_its_ttl_is_never_served() {
        // ADR 0021's whole guarantee, and the only one this cache makes.
        let (cache, clock) = store(64 * 1024);
        cache.put(key("acme", "SELECT 1"), result(16, 1000));

        clock.advance(Duration::from_millis(999));
        assert!(
            cache.get(&key("acme", "SELECT 1")).is_some(),
            "expired a millisecond early"
        );

        clock.advance(Duration::from_millis(1));
        assert!(
            cache.get(&key("acme", "SELECT 1")).is_none(),
            "served an entry at exactly its TTL"
        );
        assert_eq!(cache.stats().expired, 1);
    }

    #[test]
    fn an_expired_entry_stops_costing_memory_when_it_is_found() {
        // Left resident, it is memory spent on something that can never be
        // served, and the budget would evict live entries to make room for it.
        let (cache, clock) = store(64 * 1024);
        cache.put(key("acme", "SELECT 1"), result(1024, 10));
        assert!(cache.stats().bytes >= 1024);

        clock.advance(Duration::from_millis(11));
        cache.get(&key("acme", "SELECT 1"));

        let stats = cache.stats();
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.bytes, 0, "an expired entry kept its bytes");
    }

    #[test]
    fn the_budget_is_bytes_rather_than_entries() {
        // One large result costs what ten small ones do. A cache bounded by
        // count would hold either without noticing the difference.
        let (cache, _clock) = store(4096);
        cache.put(key("acme", "big"), result(3000, 1000));
        assert_eq!(cache.stats().entries, 1);

        cache.put(key("acme", "also-big"), result(3000, 1000));
        let stats = cache.stats();
        assert_eq!(stats.entries, 1, "two results over budget both stayed");
        assert!(stats.bytes <= 4096, "over budget: {} bytes", stats.bytes);
        assert_eq!(stats.evicted, 1);
    }

    #[test]
    fn eviction_takes_the_least_recently_used() {
        let (cache, _clock) = store(4000);
        cache.put(key("acme", "a"), result(900, 1000));
        cache.put(key("acme", "b"), result(900, 1000));
        cache.put(key("acme", "c"), result(900, 1000));

        // The setup, asserted rather than assumed. A budget that turned out to
        // fit two rather than three would make every conclusion below true for
        // the wrong reason, and the first version of this test did exactly
        // that: `weigh` adds about 115 bytes of struct and key to each 900-byte
        // result, so three of them did not fit in 3,000.
        assert_eq!(cache.stats().entries, 3, "the budget does not fit three");

        // Reading `a` makes `b` the oldest, so `b` is what the next insert
        // throws out. Without the touch on the read path this would evict `a`.
        assert!(cache.get(&key("acme", "a")).is_some());
        cache.put(key("acme", "d"), result(900, 1000));

        assert!(cache.get(&key("acme", "a")).is_some(), "a was evicted");
        assert!(cache.get(&key("acme", "b")).is_none(), "b survived");
        assert!(cache.get(&key("acme", "d")).is_some(), "d was not stored");
    }

    #[test]
    fn a_result_larger_than_the_whole_budget_is_refused() {
        // Storing it would evict everything and then be evicted by the next
        // insert, which is a cache that does nothing but churn.
        let (cache, _clock) = store(1024);
        cache.put(key("acme", "small"), result(64, 1000));
        cache.put(key("acme", "huge"), result(8192, 1000));

        let stats = cache.stats();
        assert_eq!(stats.rejected, 1);
        assert_eq!(stats.evicted, 0, "the refusal evicted something");
        assert!(
            cache.get(&key("acme", "small")).is_some(),
            "refusing a large result threw out a small one"
        );
    }

    #[test]
    fn a_caller_asking_to_keep_something_forever_gets_the_configured_ttl() {
        // The relay asks for `Duration::MAX`, meaning it has no staleness
        // bound of its own. ADR 0021's bound is the configured one, applied
        // here, and this is what makes an entry that never expires
        // unrepresentable rather than merely refused.
        let clock = Arc::new(FakeClock::new());
        let cache = Store::new(clock.clone());
        cache.reconfigure(&QueryCacheConfig {
            ttl_cap: Duration::from_secs(30),
            ..config(64 * 1024, &["acme"])
        });

        let forever = CachedResult {
            frames: Arc::from(vec![0_u8; 16].as_slice()),
            ttl: Duration::MAX,
        };
        cache.put(key("acme", "forever"), forever);

        assert_eq!(cache.stats().rejected, 0, "the entry was refused, not cut");
        // Cut to the cap, and reported as the TTL it actually got rather than
        // the one that was asked for.
        assert_eq!(
            cache.get(&key("acme", "forever")).unwrap().ttl,
            Duration::from_secs(30)
        );

        clock.advance(Duration::from_secs(30));
        assert!(
            cache.get(&key("acme", "forever")).is_none(),
            "an entry outlived the cap"
        );
    }

    #[test]
    fn a_caller_asking_for_less_than_the_cap_keeps_its_own_number() {
        // The cap is a ceiling, not a setting. A caller with a shorter bound
        // of its own keeps it.
        let (cache, clock) = store(64 * 1024);
        cache.put(key("acme", "brief"), result(16, 50));

        clock.advance(Duration::from_millis(50));
        assert!(
            cache.get(&key("acme", "brief")).is_none(),
            "a short TTL was stretched to the configured one"
        );
    }

    #[test]
    fn a_fresh_store_serves_nobody() {
        // What the composition root builds before it has read a document, and
        // what a node with no `query_cache` section keeps for its whole life.
        let cache = Store::new(Arc::new(FakeClock::new()));
        assert!(!cache.serves(&TenantId::new("acme")));

        cache.put(key("acme", "SELECT 1"), result(16, 1000));
        assert!(cache.get(&key("acme", "SELECT 1")).is_none());

        let stats = cache.stats();
        assert_eq!(stats.entries, 0);
        // Not counted as a miss or a rejection. A miss says the working set
        // does not fit and a rejection says a result was too big; neither is
        // true of a node that was never turned on, and counting them here
        // would make the hit rate of a node that is on unreadable.
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.rejected, 0);
    }

    #[test]
    fn a_tenant_that_never_opted_in_is_not_served_by_a_store_that_is_on() {
        let (cache, _clock) = store(64 * 1024);
        assert!(cache.serves(&TenantId::new("acme")));
        assert!(!cache.serves(&TenantId::new("globex")));

        cache.put(key("globex", "SELECT 1"), result(16, 1000));
        assert!(cache.get(&key("globex", "SELECT 1")).is_none());
        assert_eq!(cache.stats().entries, 0);
    }

    #[test]
    fn a_document_adding_a_tenant_starts_serving_it_without_a_restart() {
        // The acceptance for hot reload, at the level this crate owns: the
        // same store, no rebuild, and a tenant that was refused a moment ago.
        let (cache, _clock) = store(64 * 1024);
        assert!(!cache.serves(&TenantId::new("globex")));

        cache.reconfigure(&config(64 * 1024, &["acme", "other", "globex"]));

        assert!(cache.serves(&TenantId::new("globex")));
        cache.put(key("globex", "SELECT 1"), result(16, 1000));
        assert!(cache.get(&key("globex", "SELECT 1")).is_some());
    }

    #[test]
    fn a_tenant_taken_out_of_the_document_leaves_nothing_behind() {
        // An opt-in that was revoked. Leaving the entries resident would mean
        // an operator who turned a tenant's cache off still had a node holding
        // that tenant's rows, which is the thing they turned it off to stop.
        let (cache, _clock) = store(64 * 1024);
        cache.put(key("acme", "SELECT 1"), result(1024, 1000));
        cache.put(key("other", "SELECT 1"), result(1024, 1000));
        assert_eq!(cache.stats().entries, 2);

        cache.reconfigure(&config(64 * 1024, &["other"]));

        let stats = cache.stats();
        assert_eq!(stats.entries, 1);
        assert!(
            cache.get(&key("other", "SELECT 1")).is_some(),
            "the tenant that stayed lost its entries"
        );
        assert_eq!(stats.invalidated, 1);
        // And the bytes came back, rather than a budget that slowly stops
        // meaning anything as tenants come and go.
        assert!(stats.bytes < 1200, "held {} bytes", stats.bytes);
    }

    #[test]
    fn lowering_the_budget_evicts_down_to_it_immediately() {
        // Rather than at the next insert. An operator lowering this is usually
        // doing it because the node is using too much memory now.
        let (cache, _clock) = store(64 * 1024);
        for name in ["a", "b", "c", "d"] {
            cache.put(key("acme", name), result(1024, 1000));
        }
        assert_eq!(cache.stats().entries, 4);

        cache.reconfigure(&config(2400, &["acme", "other"]));

        let stats = cache.stats();
        assert!(stats.bytes <= 2400, "still holding {} bytes", stats.bytes);
        assert!(stats.evicted >= 2, "evicted {}", stats.evicted);
        assert_eq!(cache.max_bytes(), 2400);
    }

    #[test]
    fn a_reconfigure_that_changes_nothing_disturbs_nothing() {
        // The common case: a tick a second, for weeks. If this evicted or
        // counted anything the cache would be emptied by its own config loop.
        let (cache, _clock) = store(64 * 1024);
        cache.put(key("acme", "SELECT 1"), result(16, 1000));

        for _ in 0..10 {
            cache.reconfigure(&config(64 * 1024, &["acme", "other"]));
        }

        let stats = cache.stats();
        assert_eq!(stats.entries, 1);
        assert_eq!(stats.evicted, 0);
        assert_eq!(stats.invalidated, 0);
        assert!(cache.get(&key("acme", "SELECT 1")).is_some());
    }

    #[test]
    fn storing_the_same_key_twice_replaces_rather_than_accumulates() {
        let (cache, _clock) = store(64 * 1024);
        cache.put(key("acme", "SELECT 1"), result(2048, 1000));
        let after_first = cache.stats().bytes;

        cache.put(key("acme", "SELECT 1"), result(16, 1000));
        let stats = cache.stats();

        assert_eq!(stats.entries, 1);
        assert!(
            stats.bytes < after_first,
            "a smaller replacement kept the old size: {} then {}",
            after_first,
            stats.bytes
        );
        assert_eq!(
            cache.get(&key("acme", "SELECT 1")).unwrap().frames.len(),
            16
        );
    }

    #[test]
    fn invalidating_a_tenant_leaves_every_other_tenant_alone() {
        let (cache, _clock) = store(64 * 1024);
        cache.put(key("acme", "SELECT 1"), result(16, 1000));
        cache.put(key("acme", "SELECT 2"), result(16, 1000));
        cache.put(key("other", "SELECT 1"), result(16, 1000));

        cache.invalidate_tenant(&TenantId::new("acme"));

        assert!(cache.get(&key("acme", "SELECT 1")).is_none());
        assert!(cache.get(&key("acme", "SELECT 2")).is_none());
        assert!(
            cache.get(&key("other", "SELECT 1")).is_some(),
            "invalidating one tenant dropped another's entries"
        );
        assert_eq!(cache.stats().invalidated, 2);
    }

    #[test]
    fn invalidating_a_tenant_gives_its_bytes_back() {
        let (cache, _clock) = store(64 * 1024);
        cache.put(key("acme", "SELECT 1"), result(2048, 1000));
        cache.invalidate_tenant(&TenantId::new("acme"));

        let stats = cache.stats();
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.bytes, 0, "invalidation leaked the byte total");
    }

    #[test]
    fn invalidating_a_tenant_with_nothing_stored_does_nothing() {
        let (cache, _clock) = store(64 * 1024);
        cache.invalidate_tenant(&TenantId::new("nobody"));
        assert_eq!(cache.stats().invalidated, 0);
    }

    #[test]
    fn two_tenants_running_the_same_sql_do_not_share_an_entry() {
        // The reason the tenant is in the key. Sharing here is one tenant
        // reading another's rows, which is the worst thing this crate could do.
        let (cache, _clock) = store(64 * 1024);
        cache.put(key("acme", "SELECT 1"), result(16, 1000));
        assert!(
            cache.get(&key("other", "SELECT 1")).is_none(),
            "a tenant was served another tenant's entry"
        );
    }

    #[test]
    fn the_same_sql_under_a_different_search_path_is_a_different_entry() {
        // The same reason, one level subtler: `SELECT * FROM t` names a
        // different table under a different path, so the text alone is not the
        // question the server was asked.
        let (cache, _clock) = store(64 * 1024);
        let mut other_path = key("acme", "SELECT * FROM t");
        other_path.search_path = Arc::from("tenant_acme");

        cache.put(key("acme", "SELECT * FROM t"), result(16, 1000));
        assert!(
            cache.get(&other_path).is_none(),
            "search_path was not part of the key"
        );
    }

    #[test]
    fn the_same_sql_against_a_different_database_is_a_different_entry() {
        // `M24.4`. A tenant is not one database. The grant resolves per startup
        // database, so one tenant reaching two of them gets two backends, and
        // `SELECT * FROM t` names a different table in each. `PoolKey` carries
        // the database for exactly this reason and the cache key did not.
        let (cache, _clock) = store(64 * 1024);
        let mut other_database = key("acme", "SELECT * FROM t");
        other_database.database = Arc::from("acme_reporting");

        cache.put(key("acme", "SELECT * FROM t"), result(16, 1000));
        assert!(
            cache.get(&other_database).is_none(),
            "one tenant's two databases shared an entry"
        );
    }

    #[test]
    fn the_same_sql_under_a_different_role_is_a_different_entry() {
        // The worse half. Row-level security and column privileges belong to
        // the role, so the same statement under two roles is two different
        // answers, and sharing an entry publishes rows one of them cannot see.
        let (cache, _clock) = store(64 * 1024);
        let mut other_role = key("acme", "SELECT * FROM t");
        other_role.user = Arc::from("acme_readonly");

        cache.put(key("acme", "SELECT * FROM t"), result(16, 1000));
        assert!(
            cache.get(&other_role).is_none(),
            "two roles of one tenant shared an entry"
        );
    }

    #[test]
    fn different_parameters_are_different_entries() {
        let (cache, _clock) = store(64 * 1024);
        let mut bound = key("acme", "SELECT $1");
        bound.params = Arc::from(&b"\0\0\0\x011"[..]);
        let mut other = key("acme", "SELECT $1");
        other.params = Arc::from(&b"\0\0\0\x012"[..]);

        cache.put(bound.clone(), result(16, 1000));
        assert!(cache.get(&bound).is_some());
        assert!(
            cache.get(&other).is_none(),
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
        long_sql.params = Arc::from(vec![0_u8; 4096].as_slice());
        long_sql.search_path = Arc::from("y".repeat(4096));
        let large = weigh(&long_sql, &result(4096, 1000));

        assert!(
            large > small + 16_000,
            "the accounting missed a caller-controlled field: {small} then {large}"
        );
    }

    #[test]
    fn the_byte_total_is_the_sum_of_the_parts() {
        // The inequality above says the accounting notices a field. It does not
        // say it adds them up, and `M10.3` proved it: replacing a `+` in `weigh`
        // with a `*` survived every test in this file three times over. A
        // property that is arithmetic has to be asserted as arithmetic.
        let mut counted = key("acme", "select 1");
        counted.params = Arc::from(&b"\0\0\0\x011"[..]);
        counted.search_path = Arc::from("public");
        let value = result(64, 1000);

        assert_eq!(
            weigh(&counted, &value),
            size_of::<CacheKey>()
                + size_of::<CachedResult>()
                + counted.tenant.as_str().len()
                + counted.database.len()
                + counted.user.len()
                + counted.normalized_sql.len()
                + counted.search_path.len()
                + counted.params.len()
                + value.frames.len()
        );
    }

    #[test]
    fn an_entry_that_exactly_fills_the_budget_is_stored() {
        // The boundary in `put`. Refusing at the budget rather than past it
        // makes the largest storable entry one byte smaller than the budget,
        // which is not what "bigger than the whole budget" means, and nothing
        // said so until `M10.3` replaced the `>` with `>=` and no test noticed.
        let exact = key("acme", "select 1");
        let value = result(64, 1000);
        let clock = Arc::new(FakeClock::new());
        let cache = Store::new(clock);
        cache.reconfigure(&config(weigh(&exact, &value), &["acme"]));

        cache.put(exact.clone(), value);
        assert!(
            cache.get(&exact).is_some(),
            "an entry the size of the budget was refused"
        );
        assert_eq!(cache.stats().rejected, 0);
    }

    #[test]
    fn the_held_total_is_the_sum_of_what_is_held() {
        // `put` adds an entry's weight to the total. `M10.3` replaced that `+=`
        // with `-=` and with `*=` and both survived, because every test here
        // asserted the total was over or under something rather than what it
        // was.
        let (cache, _clock) = store(64 * 1024);
        let first = key("acme", "select 1");
        let second = key("acme", "select 2");
        let value = result(64, 1000);

        cache.put(first.clone(), value.clone());
        cache.put(second.clone(), value.clone());

        assert_eq!(
            cache.stats().bytes,
            (weigh(&first, &value) + weigh(&second, &value)) as u64
        );
    }

    #[test]
    fn filling_the_budget_exactly_evicts_nothing() {
        // The other side of the same boundary, in `evict_to_fit`: evicting while
        // the total is *at* the budget throws out an entry that fits.
        let value = result(64, 1000);
        let first = key("acme", "select 1");
        let second = key("acme", "select 2");
        let budget = weigh(&first, &value) + weigh(&second, &value);

        let clock = Arc::new(FakeClock::new());
        let cache = Store::new(clock);
        cache.reconfigure(&config(budget, &["acme"]));

        cache.put(first.clone(), value.clone());
        cache.put(second.clone(), value);

        assert_eq!(cache.stats().entries, 2, "an entry that fitted was evicted");
        assert_eq!(cache.stats().evicted, 0);
    }

    #[test]
    fn a_hit_makes_an_entry_the_last_one_evicted() {
        // What the recency counter is for. `M10.3` replaced `next_seq += 1` in
        // `touch` with `-=` and with `*=`, and both survived: nothing asserted
        // that a hit changes which entry eviction takes next, which is the only
        // thing the counter exists to decide.
        let value = result(64, 1000);
        let old = key("acme", "select 1");
        let new = key("acme", "select 2");
        let third = key("acme", "select 3");
        let budget = weigh(&old, &value) + weigh(&new, &value);

        let clock = Arc::new(FakeClock::new());
        let cache = Store::new(clock);
        cache.reconfigure(&config(budget, &["acme"]));

        cache.put(old.clone(), value.clone());
        cache.put(new.clone(), value.clone());
        // A hit on the older one, which makes the newer one the oldest.
        assert!(cache.get(&old).is_some());

        cache.put(third.clone(), value);

        assert!(
            cache.get(&old).is_some(),
            "the entry that was just read is the one eviction took"
        );
        assert!(cache.get(&new).is_none());
        assert!(cache.get(&third).is_some());
    }

    #[test]
    fn the_recency_index_holds_one_place_per_entry() {
        // This was a `BTreeMap` keyed by a sequence number, and two entries
        // sharing one lost a place between them while the byte total drifted
        // from the map. `M10.3` found that `next_seq *= 1` in `touch` is a
        // no-op, which hands the next insert a sequence a live entry already
        // has, and no test noticed because eviction order happened to come out
        // the same.
        //
        // `M26.4` made it a list the entries hold between them, where the same
        // mistake is a link pointing at the wrong slot. The assertion is
        // stronger for it: walking from the least recently used has to reach
        // every entry exactly once, and `recency` refuses to walk a cycle.
        let (cache, _clock) = store(64 * 1024);
        let first = key("acme", "select 1");
        cache.put(first.clone(), result(16, 1000));
        cache.put(key("acme", "select 2"), result(16, 1000));
        cache.get(&first);
        cache.put(key("acme", "select 3"), result(16, 1000));

        let inner = cache.lock();
        let walked = inner.recency();
        assert_eq!(
            walked.len(),
            inner.entries.len(),
            "an entry has no place in the recency order, or two share one"
        );

        let distinct: std::collections::BTreeSet<_> = walked.iter().copied().collect();
        assert_eq!(distinct.len(), walked.len(), "a slot appears twice");

        // And the order itself: `select 1` was used after `select 2`, so it is
        // the newer of the two, and `select 3` is newer than either.
        let ends = (inner.least, inner.most);
        assert!(ends.0.is_some() && ends.1.is_some(), "the list has no ends");
        assert_eq!(walked.first().copied(), ends.0, "the walk starts elsewhere");
        assert_eq!(walked.last().copied(), ends.1, "the walk ends elsewhere");
    }

    #[test]
    fn the_recency_order_is_walkable_from_both_ends() {
        // A doubly linked list has two ways to be wrong and one of them is
        // invisible from the front. Every `older` link has to be the mirror of
        // some `newer` link, and a `touch` that fixed one and not the other
        // would leave eviction taking the right entry and `remove` splicing
        // the list in half.
        let (cache, _clock) = store(64 * 1024);
        let keys: Vec<CacheKey> = (0..8)
            .map(|i| key("acme", &format!("select {i}")))
            .collect();
        for one in &keys {
            cache.put(one.clone(), result(16, 1000));
        }
        // Touch a few, in an order that is not the insertion order, so the
        // links are not simply what `put` left.
        for i in [3_usize, 0, 6, 3] {
            cache.get(&keys[i]);
        }

        let inner = cache.lock();
        let forward = inner.recency();

        let mut backward = Vec::new();
        let mut at = inner.most;
        while let Some(slot) = at {
            backward.push(slot);
            at = inner.slots[slot.0 as usize].older;
            assert!(backward.len() <= forward.len(), "the older links cycle");
        }
        backward.reverse();

        assert_eq!(forward, backward, "the two directions disagree");
    }

    #[test]
    fn the_tenant_index_holds_exactly_what_the_entry_map_holds() {
        // `M26.1` added a second index so a write stops walking every entry on
        // the node, and a second index is a second thing to drift. Every path
        // that removes an entry has to remove it from both, and the ones that
        // do it least visibly are eviction, expiry on read, and a tenant
        // dropped by a reconfigure.
        let (cache, clock) = store(64 * 1024);

        // Put, across two tenants.
        for tenant in ["acme", "other"] {
            for sql in ["select 1", "select 2", "select 3"] {
                cache.put(key(tenant, sql), result(16, 1000));
            }
        }
        assert_indexed(&cache, 6);

        // Replacing a key already held must not add a second place for it.
        cache.put(key("acme", "select 1"), result(32, 1000));
        assert_indexed(&cache, 6);

        // Expiry, found on read.
        clock.advance(Duration::from_millis(1001));
        assert!(cache.get(&key("acme", "select 1")).is_none());
        assert_indexed(&cache, 5);

        // Eviction, down to a budget that fits one.
        cache.reconfigure(&config(600, &["acme", "other"]));
        assert_indexed(&cache, usize::try_from(cache.stats().entries).unwrap());

        // A tenant taken out of the document.
        cache.reconfigure(&config(64 * 1024, &["other"]));
        assert_indexed(&cache, usize::try_from(cache.stats().entries).unwrap());

        // And invalidation itself.
        cache.invalidate_tenant(&TenantId::new("other"));
        assert_indexed(&cache, 0);
    }

    /// Asserts the entry map, the recency order and the tenant index agree.
    ///
    /// All three, because the failure mode of a second index is that it holds
    /// a key the entry map no longer does, and the next invalidation then
    /// walks a key that is not there. Nothing about that looks wrong until the
    /// byte total drifts.
    fn assert_indexed(cache: &Store, expected: usize) {
        let inner = cache.lock();
        assert_eq!(inner.entries.len(), expected, "the entry map");
        assert_eq!(inner.recency().len(), expected, "the recency order");

        let indexed: usize = inner.by_tenant.values().map(HashSet::len).sum();
        assert_eq!(indexed, expected, "the tenant index");

        for (tenant, seqs) in &inner.by_tenant {
            assert!(!seqs.is_empty(), "{tenant} is indexed with nothing in it");
            for slot in seqs {
                let found = inner.slots[slot.0 as usize].key.as_ref();
                assert!(
                    found.is_some(),
                    "the tenant index holds a slot that names no key"
                );
                let Some(key) = found else { continue };
                assert!(
                    inner.entries.contains_key(key),
                    "the tenant index holds a key the entry map does not"
                );
                assert_eq!(&key.tenant, tenant, "a key is filed under another tenant");
            }
        }
    }

    #[test]
    fn a_ttl_that_overflows_the_clock_is_refused_rather_than_stored() {
        // Not reachable through a document, because `parse_duration` refuses a
        // TTL this long, and reachable through `reconfigure`, which takes a
        // config and does not get to assume where it came from. That is what the
        // guard is for and `M10.3` showed nothing exercised it.
        let clock = Arc::new(FakeClock::new());
        let cache = Store::new(clock);
        cache.reconfigure(&QueryCacheConfig {
            max_bytes: 64 * 1024,
            max_entry_bytes: QueryCacheConfig::default().max_entry_bytes,
            ttl_cap: Duration::MAX,
            tenants: [(TenantId::new("acme"), TenantCache { ttl: Duration::MAX })]
                .into_iter()
                .collect(),
        });

        // The relay asks for the longest TTL there is, which is how a caller with
        // no opinion about staleness says so, and the cache takes the smaller of
        // that and the configured one. With both at the maximum there is no
        // instant to expire at.
        let doomed = key("acme", "select 1");
        cache.put(
            doomed.clone(),
            CachedResult {
                frames: Arc::from(vec![0_u8; 16].as_slice()),
                ttl: Duration::MAX,
            },
        );

        assert!(
            cache.get(&doomed).is_none(),
            "an unexpirable entry was stored"
        );
        assert_eq!(cache.stats().rejected, 1);
    }

    #[test]
    fn stats_report_what_is_held_rather_than_what_was_ever_stored() {
        let (cache, _clock) = store(64 * 1024);
        cache.put(key("acme", "a"), result(16, 1000));
        cache.put(key("acme", "b"), result(16, 1000));
        cache.invalidate_tenant(&TenantId::new("acme"));

        let stats = cache.stats();
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.invalidated, 2);
    }

    #[test]
    fn the_budget_it_was_built_with_is_readable() {
        let (cache, _clock) = store(4096);
        assert_eq!(cache.max_bytes(), 4096);
    }
}
