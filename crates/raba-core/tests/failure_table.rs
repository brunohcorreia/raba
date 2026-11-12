//! The failure table.
//!
//! This is the project's central promise expressed as code: **no input makes the
//! decoder panic, hang, or allocate without bound.** `raba` runs as root, early in
//! boot, parsing a file nobody has supervised, and the worst thing it could do is
//! prevent a machine from starting.
//!
//! Every case here is a shape of malformed input and the specific error it must
//! produce. A new error variant without a row here is an untested claim.
//!
//! Reviewed more carefully than anything else in the project. See
//! `docs/development.md`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use raba_core::format::{crc32, limits, FRAME_SIZE, HEADER_SIZE, OP_SIZE};
use raba_core::{decode, encode, DecodeError, Frame, Movie, Op, Palette};

/// A small, valid movie: 4x2 canvas, two frames, three ops.
fn valid_movie() -> Movie {
    Movie {
        columns: 4,
        lines: 2,
        palette: Palette::Ansi16,
        looping: true,
        frames: vec![
            Frame { delay_ms: 100, op_start: 0, op_count: 2 },
            Frame { delay_ms: 0, op_start: 2, op_count: 1 },
        ],
        ops: vec![
            Op { cell: 0, ch: 'o', fg: 7, bg: 0 },
            Op { cell: 5, ch: '+', fg: 2, bg: 0 },
            Op { cell: 7, ch: '*', fg: 4, bg: 0 },
        ],
    }
}

fn valid_bytes() -> Vec<u8> {
    encode(&valid_movie())
}

/// Rewrite the CRC so a mutation is judged on its own merits rather than tripping
/// the checksum first. Without this, every corruption test would just assert
/// `CrcMismatch` and prove nothing about the validation below it.
fn refresh_crc(bytes: &mut [u8]) {
    let crc = crc32(&bytes[HEADER_SIZE..]);
    bytes[28..32].copy_from_slice(&crc.to_le_bytes());
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

// ---------------------------------------------------------------------------
// The baseline: the valid case must actually be valid, or every test below is
// vacuous.
// ---------------------------------------------------------------------------

#[test]
fn the_valid_case_decodes() {
    let movie = decode(&valid_bytes()).expect("the fixture must be valid");
    assert_eq!(movie, valid_movie());
}

// ---------------------------------------------------------------------------
// Truncation and size claims
// ---------------------------------------------------------------------------

#[test]
fn empty_input() {
    assert_eq!(decode(&[]), Err(DecodeError::TooShort { got: 0 }));
}

#[test]
fn shorter_than_the_header() {
    for len in 0..HEADER_SIZE {
        let bytes = valid_bytes();
        assert_eq!(
            decode(&bytes[..len]),
            Err(DecodeError::TooShort { got: len }),
            "length {len} must be rejected as too short"
        );
    }
}

#[test]
fn truncated_at_every_length() {
    // The decoder must reject every truncation, not merely survive it. This is the
    // case a fuzzer finds first and the one a half-written file produces in real
    // life — an interrupted package install, a full disk.
    let bytes = valid_bytes();
    for len in HEADER_SIZE..bytes.len() {
        let result = decode(&bytes[..len]);
        assert!(result.is_err(), "truncation to {len} bytes must be rejected");
    }
}

#[test]
fn declared_size_disagrees_with_reality() {
    let mut bytes = valid_bytes();
    let actual = bytes.len();
    put_u32(&mut bytes, 16, actual as u32 + 1);
    assert_eq!(
        decode(&bytes),
        Err(DecodeError::SizeMismatch { declared: actual as u32 + 1, actual })
    );
}

#[test]
fn trailing_bytes_are_rejected() {
    // Appending data without updating the header is a size mismatch, not something
    // to ignore. Silently accepting a suffix is how parsers grow smuggling bugs.
    let mut bytes = valid_bytes();
    let declared = bytes.len() as u32;
    bytes.extend_from_slice(b"extra");
    assert_eq!(
        decode(&bytes),
        Err(DecodeError::SizeMismatch { declared, actual: declared as usize + 5 })
    );
}

#[test]
fn absurd_declared_size_is_rejected_before_anything_else() {
    // The gzip-bomb case, in the shape this format allows it to take. The header
    // claims an enormous file; the check must fire on the claim itself, with no
    // allocation, rather than after reading or reserving anything.
    let mut bytes = valid_bytes();
    put_u32(&mut bytes, 16, u32::MAX);
    assert_eq!(decode(&bytes), Err(DecodeError::FileTooLarge { declared: u32::MAX }));

    put_u32(&mut bytes, 16, limits::MAX_FILE_SIZE + 1);
    assert_eq!(
        decode(&bytes),
        Err(DecodeError::FileTooLarge { declared: limits::MAX_FILE_SIZE + 1 })
    );
}

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

#[test]
fn wrong_magic() {
    let mut bytes = valid_bytes();
    bytes[0..4].copy_from_slice(b"RAB\0");
    assert_eq!(decode(&bytes), Err(DecodeError::BadMagic { got: *b"RAB\0" }));
}

#[test]
fn a_gzip_file_is_not_mistaken_for_ours() {
    // `.dur` is gzipped JSON. Someone will point RABA at one.
    let gzip = [
        0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0,
    ];
    assert_eq!(decode(&gzip), Err(DecodeError::BadMagic { got: [0x1f, 0x8b, 0x08, 0x00] }));
}

#[test]
fn unsupported_version() {
    for version in [0u16, 2, 99, u16::MAX] {
        let mut bytes = valid_bytes();
        put_u16(&mut bytes, 4, version);
        assert_eq!(
            decode(&bytes),
            Err(DecodeError::UnsupportedVersion { got: version }),
            "version {version} must be refused, not guessed at"
        );
    }
}

// ---------------------------------------------------------------------------
// Canvas and counts
// ---------------------------------------------------------------------------

#[test]
fn zero_sized_canvas() {
    let mut bytes = valid_bytes();
    put_u16(&mut bytes, 8, 0);
    refresh_crc(&mut bytes);
    assert_eq!(decode(&bytes), Err(DecodeError::BadCanvas { columns: 0, lines: 2 }));

    let mut bytes = valid_bytes();
    put_u16(&mut bytes, 10, 0);
    refresh_crc(&mut bytes);
    assert_eq!(decode(&bytes), Err(DecodeError::BadCanvas { columns: 4, lines: 0 }));
}

#[test]
fn enormous_canvas() {
    // The "canvas of 10^9 x 10^9" row. Expressed here as the largest the fields can
    // hold, which is the most an attacker can ask for.
    let mut bytes = valid_bytes();
    put_u16(&mut bytes, 8, u16::MAX);
    put_u16(&mut bytes, 10, u16::MAX);
    assert_eq!(decode(&bytes), Err(DecodeError::BadCanvas { columns: u16::MAX, lines: u16::MAX }));

    let mut bytes = valid_bytes();
    put_u16(&mut bytes, 8, limits::MAX_COLUMNS + 1);
    assert!(matches!(decode(&bytes), Err(DecodeError::BadCanvas { .. })));
}

#[test]
fn zero_frames() {
    let mut bytes = valid_bytes();
    put_u32(&mut bytes, 12, 0);
    assert_eq!(decode(&bytes), Err(DecodeError::NoFrames));
}

#[test]
fn too_many_frames() {
    let mut bytes = valid_bytes();
    put_u32(&mut bytes, 12, limits::MAX_FRAMES + 1);
    assert_eq!(decode(&bytes), Err(DecodeError::TooManyFrames { got: limits::MAX_FRAMES + 1 }));
}

#[test]
fn counts_that_do_not_add_up() {
    // Each count is individually plausible; together they cannot describe a file of
    // this length. Catching this before allocating is the whole point of deriving
    // sizes rather than trusting them.
    let mut bytes = valid_bytes();
    put_u32(&mut bytes, 12, 3); // claim three frames in a two-frame file
    let result = decode(&bytes);
    assert!(
        matches!(result, Err(DecodeError::LayoutMismatch { .. })),
        "expected LayoutMismatch, got {result:?}"
    );
}

#[test]
fn op_count_overflow_is_caught_arithmetically() {
    // u32::MAX ops times 12 bytes overflows a 32-bit size. The computation is done
    // in u64 precisely so this is a clean rejection rather than a wrap.
    let mut bytes = valid_bytes();
    put_u32(&mut bytes, 24, u32::MAX);
    let result = decode(&bytes);
    assert!(
        matches!(result, Err(DecodeError::LayoutMismatch { .. })),
        "expected LayoutMismatch, got {result:?}"
    );
}

#[test]
fn unknown_palette() {
    let mut bytes = valid_bytes();
    bytes[20] = 9;
    refresh_crc(&mut bytes);
    assert_eq!(decode(&bytes), Err(DecodeError::BadPalette { got: 9 }));
}

#[test]
fn reserved_header_bytes_must_be_zero() {
    for offset in 21..24 {
        let mut bytes = valid_bytes();
        bytes[offset] = 1;
        refresh_crc(&mut bytes);
        assert_eq!(
            decode(&bytes),
            Err(DecodeError::ReservedNotZero { offset }),
            "reserved byte {offset} must be rejected so it can be used later"
        );
    }
}

// ---------------------------------------------------------------------------
// Integrity
// ---------------------------------------------------------------------------

#[test]
fn corrupt_payload_is_caught_by_the_checksum() {
    let bytes = valid_bytes();
    for index in HEADER_SIZE..bytes.len() {
        let mut corrupted = bytes.clone();
        corrupted[index] ^= 0xFF;
        assert!(
            decode(&corrupted).is_err(),
            "a flipped byte at {index} must not decode successfully"
        );
    }
}

#[test]
fn crc_mismatch_is_reported_as_such() {
    let mut bytes = valid_bytes();
    put_u32(&mut bytes, 28, 0xDEAD_BEEF);
    assert!(matches!(decode(&bytes), Err(DecodeError::CrcMismatch { declared: 0xDEAD_BEEF, .. })));
}

// ---------------------------------------------------------------------------
// Internal references
//
// These are what let the renderer index without checking. If any of them can slip
// through, the guarantee the renderer relies on is false.
// ---------------------------------------------------------------------------

#[test]
fn frame_ops_past_the_end() {
    let mut bytes = valid_bytes();
    put_u16(&mut bytes, HEADER_SIZE + 2, 99); // first frame claims 99 ops
    refresh_crc(&mut bytes);
    assert_eq!(
        decode(&bytes),
        Err(DecodeError::FrameOpsOutOfRange { frame: 0, start: 0, count: 99 })
    );
}

#[test]
fn frame_op_start_past_the_end() {
    let mut bytes = valid_bytes();
    put_u32(&mut bytes, HEADER_SIZE + 4, u32::MAX);
    refresh_crc(&mut bytes);
    assert!(matches!(decode(&bytes), Err(DecodeError::FrameOpsOutOfRange { .. })));
}

#[test]
fn op_addressing_a_cell_outside_the_canvas() {
    let movie = valid_movie();
    let cells = u32::from(movie.columns) * u32::from(movie.lines);
    let op_table = HEADER_SIZE + movie.frames.len() * FRAME_SIZE;

    for (bad_cell, label) in [(cells, "one past the end"), (u32::MAX, "absurd")] {
        let mut bytes = valid_bytes();
        put_u32(&mut bytes, op_table, bad_cell);
        refresh_crc(&mut bytes);
        assert_eq!(
            decode(&bytes),
            Err(DecodeError::CellOutOfRange { op: 0, cell: bad_cell, cells }),
            "a cell index {label} must be rejected at load"
        );
    }
}

#[test]
fn op_with_an_invalid_codepoint() {
    let op_table = HEADER_SIZE + valid_movie().frames.len() * FRAME_SIZE;
    // A UTF-16 surrogate and a value beyond the Unicode range. Neither is a char,
    // and converting without checking would be undefined behaviour.
    for bad in [0xD800u32, 0x11_0000, u32::MAX] {
        let mut bytes = valid_bytes();
        put_u32(&mut bytes, op_table + 4, bad);
        refresh_crc(&mut bytes);
        assert_eq!(
            decode(&bytes),
            Err(DecodeError::BadCodepoint { op: 0, value: bad }),
            "codepoint {bad:#x} must be rejected"
        );
    }
}

#[test]
fn reserved_op_bytes_must_be_zero() {
    let op_table = HEADER_SIZE + valid_movie().frames.len() * FRAME_SIZE;
    let mut bytes = valid_bytes();
    put_u16(&mut bytes, op_table + 10, 1);
    refresh_crc(&mut bytes);
    assert_eq!(decode(&bytes), Err(DecodeError::ReservedNotZero { offset: op_table + 10 }));
}

// ---------------------------------------------------------------------------
// The blanket guarantee
// ---------------------------------------------------------------------------

#[test]
fn no_input_panics() {
    // Structured corruption above; indiscriminate corruption here. Deterministic,
    // so a failure is reproducible — fuzzing covers the unbounded case in CI.
    let base = valid_bytes();
    let mut seed = 0x12345678u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };

    for _ in 0..20_000 {
        let mut bytes = base.clone();
        let mutations = (next() % 8) + 1;
        for _ in 0..mutations {
            let index = (next() as usize) % bytes.len();
            bytes[index] = (next() % 256) as u8;
        }
        // Must not panic. Decoding successfully is fine — random mutation can
        // produce a valid file — but the result must be sound either way.
        if let Ok(movie) = decode(&bytes) {
            let cells = movie.cell_count();
            for op in &movie.ops {
                assert!(op.cell < cells, "a decoded movie must never address outside its canvas");
            }
            for frame in &movie.frames {
                let ops = movie.frame_ops(frame);
                assert_eq!(ops.len(), frame.op_count as usize);
            }
        }
    }
}

#[test]
fn arbitrary_short_inputs_do_not_panic() {
    for len in 0..64usize {
        for pattern in [0x00u8, 0xFF, 0x41] {
            let bytes = vec![pattern; len];
            let _ = decode(&bytes);
        }
    }
}

#[test]
fn a_header_claiming_a_huge_file_allocates_nothing() {
    // Belt and braces for the allocation guarantee: a 32-byte input whose header
    // describes a 16 MiB file must be rejected on the size claim alone.
    let mut bytes = vec![0u8; HEADER_SIZE];
    bytes[0..4].copy_from_slice(b"RABA");
    put_u16(&mut bytes, 4, 1);
    put_u16(&mut bytes, 8, 100);
    put_u16(&mut bytes, 10, 50);
    put_u32(&mut bytes, 12, 60_000);
    put_u32(&mut bytes, 16, limits::MAX_FILE_SIZE);
    put_u32(&mut bytes, 24, 1_000_000);
    assert_eq!(
        decode(&bytes),
        Err(DecodeError::SizeMismatch { declared: limits::MAX_FILE_SIZE, actual: HEADER_SIZE })
    );
}

// ---------------------------------------------------------------------------
// Round trip
// ---------------------------------------------------------------------------

#[test]
fn round_trip() {
    let movie = valid_movie();
    assert_eq!(decode(&encode(&movie)).unwrap(), movie);
}

#[test]
fn round_trip_single_frame_non_looping() {
    let movie = Movie {
        columns: 1,
        lines: 1,
        palette: Palette::Xterm256,
        looping: false,
        frames: vec![Frame { delay_ms: u16::MAX, op_start: 0, op_count: 1 }],
        ops: vec![Op { cell: 0, ch: '█', fg: 255, bg: 0 }],
    };
    assert_eq!(decode(&encode(&movie)).unwrap(), movie);
}

#[test]
fn round_trip_frame_with_no_ops() {
    // A frame identical to its predecessor has no ops. This is the normal case for
    // a held frame, not an edge case.
    let movie = Movie {
        columns: 2,
        lines: 1,
        palette: Palette::Ansi16,
        looping: true,
        frames: vec![
            Frame { delay_ms: 50, op_start: 0, op_count: 1 },
            Frame { delay_ms: 50, op_start: 1, op_count: 0 },
        ],
        ops: vec![Op { cell: 0, ch: 'x', fg: 1, bg: 0 }],
    };
    let decoded = decode(&encode(&movie)).unwrap();
    assert_eq!(decoded, movie);
    assert!(decoded.frame_ops(&decoded.frames[1]).is_empty());
}

#[test]
fn encodes_sizes_the_decoder_agrees_with() {
    let movie = valid_movie();
    let bytes = encode(&movie);
    let expected = HEADER_SIZE + movie.frames.len() * FRAME_SIZE + movie.ops.len() * OP_SIZE;
    assert_eq!(bytes.len(), expected);
    assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize, expected);
}
