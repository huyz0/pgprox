//! Instruction count for warm acquire and the release decision: the pair a
//! transaction-pooling proxy runs once per transaction.
//!
//! See `crates/pgprox-proto/benches/hot_paths.rs` for why this is a plain
//! binary rather than a benchmark harness.

#![allow(missing_docs, clippy::unwrap_used)]

use std::time::Instant;

use pgprox_core::ids::{PoolKey, ServerId};
use pgprox_core::pool::ReleaseOutcome;
use pgprox_pool::pool::{Acquired, Pool, PoolConfig};

fn warm_pool(now: Instant) -> Pool {
    let key = PoolKey::new(ServerId::new("db-1", 5432), "tenant_acme", "acme_app");
    let mut pool = Pool::new(
        key,
        PoolConfig {
            max_size: 16,
            ..PoolConfig::default()
        },
    );

    let mut open = Vec::new();
    for _ in 0..8 {
        assert!(matches!(pool.acquire(), Acquired::OpenNew));
        open.push(pool.opened(now));
    }
    for id in open {
        pool.release(id, ReleaseOutcome::Reusable, now);
    }
    pool
}

fn acquire_and_release(iterations: u64) {
    let now = Instant::now();
    let mut pool = warm_pool(now);

    for _ in 0..iterations {
        let Acquired::Reused(id) = pool.acquire() else {
            unreachable!("a warm pool did not reuse")
        };
        std::hint::black_box(pool.release(id, ReleaseOutcome::Reusable, now));
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let name = args.next().unwrap_or_default();
    let iterations: u64 = args.next().unwrap_or_default().parse().unwrap_or(0);

    match name.as_str() {
        "acquire_and_release" => acquire_and_release(iterations),
        _ => print_names(),
    }
}

fn print_names() {
    #[allow(clippy::print_stdout)]
    {
        println!("acquire_and_release");
    }
}
