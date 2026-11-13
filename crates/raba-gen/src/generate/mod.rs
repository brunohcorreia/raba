//! Generating animations that have no source file.
//!
//! Ly's built-in animations — DOOM fire, CMatrix rain, a colour-mixing shader,
//! Conway's Life — are procedural code, not art. There is nothing to import. But
//! generating them fits this project better than running them would: the rule is
//! that authoring time may depend on anything and run time depends on nothing, so
//! the arithmetic happens here, once, and the boot path only ever plays frames.
//!
//! # Everything here must loop
//!
//! `raba`(8) repeats the boot animation until the system is ready, so a generator
//! that merely stops is a generator with a visible seam. Each of these closes its
//! own loop a different way, and the one that cannot is honest about it:
//!
//! | | how it closes |
//! |---|---|
//! | `matrix` | every column's fall period divides the frame count |
//! | `gameoflife` | the cycle is *found*: Life is deterministic, so the state repeats |
//! | `colormix` | sine waves over a whole number of turns |
//! | `doom` | it does not — fire is chaotic. See [`fire`]. |
//!
//! # Everything here must be drawable
//!
//! These are composed from ASCII and the CP437 block elements only, checked by
//! `raba_core::glyph`. CMatrix's katakana and the quadrant blocks a shader would
//! like are not available on a console before `setfont` runs.

pub mod colormix;
pub mod fire;
pub mod life;
pub mod matrix;

use raba_core::{Frame, Grid, Movie, Op, Palette};

/// A deterministic generator, so that a build is reproducible and a golden test
/// means something. Any randomness comes from [`Rng`], never from the clock.
#[derive(Debug, Clone, Copy)]
pub struct Rng(u64);

impl Rng {
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        // Zero is a fixed point of xorshift, so it cannot be the state.
        Rng(if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed })
    }

    /// xorshift64*. Small, fast, and good enough to make fire look like fire;
    /// nothing here is cryptographic and nothing here should be.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A value in `0..bound`, or 0 if `bound` is 0.
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        // checked_rem rather than %: bound is already known non-zero above, but
        // the lint should not have to take that on trust from three lines away.
        u32::try_from(self.next_u64().checked_rem(u64::from(bound)).unwrap_or(0)).unwrap_or(0)
    }

    pub fn chance(&mut self, percent: u32) -> bool {
        self.below(100) < percent
    }
}

/// What every generator is asked for.
#[derive(Debug, Clone, Copy)]
pub struct Spec {
    pub columns: u16,
    pub lines: u16,
    /// How many frames the loop runs for. A generator may return fewer if it
    /// closes its loop sooner, as `gameoflife` does.
    pub frames: u16,
    pub delay_ms: u16,
    pub seed: u64,
    pub palette: Palette,
}

impl Default for Spec {
    fn default() -> Self {
        Spec { columns: 80, lines: 25, frames: 60, delay_ms: 80, seed: 1, palette: Palette::Ansi16 }
    }
}

/// Which animation. The names are Ly's, because these exist to answer "can RABA
/// do what my display manager does".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Doom,
    Matrix,
    Colormix,
    GameOfLife,
}

impl Kind {
    #[must_use]
    pub fn named(name: &str) -> Option<Kind> {
        match name.to_lowercase().as_str() {
            "doom" | "fire" => Some(Kind::Doom),
            "matrix" => Some(Kind::Matrix),
            "colormix" => Some(Kind::Colormix),
            "gameoflife" | "life" => Some(Kind::GameOfLife),
            _ => None,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Kind::Doom => "doom",
            Kind::Matrix => "matrix",
            Kind::Colormix => "colormix",
            Kind::GameOfLife => "gameoflife",
        }
    }

    /// Whether the generated loop closes without a visible jump.
    #[must_use]
    pub const fn loops_seamlessly(self) -> bool {
        !matches!(self, Kind::Doom)
    }

    pub const ALL: [Kind; 4] = [Kind::Doom, Kind::Matrix, Kind::Colormix, Kind::GameOfLife];
}

/// Generate one.
#[must_use]
pub fn generate(kind: Kind, spec: &Spec) -> Movie {
    let grids = match kind {
        Kind::Doom => fire::frames(spec),
        Kind::Matrix => matrix::frames(spec),
        Kind::Colormix => colormix::frames(spec),
        Kind::GameOfLife => life::frames(spec),
    };
    delta_encode(&grids, spec)
}

/// Turn a sequence of whole screens into a movie, storing only what moves.
///
/// Every generator here produces full grids, because that is how the arithmetic
/// wants to be written; the saving is applied once, at the end, rather than being
/// four separate opportunities to get wrong.
#[must_use]
pub fn delta_encode(grids: &[Grid], spec: &Spec) -> Movie {
    let mut ops: Vec<Op> = Vec::new();
    let mut frames: Vec<Frame> = Vec::new();
    let mut previous: Option<&Grid> = None;

    for grid in grids {
        let start = ops.len();
        for (index, cell) in grid.cells().iter().enumerate() {
            let unchanged =
                previous.and_then(|p| p.cells().get(index)).is_some_and(|before| before == cell);
            if unchanged {
                continue;
            }
            ops.push(Op {
                cell: u32::try_from(index).unwrap_or(u32::MAX),
                ch: cell.ch,
                fg: cell.fg,
                bg: cell.bg,
            });
        }
        frames.push(Frame {
            op_start: u32::try_from(start).unwrap_or(u32::MAX),
            op_count: u16::try_from(ops.len().saturating_sub(start)).unwrap_or(u16::MAX),
            delay_ms: spec.delay_ms,
        });
        previous = Some(grid);
    }

    Movie {
        columns: spec.columns,
        lines: spec.lines,
        palette: spec.palette,
        looping: true,
        frames,
        ops,
    }
}

/// A blank screen of the requested size, or a 1x1 one if the size is degenerate.
pub(crate) fn blank(spec: &Spec) -> Grid {
    Grid::new(spec.columns.max(1), spec.lines.max(1), spec.palette)
        .or_else(|| Grid::new(1, 1, spec.palette))
        .unwrap_or_else(|| unreachable!("a 1x1 grid is always constructible"))
}
