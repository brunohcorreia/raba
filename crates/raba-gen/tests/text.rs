//! Importing plain text.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use raba_gen::text::{self, Options, TextError};

fn rows(bytes: &[u8]) -> Vec<String> {
    let imported = text::parse(bytes, &Options::default()).expect("should import");
    let movie = &imported.movie;
    let columns = usize::from(movie.columns);
    movie.ops.chunks(columns).map(|row| row.iter().map(|op| op.ch).collect::<String>()).collect()
}

#[test]
fn text_arrives_as_itself() {
    assert_eq!(rows(b"ab\ncd\n"), vec!["ab", "cd"]);
}

#[test]
fn short_lines_are_padded_to_the_longest() {
    // The canvas is a rectangle, so every row has to be one. Padding with blanks
    // rather than refusing is what lets a ragged file import at all.
    assert_eq!(rows(b"a\nlonger\n"), vec!["a     ", "longer"]);
}

#[test]
fn tabs_stop_where_tabs_stop() {
    assert_eq!(rows(b"a\tb"), vec!["a       b"]);
    assert_eq!(rows(b"abcdefg\th"), vec!["abcdefg h"]);
    assert_eq!(rows(b"abcdefgh\ti"), vec!["abcdefgh        i"]);
}

#[test]
fn tab_stops_are_counted_in_characters_not_bytes() {
    // "é" is two bytes and one column. Counting bytes would put the tab stop in
    // the wrong place for every accented line in the file.
    assert_eq!(rows("é\tx".as_bytes()), vec!["é       x"]);
}

#[test]
fn crlf_is_a_line_ending_and_not_a_character() {
    assert_eq!(rows(b"a\r\nb\r\n"), vec!["a", "b"]);
}

#[test]
fn a_byte_order_mark_is_not_a_character_either() {
    assert_eq!(rows("\u{feff}hi".as_bytes()), vec!["hi"]);
}

#[test]
fn trailing_newlines_are_not_blank_lines() {
    assert_eq!(rows(b"a\n\n\n\n"), vec!["a"]);
}

#[test]
fn invalid_utf8_is_read_as_cp437_and_says_so() {
    // A .nfo from before Unicode. 0xDB is a full block there and not UTF-8 at all.
    let imported = text::parse(&[0xdb, 0xdb], &Options::default()).expect("should import");
    let drawn: String = imported.movie.ops.iter().map(|op| op.ch).collect();
    assert_eq!(drawn, "██");
    assert!(imported.notes.iter().any(|note| note.contains("CP437")));
}

#[test]
fn valid_utf8_is_never_mistaken_for_cp437() {
    let imported = text::parse("░▒▓".as_bytes(), &Options::default()).expect("should import");
    let drawn: String = imported.movie.ops.iter().map(|op| op.ch).collect();
    assert_eq!(drawn, "░▒▓");
    assert!(imported.notes.is_empty());
}

#[test]
fn colour_markers_mean_nothing_here() {
    // The difference between `convert` and `build`, and the reason it is a
    // difference: in a file written for something else a dollar is a dollar.
    assert_eq!(rows(b"$1 costs $2"), vec!["$1 costs $2"]);
}

#[test]
fn the_colours_asked_for_are_the_colours_used() {
    let imported = text::parse(b"x", &Options { fg: 10, bg: 4 }).expect("should import");
    assert_eq!(imported.movie.ops[0].fg, 10);
    assert_eq!(imported.movie.ops[0].bg, 4);
}

#[test]
fn an_empty_file_is_an_error_rather_than_an_empty_canvas() {
    assert_eq!(text::parse(b"", &Options::default()).err(), Some(TextError::Empty));
    assert_eq!(text::parse(b"\n\n\n", &Options::default()).err(), Some(TextError::Empty));
}

#[test]
fn text_is_never_resampled() {
    // Halving a letter does not give half a letter.
    let imported = text::parse(b"hello", &Options::default()).expect("should import");
    assert!(!raba_core::render::resamples(&imported.movie));
}

#[test]
fn text_too_large_for_a_canvas_says_which_axis_and_what_the_limit_is() {
    // 1024 in each direction, from the format. A `convert` of a log file or a
    // minified blob is how this is reached in practice, and the answer has to
    // be a sentence rather than a silently cropped theme.
    let wide = format!("{}\n", "x".repeat(2000));
    match text::parse(wide.as_bytes(), &Options::default()) {
        Err(TextError::TooWide { columns }) => assert_eq!(columns, 2000),
        other => panic!("{other:?}"),
    }

    let tall = "x\n".repeat(2000);
    match text::parse(tall.as_bytes(), &Options::default()) {
        Err(TextError::TooTall { lines }) => assert_eq!(lines, 2000),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_file_of_nothing_but_blank_lines_is_empty_and_not_a_canvas_of_spaces() {
    // Trailing blanks are stripped, so this reaches the "no rows" refusal; a
    // file whose only line is empty reaches the "no columns" one. Both are
    // `Empty`, and both have to be refusals rather than a 0x0 theme that
    // encodes fine and draws nothing.
    for nothing in ["", "\n", "\n\n\n", "   \n", "\r\n\r\n"] {
        match text::parse(nothing.as_bytes(), &Options::default()) {
            Err(TextError::Empty) => {}
            other => panic!("{nothing:?} gave {other:?}"),
        }
    }
}
