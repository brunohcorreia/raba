//! Importing Durdraw `.dur`.
//!
//! The fixtures are built here rather than committed as binary blobs, so that what
//! each test claims about the format is visible in the test.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic
)]

use raba_core::{apply, Grid, Movie, Palette};
use raba_gen::dur::{self, DurError};
use std::io::Write as _;

/// Build a `.dur` document. `color_map` is passed through exactly as given, so a
/// test can state the orientation it means.
fn document(
    version: u32,
    colour_format: &str,
    columns: usize,
    lines: usize,
    frames: &[(&[&str], serde_json::Value, f64)],
) -> String {
    let frames: Vec<serde_json::Value> = frames
        .iter()
        .enumerate()
        .map(|(index, (contents, colour_map, delay))| {
            serde_json::json!({
                "frameNumber": index + 1,
                "delay": delay,
                "contents": contents,
                "colorMap": colour_map,
            })
        })
        .collect();
    serde_json::json!({
        "DurMovie": {
            "formatVersion": version,
            "colorFormat": colour_format,
            "encoding": "utf-8",
            "name": "test",
            "artist": "tests",
            "framerate": 10.0,
            "columns": columns,
            "lines": lines,
            "frames": frames,
        }
    })
    .to_string()
}

fn gzip(text: &str) -> Vec<u8> {
    let mut encoder =
        flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(text.as_bytes()).expect("write");
    encoder.finish().expect("finish")
}

/// `[column][line]`, which is what version 7 stores.
fn transposed(columns: usize, lines: usize, pair: [i64; 2]) -> serde_json::Value {
    serde_json::json!(vec![vec![pair.to_vec(); lines]; columns])
}

/// `[line][column]`, which is what version 8 stores.
fn straight(columns: usize, lines: usize, pair: [i64; 2]) -> serde_json::Value {
    serde_json::json!(vec![vec![pair.to_vec(); columns]; lines])
}

fn frame_grid(movie: &Movie, index: usize) -> Grid {
    let mut grid = Grid::new(movie.columns, movie.lines, movie.palette).expect("grid");
    for step in 0..=index {
        apply(&mut grid, movie, &movie.frames[step]);
    }
    grid
}

#[test]
fn a_minimal_file_converts() {
    let text = document(8, "256", 2, 1, &[(&["ab"], straight(2, 1, [7, 0]), 0.0)]);
    let imported = dur::parse(gzip(&text).as_slice()).expect("convert");
    assert_eq!((imported.movie.columns, imported.movie.lines), (2, 1));
    assert_eq!(imported.movie.palette, Palette::Xterm256);
    assert_eq!(imported.name, "test");
    assert_eq!(imported.artist, "tests");
    let grid = frame_grid(&imported.movie, 0);
    assert_eq!(grid.get(0, 0).unwrap().ch, 'a');
    assert_eq!(grid.get(1, 0).unwrap().ch, 'b');
}

#[test]
fn plain_json_is_accepted_as_well_as_gzip() {
    // Durdraw always writes gzip, but a file that has been through a pipe or an
    // editor arrives plain, and refusing it would help nobody.
    let text = document(8, "256", 1, 1, &[(&["x"], straight(1, 1, [7, 0]), 0.0)]);
    let from_plain = dur::parse(text.as_bytes()).expect("plain");
    let from_gzip = dur::parse(gzip(&text).as_slice()).expect("gzip");
    assert_eq!(from_plain.movie, from_gzip.movie);
}

#[test]
fn a_version_7_colour_map_is_transposed_and_a_version_8_one_is_not() {
    // The trap docs/dur-support.md describes. A 3x1 canvas is not square, so the
    // two orientations have different shapes and a wrong guess cannot hide.
    let seven = document(7, "256", 3, 1, &[(&["abc"], transposed(3, 1, [40, 0]), 0.0)]);
    let eight = document(8, "256", 3, 1, &[(&["abc"], straight(3, 1, [40, 0]), 0.0)]);
    for text in [seven, eight] {
        let imported = dur::parse(gzip(&text).as_slice()).expect("convert");
        let grid = frame_grid(&imported.movie, 0);
        for column in 0..3 {
            assert_eq!(grid.get(column, 0).unwrap().fg, 40, "column {column}");
        }
    }
}

#[test]
fn the_same_artwork_as_v7_and_v8_encodes_byte_for_byte_identically() {
    // docs/milestones.md calls this M2's critical test, and it is the one that
    // actually proves the transposition is handled: two files that differ only in
    // how they store the colour map must become the same .raba, not merely two
    // that look alike.
    let contents: &[&str] = &["ab", "cd", "ef"];
    let pairs = [[40_i64, 0], [41, 0], [42, 0], [43, 0], [44, 0], [45, 0]];
    let at = |column: usize, line: usize| -> serde_json::Value {
        serde_json::json!(pairs[line * 2 + column].to_vec())
    };

    // v7 stores [column][line]; v8 stores [line][column]. Same picture either way.
    let seven_map = serde_json::json!((0..2)
        .map(|column| (0..3).map(|line| at(column, line)).collect::<Vec<_>>())
        .collect::<Vec<_>>());
    let eight_map = serde_json::json!((0..3)
        .map(|line| (0..2).map(|column| at(column, line)).collect::<Vec<_>>())
        .collect::<Vec<_>>());

    let seven = dur::parse(gzip(&document(7, "256", 2, 3, &[(contents, seven_map, 0.0)])).as_slice())
        .expect("v7");
    let eight = dur::parse(gzip(&document(8, "256", 2, 3, &[(contents, eight_map, 0.0)])).as_slice())
        .expect("v8");

    assert_eq!(
        raba_core::encode(&seven.movie),
        raba_core::encode(&eight.movie),
        "the same artwork stored two ways must encode identically"
    );
    // And the colours really are per-cell, so the test cannot pass by both being
    // uniformly wrong.
    let grid = frame_grid(&seven.movie, 0);
    assert_eq!(grid.get(0, 0).unwrap().fg, 40);
    assert_eq!(grid.get(1, 2).unwrap().fg, 45);
}

#[test]
fn the_shape_is_believed_over_the_declared_version() {
    // A real file prompted this: /etc/ly/blackhole.dur declares version 7 and uses
    // version 8's key names. Producers other than Durdraw do not keep to the
    // version's conventions, so where the shape is unambiguous it decides.
    let text = document(7, "256", 3, 1, &[(&["abc"], straight(3, 1, [40, 0]), 0.0)]);
    let imported = dur::parse(gzip(&text).as_slice()).expect("convert");
    assert_eq!(frame_grid(&imported.movie, 0).get(2, 0).unwrap().fg, 40);
    assert!(
        imported.notes.iter().any(|n| n.contains("formatVersion")),
        "a disagreement with the version should be reported: {:?}",
        imported.notes
    );
}

#[test]
fn durdraw_indices_are_curses_order_not_ansi_order() {
    // Durdraw's 1 is blue and its 4 is red. Copying the numbers across unchanged
    // swaps red and blue in every imported file, which reads as bad art rather
    // than as a bug.
    let text = document(8, "256", 2, 1, &[(&["ab"], serde_json::json!([[[1, 0], [4, 0]]]), 0.0)]);
    let imported = dur::parse(gzip(&text).as_slice()).expect("convert");
    let grid = frame_grid(&imported.movie, 0);
    assert_eq!(grid.get(0, 0).unwrap().fg, 4, "durdraw 1 (blue) is ANSI 4");
    assert_eq!(grid.get(1, 0).unwrap().fg, 1, "durdraw 4 (red) is ANSI 1");
}

#[test]
fn the_256_colour_cube_is_left_alone() {
    let text = document(8, "256", 1, 1, &[(&["x"], serde_json::json!([[[149, 0]]]), 0.0)]);
    let imported = dur::parse(gzip(&text).as_slice()).expect("convert");
    assert_eq!(frame_grid(&imported.movie, 0).get(0, 0).unwrap().fg, 149);
}

#[test]
fn colour_format_16_counts_from_one() {
    // docs/dur-support.md: in colorFormat "16" the same colours are indexed from 1.
    let text = document(8, "16", 1, 1, &[(&["x"], serde_json::json!([[[5, 1]]]), 0.0)]);
    let imported = dur::parse(gzip(&text).as_slice()).expect("convert");
    assert_eq!(imported.movie.palette, Palette::Ansi16);
    let grid = frame_grid(&imported.movie, 0);
    let cell = grid.get(0, 0).unwrap();
    assert_eq!(cell.fg, 1, "durdraw 5 is index 4, which is curses red, which is ANSI 1");
    assert_eq!(cell.bg, 0, "durdraw 1 is index 0, black");
}

#[test]
fn a_background_above_seven_is_clamped() {
    // On a console the high background bit is blink, not brightness.
    let text = document(8, "256", 1, 1, &[(&["x"], serde_json::json!([[[7, 12]]]), 0.0)]);
    let imported = dur::parse(gzip(&text).as_slice()).expect("convert");
    assert!(frame_grid(&imported.movie, 0).get(0, 0).unwrap().bg <= 7);
}

#[test]
fn an_unchanged_frame_costs_nothing() {
    // The whole reason .raba is a delta format. If this regresses, every imported
    // animation grows by its canvas size per frame.
    let still = straight(2, 1, [7, 0]);
    let text = document(
        8,
        "256",
        2,
        1,
        &[(&["ab"], still.clone(), 0.0), (&["ab"], still.clone(), 0.0), (&["ax"], still, 0.0)],
    );
    let imported = dur::parse(gzip(&text).as_slice()).expect("convert");
    assert_eq!(imported.movie.frames[0].op_count, 2, "frame 0 is the whole canvas");
    assert_eq!(imported.movie.frames[1].op_count, 0, "nothing moved");
    assert_eq!(imported.movie.frames[2].op_count, 1, "one cell moved");
}

#[test]
fn the_framerate_becomes_a_delay_and_a_frame_can_override_it() {
    let text = document(
        8,
        "256",
        1,
        1,
        &[(&["a"], straight(1, 1, [7, 0]), 0.0), (&["b"], straight(1, 1, [7, 0]), 0.25)],
    );
    let imported = dur::parse(gzip(&text).as_slice()).expect("convert");
    assert_eq!(imported.movie.frames[0].delay_ms, 100, "10 fps is 100ms");
    assert_eq!(imported.movie.frames[1].delay_ms, 250, "a delay is in seconds");
}

#[test]
fn what_is_refused_is_refused_clearly() {
    let bad_version = document(6, "256", 1, 1, &[(&["a"], straight(1, 1, [7, 0]), 0.0)]);
    assert_eq!(
        dur::parse(gzip(&bad_version).as_slice()).unwrap_err(),
        DurError::UnsupportedVersion(6)
    );

    let bad_colour = document(8, "RGB", 1, 1, &[(&["a"], straight(1, 1, [7, 0]), 0.0)]);
    assert_eq!(
        dur::parse(gzip(&bad_colour).as_slice()).unwrap_err(),
        DurError::UnsupportedColorFormat("RGB".to_owned())
    );

    assert!(matches!(dur::parse(b"not json at all"), Err(DurError::Json(_))));

    let no_frames = serde_json::json!({"DurMovie":{"formatVersion":8,"colorFormat":"256",
        "columns":1,"lines":1,"frames":[]}}).to_string();
    assert_eq!(dur::parse(no_frames.as_bytes()).unwrap_err(), DurError::NoFrames);
}

#[test]
fn a_colour_map_that_fits_neither_dimension_is_an_error() {
    let text = document(8, "256", 4, 2, &[(&["abcd", "efgh"], straight(3, 3, [7, 0]), 0.0)]);
    assert!(matches!(
        dur::parse(gzip(&text).as_slice()).unwrap_err(),
        DurError::ColourMapMismatch { .. }
    ));
}

#[test]
fn short_lines_are_padded_rather_than_refused() {
    // Durdraw trims trailing blanks, so `contents` rows are routinely shorter than
    // the canvas and some are missing altogether. A converter that indexed blindly
    // would panic on the first real file it met.
    let text = document(8, "256", 4, 2, &[(&["ab"], straight(4, 2, [7, 0]), 0.0)]);
    let imported = dur::parse(gzip(&text).as_slice()).expect("convert");
    let grid = frame_grid(&imported.movie, 0);
    assert_eq!(grid.get(1, 0).unwrap().ch, 'b');
    assert_eq!(grid.get(3, 0).unwrap().ch, ' ', "past the end of a short line");
    assert_eq!(grid.get(0, 1).unwrap().ch, ' ', "a line that is not there at all");
}

#[test]
fn a_short_colour_row_is_padded_but_an_ambiguous_map_is_refused() {
    // Two different kinds of short. A row that runs out is filled in, because the
    // orientation is still known. An outer dimension matching neither columns nor
    // lines means the orientation cannot be worked out at all, and guessing it
    // would scramble every colour in the file -- so that is an error, with both
    // numbers in the message.
    let ragged = serde_json::json!([[[40, 0], [40, 0]], [[40, 0]]]);
    let text = document(8, "256", 4, 2, &[(&["abcd", "efgh"], ragged, 0.0)]);
    let imported = dur::parse(gzip(&text).as_slice()).expect("convert");
    let grid = frame_grid(&imported.movie, 0);
    assert_eq!(grid.get(0, 0).unwrap().fg, 40);
    assert_eq!(grid.get(3, 1).unwrap().fg, 7, "past the end of a short colour row");

    let ambiguous = document(8, "256", 4, 2, &[(&["abcd", "efgh"], serde_json::json!([[[7, 0]]]), 0.0)]);
    let error = dur::parse(gzip(&ambiguous).as_slice()).unwrap_err();
    assert!(matches!(error, DurError::ColourMapMismatch { got: 1, columns: 4, lines: 2, .. }), "{error:?}");
    let text = error.to_string();
    assert!(text.contains('4') && text.contains('2'), "{text}");
}
