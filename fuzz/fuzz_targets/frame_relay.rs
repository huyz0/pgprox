#![no_main]
//! Streaming reassembly against arbitrary, arbitrarily-chunked bytes.
//!
//! `FrameRelay::push` is the one decoder in this crate that a caller drives
//! more than once per message: a socket read returns whatever the kernel
//! had, not a message boundary, so a header or a body routinely arrives
//! split across two or more `push` calls. `frame_decode` and
//! `message_decode` both hand a whole message to a decoder in one call and
//! never exercise that reassembly path. `M88.17` is this target: the
//! property is no panic, `consumed` never claiming more than the chunk it
//! was just given, and what the relay holds for inspection never exceeding
//! the cap it was built with, however the same bytes are chopped up before
//! they arrive.

use libfuzzer_sys::fuzz_target;
use pgprox_proto::frame::Direction;
use pgprox_proto::relay::FrameRelay;

fuzz_target!(|data: &[u8]| {
    let Some((&chunk_len, rest)) = data.split_first() else {
        return;
    };
    // The chunk size comes from the input rather than being fixed, so the
    // same corpus can discover both a single large read (a big chunk_len)
    // and a byte-at-a-time read (chunk_len == 1) without two targets.
    let chunk_len = usize::from(chunk_len).max(1);

    let Some((&direction_byte, rest)) = rest.split_first() else {
        return;
    };
    let direction = if direction_byte & 1 == 1 {
        Direction::Backend
    } else {
        Direction::Frontend
    };

    let mut relay = FrameRelay::new(direction);
    let max_inspect = relay.max_inspect();

    for chunk in rest.chunks(chunk_len) {
        let Ok(outcome) = relay.push(chunk) else {
            // A decode error ends the stream for a real caller too - nothing
            // further to check on this input.
            return;
        };
        assert!(
            outcome.consumed <= chunk.len(),
            "consumed past the chunk it was given"
        );
        assert!(
            relay.buffered() <= max_inspect,
            "held more for inspection than the cap this relay was built with"
        );
    }
});
