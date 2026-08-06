//! What one large result costs, measured rather than read.
//!
//! `M16.1`. `pgprox_proto::relay`'s module header says a relay built on
//! `decode` "must accumulate an entire body before forwarding a byte", and that
//! "a single large `DataRow` would then hold up to a gigabyte, and ADR 0008's
//! whole premise is that an idle connection costs roughly 200 bytes".
//!
//! The relay loop in `bin/pgprox` is built on `decode`, through
//! `Wire::read_tagged`. This measures the difference between the two on the
//! same bytes, so `M16.2`'s rewrite is justified by a number rather than by the
//! paragraph above.
//!
//! # Why the numbers are asserted and not only printed
//!
//! A measurement nobody checks stops being true without anyone noticing, which
//! is what `M10` was about. The assertions are loose on purpose: they say the
//! buffering path holds most of the message and the streaming path holds
//! almost none of it, which is the claim, and they do not pin an allocator's
//! exact behaviour.

// A measurement, so it prints the numbers it took as well as asserting them:
// the verdict says the difference is there and the numbers say how big, and
// `docs/internal/product/perf/` quotes them. The bench binary carries the same allow for the
// same reason. `expect` rather than `?` because a failure here is a broken
// fixture, and the message names which part.
#![allow(clippy::print_stdout, clippy::expect_used)]

use pgprox_core::buf::BufferSlab;
use pgprox_proto::frame::{DEFAULT_MAX_FRAME, Direction, LEN_PREFIX, Tag};
use pgprox_proto::relay::FrameRelay;
use pgprox_session::shell::Wire;
use tokio::io::AsyncWriteExt;

/// The result row under test: sixteen megabytes in one `DataRow`.
///
/// Chosen to be large enough that the difference is unarguable and small
/// enough to run in tier 1. `DEFAULT_MAX_FRAME` permits sixty-four times this,
/// so it is a fraction of what a client may legitimately ask for: a `SELECT` of
/// a 100 MB `bytea` is a real query that real Postgres answers.
const BODY: usize = 16 * 1024 * 1024;

fn data_row(body_len: usize) -> Vec<u8> {
    let mut out = vec![Tag::DATA_ROW.get()];
    out.extend_from_slice(
        &u32::try_from(body_len + LEN_PREFIX)
            .expect("the fixture fits in a length prefix")
            .to_be_bytes(),
    );
    out.extend_from_slice(&vec![b'x'; body_len]);
    out
}

#[tokio::test]
async fn one_large_row_through_the_buffering_path_and_the_streaming_one() {
    let bytes = data_row(BODY);

    // --- the path the proxy actually takes ---------------------------------
    //
    // `Wire::read_tagged` is what `bin/pgprox`'s pump calls for every server
    // frame. It returns when the whole message has arrived and hands back the
    // body, so the body is held in full before a byte of it is forwarded.
    let slab = BufferSlab::new(16 * 1024, 4);
    let (server, mut client) = tokio::io::duplex(64 * 1024);
    let mut wire = Wire::new(server, slab);

    let writer = tokio::spawn(async move {
        client
            .write_all(&bytes)
            .await
            .expect("the fixture is written");
        client
    });

    let mut body = Vec::new();
    let tag = wire
        .read_tagged(&mut body, DEFAULT_MAX_FRAME)
        .await
        .expect("the row decodes");
    let held_buffering = body.len();
    drop(writer.await.expect("the writer finishes"));

    assert_eq!(tag, Tag::DATA_ROW);
    assert_eq!(held_buffering, BODY);

    // --- the path pgprox-proto built for this -------------------------------
    //
    // Same bytes, same message, offered in the chunks a socket would deliver
    // them in. `buffered` is what the relay holds at that instant.
    let bytes = data_row(BODY);
    let mut relay = FrameRelay::new(Direction::Backend);
    let mut held_streaming = 0;
    let mut offset = 0;
    while offset < bytes.len() {
        let end = (offset + 16 * 1024).min(bytes.len());
        let mut window = &bytes[offset..end];
        while !window.is_empty() {
            let outcome = relay.push(window).expect("the row relays");
            held_streaming = held_streaming.max(relay.buffered());
            if outcome.consumed == 0 {
                break;
            }
            window = &window[outcome.consumed..];
        }
        offset = end;
    }

    // The claim, as a ratio rather than as a pair of numbers, so it survives a
    // change to the fixture size.
    assert!(
        held_streaming * 1000 < held_buffering,
        "streaming held {held_streaming} against {held_buffering} buffering, \
         which is not the difference this is about"
    );
    assert!(
        held_streaming <= 1 + LEN_PREFIX,
        "the streaming path held {held_streaming} bytes, more than a header"
    );

    // Printed so a run records the numbers rather than only the verdict. The
    // note in docs/internal/product/perf/ quotes these.
    println!(
        "M16.1: one {BODY}-byte DataRow. buffering held {held_buffering}, \
         streaming held {held_streaming}."
    );
}

#[tokio::test]
async fn the_pair_the_pump_now_uses_holds_a_chunk_rather_than_a_row() {
    // `M16.3`. The measurement above compares `read_tagged` against
    // `FrameRelay`, which is the gap. This one measures what the pump actually
    // does now: `read_header`, then `take_body` until the body is gone.
    //
    // `take_body` hands out a borrowed slice of the wire's own read buffer, so
    // the number to watch is the largest slice it ever returns. That is bounded
    // by the buffer the slab lends, not by the message.
    let bytes = data_row(BODY);

    let slab = BufferSlab::new(16 * 1024, 4);
    let (server, mut client) = tokio::io::duplex(64 * 1024);
    let mut wire = Wire::new(server, slab);

    let writer = tokio::spawn(async move {
        client
            .write_all(&bytes)
            .await
            .expect("the fixture is written");
        client
    });

    let header = wire
        .read_header(DEFAULT_MAX_FRAME)
        .await
        .expect("the header decodes");
    assert_eq!(header.tag, Tag::DATA_ROW);
    assert_eq!(header.body_len, BODY);

    let mut remaining = header.body_len;
    let mut largest = 0;
    let mut moved = 0;
    while remaining > 0 {
        let chunk = wire.take_body(remaining).await.expect("the body streams");
        assert!(
            !chunk.is_empty(),
            "take_body returned nothing and would spin"
        );
        largest = largest.max(chunk.len());
        moved += chunk.len();
        remaining -= chunk.len();
    }
    drop(writer.await.expect("the writer finishes"));

    assert_eq!(moved, BODY, "the body was not moved in full");
    assert!(
        largest < BODY / 100,
        "the largest piece held was {largest} of a {BODY}-byte row, which is \
         not streaming"
    );

    println!(
        "M16.3: the same {BODY}-byte DataRow through the pump's pair. \
         largest piece held {largest}."
    );
}
