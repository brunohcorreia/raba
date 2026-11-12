//! A terminal, modelled well enough to check the renderer against.
//!
//! The renderer's job is to produce bytes that *turn one grid into another*. Asserting
//! on the bytes themselves only checks that it still emits what it emitted yesterday.
//! This interprets them the way a terminal would, so the property that matters can be
//! stated directly:
//!
//! > applying `to_ansi(grid, prev)` to `prev` yields `grid`
//!
//! It is deliberately strict. Anything the renderer emits that is not CUP, SGR or a
//! printable character is a panic rather than a shrug, which makes this an assertion
//! about the renderer's whole output alphabet and not only about the parts a test
//! thought to look at.
//!
//! Dev-only. It never ships; see `docs/maintenance.md` on dev-dependencies being free.

// Panicking is this module's whole output channel: it is an oracle, and an oracle
// that returned `Result` would let a test quietly ignore it. The boot-path lints do
// not apply to a thing that never ships.
#![allow(
    dead_code,
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use raba_core::{Cell, Grid, Palette};

pub struct Terminal {
    grid: Grid,
    column: u16,
    line: u16,
    /// DEC deferred wrap: a character written in the last column leaves the cursor
    /// there until another arrives. Modelled so that a renderer which stopped
    /// emitting an absolute move after the last column is caught rather than excused.
    pending_wrap: bool,
    /// `None` means no SGR has been seen. Writing a glyph in that state is a bug in
    /// the renderer, not in the model: the output is supposed to assume nothing about
    /// the colour already in effect.
    fg: Option<u8>,
    bg: Option<u8>,
}

impl Terminal {
    /// A terminal showing `screen`.
    pub fn showing(screen: &Grid) -> Self {
        Terminal {
            grid: screen.clone(),
            column: 0,
            line: 0,
            pending_wrap: false,
            fg: None,
            bg: None,
        }
    }

    /// A blank terminal of the given shape.
    pub fn blank(columns: u16, lines: u16, palette: Palette) -> Self {
        let grid = Grid::new(columns, lines, palette).expect("dimensions within limits");
        Terminal::showing(&grid)
    }

    pub fn screen(&self) -> &Grid {
        &self.grid
    }

    pub fn into_screen(self) -> Grid {
        self.grid
    }

    /// Feed the terminal a frame.
    ///
    /// # Panics
    ///
    /// On anything outside the alphabet the renderer is allowed to produce. That is
    /// the point: the panic message names what leaked.
    pub fn apply(&mut self, bytes: &[u8]) {
        let text = std::str::from_utf8(bytes).expect("renderer output is valid UTF-8");
        let mut chars = text.chars().peekable();

        while let Some(ch) = chars.next() {
            match ch {
                '\u{1b}' => {
                    assert_eq!(chars.next(), Some('['), "only CSI escapes are expected");
                    let mut params: Vec<u16> = Vec::new();
                    let mut current: Option<u16> = None;
                    let final_byte = loop {
                        let next = chars.next().expect("escape sequence ends");
                        match next {
                            '0'..='9' => {
                                let digit = u16::from(next as u8 - b'0');
                                current = Some(current.unwrap_or(0) * 10 + digit);
                            }
                            ';' => {
                                params.push(current.take().unwrap_or(0));
                            }
                            other => {
                                if let Some(value) = current.take() {
                                    params.push(value);
                                }
                                break other;
                            }
                        }
                    };
                    match final_byte {
                        'H' => self.cup(&params),
                        'm' => self.sgr(&params),
                        other => panic!("unexpected CSI final byte {other:?}"),
                    }
                }
                '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}' => {
                    panic!("control character {:#06x} reached the terminal", ch as u32)
                }
                printable => self.put(printable),
            }
        }
    }

    fn cup(&mut self, params: &[u16]) {
        // CUP is 1-based and omitted parameters mean 1.
        let line = params.first().copied().unwrap_or(1).max(1) - 1;
        let column = params.get(1).copied().unwrap_or(1).max(1) - 1;
        assert!(line < self.grid.lines(), "cursor moved off the bottom");
        assert!(column < self.grid.columns(), "cursor moved off the right");
        self.line = line;
        self.column = column;
        self.pending_wrap = false;
    }

    fn sgr(&mut self, params: &[u16]) {
        let mut index = 0;
        // No parameters means SGR 0.
        if params.is_empty() {
            self.fg = Some(Cell::BLANK.fg);
            self.bg = Some(Cell::BLANK.bg);
            return;
        }
        while let Some(&param) = params.get(index) {
            index += 1;
            match param {
                0 => {
                    self.fg = Some(Cell::BLANK.fg);
                    self.bg = Some(Cell::BLANK.bg);
                }
                30..=37 => self.fg = Some((param - 30) as u8),
                40..=47 => self.bg = Some((param - 40) as u8),
                90..=97 => self.fg = Some((param - 90 + 8) as u8),
                100..=107 => self.bg = Some((param - 100 + 8) as u8),
                38 | 48 => {
                    assert_eq!(params.get(index).copied(), Some(5), "only 256-colour form");
                    let value = params.get(index + 1).copied().expect("colour index");
                    index += 2;
                    let value = u8::try_from(value).expect("colour index fits a byte");
                    if param == 38 {
                        self.fg = Some(value);
                    } else {
                        self.bg = Some(value);
                    }
                }
                other => panic!("unexpected SGR parameter {other}"),
            }
        }
    }

    fn put(&mut self, ch: char) {
        if self.pending_wrap {
            self.column = 0;
            self.line += 1;
            self.pending_wrap = false;
            assert!(self.line < self.grid.lines(), "wrapped off the bottom of the screen");
        }
        let fg = self.fg.expect("renderer set a foreground before drawing");
        let bg = self.bg.expect("renderer set a background before drawing");
        let written = self.grid.set(self.column, self.line, Cell { ch, fg, bg });
        assert!(written, "wrote outside the screen");
        if self.column + 1 < self.grid.columns() {
            self.column += 1;
        } else {
            self.pending_wrap = true;
        }
    }
}
