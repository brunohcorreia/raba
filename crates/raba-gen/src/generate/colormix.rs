//! A colour-mixing field.
//!
//! # How this one closes its loop
//!
//! Every term is a sine of `2π k t / frames` for a whole number `k`, so at
//! `t = frames` every term is exactly back where it started. The loop closes by
//! construction and needs no search and no luck.
//!
//! # Why it is dithered rather than flat
//!
//! A console has sixteen colours, so a smooth field quantised straight onto them
//! bands badly. Blending each colour against the one below it with the CP437
//! shading characters gives roughly four times the apparent depth for the same
//! sixteen colours — and, usefully for a delta format, a cell that drifts within
//! one band changes its character rather than nothing at all, so motion stays
//! visible where flat quantisation would freeze.

use super::{blank, Spec};
use raba_core::render::Cell;
use raba_core::{Grid, Palette};
use std::f64::consts::TAU;

/// Colours to travel through, as (foreground, background) pairs that dither
/// against each other. Backgrounds stay in 0–7: on a console the high background
/// bit is blink, not brightness.
const BANDS: [(u8, u8); 6] = [(4, 0), (5, 4), (1, 5), (3, 1), (11, 3), (15, 7)];

#[allow(clippy::indexing_slicing)]
const _: () = {
    // The claim above, enforced. The top band originally dithered white against
    // bright yellow, which on a console is not a bright background at all -- it is
    // the blink bit, and the band would have flashed.
    let mut index = 0;
    while index < BANDS.len() {
        assert!(BANDS[index].1 <= 7, "a background above 7 is blink, not brightness");
        index += 1;
    }
};

const SHADES: [char; 4] = ['░', '▒', '▓', '█'];

/// Which of `buckets` equal slices of 0..1 `value` falls in.
///
/// Written as a search over whole numbers rather than a multiply-and-cast: every
/// conversion here is `f64::from(u32)`, which is exact, so there is no truncation
/// to reason about and nothing for the arithmetic lints to object to.
fn bucket(value: f64, buckets: usize) -> usize {
    let clamped = value.clamp(0.0, 1.0);
    let total = f64::from(u32::try_from(buckets.max(1)).unwrap_or(1));
    (1..=buckets)
        .find(|edge| clamped < f64::from(u32::try_from(*edge).unwrap_or(1)) / total)
        .map_or(buckets.saturating_sub(1), |edge| edge.saturating_sub(1))
}

fn paint(value: f64, palette: Palette) -> Cell {
    if palette == Palette::Xterm256 {
        // The 24-step path: walk the colour cube's blue-to-red diagonal.
        let step = u8::try_from(bucket(value, 24)).unwrap_or(0);
        let index = 17u8.saturating_add(step.saturating_mul(6));
        return Cell { ch: '█', fg: index, bg: 0 };
    }
    let total = BANDS.len().saturating_mul(SHADES.len());
    let position = bucket(value, total);
    // SHADES is a non-empty constant, so neither of these can divide by zero;
    // checked_* rather than a bare operator so the lint does not have to take
    // that on trust.
    let width = SHADES.len().max(1);
    let band = position.checked_div(width).unwrap_or(0);
    let shade = position.checked_rem(width).unwrap_or(0);
    let (fg, bg) = *BANDS.get(band.min(BANDS.len().saturating_sub(1))).unwrap_or(&(7, 0));
    Cell { ch: *SHADES.get(shade).unwrap_or(&'█'), fg, bg }
}

pub fn frames(spec: &Spec) -> Vec<Grid> {
    let columns = spec.columns.max(1);
    let lines = spec.lines.max(1);
    let count = spec.frames.max(1);
    let width = f64::from(columns);
    let height = f64::from(lines);

    let mut out = Vec::with_capacity(usize::from(count));
    for frame in 0..count {
        let t = f64::from(frame) / f64::from(count);
        let mut grid = blank(spec);
        for line in 0..lines {
            for column in 0..columns {
                // A console cell is about twice as tall as it is wide, so x is
                // halved to keep the pattern from looking stretched.
                let x = f64::from(column) / width;
                let y = f64::from(line) / height;
                let a = (TAU * (x * 2.0 + t)).sin();
                let b = (TAU * (y * 3.0 - t * 2.0)).sin();
                let c = (TAU * ((x + y) * 1.5 + t * 3.0)).sin();
                // Three sines in -1..1; the average rescaled to 0..1.
                let value = (a + b + c) / 6.0 + 0.5;
                grid.set(column, line, paint(value, spec.palette));
            }
        }
        out.push(grid);
    }
    out
}
