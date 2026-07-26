//! Allocation budget for the grant cache lookup.
//!
//! The declared hot path is the *hit*: every connection that arrives with a
//! token the node has seen recently takes it, and at the connection rates this
//! proxy is built for that is the difference between a cache and a bottleneck.
//! A miss makes a call over a socket to the sidecar, so its allocations are
//! not the interesting number.
//!
//! # Why this budget is not zero
//!
//! A hit hashes the token, builds a key that owns its database name, and
//! clones the grant it found, because the caller keeps the grant for the life
//! of the session while the cache keeps its own copy. Those are real costs
//! with reasons, so the budget is a stated ceiling rather than zero, and the
//! test's job is to notice when it grows.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use pgprox_auth::cache::{CacheConfig, CachingResolver};
use std::time::Duration;

use pgprox_core::auth::{
    AuthRequest, Backend, ClaimSet, CredentialResolver, FakeCredentialResolver, Grant, PoolHints,
    TlsMode,
};
use pgprox_core::clock::FakeClock;
use pgprox_core::ids::{ServerId, TenantId};
use pgprox_core::secret::SecretString;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// What one connection's grant lookup is allowed to cost on a hit.
///
/// Measured at 15 and set two above it, so ordinary noise does not fail the
/// build while a change that adds a copy does. It covers the whole path a
/// connection takes, request included, because that is the thing that happens
/// per connection: the token and database strings the startup packet produced,
/// the key that owns its database name, and the grant clone the caller keeps
/// for the life of the session while the cache keeps its own.
///
/// Fifteen is not obviously right. The key could borrow rather than own, and
/// the grant could be an `Arc` rather than a clone. Both are optimizations,
/// and this milestone measures before it optimizes, so they belong on the
/// hot-and-expensive list rather than in this commit.
const HIT_BUDGET: u64 = 17;

fn allocations(body: impl FnOnce()) -> u64 {
    let before = dhat::HeapStats::get().total_blocks;
    body();
    dhat::HeapStats::get().total_blocks - before
}

/// A token whose header names an approved algorithm, since the resolver checks
/// that before it looks in the cache.
fn token() -> String {
    use base64::Engine as _;
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    format!(
        "{}.{}.not-a-signature",
        engine.encode(r#"{"alg":"RS256","typ":"JWT"}"#),
        engine.encode(r#"{"sub":"acme"}"#)
    )
}

fn request(token: &str) -> AuthRequest {
    AuthRequest {
        token: SecretString::new(token.to_owned()),
        startup_user: "acme_app".to_owned(),
        startup_database: "tenant_acme".to_owned(),
        client_addr: "10.0.0.1".parse().unwrap(),
    }
}

fn grant() -> Grant {
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
        ttl: Duration::from_secs(300),
        claims: ClaimSet {
            subject: None,
            expires_at: None,
            issued_at: None,
        },
    }
}

#[test]
fn a_grant_cache_hit_stays_inside_its_budget() {
    let _profiler = dhat::Profiler::builder().testing().build();

    // --- the counter counts ------------------------------------------------
    let sanity = allocations(|| {
        std::hint::black_box(vec![0_u8; 64]);
    });
    assert!(sanity > 0, "the allocation counter is not counting");

    let clock = Arc::new(FakeClock::new());
    let token = token();
    let inner = FakeCredentialResolver::new().with_grant(token.clone(), grant());
    let resolver = CachingResolver::new(inner, clock, CacheConfig::default());

    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();

    // Warm: the first call is a miss, which fills the cache and grows every
    // collection it touches. The budget is about what happens after that.
    for _ in 0..4 {
        runtime.block_on(resolver.resolve(request(&token))).unwrap();
    }

    let per_hit = allocations(|| {
        for _ in 0..100 {
            std::hint::black_box(runtime.block_on(resolver.resolve(request(&token))).unwrap());
        }
    }) / 100;

    assert!(
        per_hit <= HIT_BUDGET,
        "a cache hit allocated {per_hit} times, budget is {HIT_BUDGET}"
    );
    assert_eq!(resolver.len(), 1, "the run was measuring misses, not hits");
}
