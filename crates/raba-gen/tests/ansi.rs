//! Importing ANSI art.
//!
//! The importer is a terminal, so these tests are written the way you would check
//! a terminal: feed it a byte stream and look at what is on the screen.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use raba_gen::ansi::{self, AnsiError, Options};
use raba_gen::cp437;
use raba_gen::import::Imported;

/// Run a stream and give back the screen as rows of characters.
fn screen(bytes: &[u8]) -> Vec<String> {
    rows(&ansi::parse(bytes, &Options::default()).expect("should import"))
}

fn rows(imported: &Imported) -> Vec<String> {
    let grid = ansi::to_grid(imported).expect("a grid");
    (0..grid.lines())
        .map(|line| {
            (0..grid.columns())
                .map(|column| grid.get(column, line).unwrap().ch)
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

/// Colour of the cell at a position.
fn colour_at(imported: &Imported, column: u16, line: u16) -> (u8, u8) {
    let grid = ansi::to_grid(imported).expect("a grid");
    let cell = grid.get(column, line).expect("inside the canvas");
    (cell.fg, cell.bg)
}

fn parsed(bytes: &[u8]) -> Imported {
    ansi::parse(bytes, &Options::default()).expect("should import")
}

#[test]
fn plain_text_comes_through_as_itself() {
    assert_eq!(screen(b"hello\r\nworld\r\n"), vec!["hello", "world"]);
}

#[test]
fn high_bytes_are_cp437_and_not_utf8() {
    // 0xDB is a full block in CP437. Read as UTF-8 it is not a character at all,
    // which is the single most common way to get an .ans file wrong.
    assert_eq!(screen(&[0xdb, 0xb2, 0xb1, 0xb0]), vec!["█▓▒░"]);
    // And the box drawing every piece of ANSI art is made of.
    assert_eq!(screen(&[0xc9, 0xcd, 0xbb]), vec!["╔═╗"]);
}

#[test]
fn the_control_range_draws_pictures_rather_than_controlling_anything() {
    // CP437 has a smiley at 0x01 and a heart at 0x03, and art uses them.
    assert_eq!(screen(&[0x01, 0x03, 0x0e]), vec!["☺♥♫"]);
}

#[test]
fn the_four_real_control_bytes_still_control() {
    // Line feed and carriage return move the cursor rather than drawing.
    assert_eq!(screen(b"ab\r\ncd"), vec!["ab", "cd"]);
    // SUB ends the art: a DOS file's padding must not become a row of glyphs.
    assert_eq!(screen(b"art\x1ajunk after the end"), vec!["art"]);
}

#[test]
fn bold_means_bright_as_it_did_on_the_terminals_this_was_drawn_for() {
    let art = parsed(b"\x1b[31mA\x1b[1mB\x1b[22mC");
    assert_eq!(colour_at(&art, 0, 0).0, 1, "plain red");
    assert_eq!(colour_at(&art, 1, 0).0, 9, "bold red is bright red");
    assert_eq!(colour_at(&art, 2, 0).0, 1, "and 22 puts it back");
}

#[test]
fn bold_persists_across_a_later_colour_change() {
    // What a real terminal does, and what art relies on: SGR 1 is sticky until
    // something clears it, so a colour set afterwards is bright too.
    let art = parsed(b"\x1b[1;33mA\x1b[32mB\x1b[0mC");
    assert_eq!(colour_at(&art, 0, 0).0, 11);
    assert_eq!(colour_at(&art, 1, 0).0, 10, "still bold, so bright green");
    assert_eq!(colour_at(&art, 2, 0).0, 7, "reset");
}

#[test]
fn backgrounds_are_read_and_clamped_to_what_a_console_has() {
    let art = parsed(b"\x1b[44mA\x1b[104mB");
    assert_eq!(colour_at(&art, 0, 0).1, 4);
    // 104 is a bright background, which on a console is the blink bit.
    assert!(colour_at(&art, 1, 0).1 <= 7);
    assert!(art.notes.iter().any(|note| note.contains("bright background")));
}

#[test]
fn extended_colours_are_understood() {
    let art = parsed(b"\x1b[38;5;196mA\x1b[38;2;0;255;0mB");
    assert_eq!(colour_at(&art, 0, 0).0, 196);
    // A true-colour green has to become a palette index, and the nearest is green.
    assert!(matches!(colour_at(&art, 1, 0).0, 2 | 10));
}

#[test]
fn the_cursor_can_go_back_over_what_was_drawn() {
    // The reason this importer runs the stream rather than parsing it: real art
    // draws a shape and then jumps back inside it.
    assert_eq!(screen(b"\x1b[1;1H....\x1b[1;2HXX"), vec![".XX."]);
}

#[test]
fn every_movement_sequence_art_uses_is_handled() {
    assert_eq!(screen(b"a\x1b[2Cb"), vec!["a  b"], "CUF skips without drawing");
    assert_eq!(screen(b"abc\x1b[2Dx"), vec!["axc"], "CUB steps back");
    assert_eq!(screen(b"a\x1b[Bb"), vec!["a", " b"], "CUD drops a line");
    assert_eq!(screen(b"a\r\nb\x1b[Ax"), vec!["ax", "b"], "CUU climbs one");
    assert_eq!(screen(b"abc\x1b[1Gz"), vec!["zbc"], "CHA is an absolute column");
    assert_eq!(screen(b"a\x1b[Eb"), vec!["a", "b"], "CNL goes to the next line's start");
    assert_eq!(screen(b"ab\x1b[sxy\x1b[uZ"), vec!["abZy"], "save and restore");
}

#[test]
fn erasing_works_the_way_a_terminal_erases() {
    assert_eq!(screen(b"abcdef\x1b[1;4H\x1b[K"), vec!["abc"], "K clears to end of line");
    assert_eq!(screen(b"abcdef\x1b[1;4H\x1b[1Kz"), vec!["   zef"], "1K clears to the start");
    // ED 2 is how art starts from a known state, and how an ANSImation separates
    // its frames: what survives is the last one, which is what a terminal shows.
    assert_eq!(screen(b"first\x1b[2Jsecond"), vec!["second"]);
}

#[test]
fn a_line_wraps_at_the_declared_width() {
    let options = Options { columns: 4 };
    let imported = ansi::parse(b"abcdefgh", &options).expect("should import");
    assert_eq!(rows(&imported), vec!["abcd", "efgh"]);
}

#[test]
fn sauce_is_read_rather_than_drawn() {
    let mut bytes = b"art\x1a".to_vec();
    let mut sauce = vec![0u8; 128];
    sauce[0..7].copy_from_slice(b"SAUCE00");
    sauce[7..42].copy_from_slice(b"A Title                            ");
    sauce[42..62].copy_from_slice(b"An Artist           ");
    sauce[96] = 44; // TInfo1 low byte: width 44
    sauce[105] = 1; // iCE colours
    bytes.extend_from_slice(&sauce);

    let imported = ansi::parse(&bytes, &Options::default()).expect("should import");
    assert_eq!(imported.name, "A Title");
    assert_eq!(imported.artist, "An Artist");
    assert_eq!(imported.movie.columns, 44, "SAUCE's width wins over the default");
    assert!(imported.notes.iter().any(|note| note.contains("iCE")));
    // And none of those 128 bytes became a row of glyphs at the bottom.
    assert_eq!(rows(&imported), vec!["art"]);
}

#[test]
fn a_sauce_comment_block_is_stripped_too() {
    let mut bytes = b"art\x1a".to_vec();
    bytes.extend_from_slice(b"COMNT");
    bytes.extend_from_slice(&[b'x'; 64]);
    let mut sauce = vec![0u8; 128];
    sauce[0..7].copy_from_slice(b"SAUCE00");
    sauce[104] = 1; // one comment line
    bytes.extend_from_slice(&sauce);

    let imported = ansi::parse(&bytes, &Options::default()).expect("should import");
    assert_eq!(rows(&imported), vec!["art"], "the comment must not be drawn");
}

#[test]
fn trailing_blank_lines_are_not_art() {
    assert_eq!(screen(b"a\r\n\r\n\r\n\r\n"), vec!["a"]);
}

#[test]
fn art_that_draws_nothing_is_an_error_rather_than_an_empty_file() {
    assert_eq!(ansi::parse(b"", &Options::default()).err(), Some(AnsiError::Empty));
    assert_eq!(ansi::parse(b"\r\n\r\n", &Options::default()).err(), Some(AnsiError::Empty));
}

#[test]
fn a_canvas_past_the_formats_limit_is_refused() {
    let options = Options { columns: 2000 };
    assert!(matches!(ansi::parse(b"x", &options), Err(AnsiError::TooWide { columns: 2000 })));
}

#[test]
fn a_truncated_escape_sequence_does_not_hang_or_panic() {
    // Files get cut off mid-download; this must not be a crash at build time.
    for tail in [&b"\x1b"[..], b"\x1b[", b"\x1b[38;5", b"\x1b[999999999"] {
        let mut bytes = b"ok".to_vec();
        bytes.extend_from_slice(tail);
        let _ = ansi::parse(&bytes, &Options::default());
    }
}

#[test]
fn the_cp437_table_is_total_and_round_trips() {
    assert_eq!(cp437::TABLE.len(), 256);
    for byte in 0u8..=255 {
        let ch = cp437::decode(byte);
        // Every byte decodes, which is why ANSI art cannot fail to decode.
        assert_eq!(cp437::encode(ch), Some(first_byte_for(ch)), "byte {byte:#04x}");
    }
    assert_eq!(cp437::decode(0xdb), '█');
    assert_eq!(cp437::decode(0x20), ' ');
    assert_eq!(cp437::encode('≈'), Some(0xf7));
    assert_eq!(cp437::encode('€'), None, "not in the code page");
}

/// CP437 has two bytes that draw a blank (0x00 and 0x20); `encode` picks the first.
fn first_byte_for(ch: char) -> u8 {
    u8::try_from(cp437::TABLE.iter().position(|c| *c == ch).unwrap()).unwrap()
}

// --- the escape sequences real art uses and nothing tested --------------------
//
// Added when coverage said the SGR table and the erase sequences were
// unexercised. Every one of these appears in art people actually have: the
// bright-colour codes are how most .ans files colour anything, and the erase
// forms are what an editor emits when it clears the screen before drawing.

#[test]
fn a_bright_foreground_is_the_top_half_of_the_palette() {
    // 90-97 is how almost every .ans file draws a bright colour; mapping it to
    // the dim eight would quietly flatten the art.
    let art = parsed(b"\x1b[91mA\x1b[0m");
    assert_eq!(colour_at(&art, 0, 0).0, 9, "bright red foreground");
}

/// A console has eight background colours, not sixteen: the ninth bit is the
/// blink attribute. So 100-107 lands on the dim half deliberately, and the
/// note is how the author hears about it.
#[test]
fn a_bright_background_lands_on_the_dim_half_and_the_note_says_so() {
    let art = parsed(b"\x1b[101mB\x1b[0m");
    assert_eq!(colour_at(&art, 0, 0).1, 1, "bright red background folded to red");
    assert!(
        art.notes.iter().any(|note| note.to_lowercase().contains("background")),
        "the fold happened without a word: {:?}",
        art.notes
    );
}

#[test]
fn reverse_video_swaps_the_two_colours_rather_than_being_ignored() {
    let art = parsed(b"\x1b[31;44m\x1b[7mX");
    assert_eq!(colour_at(&art, 0, 0), (4, 1), "7m did not swap foreground and background");
}

#[test]
fn the_default_colour_codes_put_the_defaults_back() {
    let art = parsed(b"\x1b[31;44mA\x1b[39mB\x1b[49mC");
    assert_eq!(colour_at(&art, 1, 0).0, 7, "39m did not restore the default foreground");
    assert_eq!(colour_at(&art, 2, 0).1, 0, "49m did not restore the default background");
}

#[test]
fn a_256_colour_background_is_kept_as_the_index_it_asked_for() {
    let art = parsed(b"\x1b[48;5;123mX");
    assert_eq!(colour_at(&art, 0, 0).1, 123);
}

#[test]
fn blinking_text_is_imported_and_the_note_says_it_was_lost() {
    // Nothing in a .raba blinks, so the only honest thing is to say so: a
    // decision made silently is one nobody can check.
    let art = parsed(b"\x1b[5mX");
    assert!(
        art.notes.iter().any(|note| note.contains("blink")),
        "blinking was dropped without a word: {:?}",
        art.notes
    );
}

#[test]
fn an_unknown_sgr_parameter_is_ignored_rather_than_refused() {
    // Art in the wild carries codes from editors nobody has any more. Dropping
    // one costs a colour; refusing the file costs the art.
    let art = parsed(b"\x1b[73mX\x1b[0m");
    assert_eq!(rows(&art), vec!["X"]);
}

#[test]
fn erase_to_the_end_of_the_screen_leaves_what_came_before_it() {
    let art = parsed(b"keep\r\ngone\x1b[1;5H\x1b[0J");
    assert_eq!(rows(&art).first().map(String::as_str), Some("keep"));
}

#[test]
fn erase_to_the_start_of_the_screen_clears_the_lines_above() {
    // 1J erases from the top of the screen down to the cursor, so the first
    // line goes and what is written afterwards stays.
    let art = parsed(b"gone\r\nxxxx\x1b[1Jkeep");
    let rows = rows(&art);
    assert_eq!(rows.first().map(String::as_str), Some(""), "the line above was not cleared");
    assert!(rows.iter().any(|row| row.contains("keep")), "{rows:?}");
}

#[test]
fn erase_the_whole_screen_leaves_nothing_before_it() {
    let art = parsed(b"gone\r\nalso gone\x1b[2Jkept");
    let rows = rows(&art);
    assert!(rows.iter().all(|row| !row.contains("gone")), "2J left something behind: {rows:?}");
}

#[test]
fn a_tab_moves_to_the_next_multiple_of_eight() {
    let art = parsed(b"a\tb");
    assert_eq!(rows(&art).first().map(String::as_str), Some("a       b"));
}

#[test]
fn art_with_nothing_on_it_is_refused_rather_than_becoming_an_empty_animation() {
    let outcome = ansi::parse(b"", &Options::default());
    assert!(matches!(outcome, Err(AnsiError::Empty)), "{outcome:?}");
}

#[test]
fn art_taller_than_the_format_allows_is_refused_by_height() {
    let mut bytes = Vec::new();
    for _ in 0..1100 {
        bytes.extend_from_slice(b"x\r\n");
    }
    let outcome = ansi::parse(&bytes, &Options::default());
    assert!(matches!(outcome, Err(AnsiError::TooTall { .. })), "{outcome:?}");
}

#[test]
fn every_reason_ansi_art_is_refused_says_what_the_limit_is() {
    let cases = [
        AnsiError::Empty,
        AnsiError::TooWide { columns: 2000 },
        AnsiError::TooTall { lines: 2000 },
    ];
    for error in cases {
        let said = error.to_string();
        assert!(said.len() > 10, "{error:?} says {said:?}, which is not a sentence");
        if !matches!(error, AnsiError::Empty) {
            assert!(said.contains("2000"), "{error:?} did not say what it got: {said}");
            assert!(
                said.contains("1024"),
                "{error:?} said what was wrong and not what is allowed: {said}"
            );
        }
    }
}

// --- the sequences real art uses that nothing had fed it ---------------------

#[test]
fn erase_in_line_clears_the_half_the_parameter_asks_for() {
    // 0 (or nothing) is cursor to end, 1 is start to cursor inclusive, 2 is the
    // whole line. BBS art uses all three to repaint a status line in place.
    assert_eq!(screen(b"abcdef\x1b[4G\x1b[K"), vec!["abc"]);
    // 1 erases up to and including the cursor's own column and leaves the
    // rest; the `z` then lands on that column, where the cursor still is.
    assert_eq!(screen(b"abcdef\x1b[4G\x1b[1Kz"), vec!["   zef"]);
    assert_eq!(screen(b"abcdef\x1b[2Kz"), vec!["      z"]);
}

#[test]
fn cursor_up_and_down_move_without_erasing_what_is_already_there() {
    // \x1b[A and \x1b[B, which is how an ANSImation redraws one changed line
    // without sending the rest of the screen again.
    assert_eq!(screen(b"one\ntwo\x1b[A\x1b[GX"), vec!["Xne", "two"]);
    assert_eq!(screen(b"one\x1b[B\x1b[Gtwo"), vec!["one", "two"]);
}

#[test]
fn cursor_next_and_previous_line_go_to_the_start_of_a_line_and_not_to_the_column() {
    // \x1b[E and \x1b[F differ from \x1b[B and \x1b[A in exactly that, which is
    // the difference between art that lands in column 0 and art that staircases.
    assert_eq!(screen(b"abc\x1b[Exyz"), vec!["abc", "xyz"]);
    assert_eq!(screen(b"abc\ndef\x1b[Fxyz"), vec!["xyz", "def"]);
}

#[test]
fn a_cursor_move_above_the_first_line_stops_there_rather_than_wrapping_around() {
    // Saturating, because art written for an 80x25 terminal is routinely
    // replayed somewhere taller and a wrap here would scatter it.
    assert_eq!(screen(b"top\x1b[99A\x1b[GX"), vec!["Xop"]);
    assert_eq!(screen(b"top\x1b[99F=="), vec!["==p"]);
}

#[test]
fn hiding_the_cursor_is_accepted_and_changes_nothing_that_gets_imported() {
    // \x1b[?25l and \x1b[?25h bracket almost every ANSImation. There is no
    // cursor in a .raba, so the right behaviour is to take them and carry on
    // rather than to let them reach the screen as text.
    assert_eq!(screen(b"\x1b[?25labc\x1b[?25h"), vec!["abc"]);
    assert_eq!(screen(b"ab\x1b[25mc"), vec!["abc"]);
}

#[test]
fn save_and_restore_put_the_cursor_back_where_it_was() {
    assert_eq!(screen(b"ab\x1b[s\ncd\x1b[uZ"), vec!["abZ", "cd"]);
}

#[test]
fn an_sgr_sequence_with_no_parameters_resets_the_colours() {
    // `\x1b[m` is `\x1b[0m` written short, and art does write it short.
    let imported = parsed(b"\x1b[31mred\x1b[mplain");
    assert_eq!(colour_at(&imported, 0, 0).0, 1);
    assert_eq!(colour_at(&imported, 3, 0), (7, 0));
}

#[test]
fn an_sgr_parameter_this_importer_has_no_use_for_is_stepped_over() {
    // 53 is overlined. Skipping it must not swallow the parameter after it,
    // which is the bug this shape of loop invites.
    let imported = parsed(b"\x1b[53;31mred");
    assert_eq!(colour_at(&imported, 0, 0).0, 1);
}

#[test]
fn a_truncated_extended_colour_sequence_does_not_eat_the_rest_of_the_line() {
    // `38;` with nothing after it is malformed. The importer steps over one
    // parameter and keeps going, rather than consuming the colour that follows.
    let imported = parsed(b"\x1b[38m\x1b[41mx");
    assert_eq!(colour_at(&imported, 0, 0).1, 1);
}

#[test]
fn a_csi_sequence_this_importer_does_not_implement_is_skipped_rather_than_printed() {
    // \x1b[2L inserts lines. Not supported, and the one thing that must not
    // happen is `2L` appearing in the art as two characters.
    assert_eq!(screen(b"ab\x1b[2Lcd"), vec!["abcd"]);
}

#[test]
fn a_file_that_is_only_blank_lines_is_empty_rather_than_a_canvas_of_spaces() {
    // Trailing blank rows are an artefact of the file ending in newlines, and
    // stripping them to nothing means there was no art.
    let why = ansi::parse(b"\n\n\n   \n", &Options::default()).expect_err("nothing to import");
    assert!(matches!(why, AnsiError::Empty), "{why:?}");
}

#[test]
fn blank_rows_at_the_end_are_dropped_and_blank_rows_in_the_middle_are_not() {
    assert_eq!(screen(b"top\n\n\nbottom\n\n\n\n"), vec!["top", "", "", "bottom"]);
}

#[test]
fn a_sauce_record_that_says_eighty_columns_says_nothing_new() {
    // The notes are what a person has to read before trusting the import, so
    // every line in there has to be worth reading. Eighty is the width this
    // importer already assumes; printing "SAUCE says the art is 80 columns
    // wide" trains people to skip the list that also says "blinking was
    // dropped".
    let mut bytes = b"art\x1a".to_vec();
    let mut sauce = vec![0u8; 128];
    sauce[0..7].copy_from_slice(b"SAUCE00");
    sauce[96] = 80; // TInfo1: the default width, stated outright
    bytes.extend_from_slice(&sauce);

    let imported = ansi::parse(&bytes, &Options::default()).expect("should import");
    assert_eq!(imported.movie.columns, 80);
    assert!(
        !imported.notes.iter().any(|note| note.contains("columns wide")),
        "{:?}: the default width is not news",
        imported.notes
    );
}

#[test]
fn a_sauce_record_that_claims_a_comment_block_that_is_not_there_keeps_the_art() {
    // The comment block is found by counting back from the SAUCE record, so a
    // file whose comment count is wrong — truncated, or written by a tool that
    // miscounted — points that search into the middle of the drawing. Nothing
    // may be stripped unless the "COMNT" marker is actually at the spot.
    let mut bytes = b"the art itself, sixty-four characters of it or thereabouts".to_vec();
    bytes.resize(80, b'#');
    bytes.push(0x1a);
    let mut sauce = vec![0u8; 128];
    sauce[0..7].copy_from_slice(b"SAUCE00");
    sauce[104] = 1; // one comment line, which this file does not actually have
    bytes.extend_from_slice(&sauce);

    let imported = ansi::parse(&bytes, &Options::default()).expect("should import");
    assert!(rows(&imported)[0].starts_with("the art itself"), "{:?}", rows(&imported));
    assert!(rows(&imported)[0].ends_with('#'), "{:?}: art was eaten", rows(&imported));
}

#[test]
fn an_sgr_with_no_parameters_is_a_reset() {
    // `ESC[m` is how most art turns colour off, and it is the same as
    // `ESC[0m`. Reading it as "no parameters, nothing to do" leaves the rest
    // of the drawing in whatever colour the last sequence set.
    let imported = parsed(b"\x1b[31;44mred\x1b[mplain");
    assert_eq!(colour_at(&imported, 0, 0), (1, 4), "the colour it did ask for");
    assert_eq!(colour_at(&imported, 3, 0), (7, 0), "and back to the default");
}
