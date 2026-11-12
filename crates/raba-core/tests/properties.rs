//! Property tests for the format.
//!
//! The failure table covers specific shapes of malformed input. This covers the
//! universal claims — the ones that should hold for *every* input, which is exactly
//! the kind of statement examples are bad at testing.
//!
//! `proptest` is a dev-dependency and does not ship. See `DEPENDENCIES.md`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use proptest::prelude::*;
use raba_core::format::{limits, HEADER_SIZE};
use raba_core::{decode, encode, Frame, Movie, Op, Palette};

/// Generate a well-formed movie: ops inside the canvas, frame ranges inside `ops`.
fn arb_movie() -> impl Strategy<Value = Movie> {
    (1u16..40, 1u16..20, any::<bool>(), prop::bool::ANY).prop_flat_map(
        |(columns, lines, looping, wide_palette)| {
            let cells = u32::from(columns) * u32::from(lines);
            let palette = if wide_palette { Palette::Xterm256 } else { Palette::Ansi16 };
            // Colours stay inside the declared palette: a 16-colour movie with
            // index 200 is not a well-formed movie, and the decoder says so.
            let top = palette.max_index();
            let ops = prop::collection::vec(
                (0..cells, any::<char>(), 0..=top, 0..=top).prop_map(|(cell, ch, fg, bg)| Op {
                    cell,
                    ch,
                    fg,
                    bg,
                }),
                0..60,
            );
            ops.prop_flat_map(move |ops| {
                let op_count = ops.len() as u32;
                // Partition the ops into consecutive frames, which is the shape the
                // encoder produces and the only shape the decoder accepts.
                prop::collection::vec((any::<u16>(), 0u32..=op_count), 1..8).prop_map(move |cuts| {
                    let mut boundaries: Vec<u32> = cuts.iter().map(|&(_, c)| c).collect();
                    boundaries.sort_unstable();
                    let mut frames = Vec::new();
                    let mut start = 0u32;
                    for (index, &end) in boundaries.iter().enumerate() {
                        let end = end.max(start);
                        frames.push(Frame {
                            delay_ms: cuts[index].0,
                            op_start: start,
                            op_count: (end - start) as u16,
                        });
                        start = end;
                    }
                    if start < op_count {
                        frames.push(Frame {
                            delay_ms: 0,
                            op_start: start,
                            op_count: (op_count - start) as u16,
                        });
                    }
                    Movie { columns, lines, palette, looping, frames, ops: ops.clone() }
                })
            })
        },
    )
}

proptest! {
    /// The central property of the format: encoding and decoding are inverses.
    #[test]
    fn encode_decode_round_trip(movie in arb_movie()) {
        let bytes = encode(&movie);
        let decoded = decode(&bytes).expect("an encoded movie must decode");
        prop_assert_eq!(decoded, movie);
    }

    /// Encoding is deterministic. Without this, golden tests and reproducible
    /// builds are both unreliable.
    #[test]
    fn encoding_is_deterministic(movie in arb_movie()) {
        prop_assert_eq!(encode(&movie), encode(&movie));
    }

    /// The size in the header is the real size. This is the claim every downstream
    /// bound is derived from, so it had better hold for everything we produce.
    #[test]
    fn declared_size_is_always_the_real_size(movie in arb_movie()) {
        let bytes = encode(&movie);
        let declared = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
        prop_assert_eq!(declared as usize, bytes.len());
        prop_assert!(declared <= limits::MAX_FILE_SIZE);
    }

    /// What the renderer relies on: a movie that decoded successfully can be
    /// indexed without further checking. Every op is inside the canvas and every
    /// frame's op range is inside the op array.
    #[test]
    fn decoded_movies_are_internally_consistent(movie in arb_movie()) {
        let decoded = decode(&encode(&movie)).expect("must decode");
        let cells = decoded.cell_count();
        for op in &decoded.ops {
            prop_assert!(op.cell < cells);
        }
        for frame in &decoded.frames {
            prop_assert_eq!(decoded.frame_ops(frame).len(), frame.op_count as usize);
        }
    }

    /// Truncation is always rejected, never partially accepted. A half-written file
    /// must not produce half an animation.
    #[test]
    fn any_truncation_is_rejected(movie in arb_movie(), cut in 0.0f64..1.0) {
        let bytes = encode(&movie);
        let len = ((bytes.len() as f64) * cut) as usize;
        prop_assume!(len < bytes.len());
        prop_assert!(decode(&bytes[..len]).is_err());
    }

    /// Arbitrary bytes never panic. The failure table proves this for mutations of
    /// a valid file; this proves it for input with no relationship to one.
    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        let _ = decode(&bytes);
    }

    /// The same, for input that gets past the magic and version checks — where the
    /// interesting validation lives and where a fuzzer would spend its time.
    #[test]
    fn plausible_headers_never_panic(tail in prop::collection::vec(any::<u8>(), 0..512)) {
        let mut bytes = Vec::with_capacity(HEADER_SIZE + tail.len());
        bytes.extend_from_slice(b"RABA");
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&tail);
        bytes.resize(bytes.len().max(HEADER_SIZE), 0);
        let _ = decode(&bytes);
    }
}
