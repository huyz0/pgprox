//! Instruction count for a read that continues into the held buffer.
//!
//! See `crates/pgprox-proto/benches/hot_paths.rs` for why this is a plain
//! binary rather than a benchmark harness.
//!
//! # Why this crate has one at all
//!
//! `Wire::fill_held` is every read a connection makes after its first, and it
//! grew its buffer with `resize(.., 0)` for twenty-nine milestones. That is a
//! 16 KiB memset per read and nothing here could see it: the declared hot paths
//! with benchmarks were the four sans-I/O crates, and this one reads a socket.
//!
//! # Why a socket does not make it unmeasurable
//!
//! The shell is generic over `AsyncRead + AsyncWrite + Unpin`, which is what
//! lets the tests run a whole session over `tokio::io::duplex`. The same trick
//! works here, and a duplex read is deterministic, so the N and 2N subtraction
//! cancels the runtime along with everything else.
//!
//! It measures more than the memset: a runtime poll, a duplex read, and a frame
//! decode are all in the number. That is the point of measuring in place rather
//! than measuring a `Vec` on its own.

#![allow(missing_docs, clippy::unwrap_used)]

use pgprox_core::buf::BufferSlab;
use pgprox_proto::frame::{DEFAULT_MAX_FRAME, Tag};
use pgprox_session::shell::Wire;
use tokio::io::AsyncWriteExt;

/// A frame longer than the wire's first read, so the rest arrives through the
/// held buffer.
///
/// 4 KiB, which is a `DataRow` carrying a text column of the size an
/// application actually selects. Anything under the 512-byte first read never
/// reaches the path this measures.
const BODY: usize = 4 * 1024;

/// One `DataRow` whose body is `BODY` bytes.
fn frame() -> Vec<u8> {
    let mut body = 1_i16.to_be_bytes().to_vec();
    body.extend_from_slice(&i32::try_from(BODY).unwrap().to_be_bytes());
    body.extend_from_slice(&vec![b'x'; BODY]);

    let mut out = vec![Tag::DATA_ROW.get()];
    out.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_be_bytes());
    out.extend_from_slice(&body);
    out
}

/// Reads one oversized frame, repeatedly, over a duplex pipe.
///
/// Each iteration writes the frame and reads it back, so the wire borrows a
/// buffer, fills it once from the stack chunk and once through `fill_held`, and
/// returns the buffer when the frame consumed everything. That is the whole
/// cycle a relaying connection repeats.
fn held_read(iterations: u64) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();

    runtime.block_on(async {
        let one = frame();
        // Room for a whole frame, so the write never waits for the read and the
        // iteration stays one unit of work rather than two halves of a rendezvous.
        let (server, mut client) = tokio::io::duplex(64 * 1024);
        let slab = BufferSlab::new(16 * 1024, 4);
        let mut wire = Wire::new(server, slab);
        let mut body = Vec::with_capacity(BODY + 8);

        // Warm: the slab allocates its first buffer here rather than inside the
        // measurement, and `body` grows to its final size.
        client.write_all(&one).await.unwrap();
        wire.read_tagged(&mut body, DEFAULT_MAX_FRAME)
            .await
            .unwrap();

        for _ in 0..iterations {
            client.write_all(std::hint::black_box(&one)).await.unwrap();
            std::hint::black_box(
                wire.read_tagged(&mut body, DEFAULT_MAX_FRAME)
                    .await
                    .unwrap(),
            );
        }
    });
}

fn main() {
    let mut args = std::env::args().skip(1);
    let name = args.next().unwrap_or_default();
    let iterations: u64 = args.next().unwrap_or_default().parse().unwrap_or(0);

    match name.as_str() {
        "held_read" => held_read(iterations),
        _ => print_names(),
    }
}

fn print_names() {
    #[allow(clippy::print_stdout)]
    {
        println!("held_read");
    }
}
