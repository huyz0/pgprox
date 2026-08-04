//! Query cache contract.
//!
//! The trait exists from M0 so a cache can be added later without touching the
//! session or pool layers, and `pgprox-cache` implements it from M9.
//!
//! # What a cache built on this may promise
//!
//! Bounded staleness, and nothing stronger. ADR 0021 is the contract: off by
//! default, opt-in per tenant, one node rather than the fleet, and the TTL on
//! [`CachedResult`] is the guarantee.
//!
//! The reason is that a cache entry cannot be checked the way a replica can. A
//! replica's staleness is measurable, and ADR 0009 gates read routing on
//! exactly that measurement. An entry here is a copy of bytes the server
//! produced at some past moment, carrying no version of the rows behind them.
//! A proxy also cannot see a write that never passed through it, and a
//! migration or an operator with psql never will.
//!
//! # The key names the connection that would have answered
//!
//! Omitting a field is a correctness bug, not an optimization detail. The same
//! SQL text resolves to different tables under different search paths, so
//! `search_path` is part of the key; the same SQL resolves to different tables
//! in different databases and to different rows under different roles, so the
//! database and the role are too.
//!
//! All three are the same observation, and only the first was carried far
//! enough. A tenant is not one database and is not one role: a grant resolves
//! per startup database and yields a `Backend`, which is why `PoolKey` carries
//! both. See ADR 0024, and `M24.4` for what sharing them looked like.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use crate::ids::TenantId;

/// What a cached result is keyed by.
///
/// Every field is part of the key. Dropping one is how a cache starts returning
/// another tenant's data.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct CacheKey {
    /// Whose query this was. Without this, tenants share entries.
    pub tenant: TenantId,
    /// The database the answer came from.
    ///
    /// A tenant is not one database. A grant resolves per startup database, so
    /// one tenant reaching two of them gets two backends, and `SELECT * FROM t`
    /// names a different table in each. `M24.4`: this and [`CacheKey::user`]
    /// were both absent, and `PoolKey` carries both precisely because they vary
    /// within a tenant.
    pub database: Arc<str>,
    /// The role the answer was produced under.
    ///
    /// Row-level security and column privileges are properties of the role, so
    /// the same statement under two roles is two different answers. Sharing an
    /// entry between them publishes rows one of them cannot see.
    pub user: Arc<str>,
    /// The statement, normalized so parameter placeholders are stable.
    pub normalized_sql: Arc<str>,
    /// Bound parameter values, as the `Bind` carried them.
    ///
    /// Length-prefixed with `-1` for a SQL `NULL`, contiguous, and empty when
    /// nothing was bound. The wire form rather than a parsed one for two
    /// reasons, and neither is about saving an allocation, although it does.
    ///
    /// A null is not a zero-length value. `WHERE name IS NULL` and `WHERE name
    /// = ''` are different questions with different answers, and a
    /// `Vec<Vec<u8>>` cannot tell them apart, so two bindings of one statement
    /// would have shared an entry. The wire draws the distinction already.
    ///
    /// And empty means "nothing bound" rather than "no `Bind` involved", so an
    /// extended-protocol statement with no parameters keys the same as the
    /// simple query of the same SQL. That is the same question asked two ways
    /// and one entry should answer both.
    pub params: Arc<[u8]>,
    /// The session's `search_path`, which decides what the SQL actually names.
    pub search_path: Arc<str>,
}

/// A cached result, stored as the raw wire bytes that produced it.
///
/// Bytes rather than parsed rows, for the same reason the proxy never parses
/// `DataRow` on the relay path.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CachedResult {
    /// The response frames, verbatim.
    pub frames: Arc<[u8]>,
    /// How long this entry may be served.
    pub ttl: Duration,
}

/// A query result cache.
///
/// # Synchronous, because no implementation waits
///
/// The store this contract exists for holds a `std::sync::Mutex` and a hash
/// map, and its own module docs say "Nothing here waits". An `#[async_trait]`
/// over that boxed the future of every method, and `pgprox-core` also
/// implements the trait for `Arc<T>`, so a caller holding an
/// `Arc<dyn QueryCache>` paid two heap allocations per statement to express
/// something no implementation does. `M26.3` measured it: a *miss*, which
/// touches nothing and returns `None`, allocated twice.
///
/// What this forecloses is an implementation that reaches the network. ADR 0025
/// argues that rather than assuming it, and ADR 0021 had already decided this
/// cache is one node's own.
///
/// An implementation may return an entry up to its TTL old and no older. It may
/// not return one to a session that has written, or for a statement the caller
/// has not established is cacheable: those are the caller's obligations, and
/// they are the ones a TTL cannot repair. See ADR 0021.
pub trait QueryCache: Send + Sync + fmt::Debug {
    /// Whether this cache would hold anything for a tenant.
    ///
    /// Cheap enough to call before deciding to build a [`CacheKey`]. That is what it is for: normalizing a statement allocates,
    /// and off is the default, so on most nodes every statement would pay to
    /// discover there was nothing to look it up in.
    ///
    /// Defaulted to true because an implementation that serves everybody it is
    /// asked about is a valid one, and the default a trait method takes should
    /// be the behaviour of the simplest implementation rather than the safest
    /// answer for the caller. A cache that serves nobody would make the fake
    /// useless by default.
    ///
    /// It is a hint about configuration, not a promise about content:
    /// [`QueryCache::get`] may still return `None`, and a caller must not read
    /// a `true` here as "there is an entry".
    fn serves(&self, _tenant: &TenantId) -> bool {
        true
    }

    /// Looks up a result.
    fn get(&self, key: &CacheKey) -> Option<CachedResult>;

    /// Stores a result.
    fn put(&self, key: CacheKey, value: CachedResult);

    /// Drops every entry for a tenant, for invalidation and for eviction on
    /// tenant removal.
    fn invalidate_tenant(&self, tenant: &TenantId);
}

impl<T: QueryCache + ?Sized> QueryCache for Arc<T> {
    // Forwarded rather than defaulted: an `Arc` around a cache that serves
    // nobody serves nobody, and taking the default here would tell every
    // caller behind an `Arc` to go on and build a key.
    fn serves(&self, tenant: &TenantId) -> bool {
        (**self).serves(tenant)
    }

    fn get(&self, key: &CacheKey) -> Option<CachedResult> {
        (**self).get(key)
    }

    fn put(&self, key: CacheKey, value: CachedResult) {
        (**self).put(key, value);
    }

    fn invalidate_tenant(&self, tenant: &TenantId) {
        (**self).invalidate_tenant(tenant);
    }
}

#[cfg(any(test, feature = "test-fakes"))]
pub use fake::FakeQueryCache;

#[cfg(any(test, feature = "test-fakes"))]
mod fake {
    use std::collections::{BTreeSet, HashMap};
    use std::sync::{Mutex, PoisonError};

    use super::{Arc, CacheKey, CachedResult, QueryCache, TenantId};

    /// An in-memory [`QueryCache`] for tests.
    #[derive(Debug, Default)]
    pub struct FakeQueryCache {
        entries: Mutex<HashMap<CacheKey, CachedResult>>,
        /// Who it serves, or everybody if unset.
        ///
        /// The real store is off until a document names a tenant, and a fake
        /// that could not be off would let a test of the "not configured for
        /// this tenant" path pass without one existing.
        served: Mutex<Option<BTreeSet<TenantId>>>,
    }

    impl FakeQueryCache {
        /// Builds an empty cache that serves everybody.
        #[must_use]
        pub fn new() -> Arc<Self> {
            Arc::new(Self::default())
        }

        /// Narrows it to these tenants, and drops what the others had.
        ///
        /// Dropping is what the real store does when a tenant leaves the
        /// document: an opt-in that was revoked leaves nothing behind.
        pub fn serve_only(&self, tenants: impl IntoIterator<Item = TenantId>) {
            let allowed: BTreeSet<TenantId> = tenants.into_iter().collect();
            self.lock().retain(|key, _| allowed.contains(&key.tenant));
            *self.served.lock().unwrap_or_else(PoisonError::into_inner) = Some(allowed);
        }

        /// How many entries it holds.
        #[must_use]
        pub fn len(&self) -> usize {
            self.lock().len()
        }

        /// Whether it holds nothing.
        #[must_use]
        pub fn is_empty(&self) -> bool {
            self.len() == 0
        }

        fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<CacheKey, CachedResult>> {
            self.entries.lock().unwrap_or_else(PoisonError::into_inner)
        }
    }

    impl QueryCache for FakeQueryCache {
        fn serves(&self, tenant: &TenantId) -> bool {
            self.served
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .is_none_or(|allowed| allowed.contains(tenant))
        }

        fn get(&self, key: &CacheKey) -> Option<CachedResult> {
            if !self.serves(&key.tenant) {
                return None;
            }
            self.lock().get(key).cloned()
        }

        fn put(&self, key: CacheKey, value: CachedResult) {
            if !self.serves(&key.tenant) {
                return;
            }
            self.lock().insert(key, value);
        }

        fn invalidate_tenant(&self, tenant: &TenantId) {
            self.lock().retain(|key, _| &key.tenant != tenant);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn key(tenant: &str, sql: &str, search_path: &str) -> CacheKey {
        CacheKey {
            tenant: TenantId::new(tenant),
            database: Arc::from("tenant_db"),
            user: Arc::from("app"),
            normalized_sql: Arc::from(sql),
            params: Arc::from(&b"\0\0\0\x011"[..]),
            search_path: Arc::from(search_path),
        }
    }

    /// `M14.33`. Two mutants survived here, one in the trait's default body and
    /// one in the fake, which is the pairing `scripts/mutants.sh` opens by
    /// warning about: M9 hid three defects behind a fake that answered
    /// something the real thing would refuse.
    #[test]
    fn a_cache_that_does_not_say_otherwise_serves_every_tenant() {
        // `QueryCache::serves` defaults to `true`, and could be flipped to
        // `false`. Every implementation in this tree overrides it, so the
        // default is what an implementation written elsewhere gets, and `false`
        // would silently turn caching off for every tenant behind it while
        // every test here still passed.
        #[derive(Debug)]
        struct MinimalCache;

        impl QueryCache for MinimalCache {
            fn get(&self, _key: &CacheKey) -> Option<CachedResult> {
                None
            }
            fn put(&self, _key: CacheKey, _value: CachedResult) {}
            fn invalidate_tenant(&self, _tenant: &TenantId) {}
        }

        assert!(
            MinimalCache.serves(&TenantId::new("acme")),
            "the default must serve: a cache with no opinion has not opted out"
        );
    }

    #[test]
    fn the_fake_reports_emptiness_from_its_contents() {
        // `FakeQueryCache::is_empty` could return `true` unconditionally, which
        // would make every assertion of the form "nothing was cached" pass
        // whether or not anything was.
        let cache = FakeQueryCache::new();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);

        cache.put(key("acme", "SELECT 1", "public"), result());
        assert!(
            !cache.is_empty(),
            "a cache holding an entry called itself empty"
        );
        assert_eq!(cache.len(), 1);

        cache.invalidate_tenant(&TenantId::new("acme"));
        assert!(cache.is_empty());
    }

    #[test]
    fn a_cache_behind_an_arc_is_the_same_cache() {
        // The forwarding impl, which nothing covered until `M26.3` changed it.
        // Every method has to reach the inner cache, and `serves` is the one
        // the comment beside the impl argues about: taking the trait's default
        // here would tell a caller holding an `Arc` around a cache that serves
        // nobody to go on and build a key.
        let inner = Arc::new(FakeQueryCache::new());
        inner.serve_only([TenantId::new("acme")]);
        let behind: Arc<dyn QueryCache> = inner.clone();

        assert!(behind.serves(&TenantId::new("acme")));
        assert!(
            !behind.serves(&TenantId::new("globex")),
            "an Arc around a cache that serves nobody served somebody"
        );

        behind.put(key("acme", "SELECT 1", "public"), result());
        assert_eq!(inner.len(), 1, "the put did not reach the inner cache");
        assert_eq!(
            behind.get(&key("acme", "SELECT 1", "public")),
            Some(result())
        );

        behind.invalidate_tenant(&TenantId::new("acme"));
        assert!(inner.is_empty(), "the invalidation did not reach it");
    }

    #[test]
    fn a_null_parameter_and_an_empty_one_are_different_keys() {
        // The reason the field holds the wire form. These two are different
        // questions, and a shape that could not tell them apart would answer
        // one with the other's rows.
        let mut null = key("acme", "SELECT $1", "public");
        null.params = Arc::from(&(-1_i32).to_be_bytes()[..]);
        let mut empty = key("acme", "SELECT $1", "public");
        empty.params = Arc::from(&0_i32.to_be_bytes()[..]);

        assert_ne!(null, empty);
    }

    fn result() -> CachedResult {
        CachedResult {
            frames: Arc::from(&b"row-data"[..]),
            ttl: Duration::from_secs(30),
        }
    }

    #[test]
    fn a_stored_result_comes_back() {
        let cache = FakeQueryCache::new();
        assert!(cache.is_empty());

        cache.put(key("acme", "SELECT 1", "public"), result());
        assert_eq!(
            cache.get(&key("acme", "SELECT 1", "public")),
            Some(result())
        );
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn tenants_never_share_an_entry() {
        // Identical SQL from two tenants must not collide.
        let cache = FakeQueryCache::new();
        cache.put(key("acme", "SELECT 1", "public"), result());
        assert!(
            cache.get(&key("globex", "SELECT 1", "public")).is_none(),
            "one tenant read another's cached result"
        );
    }

    #[test]
    fn search_path_is_part_of_the_key() {
        // The same SQL resolves to different tables under different search
        // paths. Omitting it from the key is a correctness bug.
        let cache = FakeQueryCache::new();
        cache.put(key("acme", "SELECT * FROM orders", "tenant_a"), result());
        assert!(
            cache
                .get(&key("acme", "SELECT * FROM orders", "tenant_b"))
                .is_none(),
            "a different search_path hit the same entry"
        );
    }

    #[test]
    fn parameters_are_part_of_the_key() {
        let cache = FakeQueryCache::new();
        let mut other = key("acme", "SELECT $1", "public");
        other.params = Arc::from(&b"\0\0\0\x012"[..]);

        cache.put(key("acme", "SELECT $1", "public"), result());
        assert!(cache.get(&other).is_none(), "parameters were ignored");
    }

    #[test]
    fn invalidating_a_tenant_leaves_others_alone() {
        let cache = FakeQueryCache::new();
        cache.put(key("acme", "SELECT 1", "public"), result());
        cache.put(key("globex", "SELECT 1", "public"), result());

        cache.invalidate_tenant(&TenantId::new("acme"));
        assert!(cache.get(&key("acme", "SELECT 1", "public")).is_none());
        assert!(
            cache.get(&key("globex", "SELECT 1", "public")).is_some(),
            "invalidation hit the wrong tenant"
        );
    }

    #[test]
    fn cache_works_through_an_arc_dyn() {
        let cache: Arc<dyn QueryCache> = FakeQueryCache::new();
        assert!(cache.serves(&TenantId::new("acme")));
        cache.put(key("acme", "SELECT 1", "public"), result());
        assert!(cache.get(&key("acme", "SELECT 1", "public")).is_some());
    }

    #[test]
    fn a_cache_that_does_not_serve_a_tenant_says_so_before_being_asked() {
        // The question the relay asks before it builds a key, because building
        // one allocates and off is the default. The fake answers it the way the
        // real store does, so a test of the gate is a test of the gate.
        let cache = FakeQueryCache::new();
        cache.put(key("acme", "SELECT 1", "public"), result());
        cache.put(key("globex", "SELECT 1", "public"), result());

        cache.serve_only([TenantId::new("acme")]);

        assert!(cache.serves(&TenantId::new("acme")));
        assert!(!cache.serves(&TenantId::new("globex")));

        // And narrowing dropped what the tenant that left had, rather than
        // leaving its rows resident on a node that no longer serves it.
        assert_eq!(cache.len(), 1);
        assert!(cache.get(&key("globex", "SELECT 1", "public")).is_none());
    }

    #[test]
    fn a_tenant_that_is_not_served_stores_nothing() {
        let cache = FakeQueryCache::new();
        cache.serve_only([TenantId::new("acme")]);
        cache.put(key("globex", "SELECT 1", "public"), result());
        assert!(cache.is_empty(), "a cache stored for a tenant it refuses");
    }

    #[test]
    fn serving_everybody_is_what_an_unconfigured_cache_does() {
        // The default the trait takes, checked through the fake: a cache that
        // was never narrowed answers for anyone.
        let cache: Arc<dyn QueryCache> = FakeQueryCache::new();
        for tenant in ["acme", "globex", ""] {
            assert!(cache.serves(&TenantId::new(tenant)), "{tenant}");
        }
    }
}
