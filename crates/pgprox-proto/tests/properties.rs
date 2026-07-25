//! Property tests over the decoders.
//!
//! Weaker than coverage-guided fuzzing, and it runs on stable and in tier 1,
//! which the libFuzzer targets in `fuzz/` do not. See `fuzz/README.md`.

// A test target is a separate crate, so the workspace lints that ban these in
// production code apply here too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;

use pgprox_proto::frame::{DEFAULT_MAX_FRAME, Decoded, Frame, Tag, decode, decode_untagged};
use pgprox_proto::{backend, frontend, startup};

/// Builds a well-formed tagged frame, so the generator spends its effort on
/// bodies rather than on length prefixes that never parse.
fn well_formed_frame() -> impl Strategy<Value = Vec<u8>> {
    (any::<u8>(), prop::collection::vec(any::<u8>(), 0..512)).prop_map(|(tag, body)| {
        let mut out = vec![tag];
        out.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_be_bytes());
        out.extend_from_slice(&body);
        out
    })
}

proptest! {
    #[test]
    fn decoding_arbitrary_bytes_never_panics(data in prop::collection::vec(any::<u8>(), 0..2048)) {
        let _ = decode(&data, DEFAULT_MAX_FRAME);
        let _ = decode_untagged(&data, DEFAULT_MAX_FRAME);
        let _ = startup::decode(&data);
    }

    #[test]
    fn a_decoded_frame_never_consumes_past_the_buffer(
        data in prop::collection::vec(any::<u8>(), 0..2048)
    ) {
        if let Ok(Decoded::Frame(frame, consumed)) = decode(&data, DEFAULT_MAX_FRAME) {
            // A wrong consumed length desynchronises every later message on the
            // connection, which is worse than failing outright.
            prop_assert!(consumed <= data.len());
            prop_assert_eq!(consumed, frame.wire_len());
        }
    }

    #[test]
    fn a_well_formed_frame_always_decodes(bytes in well_formed_frame()) {
        let decoded = decode(&bytes, DEFAULT_MAX_FRAME).unwrap();
        match decoded {
            Decoded::Frame(frame, consumed) => {
                prop_assert_eq!(consumed, bytes.len());
                prop_assert_eq!(frame.tag().get(), bytes[0]);
            }
            Decoded::Incomplete { .. } => prop_assert!(false, "well-formed frame was incomplete"),
        }
    }

    #[test]
    fn message_decoding_never_panics(tag in any::<u8>(), body in prop::collection::vec(any::<u8>(), 0..512)) {
        // Every tag against every body: several tags mean different things per
        // direction, and a peer is not obliged to send well-formed messages.
        let frame = Frame::new(Tag(tag), &body);
        let _ = backend::decode(&frame);
        let _ = frontend::decode(&frame);
    }

    #[test]
    fn splitting_a_frame_anywhere_never_yields_a_wrong_answer(
        bytes in well_formed_frame(),
        split in 0_usize..2048,
    ) {
        // Decoding must be a function of how many bytes arrived, never of how
        // TCP chunked them.
        let split = split.min(bytes.len());
        match decode(&bytes[..split], DEFAULT_MAX_FRAME).unwrap() {
            Decoded::Frame(..) => prop_assert_eq!(split, bytes.len(), "decoded early"),
            Decoded::Incomplete { .. } => prop_assert!(split < bytes.len()),
        }
    }
}
