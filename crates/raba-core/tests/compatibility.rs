//! The promise that an upgrade cannot stop an existing file from playing.
//!
//! `tests/fixtures/valid/v1-generic.raba` was written by the generator on
//! 13 Nov 2026 and is committed as bytes. Everything about it is fixed: the
//! magic, the version, the dimensions, the frame count, the op count. A change
//! to the decoder that would stop a user's theme from playing fails here, which
//! is the only way a format promise is worth anything.
//!
//! See `docs/compatibility.md` for the rules this checks one instance of.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use raba_core::decode;

/// The bytes, not a path: a test that reads a file somebody could regenerate is
/// a test that passes because the file moved with the code.
const V1: &[u8] = include_bytes!("../../../tests/fixtures/valid/v1-generic.raba");

#[test]
fn a_file_written_for_version_1_still_decodes() {
    let movie = decode(V1).expect("a v1 file this build can no longer read");

    assert_eq!(&V1[0..4], b"RABA", "the magic is part of the format, not an implementation detail");
    assert_eq!(V1[4], 1, "this fixture is version 1 and stays version 1");

    assert_eq!(movie.columns, 20);
    assert_eq!(movie.lines, 7);
    assert_eq!(movie.frames.len(), 4);
    assert_eq!(movie.ops.len(), 143);
    assert!(movie.looping);
}

#[test]
fn every_frame_of_it_still_plays() {
    let movie = decode(V1).expect("a v1 file this build can no longer read");

    // Not just "it parsed": each frame's ops have to still be addressable, which
    // is what a player does with them and what a format change would break.
    let cells = usize::from(movie.columns) * usize::from(movie.lines);
    for frame in &movie.frames {
        let start = usize::try_from(frame.op_start).expect("op_start fits");
        let count = usize::from(frame.op_count);
        let ops = movie.ops.get(start..start + count).expect("a frame's ops are in range");
        for op in ops {
            assert!(
                usize::try_from(op.cell).expect("a cell index fits") < cells,
                "an op points outside the canvas"
            );
        }
        assert!(frame.delay_ms > 0, "a frame with no delay never ends");
    }
}

#[test]
fn a_version_this_build_does_not_know_is_refused_and_not_guessed_at() {
    // The other half of the promise: a file from a *newer* RABA must be turned
    // down cleanly rather than half-read. The boot that meets one carries on
    // without a splash, which is the whole exit-0 posture.
    let mut future = V1.to_vec();
    future[4] = 2;
    let error = decode(&future).expect_err("a version 2 file was accepted by a version 1 reader");
    assert!(format!("{error}").contains("version"), "{error}");
}
