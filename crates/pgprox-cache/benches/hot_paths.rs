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

/// How many entries the invalidated tenant holds.
///
/// Not one. See `invalidate_a_tenants_entries`: at one entry the benchmark moved
/// 6% between runs of the same code, because a `HashMap`'s probe count depends
/// on a per-process random seed and at that size it was a measurable share of
/// the work.
const HELD: usize = 16;

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
        store.put(key(&tenant(i % TENANTS), i), result());
    }
    store
}

/// A lookup that finds a live entry.
fn cache_hit(iterations: u64) {
    let store = populated();
    let wanted = key(&tenant(0), 0);

    for _ in 0..iterations {
        std::hint::black_box(store.get(&wanted));
    }
}

/// Hits that rotate, so the recency order actually reorders.
///
/// `cache_hit` asks for the same key every time, which is the best case: the
/// entry is already the most recently used and `touch` returns without
/// touching anything. That is a real case and it is not the only one, and
/// until `M26.4` added this the number for a hit was measured entirely on the
/// path that skips the work.
fn cache_hit_rotating(iterations: u64) {
    let store = populated();
    let wanted: Vec<CacheKey> = (0..TENANTS).map(|i| key(&tenant(i % TENANTS), i)).collect();

    for i in 0..iterations {
        let at = usize::try_from(i).unwrap_or(0) % wanted.len();
        std::hint::black_box(store.get(&wanted[at]));
    }
}

/// A lookup that finds nothing, which is what most statements are.
fn cache_miss(iterations: u64) {
    let store = populated();
    let absent = key(&tenant(0), ENTRIES + 1);

    for _ in 0..iterations {
        std::hint::black_box(store.get(&absent));
    }
}

/// Storing an answer over one already held.
fn cache_put(iterations: u64) {
    let store = populated();
    // `HELD` keys, cycled, rather than one key repeated.
    //
    // Still one put per iteration, so the number means what it has always
    // meant and the before-and-after in `docs/optimizations.md` stays a
    // comparison. What changes is which bucket each put lands in.
    //
    // A single key puts every iteration into one bucket, whose probe length is
    // a lottery on the per-process hash seed: the same code read 3,668 and
    // 3,838 on the same runner, a 4.6% spread against a 5% gate, and it broke
    // CI on a commit that did not touch this crate. Cycling spreads the puts
    // over `HELD` buckets so the run averages `HELD` draws instead of taking
    // one, which is the fix `M28.2` applied to `invalidate_a_tenants_entries`
    // for the same reason and the reason `HELD` exists at all. `M59.0`.
    let keys: Vec<CacheKey> = (0..HELD).map(|i| key(&tenant(0), i)).collect();

    for i in 0..iterations {
        // The key is what varies per call and the return is a unit, so the
        // barrier goes on the input rather than the output.
        let wanted = &keys[usize::try_from(i).unwrap_or(0) % HELD];
        store.put(std::hint::black_box(wanted.clone()), result());
    }
}

/// What one write costs a node holding other tenants' entries.
///
/// `HELD` entries stored and their tenant invalidated, on a node holding
/// `ENTRIES` entries for `TENANTS` other tenants. That is the case the cost was
/// always about: a node serving five thousand tenants used to spend every
/// invalidation walking the other four thousand nine hundred and ninety-nine.
///
/// # Why the puts are inside the measurement
///
/// The N and 2N subtraction needs every iteration to do the same work, so the
/// tenant has to be back to holding something before the next one. That makes
/// this the sum of `HELD` puts and an invalidation rather than an invalidation
/// alone.
///
/// # Why `HELD` is not one
///
/// It was, and the benchmark moved with a random seed. It read 5,689, then
/// 6,080, then 5,609 across runs that differed in nothing it measures, which is
/// +6% and -1% around the same code against a gate that fails at 5%. During
/// `M28.1` it reported a regression from an LTO change that had not touched it.
///
/// The cause is the one `M26.4` recorded for the version before this: at that
/// size, how many probes a `HashMap` lookup takes is a measurable share of the
/// work, and the probe count depends on a per-process random seed. Sixteen
/// entries makes the seed noise rather than signal, and measures a tenant with
/// a working set rather than a tenant with one row. `M28.2`.
fn invalidate_a_tenants_entries(iterations: u64) {
    let store = populated();
    let quiet = tenant(TENANTS);
    let held: Vec<CacheKey> = (0..HELD).map(|i| key(&quiet, ENTRIES + 7 + i)).collect();

    for _ in 0..iterations {
        for one in &held {
            store.put(one.clone(), result());
        }
        store.invalidate_tenant(std::hint::black_box(&quiet));
    }
}

/// Whether a tenant is served, which every statement asks before anything else.
///
/// Asked about every configured tenant and about as many that are not, once
/// each, per iteration.
///
/// Not one tenant. At one it read 147, 148, 135 and 154 across four runs of the
/// same binary, a 14% spread against a 5% tolerance, and it failed a run whose
/// only change was in `pgprox-route`. One call is one `HashMap` probe and a
/// probe count depends on a per-process random seed, which is `M26.4`'s finding
/// and `M28.2`'s, reaching a third benchmark. `M30.6`.
///
/// Both answers, because a node runs both: a tenant that opted into the cache
/// and a tenant that did not are different paths through the same lookup, and
/// measuring only the one that says yes measures half of what this costs.
///
/// The lock is taken per call rather than once for the batch, because that is
/// what a statement pays. Dividing this by `2 * TENANTS` gives the per-statement
/// figure.
fn serves_a_mix_of_tenants(iterations: u64) {
    let store = populated();
    let asked: Vec<TenantId> = (0..TENANTS)
        .map(tenant)
        .chain((0..TENANTS).map(|i| TenantId::new(format!("absent-{i:04}"))))
        .collect();

    for _ in 0..iterations {
        for one in &asked {
            std::hint::black_box(store.serves(std::hint::black_box(one)));
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let name = args.next().unwrap_or_default();
    let iterations: u64 = args.next().unwrap_or_default().parse().unwrap_or(0);

    match name.as_str() {
        "cache_hit" => cache_hit(iterations),
        "cache_hit_rotating" => cache_hit_rotating(iterations),
        "cache_miss" => cache_miss(iterations),
        "cache_put" => cache_put(iterations),
        "invalidate_a_tenants_entries" => invalidate_a_tenants_entries(iterations),
        "serves_a_mix_of_tenants" => serves_a_mix_of_tenants(iterations),
        _ => print_names(),
    }
}

fn print_names() {
    #[allow(clippy::print_stdout)]
    {
        println!("cache_hit");
        println!("cache_hit_rotating");
        println!("cache_miss");
        println!("cache_put");
        println!("invalidate_a_tenants_entries");
        println!("serves_a_mix_of_tenants");
    }
}
