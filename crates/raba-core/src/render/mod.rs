//! Composition and rendering: `Movie` plus overlays, to a cell grid, to bytes.
//!
//! Everything here is a function from data to data. The thing that owns a terminal
//! lives behind [`crate::surface::Surface`], which is the only part that cannot be
//! unit tested. See `docs/maintenance.md`.

mod ansi;
pub mod place;

pub use ansi::{to_ansi, to_ansi_into};
pub use place::{crops, fit, Placement};

use crate::format::{limits, Frame, Movie, Palette};

/// Draw one frame's ops onto a grid.
///
/// This is the whole of playback: a movie is a starting canvas and a list of cells to
/// change, so playing it is applying frames in order to a grid that persists between
/// them. Frame 0 carries the full canvas, so starting from a blank grid is correct.
///
/// Out-of-range ops are skipped rather than panicking. Decoding validates every
/// `Op::cell` against the canvas, so a decoded movie cannot contain one — but this
/// runs during boot, where a missing character beats a dead machine.
pub fn apply(grid: &mut Grid, movie: &Movie, frame: &Frame) {
    for op in movie.frame_ops(frame) {
        grid.set_index(op.cell as usize, Cell { ch: op.ch, fg: op.fg, bg: op.bg });
    }
}

/// One character cell: what to draw and in what colours.
///
/// Colour indices refer to the owning [`Grid`]'s palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: u8,
    pub bg: u8,
}

impl Cell {
    /// An empty cell: space, default foreground, black background.
    ///
    /// Index 7 rather than 15: on a Linux console 7 is the default light grey, and
    /// 15 is bright white, which looks wrong against an unstyled console.
    pub const BLANK: Cell = Cell { ch: ' ', fg: 7, bg: 0 };
}

impl Default for Cell {
    fn default() -> Self {
        Cell::BLANK
    }
}

/// A rectangle of cells: one frame of an animation, fully resolved.
///
/// This is what [`to_ansi`] turns into bytes and what the test terminal model
/// reconstructs, which is how the two are checked against each other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid {
    columns: u16,
    lines: u16,
    palette: Palette,
    cells: Vec<Cell>,
}

impl Grid {
    /// A blank grid.
    ///
    /// Returns `None` if the dimensions exceed [`limits`], so a grid can never be
    /// constructed that would not fit in a `.raba` file. The bound matters because
    /// this allocates.
    #[must_use]
    pub fn new(columns: u16, lines: u16, palette: Palette) -> Option<Self> {
        if columns > limits::MAX_COLUMNS || lines > limits::MAX_LINES {
            return None;
        }
        let count = usize::from(columns).checked_mul(usize::from(lines))?;
        Some(Grid { columns, lines, palette, cells: vec![Cell::BLANK; count] })
    }

    #[must_use]
    pub fn columns(&self) -> u16 {
        self.columns
    }

    #[must_use]
    pub fn lines(&self) -> u16 {
        self.lines
    }

    #[must_use]
    pub fn palette(&self) -> Palette {
        self.palette
    }

    /// Total cells. Equal to `columns * lines`, and to `cells().len()`.
    #[must_use]
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// All cells in row-major order.
    #[must_use]
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    /// The cell at a flat index, the same addressing `Op::cell` uses.
    #[must_use]
    pub fn cell(&self, index: usize) -> Option<&Cell> {
        self.cells.get(index)
    }

    /// The cell at a column and line.
    #[must_use]
    pub fn get(&self, column: u16, line: u16) -> Option<&Cell> {
        self.cells.get(self.index_of(column, line)?)
    }

    /// Write a cell by flat index. Returns `false` if the index is outside the grid.
    ///
    /// Out of range is a silent no-op rather than a panic, deliberately: this runs
    /// during boot, and a missing character beats a dead machine. Decoding already
    /// validates every `Op::cell` against the canvas, so a decoded movie cannot
    /// reach this path — the guard is for hand-built grids and for the day the
    /// decoder is wrong.
    pub fn set_index(&mut self, index: usize, cell: Cell) -> bool {
        match self.cells.get_mut(index) {
            Some(slot) => {
                *slot = cell;
                true
            }
            None => false,
        }
    }

    /// Write a cell by column and line.
    pub fn set(&mut self, column: u16, line: u16, cell: Cell) -> bool {
        match self.index_of(column, line) {
            Some(index) => self.set_index(index, cell),
            None => false,
        }
    }

    /// Reset every cell to [`Cell::BLANK`].
    pub fn clear(&mut self) {
        self.fill(Cell::BLANK);
    }

    /// Set every cell to `cell`. How a theme gets a background other than black.
    pub fn fill(&mut self, cell: Cell) {
        self.cells.fill(cell);
    }

    /// Draw `other` onto this grid with its top left at `column`, `line`.
    ///
    /// Cells that fall outside are dropped rather than wrapping to the next row,
    /// which is what makes this safe to use for laying out a logo on a canvas
    /// without first working out whether it fits.
    pub fn blit(&mut self, other: &Grid, column: u16, line: u16) {
        for source_line in 0..other.lines() {
            for source_column in 0..other.columns() {
                let Some(cell) = other.get(source_column, source_line) else { continue };
                let (Some(target_column), Some(target_line)) =
                    (column.checked_add(source_column), line.checked_add(source_line))
                else {
                    continue;
                };
                self.set(target_column, target_line, *cell);
            }
        }
    }

    fn index_of(&self, column: u16, line: u16) -> Option<usize> {
        if column >= self.columns || line >= self.lines {
            return None;
        }
        usize::from(line).checked_mul(usize::from(self.columns))?.checked_add(usize::from(column))
    }
}
