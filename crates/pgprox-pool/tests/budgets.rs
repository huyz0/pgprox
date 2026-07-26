//! Allocation budgets for the two hot paths this crate owns.
//!
//! `standards/testing.md` names warm-pool acquire and the release decision as
//! declared hot paths, and said both were written to be allocation-free and
//! never measured. This file is the measurement.
//!
//! Same shape as `pgprox-proto`'s: one test function because `dhat` allows one
//! profiler per process, the harness checked before any budget is, and every
//! path warmed first because the budget is about the steady state.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::time::Instant;

use pgprox_core::ids::{PoolKey, ServerId};
use pgprox_core::pool::ReleaseOutcome;
use pgprox_pool::pool::{Acquired, Pool, PoolConfig};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn allocations(body: impl FnOnce()) -> u64 {
    let before = dhat::HeapStats::get().total_blocks;
    body();
    dhat::HeapStats::get().total_blocks - before
}

fn pool() -> Pool {
    let key = PoolKey::new(ServerId::new("db-1", 5432), "tenant_acme", "acme_app");
    Pool::new(
        key,
        PoolConfig {
            max_size: 16,
            ..PoolConfig::default()
        },
    )
}

#[test]
fn the_hot_paths_this_crate_owns_stay_inside_their_budgets() {
    let _profiler = dhat::Profiler::builder().testing().build();

    // --- the counter counts ------------------------------------------------
    let sanity = allocations(|| {
        std::hint::black_box(vec![0_u8; 64]);
    });
    assert!(sanity > 0, "the allocation counter is not counting");

    let mut pool = pool();
    let now = Instant::now();

    // Warm: open the connections the pool will then hand out over and over.
    // The first acquire of a connection that does not exist has to build one,
    // and that is not the path being measured.
    let mut open = Vec::new();
    for _ in 0..8 {
        assert_eq!(pool.acquire(), Acquired::OpenNew);
        open.push(pool.opened());
    }
    for id in open.drain(..) {
        assert!(pool.release(id, ReleaseOutcome::Reusable, now));
    }
    // And once more around, so every collection has grown to its working size.
    for _ in 0..8 {
        let Acquired::Reused(id) = pool.acquire() else {
            panic!("a warm pool did not reuse");
        };
        open.push(id);
    }
    for id in open.drain(..) {
        assert!(pool.release(id, ReleaseOutcome::Reusable, now));
    }

    // --- warm acquire, and the release decision ----------------------------
    //
    // Budget: zero. Acquire moves a connection from the idle deque to the
    // checked-out map and release moves it back, and both collections keep
    // their capacity across the cycle. This is the path every transaction in a
    // transaction-pooling proxy takes, twice.
    let cycling = allocations(|| {
        for _ in 0..1_000 {
            let Acquired::Reused(id) = pool.acquire() else {
                panic!("a warm pool did not reuse");
            };
            pool.release(id, ReleaseOutcome::Reusable, now);
        }
    });
    assert_eq!(
        cycling, 0,
        "acquire and release allocated {cycling} times per thousand cycles"
    );

    // --- a discarding release ----------------------------------------------
    //
    // Budget: zero. This is the release a session outside a transaction
    // boundary takes, and it drops the connection rather than pooling it, so
    // it must not cost more than the reusing one.
    let Acquired::Reused(id) = pool.acquire() else {
        panic!("a warm pool did not reuse");
    };
    let discarding = allocations(|| {
        assert!(!pool.release(id, ReleaseOutcome::Discard, now));
    });
    assert_eq!(
        discarding, 0,
        "a discarding release allocated {discarding} times"
    );

    // --- reading the pool's own numbers ------------------------------------
    //
    // Budget: zero. `stats` is called per scrape and per admission decision,
    // and a struct of counters must not build anything.
    let stats = allocations(|| {
        for _ in 0..1_000 {
            std::hint::black_box(pool.stats());
            std::hint::black_box(pool.total());
        }
    });
    assert_eq!(stats, 0, "reading pool stats allocated {stats} times");
}
