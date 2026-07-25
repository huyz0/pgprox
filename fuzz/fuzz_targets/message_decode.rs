#![no_main]
//! Message body decoding against arbitrary bytes, in both directions.
//!
//! Every tag is tried against every body, because the proxy cannot assume a
//! peer sends well-formed messages, and several tags mean different things in
//! each direction.

use libfuzzer_sys::fuzz_target;
use pgprox_proto::frame::{Frame, Tag};
use pgprox_proto::{backend, frontend, startup};

fuzz_target!(|data: &[u8]| {
    let Some((tag, body)) = data.split_first() else {
        return;
    };
    let frame = Frame::new(Tag(*tag), body);

    // Neither may panic, whatever the tag and body combination.
    let _ = backend::decode(&frame);
    let _ = frontend::decode(&frame);

    // The startup packet is the first thing an unauthenticated peer sends, so
    // it is the most exposed parser in the process.
    let _ = startup::decode(data);
});
