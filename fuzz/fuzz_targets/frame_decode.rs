#![no_main]
//! Frame boundary scanning against arbitrary bytes.
//!
//! The property: decoding never panics, and a decoded frame never claims to
//! consume more than the buffer holds. Both directions of that matter, since a
//! wrong `consumed` would desynchronise the stream for every later message.

use libfuzzer_sys::fuzz_target;
use pgprox_proto::frame::{DEFAULT_MAX_FRAME, Decoded, decode, decode_untagged};

fuzz_target!(|data: &[u8]| {
    if let Ok(Decoded::Frame(frame, consumed)) = decode(data, DEFAULT_MAX_FRAME) {
        assert!(consumed <= data.len(), "consumed past the buffer");
        assert_eq!(consumed, frame.wire_len(), "consumed disagrees with wire_len");
    }
    if let Ok(Decoded::Frame(_, consumed)) = decode_untagged(data, DEFAULT_MAX_FRAME) {
        assert!(consumed <= data.len(), "consumed past the buffer");
    }
});
