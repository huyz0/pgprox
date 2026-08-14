//! The grant cache.
//!
//! Wraps any [`CredentialResolver`] and adds three things a raw client does not
//! have: caching, singleflight, and negative caching.
//!
//! # Why the key is a hash
//!
//! Entries are keyed by `sha256(token) || startup_database`, never by the
//! tenant claim. Keying by tenant would let a revoked token keep working as
//! long as some other valid token for the same tenant was cached, which is a
//! revocation bypass rather than a cache optimisation. Hashing rather than
//! storing the token means a memory dump of the keys is not a credential dump.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use pgprox_core::auth::{AuthError, AuthRequest, CredentialResolver, Grant, GrantInvalidation};
use pgprox_core::clock::Clock;
use pgprox_core::error::AuthRejection;
use pgprox_core::ids::ServerId;
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;

use crate::jwt;

/// How the cache is tuned.
#[derive(Clone, Copy, Debug)]
pub struct CacheConfig {
    /// Upper bound on a positive entry's lifetime, whatever the sidecar says.
    pub max_ttl: Duration,

    /// How long a refusal is remembered.
    ///
    /// Deliberately much shorter than `max_ttl`. A refusal can be reversed by
    /// something outside this process, such as a tenant being re-enabled, and a
    /// long negative TTL means that fix appears not to work.
    pub negative_ttl: Duration,

    /// Most entries held before the cache stops admitting new ones.
    pub capacity: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_ttl: Duration::from_secs(300),
            negative_ttl: Duration::from_secs(5),
            capacity: 100_000,
        }
    }
}

/// The key an entry is stored under.
///
/// `Debug` prints the hash, which is safe: it is what is stored, and it is not
/// reversible to the token.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct CacheKey {
    token_hash: [u8; 32],
    database: String,
}

impl CacheKey {
    fn new(token: &str, database: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        Self {
            token_hash: hasher.finalize().into(),
            database: database.to_owned(),
        }
    }
}

/// What is remembered about a token.
#[derive(Clone, Debug)]
enum Outcome {
    /// Boxed because a `Grant` dwarfs an `AuthRejection`, and an unboxed enum
    /// would make every negative entry as large as a positive one.
    Resolved(Box<Grant>),
    Refused(AuthRejection),
}

impl Outcome {
    /// What a cached decision means to the caller.
    ///
    /// A method rather than the match written out at each site, because
    /// `resolve` reads the cache twice and the two reads have to agree about
    /// what a refusal is.
    fn into_result(self) -> Result<Grant, AuthError> {
        match self {
            Self::Resolved(grant) => Ok(*grant),
            Self::Refused(reason) => Err(AuthError::Refused(reason)),
        }
    }
}

#[derive(Clone, Debug)]
struct Entry {
    outcome: Outcome,
    expires_at: Instant,
}

/// How often a full cache walks itself for entries that have expired.
///
/// A constant, and rate limiting rather than a policy. The sweep exists because
/// an entry otherwise leaves only when its own key is looked up again, and a
/// rotating token's key never is: the map fills with dead entries and refuses
/// every live one from then on. See [`Entries::sweep`].
///
/// It walks the whole map, and it runs from `store`, which is on the connection
/// path. At capacity with every entry live it would find nothing and do it
/// again for the next connection, so once an interval is the bound. One second
/// at a hundred thousand entries is a millisecond of a second.
const SWEEP_INTERVAL: Duration = Duration::from_secs(1);

/// The entry map, and when it was last walked.
///
/// One lock rather than two, because the sweep decision is made from inside the
/// map's own lock and a second one taken there is a second one to get wrong.
#[derive(Debug, Default)]
struct Entries {
    map: HashMap<CacheKey, Entry>,
    /// When the last sweep ran, or [`None`] if none has.
    swept_at: Option<Instant>,
    /// How many have run, for the test that the rate limit is a rate.
    sweeps: u64,
}

impl Entries {
    /// Drops every expired entry, at most once per [`SWEEP_INTERVAL`].
    ///
    /// Expired only. Refusing at capacity rather than evicting is deliberate,
    /// for the reason [`CachingResolver::store`] gives, and a sweep that
    /// started throwing out live entries would have quietly made that decision
    /// on its own.
    fn sweep(&mut self, now: Instant) {
        if self
            .swept_at
            .is_some_and(|last| now.saturating_duration_since(last) < SWEEP_INTERVAL)
        {
            return;
        }
        self.swept_at = Some(now);
        self.sweeps += 1;
        self.map.retain(|_, entry| entry.expires_at > now);
    }
}

/// A [`CredentialResolver`] that caches, collapses, and remembers refusals.
pub struct CachingResolver<R> {
    inner: R,
    clock: Arc<dyn Clock>,
    config: CacheConfig,
    entries: Mutex<Entries>,
    /// Lookups currently in flight, so concurrent callers for the same key
    /// wait rather than each making their own call.
    inflight: Mutex<HashMap<CacheKey, broadcast::Sender<()>>>,
}

impl<R> std::fmt::Debug for CachingResolver<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CachingResolver")
            .field("config", &self.config)
            .field("entries", &self.entries.lock().map_or(0, |e| e.map.len()))
            .finish_non_exhaustive()
    }
}

impl<R: CredentialResolver> CachingResolver<R> {
    /// Wraps a resolver.
    #[must_use]
    pub fn new(inner: R, clock: Arc<dyn Clock>, config: CacheConfig) -> Arc<Self> {
        Arc::new(Self {
            inner,
            clock,
            config,
            entries: Mutex::new(Entries::default()),
            inflight: Mutex::new(HashMap::new()),
        })
    }

    /// How many entries are held, expired ones included.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock_entries().map.len()
    }

    /// Whether the cache holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drops every entry. Used when configuration changes invalidate them.
    pub fn clear(&self) {
        let mut entries = self.lock_entries();
        entries.map.clear();
        entries.swept_at = None;
    }

    /// Drops entries whose grant named `server` as its primary.
    ///
    /// A scan over the whole map rather than a second index keyed by primary,
    /// because this runs on a demotion, which is rare, and not on the
    /// per-connection path a second index would be justified by. At the
    /// default capacity of 100,000 entries a full scan is comfortably under a
    /// millisecond; see `an_invalidation_scan_stays_cheap_at_capacity`.
    fn invalidate_primary_entries(&self, server: &ServerId) -> usize {
        let mut entries = self.lock_entries();
        let before = entries.map.len();
        entries.map.retain(|_, entry| {
            !matches!(&entry.outcome, Outcome::Resolved(grant) if &grant.primary.server == server)
        });
        before - entries.map.len()
    }

    /// How many sweeps have run.
    ///
    /// The rate limit on the sweep is the difference between a cache
    /// that recovers and one that walks a hundred thousand entries per
    /// connection, and a test that only proved the sweep happens would not see
    /// the difference. See `a_full_cache_does_not_sweep_on_every_miss`.
    #[must_use]
    pub fn sweeps(&self) -> u64 {
        self.lock_entries().sweeps
    }

    fn lock_entries(&self) -> std::sync::MutexGuard<'_, Entries> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn lock_inflight(&self) -> std::sync::MutexGuard<'_, HashMap<CacheKey, broadcast::Sender<()>>> {
        self.inflight.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Reads a live entry, dropping it if it has expired.
    fn lookup(&self, key: &CacheKey) -> Option<Outcome> {
        let now = self.clock.now();
        let mut entries = self.lock_entries();
        match entries.map.get(key) {
            Some(entry) if entry.expires_at > now => Some(entry.outcome.clone()),
            Some(_) => {
                entries.map.remove(key);
                None
            }
            None => None,
        }
    }

    /// One more look, for a caller that has just taken the claim.
    ///
    /// Holding the claim is not the same as having been first. The lookup and
    /// the claim are two separate locks, so a caller descheduled between them
    /// can find that the previous leader finished, stored, and released the
    /// claim in the gap, and then become a second leader for a key that is
    /// already cached. Two callers deciding they are first in sequence is the
    /// storm the claim exists to collapse.
    ///
    /// Returns the entry if there is now one, having released the claim and
    /// woken anyone who subscribed to it in the meantime.
    ///
    /// The cost lands only where a call was about to be made anyway: a cache
    /// hit returns before the claim is ever taken.
    ///
    /// This is not theoretical. Without it,
    /// `concurrent_lookups_of_a_cold_key_make_one_call` failed about once in
    /// every few full-suite runs on a loaded machine, and the comment above
    /// the claim asserted a property this code did not have.
    fn recheck_before_calling(&self, key: &CacheKey) -> Option<Outcome> {
        let outcome = self.lookup(key)?;
        if let Some(tx) = self.lock_inflight().remove(key) {
            let _ = tx.send(());
        }
        Some(outcome)
    }

    fn store(&self, key: CacheKey, outcome: Outcome, ttl: Duration) {
        // A zero TTL means "do not cache", which is what an already-expired
        // token produces. Storing it would be harmless but pointless, and it
        // would let an expired token occupy capacity.
        if ttl.is_zero() {
            return;
        }

        let now = self.clock.now();
        let mut entries = self.lock_entries();

        if entries.map.len() >= self.config.capacity && !entries.map.contains_key(&key) {
            // Dead entries first. One leaves on its own only when its own key
            // is looked up again, and a rotating token's key never is, so
            // without this the map filled with expired entries and refused
            // every live one from then on. `M24.5`.
            entries.sweep(now);
        }

        if entries.map.len() >= self.config.capacity && !entries.map.contains_key(&key) {
            // Still full, so every entry in it is live. Refuse rather than
            // evict: an eviction policy is a decision that needs measurement,
            // and admitting past capacity is the one option that is definitely
            // wrong.
            return;
        }

        entries.map.insert(
            key,
            Entry {
                outcome,
                expires_at: now + ttl,
            },
        );
    }

    /// Performs the underlying call and stores whatever it produced.
    async fn resolve_and_store(
        &self,
        key: CacheKey,
        request: AuthRequest,
    ) -> Result<Grant, AuthError> {
        let result = self.inner.resolve(request).await;

        match &result {
            Ok(grant) => {
                let ttl = grant.effective_ttl(self.clock.wall(), self.config.max_ttl);
                self.store(key, Outcome::Resolved(Box::new(grant.clone())), ttl);
            }
            // Only a refusal is remembered. An unavailable sidecar is a
            // transient condition, and caching it would keep every tenant
            // locked out for the negative TTL after it recovers.
            Err(AuthError::Refused(reason)) => {
                self.store(key, Outcome::Refused(*reason), self.config.negative_ttl);
            }
            Err(_) => {}
        }

        result
    }
}

impl<R: CredentialResolver> GrantInvalidation for CachingResolver<R> {
    fn invalidate_primary(&self, server: &ServerId) -> usize {
        self.invalidate_primary_entries(server)
    }
}

#[async_trait::async_trait]
impl<R: CredentialResolver> CredentialResolver for CachingResolver<R> {
    async fn resolve(&self, request: AuthRequest) -> Result<Grant, AuthError> {
        // Rejected before any cache work: a token with a banned algorithm must
        // not occupy an entry, and refusing costs one base64 decode.
        if let Err(reason) = jwt::check_algorithm(request.token.expose()) {
            return Err(AuthError::Refused(reason));
        }

        let key = CacheKey::new(request.token.expose(), &request.startup_database);

        loop {
            if let Some(outcome) = self.lookup(&key) {
                return outcome.into_result();
            }

            // Claim the key, or find someone already holding it, under one
            // lock so two callers cannot both hold the claim at once.
            let waiter = {
                let mut inflight = self.lock_inflight();
                if let Some(tx) = inflight.get(&key) {
                    Some(tx.subscribe())
                } else {
                    let (tx, _) = broadcast::channel(1);
                    inflight.insert(key.clone(), tx);
                    None
                }
            };

            if let Some(mut rx) = waiter {
                // Someone else is fetching. Wait, then loop and re-read the
                // cache rather than trusting them to have succeeded: a failure
                // is not cached, and this caller must then try as leader itself.
                let _ = rx.recv().await;
                continue;
            }

            if let Some(outcome) = self.recheck_before_calling(&key) {
                return outcome.into_result();
            }

            let result = self.resolve_and_store(key.clone(), request).await;

            // Wake the waiters whatever happened, so a failure does not leave
            // them blocked until their own timeouts fire.
            if let Some(tx) = self.lock_inflight().remove(&key) {
                let _ = tx.send(());
            }
            return result;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use pgprox_core::auth::{Backend, ClaimSet, FakeCredentialResolver, PoolHints, TlsMode};
    use pgprox_core::clock::FakeClock;
    use pgprox_core::ids::{ServerId, TenantId};
    use pgprox_core::secret::SecretString;
    use std::time::SystemTime;

    /// A token with a valid header, so the allowlist check passes.
    fn token(marker: &str) -> String {
        format!(
            "{}.{}.{}",
            URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256","typ":"JWT"}"#),
            URL_SAFE_NO_PAD.encode(format!(r#"{{"sub":"{marker}"}}"#)),
            URL_SAFE_NO_PAD.encode("signature")
        )
    }

    fn grant(ttl: Duration, expires_at: Option<SystemTime>) -> Grant {
        Grant {
            tenant: TenantId::new("acme"),
            primary: Backend {
                server: ServerId::new("db-1", 5432),
                database: Arc::from("tenant_acme"),
                user: Arc::from("acme_app"),
                password: SecretString::new("hunter2"),
                tls: TlsMode::Verified,
            },
            replicas: vec![],
            pool: PoolHints::default(),
            ttl,
            claims: ClaimSet {
                subject: None,
                expires_at,
                issued_at: None,
            },
        }
    }

    fn request(token: &str, database: &str) -> AuthRequest {
        AuthRequest {
            token: SecretString::new(token),
            startup_database: database.into(),
            startup_user: "acme_app".into(),
            client_addr: "10.0.0.1".parse().unwrap(),
        }
    }

    struct Fixture {
        cache: Arc<CachingResolver<Arc<FakeCredentialResolver>>>,
        inner: Arc<FakeCredentialResolver>,
        clock: FakeClock,
    }

    fn fixture(config: CacheConfig) -> Fixture {
        let inner = Arc::new(FakeCredentialResolver::new());
        let clock = FakeClock::new();
        let cache = CachingResolver::new(
            Arc::clone(&inner),
            Arc::new(clock.clone()) as Arc<dyn Clock>,
            config,
        );
        Fixture {
            cache,
            inner,
            clock,
        }
    }

    #[tokio::test]
    async fn a_cached_answer_is_gone_at_the_instant_it_expires() {
        // `entry.expires_at > now` could become `>=`, which keeps an answer
        // alive for the instant it was due to expire. The same shape `M14.11`
        // found in the quota ledger, here on an authentication decision: a
        // grant that outlives its TTL is a client admitted against a token the
        // sidecar has already stopped vouching for.
        let config = CacheConfig::default();
        let f = fixture(config);
        let tok = token("expiry");
        f.inner
            .insert(&tok, grant(Duration::from_secs(3_600), None));

        f.cache.resolve(request(&tok, "tenant_acme")).await.unwrap();
        assert_eq!(f.inner.call_count(), 1);

        // One tick before the entry expires it is still served from cache.
        f.clock
            .advance(config.max_ttl.checked_sub(Duration::from_nanos(1)).unwrap());
        f.cache.resolve(request(&tok, "tenant_acme")).await.unwrap();
        assert_eq!(f.inner.call_count(), 1, "the entry expired early");

        // At the instant itself it is gone and the inner resolver is asked
        // again. `>=` would serve the stale entry here instead.
        f.clock.advance(Duration::from_nanos(1));
        f.cache.resolve(request(&tok, "tenant_acme")).await.unwrap();
        assert_eq!(
            f.inner.call_count(),
            2,
            "a cached decision outlived its TTL by an instant"
        );
    }

    #[tokio::test]
    async fn a_hit_avoids_the_underlying_call() {
        let f = fixture(CacheConfig::default());
        let tok = token("a");
        f.inner.insert(&tok, grant(Duration::from_secs(60), None));

        for _ in 0..5 {
            f.cache.resolve(request(&tok, "tenant_acme")).await.unwrap();
        }
        assert_eq!(f.inner.call_count(), 1, "cache did not hold the grant");
    }

    #[tokio::test]
    async fn the_same_token_for_a_different_database_is_a_different_entry() {
        // The database is part of the key because a token's grant depends on
        // which database was asked for.
        let f = fixture(CacheConfig::default());
        let tok = token("a");
        f.inner.insert(&tok, grant(Duration::from_secs(60), None));

        f.cache.resolve(request(&tok, "db_one")).await.unwrap();
        f.cache.resolve(request(&tok, "db_two")).await.unwrap();
        assert_eq!(f.inner.call_count(), 2);
    }

    #[tokio::test]
    async fn an_entry_expires_on_the_injected_clock() {
        let f = fixture(CacheConfig::default());
        let tok = token("a");
        f.inner.insert(&tok, grant(Duration::from_secs(30), None));

        f.cache.resolve(request(&tok, "db")).await.unwrap();
        assert_eq!(f.inner.call_count(), 1);

        f.clock.advance(Duration::from_secs(31));
        f.cache.resolve(request(&tok, "db")).await.unwrap();
        assert_eq!(f.inner.call_count(), 2, "expired entry was still served");
    }

    #[tokio::test]
    async fn the_configured_cap_beats_a_generous_sidecar_ttl() {
        let f = fixture(CacheConfig {
            max_ttl: Duration::from_secs(10),
            ..CacheConfig::default()
        });
        let tok = token("a");
        f.inner.insert(&tok, grant(Duration::from_secs(3600), None));

        f.cache.resolve(request(&tok, "db")).await.unwrap();
        f.clock.advance(Duration::from_secs(11));
        f.cache.resolve(request(&tok, "db")).await.unwrap();

        assert_eq!(f.inner.call_count(), 2, "local cap did not apply");
    }

    #[tokio::test]
    async fn token_expiry_beats_both_other_limits() {
        // The revocation case: a token expiring sooner than either limit must
        // not keep working because the cache had a longer opinion.
        let f = fixture(CacheConfig::default());
        let tok = token("a");
        let expires = f.clock.wall() + Duration::from_secs(5);
        f.inner
            .insert(&tok, grant(Duration::from_secs(3600), Some(expires)));

        f.cache.resolve(request(&tok, "db")).await.unwrap();
        f.clock.advance(Duration::from_secs(6));
        f.cache.resolve(request(&tok, "db")).await.unwrap();

        assert_eq!(f.inner.call_count(), 2, "expired token stayed cached");
    }

    #[tokio::test]
    async fn an_already_expired_token_is_never_cached() {
        let f = fixture(CacheConfig::default());
        let tok = token("a");
        let expired = f.clock.wall() - Duration::from_secs(1);
        f.inner
            .insert(&tok, grant(Duration::from_secs(3600), Some(expired)));

        f.cache.resolve(request(&tok, "db")).await.unwrap();
        assert!(f.cache.is_empty(), "an expired token occupied an entry");
    }

    #[tokio::test]
    async fn a_refusal_is_remembered_briefly() {
        let f = fixture(CacheConfig::default());
        let tok = token("unknown");

        for _ in 0..5 {
            assert!(f.cache.resolve(request(&tok, "db")).await.is_err());
        }
        assert_eq!(f.inner.call_count(), 1, "refusal was retried every time");
    }

    #[tokio::test]
    async fn a_refusal_is_forgotten_sooner_than_a_grant() {
        // A refusal can be reversed by something outside this process. A long
        // negative TTL makes that fix appear not to work.
        let config = CacheConfig {
            negative_ttl: Duration::from_secs(5),
            max_ttl: Duration::from_secs(300),
            ..CacheConfig::default()
        };
        assert!(config.negative_ttl < config.max_ttl);

        let f = fixture(config);
        let tok = token("later-valid");
        assert!(f.cache.resolve(request(&tok, "db")).await.is_err());

        // The tenant is re-enabled elsewhere.
        f.inner.insert(&tok, grant(Duration::from_secs(60), None));
        assert!(
            f.cache.resolve(request(&tok, "db")).await.is_err(),
            "negative entry should still be held"
        );

        f.clock.advance(Duration::from_secs(6));
        assert!(
            f.cache.resolve(request(&tok, "db")).await.is_ok(),
            "negative entry outlived its TTL"
        );
    }

    #[tokio::test]
    async fn an_unavailable_sidecar_is_not_cached() {
        // Caching this would keep every tenant locked out for the negative TTL
        // after the sidecar recovers, turning a blip into an outage.
        let f = fixture(CacheConfig::default());
        let tok = token("a");
        f.inner.insert(&tok, grant(Duration::from_secs(60), None));
        f.inner.set_unavailable(true);

        assert!(f.cache.resolve(request(&tok, "db")).await.is_err());
        assert!(f.cache.is_empty(), "an outage was cached");

        f.inner.set_unavailable(false);
        assert!(
            f.cache.resolve(request(&tok, "db")).await.is_ok(),
            "recovery was blocked by a cached failure"
        );
    }

    #[tokio::test]
    async fn a_second_leader_for_an_already_cached_key_spends_no_call() {
        // The race the claim alone does not close, made deterministic.
        //
        // A caller whose lookup missed, and which then took the claim after
        // the previous leader had already stored and released, is holding a
        // claim on a key that is in the cache. It must serve the entry rather
        // than call out, and it must release the claim it holds.
        let f = fixture(CacheConfig::default());
        let tok = token("a");
        let key = CacheKey::new(&tok, "db");

        f.inner.insert(&tok, grant(Duration::from_secs(60), None));
        f.cache.resolve(request(&tok, "db")).await.unwrap();
        assert_eq!(f.inner.call_count(), 1);

        let (tx, mut rx) = broadcast::channel(1);
        f.cache.lock_inflight().insert(key.clone(), tx);

        let served = f.cache.recheck_before_calling(&key);

        assert!(served.is_some(), "the cached entry was not served");
        assert!(
            !f.cache.lock_inflight().contains_key(&key),
            "the claim was left held, so every later caller waits on nobody"
        );
        assert!(rx.try_recv().is_ok(), "subscribers were not woken");
        assert_eq!(f.inner.call_count(), 1, "a second call was spent");
    }

    #[tokio::test]
    async fn a_claim_on_a_key_that_is_still_cold_stays_with_its_leader() {
        // The other half, and the one that runs on every miss: nothing in the
        // cache means the caller keeps the claim and goes on to make the call.
        let f = fixture(CacheConfig::default());
        let tok = token("a");
        let key = CacheKey::new(&tok, "db");

        let (tx, _rx) = broadcast::channel(1);
        f.cache.lock_inflight().insert(key.clone(), tx);

        assert!(f.cache.recheck_before_calling(&key).is_none());
        assert!(
            f.cache.lock_inflight().contains_key(&key),
            "the claim was released while its holder was still going to call"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_lookups_of_a_cold_key_make_one_call() {
        // The reconnect storm this exists to survive.
        let f = fixture(CacheConfig::default());
        let tok = token("a");
        f.inner.insert(&tok, grant(Duration::from_secs(60), None));

        let mut handles = Vec::new();
        for _ in 0..64 {
            let cache = Arc::clone(&f.cache);
            let tok = tok.clone();
            handles.push(tokio::spawn(async move {
                cache.resolve(request(&tok, "db")).await
            }));
        }
        for handle in handles {
            handle.await.unwrap().unwrap();
        }

        assert_eq!(
            f.inner.call_count(),
            1,
            "singleflight collapsed nothing: {} calls",
            f.inner.call_count()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn waiters_are_woken_when_the_leader_fails() {
        // A failure that is not cached must still release the waiters, or they
        // block until their own timeouts fire.
        let f = fixture(CacheConfig::default());
        let tok = token("a");
        f.inner.set_unavailable(true);

        let mut handles = Vec::new();
        for _ in 0..8 {
            let cache = Arc::clone(&f.cache);
            let tok = tok.clone();
            handles.push(tokio::spawn(async move {
                cache.resolve(request(&tok, "db")).await
            }));
        }
        for handle in handles {
            assert!(handle.await.unwrap().is_err());
        }
    }

    #[tokio::test]
    async fn a_banned_algorithm_is_refused_without_reaching_the_resolver() {
        let f = fixture(CacheConfig::default());
        let bad = format!(
            "{}.{}.{}",
            URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#),
            URL_SAFE_NO_PAD.encode("{}"),
            URL_SAFE_NO_PAD.encode("")
        );

        let err = f.cache.resolve(request(&bad, "db")).await.unwrap_err();
        assert!(
            matches!(err, AuthError::Refused(AuthRejection::AlgorithmNotAllowed)),
            "{err:?}"
        );
        assert_eq!(f.inner.call_count(), 0, "the sidecar was called anyway");
        assert!(f.cache.is_empty(), "a banned token occupied an entry");
    }

    #[tokio::test]
    async fn the_cache_refuses_to_grow_past_capacity() {
        let f = fixture(CacheConfig {
            capacity: 3,
            ..CacheConfig::default()
        });
        for i in 0..10 {
            let tok = token(&format!("tenant-{i}"));
            f.inner.insert(&tok, grant(Duration::from_secs(60), None));
            f.cache.resolve(request(&tok, "db")).await.unwrap();
        }
        assert_eq!(f.cache.len(), 3, "cache grew past its capacity");
    }

    #[tokio::test]
    async fn a_full_cache_of_dead_entries_admits_a_live_one() {
        // `M24.5`. An entry left this cache only when the same key was looked
        // up again and found expired, or on `clear`. Tokens rotate, so a key is
        // rarely looked up twice past its expiry: the map reached `capacity`,
        // every entry in it was dead, and `store` refused every new one for the
        // life of the process. Every connection then made a sidecar RPC, on
        // what this crate's `AGENTS.md` calls a declared hot path.
        //
        // Nothing failed. It got slower, permanently, and only under the load
        // that fills it.
        let config = CacheConfig {
            capacity: 3,
            ..CacheConfig::default()
        };
        let f = fixture(config);

        for i in 0..3 {
            let tok = token(&format!("old-{i}"));
            f.inner.insert(&tok, grant(Duration::from_secs(60), None));
            f.cache.resolve(request(&tok, "db")).await.unwrap();
        }
        assert_eq!(f.cache.len(), 3, "the cache did not fill");

        // Every one of them expires, and none is ever asked for again. That is
        // what token rotation looks like from in here.
        f.clock.advance(Duration::from_secs(61));

        let fresh = token("new");
        f.inner.insert(&fresh, grant(Duration::from_secs(60), None));
        f.cache.resolve(request(&fresh, "db")).await.unwrap();
        f.cache.resolve(request(&fresh, "db")).await.unwrap();

        assert_eq!(
            f.inner.call_count(),
            4,
            "the new token was resolved twice, so it was never cached"
        );
    }

    #[tokio::test]
    async fn a_full_cache_of_live_entries_still_refuses() {
        // The other half, and the reason the sweep is not an eviction policy.
        // Refusing at capacity is deliberate: an eviction policy is a decision
        // that needs measurement, and admitting past capacity is the one option
        // that is definitely wrong. A sweep that started throwing out live
        // entries would have quietly made that decision.
        let f = fixture(CacheConfig {
            capacity: 3,
            ..CacheConfig::default()
        });
        for i in 0..10 {
            let tok = token(&format!("live-{i}"));
            f.inner.insert(&tok, grant(Duration::from_secs(600), None));
            f.cache.resolve(request(&tok, "db")).await.unwrap();
        }
        assert_eq!(f.cache.len(), 3, "a live entry was evicted to make room");
    }

    #[tokio::test]
    async fn a_full_cache_does_not_sweep_on_every_miss() {
        // The sweep walks the whole map, and `store` runs on the connection
        // path. At capacity with every entry live it would find nothing every
        // time, so it is rate limited, and a test that only proved the sweep
        // happens would not notice it happening a hundred thousand times a
        // second.
        let f = fixture(CacheConfig {
            capacity: 2,
            ..CacheConfig::default()
        });
        for i in 0..2 {
            let tok = token(&format!("live-{i}"));
            f.inner.insert(&tok, grant(Duration::from_secs(600), None));
            f.cache.resolve(request(&tok, "db")).await.unwrap();
        }

        assert_eq!(f.cache.sweeps(), 0, "nothing was full yet");

        // Ten misses against a full cache, inside one interval.
        for i in 0..10 {
            let tok = token(&format!("miss-{i}"));
            f.inner.insert(&tok, grant(Duration::from_secs(600), None));
            f.cache.resolve(request(&tok, "db")).await.unwrap();
        }
        assert_eq!(f.cache.sweeps(), 1, "the sweep ran on every miss");

        // And it is a rate rather than a one-off: past the interval it runs
        // again, which is what keeps a cache that fills with dead entries
        // recoverable rather than recoverable once.
        f.clock.advance(SWEEP_INTERVAL);
        let tok = token("later");
        f.inner.insert(&tok, grant(Duration::from_secs(600), None));
        f.cache.resolve(request(&tok, "db")).await.unwrap();
        assert_eq!(f.cache.sweeps(), 2, "the sweep never ran again");
    }

    #[tokio::test]
    async fn a_sweep_drops_only_what_has_actually_expired() {
        // `M87.6`. `Entries::sweep` keeps an entry when `expires_at > now`.
        // `a_full_cache_of_live_entries_still_refuses` proves a sweep evicts
        // nothing when every entry is live, and
        // `a_full_cache_does_not_sweep_on_every_miss` proves it runs at the
        // right rate, but neither ever advances the clock past a real
        // entry's expiry: with every `expires_at` far in the future and
        // `now` fixed, "not yet expired" and "not exactly now" are the same
        // condition, so neither test can tell `>` from `==`. `cargo mutants`
        // replacing the comparison with `==` evicts every live entry on the
        // first sweep, which is the opposite of what both tests above check
        // for, and neither noticed.
        let f = fixture(CacheConfig {
            capacity: 2,
            ..CacheConfig::default()
        });

        let short = token("short");
        f.inner.insert(&short, grant(Duration::from_secs(1), None));
        f.cache.resolve(request(&short, "db")).await.unwrap();

        let long = token("long");
        f.inner.insert(&long, grant(Duration::from_secs(600), None));
        f.cache.resolve(request(&long, "db")).await.unwrap();
        assert_eq!(f.cache.len(), 2, "both entries should be cached");

        // Past `short`'s one-second TTL and past `SWEEP_INTERVAL`, which is
        // also one second, so one advance clears both rate limits at once.
        f.clock.advance(Duration::from_secs(2));

        // A third, new key at capacity is what triggers a sweep.
        let third = token("third");
        f.inner
            .insert(&third, grant(Duration::from_secs(600), None));
        f.cache.resolve(request(&third, "db")).await.unwrap();
        assert_eq!(
            f.cache.len(),
            2,
            "the expired entry should have been swept and the new one admitted \
             without needing to refuse anything"
        );

        // And the still-live entry is untouched: resolving it again must not
        // call the inner resolver a second time.
        let calls_before = f.inner.call_count();
        f.cache.resolve(request(&long, "db")).await.unwrap();
        assert_eq!(
            f.inner.call_count(),
            calls_before,
            "a live entry was swept along with the expired one"
        );
    }

    #[tokio::test]
    async fn clearing_drops_everything() {
        let f = fixture(CacheConfig::default());
        let tok = token("a");
        f.inner.insert(&tok, grant(Duration::from_secs(60), None));
        f.cache.resolve(request(&tok, "db")).await.unwrap();
        assert!(!f.cache.is_empty());

        f.cache.clear();
        assert!(f.cache.is_empty());
        f.cache.resolve(request(&tok, "db")).await.unwrap();
        assert_eq!(f.inner.call_count(), 2);
    }

    #[tokio::test]
    async fn invalidating_a_primary_drops_only_grants_naming_it() {
        let f = fixture(CacheConfig::default());
        let demoted = token("demoted-primary");
        let other = token("other-primary");
        f.inner
            .insert(&demoted, grant(Duration::from_secs(60), None));
        f.inner.insert(
            &other,
            Grant {
                primary: Backend {
                    server: ServerId::new("db-2", 5432),
                    ..grant(Duration::from_secs(60), None).primary
                },
                ..grant(Duration::from_secs(60), None)
            },
        );
        f.cache.resolve(request(&demoted, "db")).await.unwrap();
        f.cache.resolve(request(&other, "db")).await.unwrap();
        assert_eq!(f.cache.len(), 2);

        let dropped = f.cache.invalidate_primary(&ServerId::new("db-1", 5432));

        assert_eq!(dropped, 1, "the entry naming db-1 was not the one dropped");
        assert_eq!(f.cache.len(), 1);
        f.cache.resolve(request(&demoted, "db")).await.unwrap();
        assert_eq!(
            f.inner.call_count(),
            3,
            "the invalidated entry was still served from cache"
        );
        f.cache.resolve(request(&other, "db")).await.unwrap();
        assert_eq!(
            f.inner.call_count(),
            3,
            "an entry naming a different primary was dropped too"
        );
    }

    #[tokio::test]
    async fn invalidating_a_primary_nothing_names_drops_nothing() {
        let f = fixture(CacheConfig::default());
        let tok = token("a");
        f.inner.insert(&tok, grant(Duration::from_secs(60), None));
        f.cache.resolve(request(&tok, "db")).await.unwrap();

        let dropped = f
            .cache
            .invalidate_primary(&ServerId::new("db-unrelated", 5432));

        assert_eq!(dropped, 0);
        assert_eq!(f.cache.len(), 1);
    }

    #[tokio::test]
    async fn debug_never_reveals_a_token() {
        let f = fixture(CacheConfig::default());
        let tok = token("secret-marker");
        f.inner.insert(&tok, grant(Duration::from_secs(60), None));
        f.cache.resolve(request(&tok, "db")).await.unwrap();

        let rendered = format!("{:?}", f.cache);
        assert!(!rendered.contains(&tok), "token leaked: {rendered}");
        assert!(rendered.contains("entries"));
    }

    #[test]
    fn the_key_is_a_hash_rather_than_the_token() {
        // A memory dump of the keys must not be a credential dump.
        let key = CacheKey::new("super-secret-token", "db");
        let rendered = format!("{key:?}");
        assert!(!rendered.contains("super-secret-token"), "{rendered}");

        // Same inputs, same key; different token, different key.
        assert_eq!(key, CacheKey::new("super-secret-token", "db"));
        assert_ne!(key, CacheKey::new("another-token", "db"));
        assert_ne!(key, CacheKey::new("super-secret-token", "other"));
    }

    #[test]
    fn the_default_negative_ttl_is_shorter_than_the_positive_cap() {
        let config = CacheConfig::default();
        assert!(
            config.negative_ttl < config.max_ttl,
            "a refusal outliving a grant makes a reversal look broken"
        );
    }
}
