//! Writing `.dur`, and reading it back.
//!
//! The writer exists so a display manager can show the same art RABA did, which
//! means the only property that matters is that the art survives. So these go
//! through the real importer rather than inspecting JSON: a writer checked
//! against its own idea of the format proves nothing.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use raba_core::render::{apply, Cell, Grid};
use raba_core::{Frame, Movie, Op, Palette, Scaling};
use raba_gen::{dur, durwrite};

fn movie_of(columns: u16, lines: u16, palette: Palette, cells: &[(u32, char, u8, u8)]) -> Movie {
    let ops: Vec<Op> = cells.iter().map(|&(cell, ch, fg, bg)| Op { cell, ch, fg, bg }).collect();
    Movie {
        columns,
        lines,
        palette,
        scaling: Scaling::Auto,
        looping: true,
        frames: vec![Frame {
            delay_ms: 100,
            op_start: 0,
            op_count: u16::try_from(ops.len()).unwrap(),
        }],
        ops,
    }
}

/// Every cell of every frame, resolved.
fn grids(movie: &Movie) -> Vec<Grid> {
    let mut grid = Grid::new(movie.columns, movie.lines, movie.palette).unwrap();
    let mut out = Vec::new();
    for frame in &movie.frames {
        apply(&mut grid, movie, frame);
        out.push(grid.clone());
    }
    out
}

fn round_trip(movie: &Movie) -> Movie {
    at_version(movie, durwrite::Version::default())
}

fn at_version(movie: &Movie, version: durwrite::Version) -> Movie {
    let meta = durwrite::Meta { version, ..durwrite::Meta::default() };
    let bytes = durwrite::write(movie, &meta).expect("should write");
    dur::parse(&bytes).expect("our own output must import").movie
}

#[test]
fn the_art_survives_the_round_trip_cell_for_cell() {
    let movie = movie_of(
        3,
        2,
        Palette::Ansi16,
        &[
            (0, '█', 1, 0),
            (1, '▒', 9, 4),
            (2, 'x', 7, 0),
            (3, ' ', 7, 0),
            (4, '?', 15, 1),
            (5, '─', 2, 0),
        ],
    );
    assert_eq!(grids(&round_trip(&movie)), grids(&movie));
}

#[test]
fn red_stays_red_and_blue_stays_blue() {
    // Durdraw's 0-15 are in curses order, where 1 is blue and 4 is red. Writing
    // ANSI indices straight through would swap them, and the round trip is the
    // only thing that catches it: a writer and a reader that are both wrong the
    // same way agree with each other.
    let movie = movie_of(
        4,
        1,
        Palette::Ansi16,
        &[(0, 'r', 1, 0), (1, 'g', 2, 0), (2, 'b', 4, 0), (3, 'y', 14, 0)],
    );
    let back = round_trip(&movie);
    let colours: Vec<u8> = back.ops.iter().map(|op| op.fg).collect();
    assert_eq!(colours, vec![1, 2, 4, 14]);
}

#[test]
fn backgrounds_survive_too() {
    let movie = movie_of(2, 1, Palette::Ansi16, &[(0, 'a', 7, 1), (1, 'b', 7, 6)]);
    let back = round_trip(&movie);
    assert_eq!(back.ops.iter().map(|op| op.bg).collect::<Vec<_>>(), vec![1, 6]);
}

#[test]
fn a_256_colour_movie_keeps_its_256_colours() {
    let movie = movie_of(2, 1, Palette::Xterm256, &[(0, 'a', 196, 0), (1, 'b', 33, 240)]);
    let back = round_trip(&movie);
    assert_eq!(back.palette, Palette::Xterm256);
    assert_eq!(back.ops[0].fg, 196);
    assert_eq!(back.ops[1].fg, 33);
    assert_eq!(back.ops[1].bg, 240);
}

#[test]
fn both_versions_round_trip_and_disagree_about_the_colour_map() {
    // The one real difference: 7 indexes the colour map by column and 8 by line.
    // Both have to come back identical, and the JSON has to actually differ — a
    // writer that ignored the version would pass the first half of this alone.
    let cells: Vec<(u32, char, u8, u8)> =
        (0..6).map(|i| (i, 'x', 1 + u8::try_from(i).unwrap(), 0)).collect();
    let movie = movie_of(3, 2, Palette::Ansi16, &cells);
    for version in [durwrite::Version::Seven, durwrite::Version::Eight] {
        assert_eq!(grids(&at_version(&movie, version)), grids(&movie), "{version:?}");
    }
    let json = |version| {
        durwrite::to_json(&movie, &durwrite::Meta { version, ..durwrite::Meta::default() })
    };
    assert_ne!(json(durwrite::Version::Seven), json(durwrite::Version::Eight));
    assert!(json(durwrite::Version::Seven).contains("\"sizeX\""));
    assert!(json(durwrite::Version::Eight).contains("\"columns\""));
}

#[test]
fn the_default_is_the_version_display_managers_actually_read() {
    // Ly 1.4.1's own example.dur is version 7, and so is the file a user had
    // configured. Writing 8 by default would make the option useless for the one
    // thing it exists for. This is an empirical default, so it gets a test.
    assert_eq!(durwrite::Version::default().number(), durwrite::DEFAULT_VERSION);
    assert_eq!(durwrite::DEFAULT_VERSION, 7);
    assert_eq!(durwrite::Version::from_number(9), None);
}

#[test]
fn a_non_square_canvas_proves_the_orientation() {
    // The whole v7-vs-v8 trap is the colour map's orientation, and it is only
    // detectable when the canvas is not square. 5x2 is: a transposed map would be
    // the wrong shape and the importer would say so.
    let cells: Vec<(u32, char, u8, u8)> = (0..10)
        .map(|i| {
            (i, char::from(b'a' + u8::try_from(i).unwrap()), 1 + u8::try_from(i % 6).unwrap(), 0)
        })
        .collect();
    let movie = movie_of(5, 2, Palette::Ansi16, &cells);
    for version in [durwrite::Version::Seven, durwrite::Version::Eight] {
        let meta = durwrite::Meta { version, ..durwrite::Meta::default() };
        let bytes = durwrite::write(&movie, &meta).expect("should write");
        let imported = dur::parse(&bytes).expect("should import");
        assert!(imported.notes.is_empty(), "{version:?} guessed something: {:?}", imported.notes);
        assert_eq!(grids(&imported.movie), grids(&movie), "{version:?}");
    }
}

#[test]
fn several_frames_survive_as_several_frames() {
    let mut movie = movie_of(2, 1, Palette::Ansi16, &[(0, 'a', 7, 0), (1, 'b', 7, 0)]);
    movie.ops.push(Op { cell: 0, ch: 'z', fg: 3, bg: 0 });
    movie.frames.push(Frame { delay_ms: 100, op_start: 2, op_count: 1 });
    let back = round_trip(&movie);
    assert_eq!(back.frames.len(), 2);
    // `.dur` stores whole screens, so the delta is rebuilt on import rather than
    // preserved: what has to match is the pictures, not the ops.
    assert_eq!(grids(&back), grids(&movie));
}

#[test]
fn the_metadata_comes_back() {
    let movie = movie_of(1, 1, Palette::Ansi16, &[(0, 'x', 7, 0)]);
    let meta = durwrite::Meta {
        name: "A \"Quoted\" Name".to_owned(),
        artist: "bruno".to_owned(),
        ..durwrite::Meta::default()
    };
    let bytes = durwrite::write(&movie, &meta).expect("should write");
    let imported = dur::parse(&bytes).expect("should import");
    assert_eq!(imported.name, "A \"Quoted\" Name", "the JSON escaping has to hold");
    assert_eq!(imported.artist, "bruno");
}

#[test]
fn the_output_is_gzip_because_durdraw_writes_gzip() {
    let movie = movie_of(1, 1, Palette::Ansi16, &[(0, 'x', 7, 0)]);
    let bytes = durwrite::write(&movie, &durwrite::Meta::default()).expect("should write");
    assert_eq!(bytes.get(..2), Some(&[0x1f, 0x8b][..]));
}

#[test]
fn only_the_scaling_hint_is_lost() {
    // `.dur` has nowhere to put it, so a theme that said "crop" comes back saying
    // "auto". That is not a silent behaviour change for art like this: `auto`
    // reads the alphabet, and a logo's alphabet is not the shading ramp, so it is
    // still cropped. Stated as a test so that stops being a hopeful assumption.
    let movie = movie_of(2, 1, Palette::Ansi16, &[(0, '/', 6, 0), (1, 'o', 6, 0)]);
    let crop = Movie { scaling: Scaling::Crop, ..movie };
    let back = round_trip(&crop);
    assert_eq!(back.scaling, Scaling::Auto);
    assert_eq!(
        raba_core::render::resamples(&back),
        raba_core::render::resamples(&crop),
        "the hint is lost but the behaviour is not"
    );
}

#[test]
fn a_cell_the_grid_never_set_is_still_written() {
    // `.dur` has no notion of an untouched cell, so blanks have to be real.
    let movie = movie_of(3, 1, Palette::Ansi16, &[(1, 'x', 7, 0)]);
    let back = round_trip(&movie);
    let grid = &grids(&back)[0];
    assert_eq!(grid.get(0, 0), Some(&Cell::BLANK));
    assert_eq!(grid.get(2, 0), Some(&Cell::BLANK));
}

#[test]
fn a_title_with_json_in_it_comes_back_as_itself_and_not_as_broken_json() {
    // The writer escapes by hand — `dur.rs` would otherwise need a parallel
    // set of `Serialize` structs kept matching it — and a missed case here is
    // not a wrong title, it is a `.dur` no reader can open. Durdraw's own
    // title field is free text and a theme called `C:\art\"nice"` is one
    // Windows contributor away.
    let movie = movie_of(2, 1, Palette::Ansi16, &[(0, '#', 7, 0), (1, '#', 7, 0)]);
    for awkward in [
        "a \"quoted\" title",
        "back\\slash",
        "two\nlines",
        "carriage\rreturn",
        "a\ttab",
        "a\u{1}control\u{1f}character",
        "ünïcödé ☃",
        "\\\"",
    ] {
        let meta = durwrite::Meta {
            name: awkward.to_owned(),
            artist: awkward.to_owned(),
            ..durwrite::Meta::default()
        };
        let bytes = durwrite::write(&movie, &meta).expect("should write");
        let back = dur::parse(&bytes).unwrap_or_else(|why| {
            panic!("our own output must import, and {awkward:?} did not: {why}")
        });
        assert_eq!(back.name, awkward, "the title did not survive");
        assert_eq!(back.artist, awkward, "the artist did not survive");
    }
}

#[test]
fn a_title_that_is_only_awkward_characters_still_leaves_readable_art() {
    let movie = movie_of(2, 1, Palette::Ansi16, &[(0, 'o', 3, 0), (1, 'k', 3, 0)]);
    let meta = durwrite::Meta {
        name: "\"\\\n\r\t\u{0}".to_owned(),
        artist: String::new(),
        ..durwrite::Meta::default()
    };
    let bytes = durwrite::write(&movie, &meta).expect("should write");
    let back = dur::parse(&bytes).expect("our own output must import").movie;
    assert_eq!(grids(&back), grids(&movie));
}
