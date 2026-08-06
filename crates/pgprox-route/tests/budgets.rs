//! Allocation budget for the route decision.
//!
//! `docs/internal/standards/testing.md` names classification plus replica eligibility as a
//! declared hot path and said `SessionRouter` keeps its replica-states buffer
//! for the life of the session rather than building one per statement. That is
//! the claim this file turns into an assertion.
//!
//! Same shape as the other budget files: one test function, the harness
//! checked first, and the path warmed before it is measured.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::time::Instant;

use pgprox_core::ids::Lsn;
use pgprox_route::replica::{ReplicaConfig, Replicas};
use pgprox_route::router::SessionRouter;

/// Allocations made by this thread while `body` runs.
///
/// By this thread, which is the whole point and is what `M64.0` is about. See
/// `docs/internal/standards/testing.md`.
fn allocations(body: impl FnOnce()) -> u64 {
    allocation_counter::measure(body).count_total
}

/// The four shapes the reference workload sends, in the proportions it sends
/// them. Routing a `SELECT` and routing an `UPDATE` take different branches,
/// so measuring one would leave the other unmeasured.
const WORKLOAD: [&str; 4] = [
    "SELECT abalance FROM pgbench_accounts WHERE aid = 1",
    "SELECT sum(abalance) FROM pgbench_accounts WHERE bid = 1",
    "UPDATE pgbench_accounts SET abalance = abalance + 1 WHERE aid = 1",
    "INSERT INTO pgbench_history (tid, bid, aid, delta) VALUES (1, 1, 1, 1)",
];

#[test]
fn the_route_decision_stays_inside_its_budget() {
    // --- the counter counts ------------------------------------------------
    let sanity = allocations(|| {
        std::hint::black_box(vec![0_u8; 64]);
    });
    assert!(sanity > 0, "the allocation counter is not counting");

    let now = Instant::now();
    let mut replicas = Replicas::new(2, ReplicaConfig::default());
    replicas.observe(0, Lsn::new(0x1600_0000), true, now);
    replicas.observe(1, Lsn::new(0x1600_0000), true, now);

    let mut router = SessionRouter::new();

    // Warm: the session's replica-states buffer grows once and is then reused,
    // which is the design the budget is about.
    for sql in WORKLOAD {
        std::hint::black_box(router.route(sql, false, &replicas, now));
        router.end_transaction();
    }
    router.reset();

    // --- classification plus replica eligibility ---------------------------
    //
    // Budget: zero. This runs once per statement, on every statement, on every
    // session. The classifier iterates the shared lexer without lowercasing
    // and the replica states go into a buffer the session already owns.
    let routing = allocations(|| {
        for _ in 0..250 {
            for sql in WORKLOAD {
                std::hint::black_box(router.route(sql, false, &replicas, now));
                router.end_transaction();
            }
        }
    });
    assert_eq!(
        routing, 0,
        "the route decision allocated {routing} times per thousand statements"
    );

    // --- a session that has written ----------------------------------------
    //
    // Budget: zero. After a write the router compares each replica's replayed
    // LSN against the session's watermark, which is the branch that reads the
    // states buffer rather than skipping it.
    router.record_write(Lsn::new(0x1600_1000));
    let after_write = allocations(|| {
        for _ in 0..1_000 {
            std::hint::black_box(router.route(WORKLOAD[0], false, &replicas, now));
            router.end_transaction();
        }
    });
    assert_eq!(
        after_write, 0,
        "routing behind a write watermark allocated {after_write} times"
    );

    // --- replica observation -----------------------------------------------
    //
    // Budget: zero. The poller writes one of these per replica per interval,
    // and it shares the structure the route decision reads.
    let observing = allocations(|| {
        for _ in 0..1_000 {
            replicas.observe(0, Lsn::new(0x1600_2000), true, now);
        }
    });
    assert_eq!(
        observing, 0,
        "observing a replica allocated {observing} times"
    );
}
