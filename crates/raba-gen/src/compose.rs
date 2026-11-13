//! Static art plus things that move, turned into a `.raba`.
//!
//! # Why the frame count is a least common multiple
//!
//! A spinner with four steps and a pulsing element with six do not agree about when
//! the animation is back where it started. Playing six frames would cut the spinner
//! mid-cycle; playing four would cut the pulse. The animation repeats cleanly only
//! after LCM(4, 6) = 12 frames, so that is how many are emitted.
//!
//! This is also the argument for choosing periods that divide each other. Two
//! overlays of 7 and 11 steps need 77 frames to loop, and three co-prime periods get
//! expensive quickly — which is why [`ComposeError::PeriodsDoNotFit`] exists rather
//! than silently producing a megabyte.
//!
//! # Deltas
//!
//! Frame 0 carries the whole canvas; every later frame carries only the cells that
//! changed. A spinner over a static logo therefore costs one cell per frame rather
//! than a canvas, which is the entire reason the format is shaped this way.

use raba_core::format::{limits, Palette};
use raba_core::{Cell, Frame, Grid, Movie, Op};
use std::fmt;

/// Something drawn on top of the static art, which changes from frame to frame.
///
/// A spinner is one cell wide. A progress bar would be many. The shape is a run of
/// cells starting at a position, because everything RABA animates so far is a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overlay {
    pub column: u16,
    pub line: u16,
    /// One entry per step of the cycle; each entry is drawn left to right from
    /// `column`. Every step must be the same length.
    pub steps: Vec<Vec<Cell>>,
}

impl Overlay {
    /// How many frames before this overlay repeats.
    #[must_use]
    pub fn period(&self) -> usize {
        self.steps.len()
    }

    /// A one-cell spinner cycling through `glyphs`.
    #[must_use]
    pub fn spinner(column: u16, line: u16, glyphs: &[char], fg: u8, bg: u8) -> Self {
        Overlay {
            column,
            line,
            steps: glyphs.iter().map(|&ch| vec![Cell { ch, fg, bg }]).collect(),
        }
    }
}

/// Built-in spinner glyph sequences.
///
/// All console-safe: ASCII, or the CP437 shade blocks. The obvious choice, braille,
/// is not available — see `docs/glyphs.md`.
pub mod spinners {
    /// The classic. Four steps, pure ASCII, works on anything.
    pub const ASCII: &[char] = &['|', '/', '-', '\\'];
    /// Four steps, reads as a pulse rather than a rotation.
    pub const DOTS: &[char] = &['.', 'o', 'O', 'o'];
    /// Six steps using the CP437 shades, which are in every console font.
    pub const BLOCKS: &[char] = &['░', '▒', '▓', '█', '▓', '▒'];

    /// Every built-in, for the test that asserts they are all console-safe.
    pub const ALL: &[(&str, &[char])] = &[("ascii", ASCII), ("dots", DOTS), ("blocks", BLOCKS)];
}

/// What can go wrong assembling a movie. All of it is caught at build time, so the
/// boot path never meets any of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeError {
    /// An overlay runs off the edge of the art.
    OverlayOffCanvas { column: u16, line: u16, width: usize },
    /// An overlay's steps are not all the same length.
    RaggedOverlay { expected: usize, got: usize, step: usize },
    /// An overlay with no steps animates nothing.
    EmptyOverlay,
    /// The overlay periods have a least common multiple too large to be worth
    /// emitting. Pick periods that divide each other.
    PeriodsDoNotFit { periods: Vec<usize>, frames: u64 },
    /// A colour index the declared palette does not have. The decoder rejects these,
    /// so catching it here turns an unplayable file into a build error.
    ColourOutOfPalette { value: u8, max: u8 },
}

impl fmt::Display for ComposeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OverlayOffCanvas { column, line, width } => {
                write!(f, "an overlay {width} wide at {column},{line} runs off the art")
            }
            Self::RaggedOverlay { expected, got, step } => write!(
                f,
                "overlay step {step} is {got} cells wide; step 0 is {expected}, and every \
                 step must match"
            ),
            Self::EmptyOverlay => write!(f, "an overlay has no steps"),
            Self::PeriodsDoNotFit { periods, frames } => write!(
                f,
                "overlay periods {periods:?} need {frames} frames to line up, past the \
                 {} ceiling; choose periods that divide each other",
                limits::MAX_FRAMES
            ),
            Self::ColourOutOfPalette { value, max } => {
                write!(f, "colour index {value} is above the palette maximum {max}")
            }
        }
    }
}

impl std::error::Error for ComposeError {}

/// Assemble static art and overlays into a movie.
///
/// With no overlays the result is a single frame, which is a legitimate theme: a
/// logo that sits there.
pub fn compose(
    base: &Grid,
    overlays: &[Overlay],
    delay_ms: u16,
    looping: bool,
) -> Result<Movie, ComposeError> {
    let max_colour = base.palette().max_index();
    check_colours(base.cells().iter().copied(), max_colour)?;

    for overlay in overlays {
        let Some(first) = overlay.steps.first() else {
            return Err(ComposeError::EmptyOverlay);
        };
        let width = first.len();
        for (step, cells) in overlay.steps.iter().enumerate() {
            if cells.len() != width {
                return Err(ComposeError::RaggedOverlay {
                    expected: width,
                    got: cells.len(),
                    step,
                });
            }
            check_colours(cells.iter().copied(), max_colour)?;
        }
        let end = usize::from(overlay.column).saturating_add(width);
        if overlay.line >= base.lines() || end > usize::from(base.columns()) {
            return Err(ComposeError::OverlayOffCanvas {
                column: overlay.column,
                line: overlay.line,
                width,
            });
        }
    }

    let frame_count = cycle_length(overlays)?;

    let mut frames: Vec<Frame> = Vec::with_capacity(frame_count);
    let mut ops: Vec<Op> = Vec::new();
    let mut previous: Option<Grid> = None;

    for step in 0..frame_count {
        let current = paint(base, overlays, step);
        let op_start = u32::try_from(ops.len()).unwrap_or(u32::MAX);

        match &previous {
            // Frame 0 carries the whole canvas, so a renderer has somewhere to start.
            None => {
                for (index, cell) in current.cells().iter().enumerate() {
                    ops.push(op_for(index, cell));
                }
            }
            Some(previous) => {
                for (index, cell) in current.cells().iter().enumerate() {
                    if previous.cell(index) != Some(cell) {
                        ops.push(op_for(index, cell));
                    }
                }
            }
        }

        let op_count = u32::try_from(ops.len()).unwrap_or(u32::MAX).saturating_sub(op_start);
        frames.push(Frame {
            delay_ms,
            op_start,
            op_count: u16::try_from(op_count).unwrap_or(u16::MAX),
        });
        previous = Some(current);
    }

    Ok(Movie {
        // A logo is a drawing, not a picture of one: it is composed, it is meant to
        // be centred, and a blown-up or blurred trademark looks broken rather than
        // scaled. Stated rather than left to the alphabet, because a logo drawn
        // entirely in block characters would otherwise be resampled.
        scaling: raba_core::Scaling::Crop,
        columns: base.columns(),
        lines: base.lines(),
        palette: base.palette(),
        looping,
        frames,
        ops,
    })
}

/// The grid as it looks at `step`: the art, with every overlay drawn over it.
///
/// Public because `--preview` draws from these directly, and because it is the
/// reference a delta reconstruction is checked against.
#[must_use]
pub fn paint(base: &Grid, overlays: &[Overlay], step: usize) -> Grid {
    let mut grid = base.clone();
    for overlay in overlays {
        let period = overlay.period();
        if period == 0 {
            continue;
        }
        let Some(cells) = overlay.steps.get(step.checked_rem(period).unwrap_or(0)) else {
            continue;
        };
        for (offset, cell) in cells.iter().enumerate() {
            let Ok(offset) = u16::try_from(offset) else { continue };
            let Some(column) = overlay.column.checked_add(offset) else { continue };
            grid.set(column, overlay.line, *cell);
        }
    }
    grid
}

/// How many frames before every overlay is back where it started.
fn cycle_length(overlays: &[Overlay]) -> Result<usize, ComposeError> {
    let mut total: u64 = 1;
    for overlay in overlays {
        let period = u64::try_from(overlay.period()).unwrap_or(u64::MAX);
        if period == 0 {
            return Err(ComposeError::EmptyOverlay);
        }
        total = lcm(total, period);
        if total > u64::from(limits::MAX_FRAMES) {
            return Err(ComposeError::PeriodsDoNotFit {
                periods: overlays.iter().map(Overlay::period).collect(),
                frames: total,
            });
        }
    }
    usize::try_from(total.max(1)).map_err(|_| ComposeError::PeriodsDoNotFit {
        periods: overlays.iter().map(Overlay::period).collect(),
        frames: total,
    })
}

/// Saturating on overflow rather than panicking: the caller compares the result
/// against the frame ceiling and reports it, which is a better error than a crash.
fn lcm(a: u64, b: u64) -> u64 {
    a.checked_div(gcd(a, b)).and_then(|reduced| reduced.checked_mul(b)).unwrap_or(u64::MAX)
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let remainder = a.checked_rem(b).unwrap_or(0);
        a = b;
        b = remainder;
    }
    a.max(1)
}

fn op_for(index: usize, cell: &Cell) -> Op {
    Op { cell: u32::try_from(index).unwrap_or(u32::MAX), ch: cell.ch, fg: cell.fg, bg: cell.bg }
}

fn check_colours(cells: impl Iterator<Item = Cell>, max: u8) -> Result<(), ComposeError> {
    for cell in cells {
        if cell.fg > max {
            return Err(ComposeError::ColourOutOfPalette { value: cell.fg, max });
        }
        if cell.bg > max {
            return Err(ComposeError::ColourOutOfPalette { value: cell.bg, max });
        }
    }
    Ok(())
}

/// The palette a grid should declare. Convenience for callers building art.
#[must_use]
pub fn palette_for(colours: &[u8]) -> Palette {
    if colours.iter().any(|&c| c > 15) {
        Palette::Xterm256
    } else {
        Palette::Ansi16
    }
}
