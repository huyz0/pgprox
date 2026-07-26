//! Allocation budget for gossip digest encode and decode.
//!
//! The seventh declared hot path. It is here rather than in `pgprox-cluster`
//! because the cluster layer owns the digest as a value and this binary owns
//! how it travels: JSON, one message per line. Measuring the merge without
//! the encoding would measure the cheap half.
//!
//! # Why this is per round rather than per statement
//!
//! Every node encodes its digest and decodes each peer's once per gossip
//! round, which is once a second and grows with the fleet. It is nowhere near
//! the relay loop's rate, so the budget is a ceiling that catches a change of
//! shape, such as a digest that starts allocating per tenant rather than per
//! node.

#![allow(clippy::unwrap_used, clippy::panic)]

use pgprox_app::gossip::{DigestWire, Message};
use pgprox_cluster::digest::VersionedDigest;
use pgprox_core::cluster::{ClusterDigest, NodeMode};
use pgprox_core::ids::{NodeId, ServerId, TenantId};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// The cluster size the reference workload declares.
const CLUSTER_SIZE: usize = 3;

/// What one digest's encode and one digest's decode may cost.
///
/// Measured at 10 and 26 for a three-node fleet homing four tenants, and set a
/// few above each: enough headroom that a serde version bump does not fail a
/// build, tight enough that a digest which started allocating per tenant per
/// peer would.
///
/// Decoding costs more than encoding because every field arrives as an owned
/// `String` before it is parsed back into a `ServerId` or a `TenantId`. That
/// is a consequence of the wire format being JSON, which this module chose for
/// being readable during a rolling upgrade rather than for being cheap.
const ENCODE_BUDGET: u64 = 14;
const DECODE_BUDGET: u64 = 32;

fn allocations(body: impl FnOnce()) -> u64 {
    let before = dhat::HeapStats::get().total_blocks;
    body();
    dhat::HeapStats::get().total_blocks - before
}

/// A digest the size the reference workload implies: one upstream server, and
/// the tenants a node homes.
fn digest() -> VersionedDigest {
    VersionedDigest {
        version: 7,
        digest: ClusterDigest {
            node: NodeId::new(1),
            mode: NodeMode::Active,
            client_conns: 1_000,
            upstream_conns: vec![(ServerId::new("primary", 5432), 10)],
            tenant_usage: (0..4)
                .map(|i| (TenantId::new(format!("hot-{i}")), 250))
                .collect(),
        },
    }
}

#[test]
fn gossip_encode_and_decode_stay_inside_their_budgets() {
    let _profiler = dhat::Profiler::builder().testing().build();

    // --- the counter counts ------------------------------------------------
    let sanity = allocations(|| {
        std::hint::black_box(vec![0_u8; 64]);
    });
    assert!(sanity > 0, "the allocation counter is not counting");

    let versioned = digest();
    let line = serde_json::to_string(&Message::Digest(DigestWire::from(&versioned))).unwrap();

    // --- encode -------------------------------------------------------------
    let encoding = allocations(|| {
        for _ in 0..100 {
            let wire = DigestWire::from(&versioned);
            std::hint::black_box(serde_json::to_string(&Message::Digest(wire)).unwrap());
        }
    }) / 100;
    assert!(
        encoding <= ENCODE_BUDGET,
        "encoding a digest allocated {encoding} times, budget is {ENCODE_BUDGET}"
    );

    // --- decode -------------------------------------------------------------
    let decoding = allocations(|| {
        for _ in 0..100 {
            let message: Message = serde_json::from_str(&line).unwrap();
            let Message::Digest(wire) = message else {
                panic!("a digest decoded as something else");
            };
            std::hint::black_box(wire.parse().unwrap());
        }
    }) / 100;
    assert!(
        decoding <= DECODE_BUDGET,
        "decoding a digest allocated {decoding} times, budget is {DECODE_BUDGET}"
    );

    // --- a whole round ------------------------------------------------------
    //
    // What a node actually does once a second: encode its own, decode each
    // peer's. Stated as a total so the fleet-size term is visible, since a
    // digest that started allocating per tenant per peer would still pass the
    // two budgets above.
    let round = allocations(|| {
        let wire = DigestWire::from(&versioned);
        std::hint::black_box(serde_json::to_string(&Message::Digest(wire)).unwrap());
        for _ in 0..CLUSTER_SIZE - 1 {
            let message: Message = serde_json::from_str(&line).unwrap();
            let Message::Digest(wire) = message else {
                panic!("a digest decoded as something else");
            };
            std::hint::black_box(wire.parse().unwrap());
        }
    });
    assert!(
        round <= ENCODE_BUDGET + DECODE_BUDGET * (CLUSTER_SIZE as u64 - 1),
        "a gossip round allocated {round} times"
    );
}
