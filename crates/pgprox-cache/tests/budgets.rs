//! Allocation budgets for the paths a statement takes through the cache.
//!
//! Same shape as `pgprox-proto`'s and `pgprox-pool`'s: one test function
//! because `dhat` allows one profiler per process, the harness checked before
//! any budget is, and every path warmed first because the budget is about the
//! steady state.
//!
//! # What this catches, and what it does not
//!
//! `M26.2` took a hit from 4,101 instructions to 2,719 and this test would not
//! have noticed either number: the cost was a second hash of a six-field key
//! and six atomic increments, and neither allocates. That is the division
//! `standards/testing.md` draws. A budget catches a new copy; the instruction
//! count in `product/perf/baseline.json` catches work that got more expensive
//! without allocating, and the cache needed both.
//!
//! What this holds is the property that made the instruction count reachable:
//! a hit borrows and shares, and never builds anything.

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

/// Heap blocks one call through the trait costs before it does anything.
///
/// `QueryCache` is an `#[async_trait]`, which boxes the future of every
/// method, and `pgprox-core` also implements the trait for `Arc<T>`, so a
/// caller holding an `Arc<dyn QueryCache>` boxes once for the forwarding call
/// and once for the real one. Neither is the store's doing and the store never
/// awaits anything.
const BOXES_PER_CALL: u64 = 2;

fn allocations(body: impl FnOnce()) -> u64 {
    let before = dhat::HeapStats::get().total_blocks;
    body();
    dhat::HeapStats::get().total_blocks - before
}

/// Runs a future that never yields.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, Waker};
    // `pin!` rather than `Box::pin`. Boxing allocates once per call, which in
    // a budget test is the harness failing its own assertion and in a bench is
    // a malloc inside every measurement.
    let mut future = std::pin::pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut cx) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("the store yielded, which it must not"),
    }
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
        block_on(store.put(
            one.clone(),
            CachedResult {
                frames: Arc::from(vec![0_u8; 256].as_slice()),
                ttl: Duration::from_secs(60),
            },
        ));
    }
    for _ in 0..8 {
        for one in &keys {
            std::hint::black_box(block_on(store.get(one)));
        }
    }
    let absent = key("select nothing at all");

    // --- what a lookup actually costs in blocks ----------------------------
    //
    // Two per call, and neither is the store's doing. `QueryCache` is an
    // `#[async_trait]`, which boxes the future of every method, and
    // `pgprox-core` also implements the trait for `Arc<T>`, so a caller
    // holding an `Arc<dyn QueryCache>` boxes once for the forwarding call and
    // once for the real one. The store never awaits anything.
    //
    // Asserted rather than aspired to, and the numbers are what `M26.3` and
    // `M26.4` are for. A budget that said zero would fail today and teach
    // nobody where the blocks come from.
    let misses = allocations(|| {
        for _ in 0..64 {
            std::hint::black_box(block_on(store.get(&absent)));
        }
    });
    assert_eq!(
        misses,
        64 * BOXES_PER_CALL,
        "a miss allocated something beyond the trait's own boxing"
    );

    // One box rather than two, which is the whole of the difference: this
    // reaches the store's own implementation instead of going through the
    // blanket one for `Arc`. `M26.3`.
    let direct = allocations(|| {
        for _ in 0..64 {
            std::hint::black_box(block_on(<Store as QueryCache>::get(&store, &absent)));
        }
    });
    assert_eq!(
        direct, 64,
        "the forwarding impl is not what the second block is"
    );

    // --- a hit builds nothing the trait did not ----------------------------
    //
    // It reads one entry, clones two `Arc`s and moves a `u64` between two
    // places in a tree. Everything it hands back is shared with what is
    // stored, so the only blocks are the two above and the recency order's
    // own: a `BTreeMap` that has one key removed and another inserted on every
    // hit splits and merges nodes as it goes. That is `M26.4`.
    let hits = allocations(|| {
        for one in &keys {
            std::hint::black_box(block_on(store.get(one)));
        }
    });
    let per_hit = 64 * BOXES_PER_CALL;
    assert!(
        hits > per_hit,
        "the recency order stopped churning, so this budget is now wrong \
         in the good direction: {hits} against {per_hit}"
    );
    assert!(
        hits <= 64 * 3,
        "a hit allocated more than the trait's boxing and the recency \
         order's churn: {hits} across {} lookups",
        keys.len()
    );

    // --- a tenant's write does not allocate per entry it drops --------------
    //
    // `M26.1` gave invalidation an index to walk instead of the whole node.
    // The walk it replaced cloned every key it matched, so this is the
    // allocation half of that change and the instruction count is the rest:
    // sixty-four entries dropped for a constant number of blocks.
    let invalidations = allocations(|| {
        block_on(store.invalidate_tenant(&TenantId::new("acme")));
    });
    assert!(
        invalidations <= BOXES_PER_CALL + 2,
        "invalidating 64 entries allocated {invalidations} blocks, which is \
         per entry rather than constant"
    );
}
