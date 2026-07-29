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
//! # The key includes `search_path`
//!
//! Omitting it is a correctness bug, not an optimization detail: the same SQL
//! text resolves to different tables under different search paths, so two
//! tenants running identical queries would share a cache entry pointing at
//! different data.

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
    /// The statement, normalized so parameter placeholders are stable.
    pub normalized_sql: Arc<str>,
    /// Bound parameter values, in order.
    pub params: Vec<Vec<u8>>,
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
/// An implementation may return an entry up to its TTL old and no older. It may
/// not return one to a session that has written, or for a statement the caller
/// has not established is cacheable: those are the caller's obligations, and
/// they are the ones a TTL cannot repair. See ADR 0021.
#[async_trait::async_trait]
pub trait QueryCache: Send + Sync + fmt::Debug {
    /// Whether this cache would hold anything for a tenant.
    ///
    /// Not async, and cheap enough to call before deciding to build a
    /// [`CacheKey`]. That is what it is for: normalizing a statement allocates,
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
    async fn get(&self, key: &CacheKey) -> Option<CachedResult>;

    /// Stores a result.
    async fn put(&self, key: CacheKey, value: CachedResult);

    /// Drops every entry for a tenant, for invalidation and for eviction on
    /// tenant removal.
    async fn invalidate_tenant(&self, tenant: &TenantId);
}

#[async_trait::async_trait]
impl<T: QueryCache + ?Sized> QueryCache for Arc<T> {
    // Forwarded rather than defaulted: an `Arc` around a cache that serves
    // nobody serves nobody, and taking the default here would tell every
    // caller behind an `Arc` to go on and build a key.
    fn serves(&self, tenant: &TenantId) -> bool {
        (**self).serves(tenant)
    }

    async fn get(&self, key: &CacheKey) -> Option<CachedResult> {
        (**self).get(key).await
    }

    async fn put(&self, key: CacheKey, value: CachedResult) {
        (**self).put(key, value).await;
    }

    async fn invalidate_tenant(&self, tenant: &TenantId) {
        (**self).invalidate_tenant(tenant).await;
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

    #[async_trait::async_trait]
    impl QueryCache for FakeQueryCache {
        fn serves(&self, tenant: &TenantId) -> bool {
            self.served
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .is_none_or(|allowed| allowed.contains(tenant))
        }

        async fn get(&self, key: &CacheKey) -> Option<CachedResult> {
            if !self.serves(&key.tenant) {
                return None;
            }
            self.lock().get(key).cloned()
        }

        async fn put(&self, key: CacheKey, value: CachedResult) {
            if !self.serves(&key.tenant) {
                return;
            }
            self.lock().insert(key, value);
        }

        async fn invalidate_tenant(&self, tenant: &TenantId) {
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
            normalized_sql: Arc::from(sql),
            params: vec![b"1".to_vec()],
            search_path: Arc::from(search_path),
        }
    }

    fn result() -> CachedResult {
        CachedResult {
            frames: Arc::from(&b"row-data"[..]),
            ttl: Duration::from_secs(30),
        }
    }

    #[tokio::test]
    async fn a_stored_result_comes_back() {
        let cache = FakeQueryCache::new();
        assert!(cache.is_empty());

        cache.put(key("acme", "SELECT 1", "public"), result()).await;
        assert_eq!(
            cache.get(&key("acme", "SELECT 1", "public")).await,
            Some(result())
        );
        assert_eq!(cache.len(), 1);
    }

    #[tokio::test]
    async fn tenants_never_share_an_entry() {
        // Identical SQL from two tenants must not collide.
        let cache = FakeQueryCache::new();
        cache.put(key("acme", "SELECT 1", "public"), result()).await;
        assert!(
            cache
                .get(&key("globex", "SELECT 1", "public"))
                .await
                .is_none(),
            "one tenant read another's cached result"
        );
    }

    #[tokio::test]
    async fn search_path_is_part_of_the_key() {
        // The same SQL resolves to different tables under different search
        // paths. Omitting it from the key is a correctness bug.
        let cache = FakeQueryCache::new();
        cache
            .put(key("acme", "SELECT * FROM orders", "tenant_a"), result())
            .await;
        assert!(
            cache
                .get(&key("acme", "SELECT * FROM orders", "tenant_b"))
                .await
                .is_none(),
            "a different search_path hit the same entry"
        );
    }

    #[tokio::test]
    async fn parameters_are_part_of_the_key() {
        let cache = FakeQueryCache::new();
        let mut other = key("acme", "SELECT $1", "public");
        other.params = vec![b"2".to_vec()];

        cache
            .put(key("acme", "SELECT $1", "public"), result())
            .await;
        assert!(cache.get(&other).await.is_none(), "parameters were ignored");
    }

    #[tokio::test]
    async fn invalidating_a_tenant_leaves_others_alone() {
        let cache = FakeQueryCache::new();
        cache.put(key("acme", "SELECT 1", "public"), result()).await;
        cache
            .put(key("globex", "SELECT 1", "public"), result())
            .await;

        cache.invalidate_tenant(&TenantId::new("acme")).await;
        assert!(
            cache
                .get(&key("acme", "SELECT 1", "public"))
                .await
                .is_none()
        );
        assert!(
            cache
                .get(&key("globex", "SELECT 1", "public"))
                .await
                .is_some(),
            "invalidation hit the wrong tenant"
        );
    }

    #[tokio::test]
    async fn cache_works_through_an_arc_dyn() {
        let cache: Arc<dyn QueryCache> = FakeQueryCache::new();
        assert!(cache.serves(&TenantId::new("acme")));
        cache.put(key("acme", "SELECT 1", "public"), result()).await;
        assert!(
            cache
                .get(&key("acme", "SELECT 1", "public"))
                .await
                .is_some()
        );
    }

    #[tokio::test]
    async fn a_cache_that_does_not_serve_a_tenant_says_so_before_being_asked() {
        // The question the relay asks before it builds a key, because building
        // one allocates and off is the default. The fake answers it the way the
        // real store does, so a test of the gate is a test of the gate.
        let cache = FakeQueryCache::new();
        cache.put(key("acme", "SELECT 1", "public"), result()).await;
        cache
            .put(key("globex", "SELECT 1", "public"), result())
            .await;

        cache.serve_only([TenantId::new("acme")]);

        assert!(cache.serves(&TenantId::new("acme")));
        assert!(!cache.serves(&TenantId::new("globex")));

        // And narrowing dropped what the tenant that left had, rather than
        // leaving its rows resident on a node that no longer serves it.
        assert_eq!(cache.len(), 1);
        assert!(
            cache
                .get(&key("globex", "SELECT 1", "public"))
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_tenant_that_is_not_served_stores_nothing() {
        let cache = FakeQueryCache::new();
        cache.serve_only([TenantId::new("acme")]);
        cache
            .put(key("globex", "SELECT 1", "public"), result())
            .await;
        assert!(cache.is_empty(), "a cache stored for a tenant it refuses");
    }

    #[tokio::test]
    async fn serving_everybody_is_what_an_unconfigured_cache_does() {
        // The default the trait takes, checked through the fake: a cache that
        // was never narrowed answers for anyone.
        let cache: Arc<dyn QueryCache> = FakeQueryCache::new();
        for tenant in ["acme", "globex", ""] {
            assert!(cache.serves(&TenantId::new(tenant)), "{tenant}");
        }
    }
}
