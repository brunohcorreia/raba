//! Placing a logo, a spinner and a caption on one canvas.
//!
//! Each element arrives with a [`Position`] relative to what is already placed and a
//! gap in cells. Attaching above or to the left shifts everything placed so far,
//! which is the only part of this that is easy to get wrong — and the reason it is a
//! small function with its own tests rather than arithmetic scattered through the
//! builder.

use std::fmt;

/// Where an element goes relative to the art.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Position {
    Above,
    #[default]
    Below,
    Left,
    Right,
    /// On top of the art, centred, taking no space of its own.
    Overlay,
}

impl Position {
    /// Parse the spelling `raba-theme`(5) uses.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "above" => Some(Position::Above),
            "below" => Some(Position::Below),
            "left" => Some(Position::Left),
            "right" => Some(Position::Right),
            "overlay" => Some(Position::Overlay),
            _ => None,
        }
    }

    /// Every spelling, for an error message that tells the author what to write.
    pub const NAMES: &'static [&'static str] = &["above", "below", "left", "right", "overlay"];
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Position::Above => "above",
            Position::Below => "below",
            Position::Left => "left",
            Position::Right => "right",
            Position::Overlay => "overlay",
        };
        f.write_str(name)
    }
}

/// A canvas and where each element ended up on it, in the order they were attached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub columns: u16,
    pub lines: u16,
    /// `(column, line)` of each element's top left corner. Index 0 is the art.
    pub placements: Vec<(u16, u16)>,
}

impl Layout {
    /// A canvas holding only the art.
    #[must_use]
    pub fn of(columns: u16, lines: u16) -> Self {
        Layout { columns, lines, placements: vec![(0, 0)] }
    }

    /// Add an element, growing the canvas and shifting what is already on it.
    ///
    /// Centring is on the axis the element does not consume: something below is
    /// centred horizontally, something to the left is centred vertically.
    pub fn attach(&mut self, width: u16, height: u16, position: Position, gap: u16) {
        let (grow_columns, grow_lines) = match position {
            Position::Above | Position::Below => (0, height.saturating_add(gap)),
            Position::Left | Position::Right => (width.saturating_add(gap), 0),
            Position::Overlay => (0, 0),
        };

        // Attaching above or to the left pushes everything already placed along.
        let (shift_columns, shift_lines) = match position {
            Position::Above => (0, grow_lines),
            Position::Left => (grow_columns, 0),
            _ => (0, 0),
        };
        if shift_columns != 0 || shift_lines != 0 {
            for placement in &mut self.placements {
                placement.0 = placement.0.saturating_add(shift_columns);
                placement.1 = placement.1.saturating_add(shift_lines);
            }
        }

        let old_columns = self.columns;
        let old_lines = self.lines;
        self.columns = self.columns.saturating_add(grow_columns).max(width);
        self.lines = self.lines.saturating_add(grow_lines).max(height);

        let placement = match position {
            Position::Above => (centre(self.columns, width), 0),
            Position::Below => (centre(self.columns, width), old_lines.saturating_add(gap)),
            Position::Left => (0, centre(self.lines, height)),
            Position::Right => (old_columns.saturating_add(gap), centre(self.lines, height)),
            Position::Overlay => (centre(self.columns, width), centre(self.lines, height)),
        };
        self.placements.push(placement);
    }

    /// Add blank cells on every side.
    pub fn pad(&mut self, padding: u16) {
        if padding == 0 {
            return;
        }
        let both = padding.saturating_mul(2);
        self.columns = self.columns.saturating_add(both);
        self.lines = self.lines.saturating_add(both);
        for placement in &mut self.placements {
            placement.0 = placement.0.saturating_add(padding);
            placement.1 = placement.1.saturating_add(padding);
        }
    }

    /// Where element `index` ended up, if it was attached.
    #[must_use]
    pub fn placement(&self, index: usize) -> Option<(u16, u16)> {
        self.placements.get(index).copied()
    }
}

fn centre(available: u16, size: u16) -> u16 {
    available.saturating_sub(size).checked_div(2).unwrap_or(0)
}
