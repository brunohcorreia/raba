//! CMatrix rain.
//!
//! # How this one closes its loop
//!
//! Each column holds one falling drop whose head moves down one row per frame and
//! wraps after `cycle` rows, where `cycle` is longer than the screen so there is a
//! gap between drops. Pick every `cycle` from the **divisors of the frame count**
//! and every column returns to its starting row on the last frame, so the loop is
//! exact by construction rather than by luck.
//!
//! That is also why the frame count matters more than it looks: 60 frames on a
//! 25-line screen has only two usable divisors, so only two speeds; 120 has four.
//!
//! # Glyphs
//!
//! CMatrix draws half-width katakana. A Linux console has a few hundred glyphs
//! before `setfont` runs and none of them are katakana, so this uses the ASCII
//! set that still reads as falling code. Each cell's glyph is chosen once rather
//! than reshuffled every frame: a glyph that changes costs an op even when the
//! colour does not, and the flicker is not worth the file size.

use super::{blank, Rng, Spec};
use raba_core::render::Cell;
use raba_core::Grid;

const GLYPHS: &[u8] = b"01234567890ABCDEFGHIJKLMNOPQRSTUVWXYZ<>|/\\=+*-:;.#$%&@";

/// Divisors of `frames` that are long enough for a drop to clear the screen.
fn cycles(frames: u16, lines: u16) -> Vec<u16> {
    let shortest = lines.saturating_add(1);
    let mut out: Vec<u16> =
        (shortest..=frames).filter(|candidate| frames.checked_rem(*candidate) == Some(0)).collect();
    if out.is_empty() {
        // Too few frames for any divisor to clear the screen. One cycle per loop
        // still closes correctly; it is just the only speed available.
        out.push(frames.max(1));
    }
    out
}

struct Column {
    cycle: u16,
    phase: u16,
    trail: u16,
}

pub fn frames(spec: &Spec) -> Vec<Grid> {
    let columns = spec.columns.max(1);
    let lines = spec.lines.max(1);
    let frame_count = spec.frames.max(1);
    let mut rng = Rng::new(spec.seed);

    let choices = cycles(frame_count, lines);
    let drops: Vec<Column> = (0..columns)
        .map(|_| {
            let cycle = choices
                .get(usize::try_from(rng.below(u32::try_from(choices.len()).unwrap_or(1))).unwrap_or(0))
                .copied()
                .unwrap_or(frame_count);
            Column {
                cycle,
                phase: u16::try_from(rng.below(u32::from(cycle))).unwrap_or(0),
                // A trail between a third and all of the screen height.
                trail: u16::try_from(rng.below(u32::from(lines.saturating_mul(2) / 3)))
                    .unwrap_or(0)
                    .saturating_add(lines / 3)
                    .max(2),
            }
        })
        .collect();

    // One glyph per cell, fixed for the whole animation. See the note above.
    let glyphs: Vec<char> = (0..usize::from(columns).saturating_mul(usize::from(lines)))
        .map(|_| {
            let index = usize::try_from(rng.below(u32::try_from(GLYPHS.len()).unwrap_or(1))).unwrap_or(0);
            char::from(GLYPHS.get(index).copied().unwrap_or(b'0'))
        })
        .collect();

    let mut out = Vec::with_capacity(usize::from(frame_count));
    for frame in 0..frame_count {
        let mut grid = blank(spec);
        for column in 0..columns {
            let Some(drop) = drops.get(usize::from(column)) else { continue };
            // cycle is at least 1 by construction in `cycles`.
            let head =
                i32::from(frame.wrapping_add(drop.phase).checked_rem(drop.cycle).unwrap_or(0));
            for line in 0..lines {
                // How far this cell is behind the head.
                let behind = head.saturating_sub(i32::from(line));
                if behind < 0 || behind >= i32::from(drop.trail) {
                    continue;
                }
                let fg = match behind {
                    0 => 15,                                        // the head, white
                    1..=2 => 10,                                    // just behind it, bright
                    n if n < i32::from(drop.trail) / 2 => 2,        // the body, green
                    _ => 2,
                };
                // The tail thins out rather than stopping dead.
                let faded = behind >= i32::from(drop.trail).saturating_mul(3) / 4;
                let index = usize::from(line).saturating_mul(usize::from(columns)).saturating_add(usize::from(column));
                let ch = glyphs.get(index).copied().unwrap_or('0');
                grid.set(column, line, Cell { ch: if faded { '.' } else { ch }, fg, bg: 0 });
            }
        }
        out.push(grid);
    }
    out
}
