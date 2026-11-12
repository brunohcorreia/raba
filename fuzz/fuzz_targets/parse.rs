//! Fuzz the `.raba` decoder.
//!
//! The highest-value fuzz target in the project: this code parses a file as root,
//! early in boot, with no supervision, and the worst outcome is a machine that will
//! not start. See `docs/security.md`.
//!
//! The target asserts more than "does not panic". It checks the guarantee the
//! renderer depends on — that anything which decodes successfully is internally
//! consistent — because a parser that accepts a bad file quietly is worse than one
//! that crashes loudly.

#![no_main]

use libfuzzer_sys::fuzz_target;
use raba_core::{decode, encode};

fuzz_target!(|data: &[u8]| {
    let Ok(movie) = decode(data) else { return };

    // Every op addresses a cell inside the canvas. The renderer indexes with these
    // without checking again, so this must hold for every accepted input.
    let cells = movie.cell_count();
    for op in &movie.ops {
        assert!(op.cell < cells, "decoded movie addresses cell {} of {cells}", op.cell);
    }

    // Every frame's op range is inside the op array.
    for frame in &movie.frames {
        assert_eq!(
            movie.frame_ops(frame).len(),
            frame.op_count as usize,
            "frame op range escaped the op array"
        );
    }

    // Re-encoding a decoded movie must produce something that decodes to the same
    // movie. This catches a decoder that is permissive in a way the encoder is not.
    let reencoded = encode(&movie);
    match decode(&reencoded) {
        Ok(again) => assert_eq!(again, movie, "round trip changed the movie"),
        Err(e) => panic!("re-encoded movie failed to decode: {e}"),
    }
});
