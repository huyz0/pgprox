//! Allocation budgets for the two hot paths this crate owns.
//!
//! `docs/internal/standards/testing.md` names seven hot paths and says two of them were
//! written to be allocation-free and never measured. This file measures the
//! two that live here, and gates them on counts rather than on wall clock:
//! a timing on a shared runner is noise, and an allocation count is the same
//! number on every machine.
//!
//! # Why one test function
//!
//! One function, several sections, each asserting its own delta. It was once
//! required, when the counter was process-wide and separate `#[test]`
//! functions on separate threads would have measured each other; `M64.0` made
//! the counter thread-local and left the shape, because the sections share the
//! fixtures above and reading them in order is how the budgets are understood.
//!
//! # Why the paths are warmed first
//!
//! The budget is for the steady state. A relay's first frame grows its header
//! buffer once and then never again, and asserting zero on the first call
//! would be asserting something the design does not claim.

#![allow(clippy::unwrap_used, clippy::panic)]

use pgprox_proto::backend;
use pgprox_proto::frame::{DEFAULT_MAX_FRAME, Decoded, Direction, Frame, Tag, decode};
use pgprox_proto::relay::FrameRelay;

/// One `DataRow` carrying a single text column, which is what a point select
/// returns and therefore the most common frame on the wire.
fn data_row() -> Vec<u8> {
    let mut body = 1_i16.to_be_bytes().to_vec();
    let value = b"1000";
    body.extend_from_slice(&i32::try_from(value.len()).unwrap().to_be_bytes());
    body.extend_from_slice(value);

    let mut frame = vec![Tag::DATA_ROW.get()];
    frame.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_be_bytes());
    frame.extend_from_slice(&body);
    frame
}

fn ready_for_query() -> Vec<u8> {
    let mut frame = vec![Tag::READY_FOR_QUERY.get()];
    frame.extend_from_slice(&5_u32.to_be_bytes());
    frame.push(b'I');
    frame
}

/// How many allocations `body` performs.
fn allocations(body: impl FnOnce()) -> u64 {
    allocation_counter::measure(body).count_total
}

#[test]
fn the_hot_paths_this_crate_owns_stay_inside_their_budgets() {
    let row = data_row();
    let ready = ready_for_query();

    // --- the counter counts ------------------------------------------------
    //
    // A budget test that measured nothing would pass forever. This is the
    // check that the measurement itself works, and it is first so a broken
    // harness fails before any budget does.
    let sanity = allocations(|| {
        std::hint::black_box(vec![0_u8; 64]);
    });
    assert!(sanity > 0, "the allocation counter is not counting");

    // --- frame boundary scanning -------------------------------------------
    //
    // Budget: zero. The whole point of `decode` returning a borrowed `Frame`
    // is that finding a message boundary copies nothing, and a proxy doing
    // this per frame at a million frames a second cannot afford otherwise.
    let scanning = allocations(|| {
        for _ in 0..1_000 {
            match decode(&row, DEFAULT_MAX_FRAME).unwrap() {
                Decoded::Frame(frame, consumed) => {
                    std::hint::black_box((frame.tag(), frame.body().len(), consumed));
                }
                Decoded::Incomplete { .. } => panic!("a complete frame decoded as incomplete"),
            }
        }
    });
    assert_eq!(scanning, 0, "frame scanning allocated {scanning} times");

    // --- decoding a backend message ----------------------------------------
    //
    // Budget: zero. Every field is borrowed from the frame, which is what lets
    // the relay loop look at a message and forward the original bytes.
    let decoding = allocations(|| {
        for _ in 0..1_000 {
            let frame = Frame::new(Tag::DATA_ROW, &row[5..]);
            std::hint::black_box(backend::decode(&frame).unwrap());

            let frame = Frame::new(Tag::READY_FOR_QUERY, &ready[5..]);
            std::hint::black_box(backend::decode(&frame).unwrap());
        }
    });
    assert_eq!(decoding, 0, "backend decoding allocated {decoding} times");

    // --- the steady-state relay step ---------------------------------------
    //
    // Budget: zero, once warm. The relay grows its header buffer and its
    // inspect buffer once each and reuses both, so a frame after the first
    // costs nothing. This is the path every byte of every result set takes.
    let mut relay = FrameRelay::new(Direction::Backend);
    for _ in 0..8 {
        let mut offset = 0;
        while offset < row.len() {
            offset += relay.push(&row[offset..]).unwrap().consumed;
        }
    }

    let relaying = allocations(|| {
        for _ in 0..1_000 {
            let mut offset = 0;
            while offset < row.len() {
                offset += relay.push(&row[offset..]).unwrap().consumed;
            }
        }
    });
    assert_eq!(relaying, 0, "the relay step allocated {relaying} times");
}
