//! Allocation budgets for the paths a statement takes through the cache.
//!
//! Same shape as `pgprox-proto`'s and `pgprox-pool`'s: one test function
//! because `dhat` allows one profiler per process, the harness checked before
//! any budget is, and every path warmed first because the budget is about the
//! steady state.
//!
//! # What this catches, and what it does not
//!
//! `M26.2` took a hit from 4,101 instructions to 2,641 and this test would not
//! have noticed either number: the cost was a second hash of a six-field key
//! and six atomic increments, and neither allocates. That is the division
//! `standards/testing.md` draws. A budget catches a new copy; the instruction
//! count in `product/perf/baseline.json` catches work that got more expensive
//! without allocating, and the cache needed both.
//!
//! It also found what neither reading nor the instruction count did. A miss,
//! which hashes a key and returns `None`, allocated twice, and neither block
//! was the store's: see `M26.3` and the note on the trait.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use pgprox_cache::Store;
use pgprox_core::cache::{CacheKey, CachedResult, QueryCache};
use pgprox_core::clock::FakeClock;
use pgprox_core::config::{QueryCacheConfig, TenantCache};
use pgprox_core::ids::TenantId;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn allocations(body: impl FnOnce()) -> u64 {
    let before = dhat::HeapStats::get().total_blocks;
    body();
    dhat::HeapStats::get().total_blocks - before
}

fn key(sql: &str) -> CacheKey {
    CacheKey {
        tenant: TenantId::new("acme"),
        database: Arc::from("tenant_db"),
        user: Arc::from("app_role"),
        normalized_sql: Arc::from(sql),
        params: Arc::from(&b"\0\0\0\x011"[..]),
        search_path: Arc::from("public"),
    }
}

fn served() -> QueryCacheConfig {
    QueryCacheConfig {
        max_bytes: 64 * 1024 * 1024,
        max_entry_bytes: 1024 * 1024,
        ttl_cap: Duration::from_secs(300),
        tenants: [(
            TenantId::new("acme"),
            TenantCache {
                ttl: Duration::from_secs(300),
            },
        )]
        .into_iter()
        .collect(),
    }
}

#[test]
fn a_hit_serves_an_answer_without_building_anything() {
    let _profiler = dhat::Profiler::builder().testing().build();

    // --- the counter counts ------------------------------------------------
    let sanity = allocations(|| {
        std::hint::black_box(vec![0_u8; 64]);
    });
    assert!(sanity > 0, "the allocation counter is not counting");

    let store = Store::new(Arc::new(FakeClock::new()));
    store.reconfigure(&served());

    // Warm. The first `put` of each key allocates the shared key and grows the
    // two indexes, and a `BTreeMap` allocates a node now and then as it fills.
    // Neither is the steady state, and measuring across them would be
    // measuring the fixture.
    let keys: Vec<CacheKey> = (0..64).map(|i| key(&format!("select {i}"))).collect();
    for one in &keys {
        (store.put(
            one.clone(),
            CachedResult {
                frames: Arc::from(vec![0_u8; 256].as_slice()),
                ttl: Duration::from_secs(60),
            },
        ));
    }
    for _ in 0..8 {
        for one in &keys {
            std::hint::black_box(store.get(one));
        }
    }
    let absent = key("select nothing at all");

    // --- a miss builds nothing ---------------------------------------------
    //
    // It hashes a key, probes a map and returns `None`. There was nothing here
    // to allocate and it allocated twice: `QueryCache` was an
    // `#[async_trait]`, which boxes the future of every method on a store that
    // never awaits, and `pgprox-core` also implements the trait for `Arc<T>`,
    // so a caller holding an `Arc<dyn QueryCache>` boxed once for the
    // forwarding call and once for the real one. `M26.3` made the trait
    // synchronous and both went.
    let misses = allocations(|| {
        for _ in 0..64 {
            std::hint::black_box(store.get(&absent));
        }
    });
    assert_eq!(misses, 0, "a miss allocated {misses} time(s) across 64");

    // Through the `Arc` and through the store itself, which used to differ by
    // a block per call and now cannot differ at all.
    let direct = allocations(|| {
        for _ in 0..64 {
            std::hint::black_box(<Store as QueryCache>::get(&store, &absent));
        }
    });
    assert_eq!(direct, misses, "the forwarding impl costs something again");

    // --- a hit builds only what the recency order does ----------------------
    //
    // It reads one entry, clones two `Arc`s and moves a `u64` between two
    // places in a tree. Everything it hands back is shared with what is
    // stored, so the only blocks left are the tree's own: a `BTreeMap` that
    // has one key removed and a higher one inserted on every hit splits and
    // merges nodes for as long as the cache is used. That is `M26.4`, and
    // until it lands this is the one path here that allocates at all.
    let hits = allocations(|| {
        for one in &keys {
            std::hint::black_box(store.get(one));
        }
    });
    assert!(
        hits > 0,
        "the recency order stopped churning, so this budget is now wrong in \
         the good direction and M26.4 is done"
    );
    assert!(
        hits <= 64,
        "a hit allocated more than the recency order's churn: {hits} across \
         {} lookups",
        keys.len()
    );

    // --- a tenant's write does not allocate per entry it drops --------------
    //
    // `M26.1` gave invalidation an index to walk instead of the whole node.
    // The walk it replaced cloned every key it matched, so this is the
    // allocation half of that change: sixty-four entries dropped for a
    // constant, which is the set of sequence numbers taken out of the index
    // and iterated.
    let invalidations = allocations(|| {
        store.invalidate_tenant(&TenantId::new("acme"));
    });
    assert!(
        invalidations <= 2,
        "invalidating 64 entries allocated {invalidations} block(s), which is \
         per entry rather than constant"
    );
}
