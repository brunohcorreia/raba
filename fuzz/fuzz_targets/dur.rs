//! Fuzz the `.dur` importer.
//!
//! Lower stakes than the `.raba` decoder — this runs in `raba-gen` at build time,
//! never as root and never in an initramfs — but it is the one parser here fed
//! files from elsewhere, and it reaches a gzip decoder and a JSON parser before
//! any of our own validation. A crash is a build tool that dies on somebody's
//! art; a hang is worse, because gzip and JSON can both be made to expand.
//!
//! The target asserts the same guarantee the `.raba` fuzzer does: whatever comes
//! out is internally consistent, so that `encode` cannot be handed a movie the
//! decoder would reject.

#![no_main]

use libfuzzer_sys::fuzz_target;
use raba_core::{decode, encode};

fuzz_target!(|data: &[u8]| {
    let Ok(imported) = raba_gen::dur::parse(data) else { return };
    let movie = imported.movie;

    let cells = u32::from(movie.columns).saturating_mul(u32::from(movie.lines));
    assert!(cells > 0, "a movie with no cells was accepted");

    for op in &movie.ops {
        assert!(op.cell < cells, "op addresses cell {} of {cells}", op.cell);
        assert!(
            op.fg <= movie.palette.max_index() && op.bg <= movie.palette.max_index(),
            "colour outside the palette"
        );
        assert!(op.bg <= 7, "background above 7; the console reads that as blink");
    }

    let mut covered = 0usize;
    for frame in &movie.frames {
        let start = frame.op_start as usize;
        let end = start.saturating_add(usize::from(frame.op_count));
        assert!(end <= movie.ops.len(), "frame runs past the op table");
        covered = covered.max(end);
    }
    assert_eq!(covered, movie.ops.len(), "ops nothing points at");

    // The round trip is the real contract: anything the importer produces must
    // survive the format it exists to produce.
    let bytes = encode(&movie);
    assert_eq!(decode(&bytes), Ok(movie), "an imported movie did not round-trip");
});
