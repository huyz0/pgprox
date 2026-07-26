//! Instruction count for the route decision.
//!
//! See `crates/pgprox-proto/benches/hot_paths.rs` for why this is a plain
//! binary rather than a benchmark harness.

#![allow(missing_docs, clippy::unwrap_used)]

use std::time::Instant;

use pgprox_core::ids::Lsn;
use pgprox_route::replica::{ReplicaConfig, Replicas};
use pgprox_route::router::{Routed, SessionRouter};

fn replicas(now: Instant) -> Replicas {
    let mut replicas = Replicas::new(2, ReplicaConfig::default());
    replicas.observe(0, Lsn::new(0x1600_0000), true, now);
    replicas.observe(1, Lsn::new(0x1600_0000), true, now);
    replicas
}

/// Routes one statement, repeatedly, on a warmed session.
///
/// Warmed because the session's replica-states buffer grows on the first call
/// and never again.
fn route(sql: &str, iterations: u64) {
    let now = Instant::now();
    let replicas = replicas(now);
    let mut router = SessionRouter::new();
    router.route(sql, false, &replicas, now);
    router.end_transaction();

    for _ in 0..iterations {
        std::hint::black_box(matches!(
            router.route(std::hint::black_box(sql), false, &replicas, now),
            Routed::To(_)
        ));
        router.end_transaction();
    }
}

const POINT_SELECT: &str = "SELECT abalance FROM pgbench_accounts WHERE aid = 1";
const UPDATE: &str = "UPDATE pgbench_accounts SET abalance = abalance + 1 WHERE aid = 1";

fn main() {
    let mut args = std::env::args().skip(1);
    let name = args.next().unwrap_or_default();
    let iterations: u64 = args.next().unwrap_or_default().parse().unwrap_or(0);

    match name.as_str() {
        "route_point_select" => route(POINT_SELECT, iterations),
        "route_update" => route(UPDATE, iterations),
        "route_begin" => route("BEGIN", iterations),
        _ => print_names(),
    }
}

fn print_names() {
    #[allow(clippy::print_stdout)]
    {
        println!("route_point_select");
        println!("route_update");
        println!("route_begin");
    }
}
