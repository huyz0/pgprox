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

    // And the one decoder that reads past the two names into counted,
    // length-prefixed data the client controls. It is reached only from the
    // cache path, so `frontend::decode` above does not cover it, and a length
    // a decoder trusts is how a nine-byte message becomes an allocation.
    if let Ok(params) = frontend::bind_parameters(&frame) {
        // The run a cache key is built from, which is a subslice measured
        // against what the reader had left rather than one taken on trust.
        assert!(params.raw().len() <= frame.body().len());
    }

    // The startup packet is the first thing an unauthenticated peer sends, so
    // it is the most exposed parser in the process.
    let _ = startup::decode(data);
});
