//! Properties of the renderer.
//!
//! The one that matters is `redrawing_prev_produces_grid`. Redraw minimisation is
//! easy to get subtly wrong — an off-by-one in cursor tracking, a colour assumed to
//! still be in effect — and wrong in a way that only shows on the one frame nobody
//! wrote a test for. Stated as a property over arbitrary pairs of grids, the question
//! "did we skip a cell we should have drawn?" is answered by construction.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

mod terminal;

use proptest::prelude::*;
use raba_core::render::to_ansi;
use raba_core::{Cell, Grid, Palette};
use terminal::Terminal;

/// Glyphs a theme plausibly uses: ASCII, the CP437 blocks, box drawing, and one
/// multi-byte character to keep the UTF-8 path honest.
const GLYPHS: &[char] =
    &[' ', '.', '-', '#', '@', 'H', 'm', '[', '█', '▀', '▄', '░', '▒', '│', '─', '┌', 'é'];

fn palette() -> impl Strategy<Value = Palette> {
    prop_oneof![Just(Palette::Ansi16), Just(Palette::Xterm256)]
}

/// `count` grids that share a shape and palette — successive states of one screen.
///
/// They must share a shape: rendering the difference between two differently shaped
/// screens is a full redraw by definition, so generating mismatched pairs would test
/// the fallback over and over and the interesting path almost never.
fn screens(count: usize) -> impl Strategy<Value = Vec<Grid>> {
    (1u16..=16, 1u16..=8, palette()).prop_flat_map(move |(columns, lines, palette)| {
        let cells = usize::from(columns) * usize::from(lines);
        // Colours stay inside the palette, which is an invariant the decoder
        // enforces on every file. A grid that violated it could not have come from
        // a `.raba`.
        let top = palette.max_index();
        let cell = (0..GLYPHS.len(), 0..=top, 0..=top).prop_map(|(glyph, fg, bg)| Cell {
            ch: GLYPHS[glyph],
            fg,
            bg,
        });
        prop::collection::vec(prop::collection::vec(cell, cells), count).prop_map(move |states| {
            states
                .into_iter()
                .map(|cells| {
                    let mut grid = Grid::new(columns, lines, palette).unwrap();
                    for (index, cell) in cells.into_iter().enumerate() {
                        grid.set_index(index, cell);
                    }
                    grid
                })
                .collect()
        })
    })
}

proptest! {
    /// The central claim. Everything else the renderer does is an optimisation of it.
    #[test]
    fn redrawing_prev_produces_grid(states in screens(2)) {
        let (before, after) = (&states[0], &states[1]);
        let mut terminal = Terminal::showing(before);
        terminal.apply(&to_ansi(after, Some(before)));
        prop_assert_eq!(&terminal.into_screen(), after);
    }

    /// And without a previous screen, from blank.
    #[test]
    fn a_full_redraw_produces_grid(states in screens(1)) {
        let after = &states[0];
        let mut terminal = Terminal::blank(after.columns(), after.lines(), after.palette());
        terminal.apply(&to_ansi(after, None));
        prop_assert_eq!(&terminal.into_screen(), after);
    }

    /// Minimisation is not merely correct, it actually minimises: a frame that
    /// changes nothing writes nothing, so a static logo costs no bytes at all.
    #[test]
    fn an_identical_frame_is_free(states in screens(1)) {
        prop_assert!(to_ansi(&states[0], Some(&states[0])).is_empty());
    }

    /// Output is deterministic, which is what lets golden render tests exist in M1.
    #[test]
    fn rendering_is_deterministic(states in screens(2)) {
        let (before, after) = (&states[0], &states[1]);
        prop_assert_eq!(to_ansi(after, Some(before)), to_ansi(after, Some(before)));
    }

    /// Playing two frames in sequence must land where drawing the second directly
    /// lands. This is what makes delta frames safe to chain: if it failed, an
    /// animation would drift further from the truth the longer it ran.
    #[test]
    fn replaying_frames_equals_drawing_the_last(states in screens(3)) {
        let (before, middle, after) = (&states[0], &states[1], &states[2]);

        let mut stepwise = Terminal::showing(before);
        stepwise.apply(&to_ansi(middle, Some(before)));
        stepwise.apply(&to_ansi(after, Some(middle)));

        let mut direct = Terminal::showing(before);
        direct.apply(&to_ansi(after, Some(before)));

        prop_assert_eq!(stepwise.into_screen(), direct.into_screen());
    }

    /// Nothing but ESC-introduced sequences and printable text leaves this module,
    /// whatever the input. `Terminal::apply` panics on the rest, so the assertion is
    /// that it returns.
    ///
    /// The grid is corrupted on purpose first: the decoder rejects control
    /// characters, so reaching the renderer with one means the decoder was bypassed
    /// or wrong, and this is the layer that has to hold anyway.
    #[test]
    fn no_control_character_escapes(
        states in screens(2),
        intruders in prop::collection::vec((0usize..64, 0u32..=0x9f), 0..8),
    ) {
        let before = &states[0];
        let mut after = states[1].clone();
        for (position, codepoint) in intruders {
            if let Some(ch) = char::from_u32(codepoint) {
                after.set_index(position % after.cell_count(), Cell { ch, fg: 7, bg: 0 });
            }
        }
        let mut terminal = Terminal::showing(before);
        terminal.apply(&to_ansi(&after, Some(before)));
    }
}
