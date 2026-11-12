//! The renderer, checked against a model of a terminal rather than against itself.
//!
//! Asserting on exact bytes is done only where the byte sequence *is* the claim —
//! "one cursor move", "no colour re-emitted". Everything about correctness goes
//! through `terminal`, because a golden-bytes test passes just as happily when the
//! renderer is consistently wrong.

// These lints exist for the boot path, not for the harness. A test that cannot
// index an array is a test nobody writes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

mod terminal;

use raba_core::render::{to_ansi, to_ansi_into};
use raba_core::surface::{BufferSurface, Surface};
use raba_core::{Cell, Grid, Palette};
use terminal::Terminal;

fn grid(columns: u16, lines: u16) -> Grid {
    Grid::new(columns, lines, Palette::Ansi16).unwrap()
}

/// Count escapes ending in `final_byte`, by parsing.
///
/// Substring counting looks equivalent and is not: `[3` occurs in a foreground
/// colour *and* in a move to line 3, and a glyph may be `H` or `m`.
fn escapes(bytes: &[u8], final_byte: char) -> usize {
    let text = std::str::from_utf8(bytes).unwrap();
    let mut chars = text.chars();
    let mut found = 0;
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            continue;
        }
        assert_eq!(chars.next(), Some('['));
        for next in chars.by_ref() {
            if !matches!(next, '0'..='9' | ';') {
                if next == final_byte {
                    found += 1;
                }
                break;
            }
        }
    }
    found
}

fn cursor_moves(bytes: &[u8]) -> usize {
    escapes(bytes, 'H')
}

fn colour_changes(bytes: &[u8]) -> usize {
    escapes(bytes, 'm')
}

#[test]
fn nothing_changed_means_nothing_is_written() {
    let screen = grid(10, 4);
    assert!(to_ansi(&screen, Some(&screen)).is_empty());
}

#[test]
fn a_contiguous_run_costs_one_cursor_move() {
    let mut screen = grid(2, 1);
    screen.set(0, 0, Cell::BLANK);
    // Two adjacent cells, same colours: one move, one colour change, two glyphs.
    assert_eq!(to_ansi(&screen, None), b"\x1b[1;1H\x1b[37;40m  \x1b[0m");
}

#[test]
fn one_changed_cell_draws_one_cell() {
    let before = grid(3, 2);
    let mut after = before.clone();
    after.set(1, 0, Cell { ch: 'X', fg: 7, bg: 0 });

    let out = to_ansi(&after, Some(&before));
    assert_eq!(out, b"\x1b[1;2H\x1b[37;40mX\x1b[0m");
}

#[test]
fn scattered_cells_each_cost_a_move_and_nothing_else() {
    let before = grid(8, 3);
    let mut after = before.clone();
    for (column, line) in [(0u16, 0u16), (4, 1), (7, 2)] {
        after.set(column, line, Cell { ch: '*', fg: 7, bg: 0 });
    }

    let out = to_ansi(&after, Some(&before));
    assert_eq!(cursor_moves(&out), 3);
    // One SGR for the first cell and one reset at the end: colour is tracked across
    // cursor moves, so the two later cells cost a move and a glyph and nothing more.
    assert_eq!(colour_changes(&out), 2);
    assert_eq!(Terminal::showing(&before).tap(&out), after);
}

#[test]
fn colour_is_re_emitted_only_when_it_changes() {
    let before = grid(4, 1);
    let mut after = before.clone();
    after.set(0, 0, Cell { ch: 'a', fg: 1, bg: 0 });
    after.set(1, 0, Cell { ch: 'b', fg: 1, bg: 0 });
    after.set(2, 0, Cell { ch: 'c', fg: 2, bg: 0 });
    after.set(3, 0, Cell { ch: 'd', fg: 2, bg: 0 });

    // Background set once, foreground twice. `abcd` with two SGRs between them.
    assert_eq!(to_ansi(&after, Some(&before)), b"\x1b[1;1H\x1b[31;40mab\x1b[32mcd\x1b[0m");
}

#[test]
fn bright_colours_use_the_aixterm_range() {
    let before = grid(2, 1);
    let mut after = before.clone();
    after.set(0, 0, Cell { ch: 'x', fg: 15, bg: 9 });
    assert_eq!(to_ansi(&after, Some(&before)), b"\x1b[1;1H\x1b[97;101mx\x1b[0m");
}

#[test]
fn the_256_colour_palette_uses_the_extended_form() {
    let before = Grid::new(2, 1, Palette::Xterm256).unwrap();
    let mut after = before.clone();
    after.set(0, 0, Cell { ch: 'x', fg: 208, bg: 236 });
    assert_eq!(to_ansi(&after, Some(&before)), b"\x1b[1;1H\x1b[38;5;208;48;5;236mx\x1b[0m");
}

#[test]
fn the_last_column_forces_an_absolute_move() {
    // Two full rows. Without the forced move after the last column, the renderer
    // would be betting on how this terminal handles deferred wrap.
    let mut after = grid(3, 2);
    for line in 0..2u16 {
        for column in 0..3u16 {
            after.set(column, line, Cell { ch: 'o', fg: 7, bg: 0 });
        }
    }
    let out = to_ansi(&after, None);
    assert_eq!(cursor_moves(&out), 2, "one move per row, not one per cell");
    assert_eq!(Terminal::blank(3, 2, Palette::Ansi16).tap(&out), after);
}

#[test]
fn control_characters_never_reach_the_terminal() {
    let before = grid(6, 1);
    let mut after = before.clone();
    // A file that did not come from raba-gen. ESC would start a sequence; 0x9b is a
    // single-byte CSI on a terminal in 8-bit mode, which filtering ESC alone misses.
    for (column, ch) in ['\u{1b}', '\u{9b}', '\0', '\u{7f}', '\n', '\r'].iter().enumerate() {
        after.set(column as u16, 0, Cell { ch: *ch, fg: 7, bg: 0 });
    }

    let out = to_ansi(&after, Some(&before));
    // The terminal model panics on any control character, so arriving here is the
    // assertion. What lands on screen is six spaces.
    let screen = Terminal::showing(&before).tap(&out);
    for column in 0..6u16 {
        assert_eq!(screen.get(column, 0).unwrap().ch, ' ');
    }
}

#[test]
fn a_resize_forces_a_full_redraw() {
    let before = grid(4, 1);
    let after = grid(8, 1);
    // Drawing the difference between two different-sized screens is meaningless, so
    // the renderer must ignore `prev` rather than compare against it cell by cell.
    assert_eq!(to_ansi(&after, Some(&before)), to_ansi(&after, None));
}

#[test]
fn a_palette_change_forces_a_full_redraw() {
    let before = Grid::new(4, 1, Palette::Ansi16).unwrap();
    let after = Grid::new(4, 1, Palette::Xterm256).unwrap();
    assert_eq!(to_ansi(&after, Some(&before)), to_ansi(&after, None));
}

#[test]
fn a_frame_leaves_no_colour_in_effect() {
    let before = grid(4, 1);
    let mut after = before.clone();
    after.set(0, 0, Cell { ch: 'x', fg: 3, bg: 4 });
    assert!(to_ansi(&after, Some(&before)).ends_with(b"\x1b[0m"));
}

#[test]
fn an_animation_is_one_present_per_frame() {
    let mut surface = BufferSurface::new(10, 3);
    let mut screen = grid(10, 3);
    let mut buffer = Vec::new();
    let mut shown = grid(10, 3);

    for step in 0..4u16 {
        screen.set(step, 0, Cell { ch: '#', fg: 7, bg: 0 });
        to_ansi_into(&mut buffer, &screen, Some(&shown));
        surface.present(&buffer).unwrap();
        shown = screen.clone();
    }

    assert_eq!(surface.frames().len(), 4, "four frames, four writes");
    // And the whole transcript, replayed, is the final state.
    assert_eq!(Terminal::blank(10, 3, Palette::Ansi16).tap(&surface.transcript()), screen);
}

#[test]
fn playback_survives_a_failed_write() {
    let mut surface = BufferSurface::new(4, 1);
    surface.fail_next_present();
    assert!(surface.present(b"x").is_err());
    assert!(surface.present(b"y").is_ok(), "one bad write is not the end");
    assert_eq!(surface.frames().len(), 1);
}

#[test]
fn restoring_twice_is_allowed() {
    let mut surface = BufferSurface::new(4, 1);
    surface.restore().unwrap();
    surface.restore().unwrap();
    assert_eq!(surface.restores(), 2);
}

#[test]
fn reusing_the_buffer_gives_the_same_bytes() {
    let before = grid(6, 2);
    let mut after = before.clone();
    after.set(2, 1, Cell { ch: 'z', fg: 5, bg: 0 });

    let mut buffer = vec![0xff; 4096];
    to_ansi_into(&mut buffer, &after, Some(&before));
    assert_eq!(buffer, to_ansi(&after, Some(&before)));
}

/// `Terminal::apply` followed by `into_screen`, which every test above wants.
trait Tap {
    fn tap(self, bytes: &[u8]) -> Grid;
}

impl Tap for Terminal {
    fn tap(mut self, bytes: &[u8]) -> Grid {
        self.apply(bytes);
        self.into_screen()
    }
}
