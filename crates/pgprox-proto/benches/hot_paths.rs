//! Instruction counts for the hot paths this crate owns.
//!
//! # Why this is a plain binary and not a benchmark harness
//!
//! `scripts/bench.sh` runs it twice under `callgrind`, at N iterations and at
//! 2N, and takes the difference. That cancels process startup, the loader, and
//! everything else that is not the loop, exactly, without a harness having to
//! model it. It also costs no dependency: the two harnesses that do this for a
//! living both pull crates this project's supply-chain gate refuses, and a
//! measurement tool is not worth an exception to that.
//!
//! Counts rather than wall clock, because `callgrind` returns the same number
//! on a busy machine as on an idle one.

#![allow(missing_docs, clippy::unwrap_used)]

use pgprox_proto::backend;
use pgprox_proto::frame::{DEFAULT_MAX_FRAME, Decoded, Direction, Frame, Tag, decode};
use pgprox_proto::relay::FrameRelay;

/// One `DataRow` with a single text column: what a point select returns, and
/// so the most common frame on the wire.
fn data_row() -> Vec<u8> {
    let mut body = 1_i16.to_be_bytes().to_vec();
    body.extend_from_slice(&4_i32.to_be_bytes());
    body.extend_from_slice(b"1000");

    let mut frame = vec![Tag::DATA_ROW.get()];
    frame.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_be_bytes());
    frame.extend_from_slice(&body);
    frame
}

fn scan_frame(row: &[u8], iterations: u64) {
    for _ in 0..iterations {
        match decode(std::hint::black_box(row), DEFAULT_MAX_FRAME).unwrap() {
            Decoded::Frame(frame, consumed) => {
                std::hint::black_box((frame.tag(), consumed));
            }
            Decoded::Incomplete { .. } => unreachable!(),
        }
    }
}

fn decode_backend_message(row: &[u8], iterations: u64) {
    for _ in 0..iterations {
        let frame = Frame::new(Tag::DATA_ROW, std::hint::black_box(&row[5..]));
        std::hint::black_box(backend::decode(&frame).unwrap());
    }
}

fn relay_frame(row: &[u8], iterations: u64) {
    let mut relay = FrameRelay::new(Direction::Backend);
    // Warmed once. The relay grows its buffers on the first frame and never
    // again, and counting that would count a one-off as per-frame work.
    let mut offset = 0;
    while offset < row.len() {
        offset += relay.push(&row[offset..]).unwrap().consumed;
    }

    for _ in 0..iterations {
        let mut offset = 0;
        while offset < row.len() {
            offset += relay
                .push(std::hint::black_box(&row[offset..]))
                .unwrap()
                .consumed;
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let name = args.next().unwrap_or_default();
    let iterations: u64 = args.next().unwrap_or_default().parse().unwrap_or(0);

    let row = data_row();
    match name.as_str() {
        "scan_frame" => scan_frame(&row, iterations),
        "decode_backend_message" => decode_backend_message(&row, iterations),
        "relay_frame" => relay_frame(&row, iterations),
        // Nothing, which is what `scripts/bench.sh` uses to list them.
        _ => print_names(),
    }
}

fn print_names() {
    // The one place a bench binary is allowed to write to stdout: the script
    // asks it what it can run.
    #[allow(clippy::print_stdout)]
    {
        println!("scan_frame");
        println!("decode_backend_message");
        println!("relay_frame");
    }
}
