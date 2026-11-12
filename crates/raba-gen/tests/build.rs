//! The authoring path end to end, and the theme that ships with it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use raba_core::format::Palette;
use raba_core::glyph::{audit, Safety};
use raba_core::{apply, decode, encode, Grid, Movie};
use raba_gen::build::{build, Recipe, SpinnerSpec};
use raba_gen::compose::spinners;

/// The art that ships in `themes/`, read from the repository rather than copied, so
/// that changing the theme and forgetting to run the tests is not possible.
const CACHYOS: &str = include_str!("../../../themes/cachyos.txt");

fn frame(movie: &Movie, index: usize) -> Grid {
    let mut grid = Grid::new(movie.columns, movie.lines, movie.palette).unwrap();
    for step in 0..=index {
        apply(&mut grid, movie, &movie.frames[step]);
    }
    grid
}

fn text(grid: &Grid, line: u16) -> String {
    (0..grid.columns()).map(|c| grid.get(c, line).unwrap().ch).collect::<String>()
}

#[test]
fn a_spinner_is_centred_two_lines_under_the_art() {
    // One blank line so it is not crowded against the logo, one for the spinner.
    let built = build("$1####", &Recipe::default()).unwrap();
    assert_eq!((built.movie.columns, built.movie.lines), (4, 3));

    let first = frame(&built.movie, 0);
    assert_eq!(text(&first, 0), "####");
    assert_eq!(text(&first, 1), "    ", "a blank line between logo and spinner");
    assert_eq!(text(&first, 2), " |  ");
}

#[test]
fn no_spinner_means_the_canvas_is_exactly_the_art() {
    let recipe = Recipe { spinner: None, ..Recipe::default() };
    let built = build("$1####\n$1####", &recipe).unwrap();
    assert_eq!((built.movie.columns, built.movie.lines), (4, 2));
    assert_eq!(built.movie.frames.len(), 1, "nothing moves, so one frame");
}

#[test]
fn an_explicit_position_is_taken_literally() {
    let recipe = Recipe {
        spinner: Some(SpinnerSpec { glyphs: vec!['x'], fg: 7, at: Some((2, 0)) }),
        ..Recipe::default()
    };
    let built = build("$1####", &recipe).unwrap();
    assert_eq!((built.movie.columns, built.movie.lines), (4, 1), "no room added");
    assert_eq!(text(&frame(&built.movie, 0), 0), "##x#");
}

#[test]
fn a_spinner_placed_past_the_art_grows_the_canvas() {
    let recipe = Recipe {
        spinner: Some(SpinnerSpec { glyphs: vec!['x'], fg: 7, at: Some((6, 3)) }),
        ..Recipe::default()
    };
    let built = build("$1####", &recipe).unwrap();
    assert_eq!((built.movie.columns, built.movie.lines), (7, 4));
    assert_eq!(text(&frame(&built.movie, 0), 3), "      x");
}

#[test]
fn the_background_covers_the_whole_canvas_not_just_the_art() {
    // Otherwise the padding around a logo would stay black on a themed background,
    // and the logo would sit in a rectangle of the wrong colour.
    let recipe = Recipe { bg: 4, colours: vec![2], ..Recipe::default() };
    let built = build("$1#", &recipe).unwrap();
    let first = frame(&built.movie, 0);
    assert_eq!(first.get(0, 1).unwrap().bg, 4, "the blank line");
    assert_eq!(first.get(0, 2).unwrap().bg, 4, "behind the spinner");
}

#[test]
fn slots_are_reported_so_a_missing_colour_can_be_warned_about() {
    let built = build("$1a$2b$3c", &Recipe { colours: vec![2], ..Recipe::default() }).unwrap();
    // No slot 0: every cell is inside a marker and the lines are all the same width,
    // so there is no padding and nothing takes the default colour.
    assert_eq!(built.slots_used, vec![1, 2, 3]);
}

#[test]
fn a_built_theme_round_trips_through_the_format() {
    let built = build("$1/\\\n$1\\/", &Recipe::default()).unwrap();
    assert_eq!(decode(&encode(&built.movie)), Ok(built.movie));
}

#[test]
fn every_built_in_spinner_builds() {
    for (name, glyphs) in spinners::ALL {
        let recipe = Recipe { spinner: SpinnerSpec::named(name, 7), ..Recipe::default() };
        let built = build("$1####", &recipe).unwrap();
        assert_eq!(built.movie.frames.len(), glyphs.len(), "{name}");
    }
}

#[test]
fn an_unknown_spinner_name_is_not_silently_ignored() {
    assert!(SpinnerSpec::named("braille", 7).is_none());
}

// --- the shipped theme ------------------------------------------------------

#[test]
fn the_cachyos_theme_builds() {
    let recipe = Recipe {
        colours: vec![2, 10, 6],
        spinner: SpinnerSpec::named("ascii", 10),
        delay_ms: 120,
        ..Recipe::default()
    };
    let built = build(CACHYOS, &recipe).unwrap();
    assert_eq!((built.movie.columns, built.movie.lines), (54, 24));
    assert_eq!(built.movie.frames.len(), 4);
    // Slot 0 is the padding that squares off the shorter lines of the logo.
    assert_eq!(built.slots_used, vec![0, 1, 2, 3], "three colour slots, plus padding");
}

#[test]
fn the_cachyos_theme_draws_on_any_console() {
    // The point of the glyph validator, applied to the thing that ships. If this
    // fails, the theme looks right here and wrong on someone else's machine.
    let built = build(CACHYOS, &Recipe::default()).unwrap();
    let findings = audit(&built.movie);
    assert!(
        findings.iter().all(|f| f.safety != Safety::Unsafe),
        "unsafe glyphs in the shipped theme: {findings:?}"
    );
}

#[test]
fn the_cachyos_theme_is_mostly_one_frame() {
    // A spinner over a static logo costs one cell per frame. If this number grows,
    // the delta encoding has stopped working and the file is about to get large.
    let built = build(CACHYOS, &Recipe::default()).unwrap();
    let canvas = usize::from(built.movie.columns) * usize::from(built.movie.lines);
    assert_eq!(built.movie.frames[0].op_count as usize, canvas);
    for frame in &built.movie.frames[1..] {
        assert_eq!(frame.op_count, 1);
    }
    assert!(encode(&built.movie).len() < 20_000, "{} bytes", encode(&built.movie).len());
}

#[test]
fn the_cachyos_theme_fits_a_default_console() {
    // 80x25 is the fallback the boot path assumes when it cannot read the real size.
    // A theme that does not fit it is a theme that gets clipped on the machines
    // least able to tell you why.
    let built = build(CACHYOS, &Recipe::default()).unwrap();
    assert!(built.movie.columns <= 80, "{} columns", built.movie.columns);
    assert!(built.movie.lines <= 25, "{} lines", built.movie.lines);
}

#[test]
fn the_art_source_uses_no_colours_the_console_lacks() {
    let recipe = Recipe { colours: vec![2, 10, 6], ..Recipe::default() };
    let built = build(CACHYOS, &recipe).unwrap();
    assert_eq!(built.movie.palette, Palette::Ansi16);
    // Backgrounds stay in 0-7: on a Linux console the high bit is blink, not
    // brightness. See docs/format.md.
    assert!(built.movie.ops.iter().all(|op| op.bg <= 7));
}
