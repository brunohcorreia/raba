//! PSX DOOM fire.
//!
//! The classic algorithm: the bottom row is held at full heat, and every cell
//! above takes its heat from the cell below, minus a random decay, with a random
//! sideways drift. Everything that makes it look like fire comes from those two
//! random numbers.
//!
//! # This one does not loop seamlessly
//!
//! Fire is chaotic: the state never returns to where it was, so there is no frame
//! count at which the loop closes. The seam is left in rather than hidden, because
//! the alternatives are worse — forcing the state back would be a visible jump in
//! the same place every time, and a crossfade needs blending a console cannot do.
//! In practice fire has no persistent structure for an eye to track, so the seam
//! reads as one more flicker. `Kind::loops_seamlessly` reports this, and
//! `raba-gen generate` prints it.

use super::{blank, Rng, Spec};
use raba_core::render::Cell;
use raba_core::Grid;

/// The heat ramp, coolest first.
///
/// The jump from one colour to the next is dithered with the CP437 block
/// elements against the colour below, which is why each entry carries a
/// background as well: a cell of `▒` in bright yellow on red reads as the colour
/// between them. Backgrounds stay in 0–7, since a console's high background bit
/// is blink rather than brightness.
const RAMP: [(char, u8, u8); 17] = [
    (' ', 0, 0),    // cold
    ('░', 1, 0),
    ('▒', 1, 0),
    ('▓', 1, 0),
    ('█', 1, 0),    // red
    ('░', 9, 1),
    ('▒', 9, 1),
    ('▓', 9, 1),
    ('█', 9, 1),    // bright red
    ('░', 11, 1),
    ('▒', 11, 1),
    ('▓', 11, 3),
    ('█', 11, 3),   // bright yellow
    ('░', 15, 3),
    ('▒', 15, 3),
    ('▓', 15, 3),
    ('█', 15, 0),   // white hot
];

fn paint(heat: u8) -> Cell {
    let top = RAMP.len().saturating_sub(1);
    let index = usize::from(heat).min(top);
    let (ch, fg, bg) = *RAMP.get(index).unwrap_or(&(' ', 0, 0));
    Cell { ch, fg, bg }
}

pub fn frames(spec: &Spec) -> Vec<Grid> {
    let columns = usize::from(spec.columns.max(1));
    let lines = usize::from(spec.lines.max(1));
    let hottest = u8::try_from(RAMP.len().saturating_sub(1)).unwrap_or(16);
    let mut rng = Rng::new(spec.seed);

    // Heat, row-major, with row 0 at the top. The bottom row is the source.
    let mut heat = vec![0u8; columns.saturating_mul(lines)];
    for column in 0..columns {
        if let Some(cell) = heat.get_mut(columns.saturating_mul(lines.saturating_sub(1)).saturating_add(column)) {
            *cell = hottest;
        }
    }

    let step = |heat: &mut Vec<u8>, rng: &mut Rng| {
        for line in (1..lines).rev() {
            for column in 0..columns {
                let below = heat
                    .get(line.saturating_mul(columns).saturating_add(column))
                    .copied()
                    .unwrap_or(0);
                // Decay by 0 or 1, and drift sideways by -1, 0 or 1. Without the
                // drift the flames rise in straight columns and look like a bar
                // chart.
                let decay = u8::try_from(rng.below(3) / 2).unwrap_or(0);
                let drift = i32::try_from(rng.below(3)).unwrap_or(1).saturating_sub(1);
                let target = i32::try_from(column).unwrap_or(0).saturating_add(drift);
                let target = target.rem_euclid(i32::try_from(columns).unwrap_or(1));
                let target = usize::try_from(target).unwrap_or(0);
                let value = below.saturating_sub(decay);
                if let Some(cell) =
                    heat.get_mut(line.saturating_sub(1).saturating_mul(columns).saturating_add(target))
                {
                    *cell = value;
                }
            }
        }
    };

    // Burn in, so the first frame is a fire rather than a bar of heat climbing a
    // black screen.
    for _ in 0..lines.saturating_mul(2) {
        step(&mut heat, &mut rng);
    }

    let mut out = Vec::with_capacity(usize::from(spec.frames));
    for _ in 0..spec.frames.max(1) {
        step(&mut heat, &mut rng);
        let mut grid = blank(spec);
        for line in 0..lines {
            for column in 0..columns {
                let value =
                    heat.get(line.saturating_mul(columns).saturating_add(column)).copied().unwrap_or(0);
                if let (Ok(c), Ok(l)) = (u16::try_from(column), u16::try_from(line)) {
                    grid.set(c, l, paint(value));
                }
            }
        }
        out.push(grid);
    }
    out
}
