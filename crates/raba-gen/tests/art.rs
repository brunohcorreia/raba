//! Parsing fastfetch-style art.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use raba_core::format::Palette;
use raba_gen::art::{parse, Art, ArtError};

fn art(source: &str) -> Art {
    parse(source).unwrap()
}

/// The characters of a parsed row, for comparing shape without writing out cells.
fn row(art: &Art, line: u16) -> String {
    let start = usize::from(line) * usize::from(art.columns());
    art.cells()[start..start + usize::from(art.columns())].iter().map(|c| c.ch).collect()
}

fn slots(art: &Art, line: u16) -> Vec<u8> {
    let start = usize::from(line) * usize::from(art.columns());
    art.cells()[start..start + usize::from(art.columns())].iter().map(|c| c.slot).collect()
}

#[test]
fn plain_text_has_no_slots() {
    let parsed = art("ab\ncd");
    assert_eq!((parsed.columns(), parsed.lines()), (2, 2));
    assert_eq!(row(&parsed, 0), "ab");
    assert_eq!(row(&parsed, 1), "cd");
    assert_eq!(parsed.slots_used(), vec![0]);
}

#[test]
fn a_marker_colours_what_follows_it_and_occupies_nothing() {
    let parsed = art("$1ab$2cd");
    assert_eq!(parsed.columns(), 4, "markers are not characters");
    assert_eq!(row(&parsed, 0), "abcd");
    assert_eq!(slots(&parsed, 0), vec![1, 1, 2, 2]);
}

#[test]
fn a_slot_carries_across_lines() {
    // fastfetch logos in a single colour set $1 once, on the first line.
    let parsed = art("$3xx\nyy\nzz");
    assert_eq!(slots(&parsed, 2), vec![3, 3]);
    assert_eq!(parsed.slots_used(), vec![3]);
}

#[test]
fn text_before_the_first_marker_is_slot_zero() {
    // Not slot 1. "Before any marker" is the theme's default colour, which is a
    // different thing from "the logo's first colour".
    let parsed = art("ab$1cd");
    assert_eq!(slots(&parsed, 0), vec![0, 0, 1, 1]);
    assert_eq!(parsed.slots_used(), vec![0, 1]);
}

#[test]
fn a_doubled_dollar_is_a_literal_dollar() {
    let parsed = art("$1a$$b");
    assert_eq!(row(&parsed, 0), "a$b");
    assert_eq!(slots(&parsed, 0), vec![1, 1, 1]);
}

#[test]
fn short_lines_are_padded_to_the_widest() {
    let parsed = art("$1xxxxx\n$1x");
    assert_eq!(parsed.columns(), 5);
    assert_eq!(row(&parsed, 1), "x    ");
    // Padding is blank, not slot 1: it is absence of art, not art in a colour.
    assert_eq!(slots(&parsed, 1), vec![1, 0, 0, 0, 0]);
}

#[test]
fn invisible_whitespace_does_not_decide_the_width() {
    // Art files pick up trailing spaces and surrounding blank lines from editors and
    // from being embedded in source. None of it is visible, so none of it should
    // change the logo's shape.
    let parsed = art("\n\n$1ab   \n$1cd\n\n\n");
    assert_eq!((parsed.columns(), parsed.lines()), (2, 2));
    assert_eq!(row(&parsed, 0), "ab");
}

#[test]
fn an_interior_blank_line_is_part_of_the_logo() {
    let parsed = art("$1ab\n\n$1cd");
    assert_eq!(parsed.lines(), 3);
    assert_eq!(row(&parsed, 1), "  ");
}

#[test]
fn crlf_files_parse() {
    let parsed = art("$1ab\r\n$1cd\r\n");
    assert_eq!((parsed.columns(), parsed.lines()), (2, 2));
    assert_eq!(row(&parsed, 1), "cd");
}

#[test]
fn neofetch_syntax_is_rejected_by_name() {
    // `${c1}` is close enough to `$1` to be pasted in from the wrong fetch tool, and
    // silently treating `{` as art would put a stray brace in the logo.
    assert_eq!(parse("${c1}  /\\"), Err(ArtError::UnknownMarker { line: 1, column: 1, got: '{' }));
}

#[test]
fn slot_zero_is_not_a_marker() {
    // Slots are 1-9. `$0` is a typo, and reading it as "the default" would make two
    // different spellings of the same thing.
    assert_eq!(parse("$0ab"), Err(ArtError::UnknownMarker { line: 1, column: 1, got: '0' }));
}

#[test]
fn a_dollar_at_the_end_of_a_line_is_an_error() {
    assert_eq!(parse("ab$"), Err(ArtError::DanglingMarker { line: 1, column: 3 }));
    // Including when there is a next line to borrow a character from.
    assert_eq!(parse("ab$\ncd"), Err(ArtError::DanglingMarker { line: 1, column: 3 }));
}

#[test]
fn tabs_are_rejected() {
    // A tab has no definite width, so art containing one has no definite shape.
    assert_eq!(parse("a\tb"), Err(ArtError::Tab { line: 1, column: 2 }));
}

#[test]
fn rendered_output_is_rejected_rather_than_absorbed() {
    // Pasting what a fetch tool printed, instead of what it reads. The escape would
    // otherwise become cells in the logo and reach a terminal as a command.
    let pasted = "\u{1b}[36m  /\\\u{1b}[0m";
    assert_eq!(parse(pasted), Err(ArtError::ControlCharacter { line: 1, column: 1, value: 0x1b }));
}

#[test]
fn nothing_is_an_error_not_an_empty_logo() {
    assert_eq!(parse(""), Err(ArtError::Empty));
    assert_eq!(parse("\n\n   \n"), Err(ArtError::Empty));
}

#[test]
fn errors_point_at_a_line_and_column_an_editor_agrees_with() {
    assert_eq!(
        parse("$1ok\n$1also ok\n  $x"),
        Err(ArtError::UnknownMarker { line: 3, column: 3, got: 'x' })
    );
}

#[test]
fn slots_become_colours_through_the_theme() {
    let parsed = art("$1ab$2cd");
    // Slot 1 is green, slot 2 is blue, anything else grey.
    let grid = parsed.to_grid(Palette::Ansi16, &[2, 4], 7, 0).unwrap();
    assert_eq!(grid.cell(0).unwrap().fg, 2);
    assert_eq!(grid.cell(2).unwrap().fg, 4);
}

#[test]
fn a_slot_the_theme_forgot_falls_back_rather_than_failing() {
    // The logo uses three colours and the theme supplies two. Drawing the third in
    // the default beats refusing to build, and `slots_used` is how a tool warns.
    let parsed = art("$1a$2b$3c");
    assert_eq!(parsed.slots_used(), vec![1, 2, 3]);
    let grid = parsed.to_grid(Palette::Ansi16, &[2, 4], 7, 0).unwrap();
    assert_eq!(grid.cell(2).unwrap().fg, 7);
}

#[test]
fn padding_takes_the_background_too() {
    let parsed = art("$1xx\n$1x");
    let grid = parsed.to_grid(Palette::Ansi16, &[2], 7, 1).unwrap();
    let pad = grid.get(1, 1).unwrap();
    assert_eq!((pad.ch, pad.fg, pad.bg), (' ', 7, 1));
}

#[test]
fn a_real_logo_shape_survives_the_round_trip() {
    // The Arch logo's first lines, in the shape fastfetch stores them.
    let source = "\
$1                   -`
$1                  .o+`
$1                 `ooo/
$1                `+oooo:";
    let parsed = art(source);
    assert_eq!(parsed.lines(), 4);
    assert_eq!(parsed.columns(), 23, "the widest line, not the longest source line");
    assert_eq!(row(&parsed, 3), "                `+oooo:");
    // Every character of the logo is slot 1; the padding that squares off the
    // shorter lines is slot 0, because it is absence of art rather than art.
    assert!(parsed.cells().iter().filter(|cell| cell.ch != ' ').all(|cell| cell.slot == 1));
    assert_eq!(parsed.slots_used(), vec![0, 1]);
}

#[test]
fn art_past_the_formats_limits_says_how_big_it_was_and_what_fits() {
    // 1024 cells in each direction. Reached by anybody converting a terminal
    // recording or a wide log, and the message has to carry both the size
    // that was offered and the one that would have worked.
    let wide = format!("{}\n", "#".repeat(2000));
    let outcome = parse(&wide);
    assert!(matches!(outcome, Err(ArtError::TooLarge { columns: 2000, lines: 1 })), "{outcome:?}");

    let tall = "#\n".repeat(2000);
    let outcome = parse(&tall);
    assert!(matches!(outcome, Err(ArtError::TooLarge { columns: 1, lines: 2000 })), "{outcome:?}");
}

#[test]
fn a_line_longer_than_a_canvas_can_count_is_still_reported_in_full() {
    // The canvas limits are u16s and so is every size in the format, so a line
    // of more than 65 535 characters is refused by a different path from the
    // one above — before the number can be narrowed. It has to come back out
    // at its real size: "65535 columns" about a line of 70 000 would send the
    // author looking for a limit they are nowhere near.
    let enormous = format!("{}\n", "#".repeat(70_000));
    let outcome = parse(&enormous);
    assert!(
        matches!(outcome, Err(ArtError::TooLarge { columns: 70_000, lines: 1 })),
        "{outcome:?}"
    );
}
