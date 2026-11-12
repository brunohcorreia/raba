//! Loading fastfetch-style ASCII art.
//!
//! The art source is plain text with `$1` to `$9` marking colour changes and `$$` for
//! a literal dollar. The digit is a *slot*, not a colour: the logo says "this part is
//! colour two" and the theme decides what colour two is. That indirection is what
//! lets one piece of art render in 16 colours on a console and in 256 in a preview.
//!
//! ```text
//! $1   /\
//! $1  /  \
//! $2 /    \
//! ```
//!
//! # Why this is strict
//!
//! Art arrives from a text file, and this is the only place that looks at it before
//! it becomes something the boot path reads. A `$` that means nothing, a tab whose
//! width depends on the terminal, an escape sequence pasted in from a rendered logo —
//! each is a question with no good default answer, so each is an error with a line
//! and column rather than a guess. `raba-gen` can afford to be fussy; the thing
//! running at boot cannot.

use raba_core::format::{limits, Palette};
use raba_core::{Cell, Grid};
use std::fmt;

/// One cell of source art: a character and which colour slot it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtCell {
    pub ch: char,
    /// 1-9 from a `$n` marker, or 0 for "before any marker" — the theme's default.
    pub slot: u8,
}

impl ArtCell {
    pub const BLANK: ArtCell = ArtCell { ch: ' ', slot: 0 };
}

/// A rectangle of source art, padded to a common width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Art {
    columns: u16,
    lines: u16,
    cells: Vec<ArtCell>,
}

/// What can be wrong with an art file. Each carries a line and column, 1-based,
/// because the author is looking at the file in an editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtError {
    /// No content, or nothing but blank lines.
    Empty,
    /// A `$` at the very end of a line, with nothing to introduce.
    DanglingMarker { line: usize, column: usize },
    /// `$` followed by something that is not 1-9 or another `$`. Catches `$0` and
    /// neofetch's `${c1}` syntax, which looks close enough to be pasted in by
    /// mistake.
    UnknownMarker { line: usize, column: usize, got: char },
    /// A tab. Its width depends on the terminal, so art containing one has no
    /// definite shape. Spaces are unambiguous.
    Tab { line: usize, column: usize },
    /// A control character or an escape sequence, usually from pasting the *output*
    /// of a fetch tool rather than its source.
    ControlCharacter { line: usize, column: usize, value: u32 },
    /// Wider or taller than a `.raba` canvas can describe.
    TooLarge { columns: usize, lines: usize },
}

impl fmt::Display for ArtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "the art is empty"),
            Self::DanglingMarker { line, column } => {
                write!(f, "{line}:{column}: '$' at end of line; write '$$' for a literal dollar")
            }
            Self::UnknownMarker { line, column, got } => write!(
                f,
                "{line}:{column}: '${got}' is not a colour marker; expected $1 to $9, or $$"
            ),
            Self::Tab { line, column } => {
                write!(f, "{line}:{column}: tab; its width depends on the terminal, use spaces")
            }
            Self::ControlCharacter { line, column, value } => write!(
                f,
                "{line}:{column}: control character {value:#06x}; this looks like rendered \
                 output rather than art source"
            ),
            Self::TooLarge { columns, lines } => {
                write!(
                    f,
                    "{columns}x{lines} is beyond the {}x{} a .raba canvas can hold",
                    limits::MAX_COLUMNS,
                    limits::MAX_LINES
                )
            }
        }
    }
}

impl std::error::Error for ArtError {}

impl Art {
    #[must_use]
    pub fn columns(&self) -> u16 {
        self.columns
    }

    #[must_use]
    pub fn lines(&self) -> u16 {
        self.lines
    }

    #[must_use]
    pub fn cells(&self) -> &[ArtCell] {
        &self.cells
    }

    /// Every colour slot the art actually uses, ascending. A theme needs a colour for
    /// each, and `raba-gen` can say which ones are missing rather than silently
    /// drawing part of a logo in the default colour.
    #[must_use]
    pub fn slots_used(&self) -> Vec<u8> {
        let mut used: Vec<u8> = self.cells.iter().map(|cell| cell.slot).collect();
        used.sort_unstable();
        used.dedup();
        used
    }

    /// Resolve slots to colours and produce a drawable grid.
    ///
    /// `colours[n]` is the foreground for slot `n + 1`; slot 0 and any slot without a
    /// colour fall back to `default_fg`.
    #[must_use]
    pub fn to_grid(
        &self,
        palette: Palette,
        colours: &[u8],
        default_fg: u8,
        bg: u8,
    ) -> Option<Grid> {
        let mut grid = Grid::new(self.columns, self.lines, palette)?;
        for (index, cell) in self.cells.iter().enumerate() {
            let fg = match cell.slot.checked_sub(1) {
                Some(slot) => colours.get(usize::from(slot)).copied().unwrap_or(default_fg),
                None => default_fg,
            };
            grid.set_index(index, Cell { ch: cell.ch, fg, bg });
        }
        Some(grid)
    }
}

/// Parse fastfetch-style art.
///
/// Blank lines at the top and bottom are dropped, and trailing spaces on each line
/// are ignored — both are invisible, and keeping them would make the logo's width
/// depend on whitespace nobody can see. Interior blank lines are kept, because a gap
/// in a logo is part of the logo.
///
/// A colour slot carries across line breaks, matching fastfetch, where a logo in a
/// single colour sets `$1` once on the first line.
pub fn parse(source: &str) -> Result<Art, ArtError> {
    let mut rows: Vec<Vec<ArtCell>> = Vec::new();
    let mut slot: u8 = 0;

    for (line_index, raw) in source.split('\n').enumerate() {
        let line = line_index.saturating_add(1);
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        let mut row: Vec<ArtCell> = Vec::new();
        let mut chars = raw.chars().enumerate().peekable();

        while let Some((offset, ch)) = chars.next() {
            let column = offset.saturating_add(1);
            match ch {
                '$' => match chars.next() {
                    None => return Err(ArtError::DanglingMarker { line, column }),
                    Some((_, '$')) => row.push(ArtCell { ch: '$', slot }),
                    Some((_, digit @ '1'..='9')) => {
                        slot = digit.to_digit(10).and_then(|d| u8::try_from(d).ok()).unwrap_or(0);
                    }
                    Some((_, got)) => return Err(ArtError::UnknownMarker { line, column, got }),
                },
                '\t' => return Err(ArtError::Tab { line, column }),
                '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}' => {
                    return Err(ArtError::ControlCharacter { line, column, value: ch as u32 })
                }
                content => row.push(ArtCell { ch: content, slot }),
            }
        }

        while row.last().is_some_and(|cell| cell.ch == ' ') {
            row.pop();
        }
        rows.push(row);
    }

    while rows.first().is_some_and(Vec::is_empty) {
        rows.remove(0);
    }
    while rows.last().is_some_and(Vec::is_empty) {
        rows.pop();
    }
    if rows.is_empty() {
        return Err(ArtError::Empty);
    }

    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let height = rows.len();
    let (Ok(columns), Ok(lines)) = (u16::try_from(width), u16::try_from(height)) else {
        return Err(ArtError::TooLarge { columns: width, lines: height });
    };
    if columns > limits::MAX_COLUMNS || lines > limits::MAX_LINES {
        return Err(ArtError::TooLarge { columns: width, lines: height });
    }

    let mut cells = Vec::with_capacity(width.saturating_mul(height));
    for mut row in rows {
        row.resize(width, ArtCell::BLANK);
        cells.extend_from_slice(&row);
    }

    Ok(Art { columns, lines, cells })
}
