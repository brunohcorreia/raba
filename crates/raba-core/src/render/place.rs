//! Fitting a canvas to a screen that is not the same size.
//!
//! Art is authored at one size and played on whatever the machine has. A logo is
//! usually smaller than the console and wants centring; a full-screen animation is
//! often larger and has to be cropped. Both are the same operation with a
//! different sign, and both are pure arithmetic, which is why they live here
//! rather than in the surface that happens to know the real size.
//!
//! Cropping loses art. That is not a failure to report — a splash that draws the
//! middle of a picture is better than one that scribbles past the last column and
//! leaves the console scrolled — but `raba-gen` warns at build time, where
//! somebody can still change the canvas.

use super::{Cell, Grid};
use crate::surface::Size;

/// Where the source canvas sits relative to the screen, in signed screen
/// coordinates. Negative means the canvas starts off-screen and is cropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub column: i32,
    pub line: i32,
}

impl Placement {
    /// Centre `canvas` on `screen`, rounding a leftover column or line towards the
    /// top left. Which way it rounds matters less than that it always rounds the
    /// same way, so a frame does not shift by one against the frame before it.
    #[must_use]
    pub fn centred(canvas: Size, screen: Size) -> Placement {
        let offset = |canvas: u16, screen: u16| -> i32 {
            (i32::from(screen).saturating_sub(i32::from(canvas))) / 2
        };
        Placement {
            column: offset(canvas.columns, screen.columns),
            line: offset(canvas.lines, screen.lines),
        }
    }
}

/// Draw `source` onto a grid of exactly `size`, centred, cropped where it does not
/// fit and padded with blanks where it does not reach.
///
/// The returned grid always has `size`, so a caller can hand it straight to the
/// encoder without checking anything.
#[must_use]
pub fn fit(source: &Grid, size: Size) -> Grid {
    let canvas = Size { columns: source.columns(), lines: source.lines() };
    if canvas == size {
        return source.clone();
    }
    let Some(mut screen) = Grid::new(size.columns, size.lines, source.palette()) else {
        return source.clone();
    };
    let at = Placement::centred(canvas, size);

    for line in 0..source.lines() {
        let target_line = at.line.saturating_add(i32::from(line));
        let Ok(target_line) = u16::try_from(target_line) else {
            continue;
        };
        if target_line >= size.lines {
            continue;
        }
        for column in 0..source.columns() {
            let target_column = at.column.saturating_add(i32::from(column));
            let Ok(target_column) = u16::try_from(target_column) else {
                continue;
            };
            if target_column >= size.columns {
                continue;
            }
            if let Some(cell) = source.get(column, line) {
                screen.set(target_column, target_line, *cell);
            }
        }
    }
    screen
}

/// Whether `canvas` would lose anything on a screen of `size`.
#[must_use]
pub fn crops(canvas: Size, size: Size) -> bool {
    canvas.columns > size.columns || canvas.lines > size.lines
}

/// The blank a cropped or padded screen is filled with.
#[must_use]
pub const fn padding() -> Cell {
    Cell::BLANK
}
