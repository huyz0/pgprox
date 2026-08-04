//! Instruction counts for the query cache's three per-statement paths.
//!
//! See `crates/pgprox-proto/benches/hot_paths.rs` for why this is a plain
//! binary rather than a benchmark harness.
//!
//! # Why the cache needs one at all
//!
//! `run-2026-07-29-cache.md` measured what the cache is worth end to end and
//! nothing has ever measured what it costs per call. The store holds one lock
//! and its own module docs say the answer, if a profile ever finds it, is to
//! shard by the hash of the key. A profile cannot find it without a number to
//! compare against.
//!
//! # Each of these is idempotent
//!
//! The N and 2N subtraction only cancels if every iteration does the same work.
//! A hit leaves the entry in place, a put of a key already held replaces it,
//! and the invalidation below names a tenant with no entries so the scan runs
//! and removes nothing. That last one is the point rather than a compromise:
//! what a write costs a node is the walk, and on a node serving five thousand
//! tenants almost all of the walk is other tenants' entries.

#![allow(missing_docs, clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use pgprox_cache::Store;
use pgprox_core::cache::{CacheKey, CachedResult, QueryCache};
use pgprox_core::clock::FakeClock;
use pgprox_core::config::{QueryCacheConfig, TenantCache};
use pgprox_core::ids::TenantId;

/// How many entries a populated store holds for the multi-tenant benches.
///
/// Small enough that the bench finishes and large enough that an O(entries)
/// walk is visible against the constant cost of taking the lock.
const ENTRIES: usize = 4_096;

/// How many tenants those entries are spread across.
const TENANTS: usize = 64;

/// Runs a future that never yields.
///
/// Every method here is async because the trait is, and none of them awaits
/// anything. The same helper `bin/pgprox`'s tests use, and for the same reason:
/// pulling a runtime into a bench binary would put its startup inside the
/// measurement.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, Waker};
    // `pin!` rather than `Box::pin`. Boxing allocates once per call, which in
    // a budget test is the harness failing its own assertion and in a bench is
    // a malloc inside every measurement.
    let mut future = std::pin::pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut cx) {
        Poll::Ready(value) => value,
        Poll::Pending => unreachable!("the store yielded, which it must not"),
    }
}

fn tenant(i: usize) -> TenantId {
    TenantId::new(format!("tenant-{i:04}"))
}

fn key(tenant: &TenantId, i: usize) -> CacheKey {
    CacheKey {
        tenant: tenant.clone(),
        database: Arc::from("tenant_db"),
        user: Arc::from("app_role"),
        normalized_sql: Arc::from(format!("select * from orders where id = ${i}")),
        params: Arc::from(&b"\0\0\0\x011"[..]),
        search_path: Arc::from("public"),
    }
}

fn result() -> CachedResult {
    CachedResult {
        // A point select's answer: a row description and one small row.
        frames: Arc::from(vec![0_u8; 256].as_slice()),
        ttl: Duration::from_secs(30),
    }
}

/// A store holding `ENTRIES` entries across `TENANTS` tenants.
fn populated() -> Arc<Store> {
    let clock = FakeClock::new();
    let store = Store::new(Arc::new(clock));
    store.reconfigure(&QueryCacheConfig {
        // Room for everything below, so nothing is evicted mid-run and the
        // bench measures the path rather than the eviction.
        max_bytes: 64 * 1024 * 1024,
        max_entry_bytes: 1024 * 1024,
        ttl_cap: Duration::from_secs(300),
        tenants: (0..=TENANTS)
            .map(|i| {
                (
                    tenant(i),
                    TenantCache {
                        ttl: Duration::from_secs(300),
                    },
                )
            })
            .collect(),
    });

    for i in 0..ENTRIES {
        block_on(store.put(key(&tenant(i % TENANTS), i), result()));
    }
    store
}

/// A lookup that finds a live entry.
fn cache_hit(iterations: u64) {
    let store = populated();
    let wanted = key(&tenant(0), 0);

    for _ in 0..iterations {
        std::hint::black_box(block_on(store.get(&wanted)));
    }
}

/// A lookup that finds nothing, which is what most statements are.
fn cache_miss(iterations: u64) {
    let store = populated();
    let absent = key(&tenant(0), ENTRIES + 1);

    for _ in 0..iterations {
        std::hint::black_box(block_on(store.get(&absent)));
    }
}

/// Storing an answer over one already held.
fn cache_put(iterations: u64) {
    let store = populated();
    let wanted = key(&tenant(0), 0);

    for _ in 0..iterations {
        // The key is what varies per call and the return is a unit, so the
        // barrier goes on the input rather than the output.
        block_on(store.put(std::hint::black_box(wanted.clone()), result()));
    }
}

/// What one write costs a node holding other tenants' entries.
///
/// One entry stored and its tenant invalidated, on a node holding `ENTRIES`
/// entries for `TENANTS` other tenants. That is the case the cost was always
/// about: a node serving five thousand tenants used to spend every
/// invalidation walking the other four thousand nine hundred and ninety-nine.
///
/// # Why the put is inside the measurement
///
/// The N and 2N subtraction needs every iteration to do the same work, so the
/// tenant has to be back to holding something before the next one. That makes
/// this the sum of a `put` and an invalidation rather than an invalidation
/// alone, and the alternative was worse: with the walk gone, invalidating a
/// tenant that holds nothing is one failed hash lookup, and at that size the
/// number moved 15% between two runs of the same binary because how many
/// probes a `HashMap` miss takes depends on a per-process random seed.
///
/// What this guards is the walk coming back. Against roughly six thousand
/// instructions, a reintroduced walk is two hundred thousand.
fn invalidate_after_one_put(iterations: u64) {
    let store = populated();
    let quiet = tenant(TENANTS);
    let only = key(&quiet, ENTRIES + 7);

    for _ in 0..iterations {
        block_on(store.put(only.clone(), result()));
        block_on(store.invalidate_tenant(std::hint::black_box(&quiet)));
    }
}

/// Whether a tenant is served, which every statement asks before anything else.
fn serves(iterations: u64) {
    let store = populated();
    let known = tenant(0);

    for _ in 0..iterations {
        std::hint::black_box(store.serves(&known));
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let name = args.next().unwrap_or_default();
    let iterations: u64 = args.next().unwrap_or_default().parse().unwrap_or(0);

    match name.as_str() {
        "cache_hit" => cache_hit(iterations),
        "cache_miss" => cache_miss(iterations),
        "cache_put" => cache_put(iterations),
        "invalidate_after_one_put" => invalidate_after_one_put(iterations),
        "serves" => serves(iterations),
        _ => print_names(),
    }
}

fn print_names() {
    #[allow(clippy::print_stdout)]
    {
        println!("cache_hit");
        println!("cache_miss");
        println!("cache_put");
        println!("invalidate_after_one_put");
        println!("serves");
    }
}
