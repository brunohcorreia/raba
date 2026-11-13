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
use crate::format::{Movie, Scaling};
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

/// The shading ramp, in order of increasing coverage.
///
/// Art built from these characters is not really text: it is a greyscale image
/// written in a coarse alphabet, which is what makes it **resamplable**. A logo
/// drawn with box-drawing characters is not, because halving a line does not give
/// you half a line.
///
/// `·` is U+00B7, which `glyph` rates unreliable rather than safe. It is on the
/// ramp because real art uses it — `/etc/ly/blackhole.dur` is full of it — and
/// recognising it costs nothing: a theme that contains it is flagged at build
/// time either way.
pub const RAMP: [char; 7] = [' ', '.', '\u{b7}', '░', '▒', '▓', '█'];

/// Where `ch` sits on [`RAMP`], or `None` if it is not on it.
#[must_use]
pub fn intensity(ch: char) -> Option<u8> {
    RAMP.iter().position(|candidate| *candidate == ch).and_then(|index| u8::try_from(index).ok())
}

/// Whether every one of these characters is on the shading ramp.
///
/// Decided once per movie rather than per frame: a picture that resampled on some
/// frames and was cropped on others would jump.
pub fn all_shading(chars: impl Iterator<Item = char>) -> bool {
    let mut any = false;
    for ch in chars {
        if intensity(ch).is_none() {
            return false;
        }
        any = true;
    }
    any
}

/// The largest size that fits inside `target` with `source`'s proportions.
///
/// Proportions are kept in **cells**, not pixels. Both ends of this are console
/// grids with roughly the same cell shape, so matching cell counts matches what a
/// person sees; converting to pixels would need the font size, which is exactly
/// the thing [`fit`] exists to avoid needing to know.
#[must_use]
pub fn scaled_size(source: Size, target: Size) -> Size {
    let (sc, sl) = (u32::from(source.columns.max(1)), u32::from(source.lines.max(1)));
    let (tc, tl) = (u32::from(target.columns.max(1)), u32::from(target.lines.max(1)));

    // Widest that fits, and tallest that fits; take whichever stays inside both.
    let by_width = (tc, sl.saturating_mul(tc).checked_div(sc).unwrap_or(1).max(1));
    let by_height = (sc.saturating_mul(tl).checked_div(sl).unwrap_or(1).max(1), tl);
    let (columns, lines) = if by_width.1 <= tl { by_width } else { by_height };

    Size {
        columns: u16::try_from(columns.min(tc)).unwrap_or(target.columns).max(1),
        lines: u16::try_from(lines.min(tl)).unwrap_or(target.lines).max(1),
    }
}

/// Resample shading art to `size`, averaging coverage and keeping the colour of
/// the brightest cell in each block.
///
/// Averaging the colours instead would wash the picture out: a bright core
/// surrounded by dark sky averages to something dim and the structure dissolves.
/// Taking the brightest contributor keeps edges where the art put them.
///
/// Only meaningful on art [`all_shading`] accepts; on anything else it will
/// happily produce nonsense, which is why [`best_fit`] is the entry point.
#[must_use]
pub fn resample(source: &Grid, size: Size) -> Grid {
    let Some(mut out) = Grid::new(size.columns.max(1), size.lines.max(1), source.palette()) else {
        return source.clone();
    };
    let (sc, sl) = (u32::from(source.columns()), u32::from(source.lines()));
    let (tc, tl) = (u32::from(out.columns()), u32::from(out.lines()));
    if sc == 0 || sl == 0 {
        return out;
    }

    for line in 0..out.lines() {
        for column in 0..out.columns() {
            // The block of source cells this output cell stands for. Each edge is
            // nudged to be at least one cell wide so that upscaling still reads a
            // real sample rather than an empty range.
            let x0 = u32::from(column).saturating_mul(sc).checked_div(tc).unwrap_or(0);
            let x1 = u32::from(column)
                .saturating_add(1)
                .saturating_mul(sc)
                .checked_div(tc)
                .unwrap_or(0)
                .max(x0.saturating_add(1));
            let y0 = u32::from(line).saturating_mul(sl).checked_div(tl).unwrap_or(0);
            let y1 = u32::from(line)
                .saturating_add(1)
                .saturating_mul(sl)
                .checked_div(tl)
                .unwrap_or(0)
                .max(y0.saturating_add(1));

            let mut total = 0u32;
            let mut count = 0u32;
            let mut best = (0u8, Cell::BLANK);
            for y in y0..y1.min(sl) {
                for x in x0..x1.min(sc) {
                    let (Ok(sx), Ok(sy)) = (u16::try_from(x), u16::try_from(y)) else {
                        continue;
                    };
                    let Some(cell) = source.get(sx, sy) else { continue };
                    let value = intensity(cell.ch).unwrap_or(0);
                    total = total.saturating_add(u32::from(value));
                    count = count.saturating_add(1);
                    if value >= best.0 {
                        best = (value, *cell);
                    }
                }
            }
            if count == 0 {
                continue;
            }

            // Round to nearest rather than truncating: truncation darkens the
            // whole picture by half a ramp step, which is visible as a wash.
            let top = u32::try_from(RAMP.len().saturating_sub(1)).unwrap_or(6);
            let half = count / 2;
            let level = total.saturating_add(half).checked_div(count).unwrap_or(0).min(top);
            let ch = usize::try_from(level)
                .ok()
                .and_then(|index| RAMP.get(index).copied())
                .unwrap_or(' ');

            out.set(column, line, Cell { ch, fg: best.1.fg, bg: best.1.bg });
        }
    }
    out
}

/// Put `source` on a screen of `size`, resampling it if that will work and
/// centring it if it will not.
///
/// The question this answers is what to do on a machine whose console is not the
/// size the art was drawn for, which is most of them: a `.dur` recorded on a
/// 1080p framebuffer is 240x67, and a console that cannot read its own size is
/// assumed to be 80x25.
///
/// Resampling is used only when the art is shading art *and* it does not already
/// fit. Scaling a logo up gains nothing — a centred logo looks deliberate and a
/// blown-up one looks broken — and scaling line art at all turns it to mush.
#[must_use]
pub fn best_fit(source: &Grid, size: Size, shading: bool) -> Grid {
    let canvas = Size { columns: source.columns(), lines: source.lines() };
    if canvas == size {
        return source.clone();
    }
    if shading && crops(canvas, size) {
        // Keep the proportions and centre what is left over, so a wide picture on
        // a tall console does not get stretched into it.
        return fit(&resample(source, scaled_size(canvas, size)), size);
    }
    fit(source, size)
}

/// Whether this movie should be resampled when the console is a different size.
///
/// Resolves [`Scaling::Auto`] by looking at the alphabet: art drawn only in the
/// shading ramp is taken to mean coverage. That is a guess, and it is wrong for a
/// Game of Life board, which is why anything RABA generates states its answer
/// instead of relying on it.
#[must_use]
pub fn resamples(movie: &Movie) -> bool {
    match movie.scaling {
        Scaling::Resample => true,
        Scaling::Crop => false,
        Scaling::Auto => all_shading(movie.ops.iter().map(|op| op.ch)),
    }
}
