//! Importing ANSI art: `.ans`, and the `.nfo` and `.asc` files that come with it.
//!
//! ANSI art is a stream of CP437 bytes with escape sequences mixed in, meant to be
//! fed straight to a DOS terminal. Importing it is therefore not parsing a
//! document — it is **being the terminal**: run the stream against a cursor and a
//! grid of cells, and whatever the grid holds at the end is the picture.
//!
//! That is the whole design, and it is why the awkward cases fall out for free.
//! Art that draws a shape and then moves the cursor back to colour it in works
//! because the real thing works that way. See `docs/ansi-support.md`.

use crate::cp437;
use crate::import::Imported;
use crate::palette;
use raba_core::format::{limits, Palette, Scaling};
use raba_core::render::Cell;
use raba_core::{Frame, Grid, Movie, Op};
use std::fmt;

/// The width a DOS terminal had, and what essentially all ANSI art is drawn for.
pub const DEFAULT_COLUMNS: u16 = 80;

/// What `raba-gen convert` could not do with an `.ans`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnsiError {
    /// The art drew nothing at all.
    Empty,
    /// Wider than the format allows.
    TooWide { columns: u16 },
    /// Taller than the format allows, after the art has been run.
    TooTall { lines: usize },
}

impl fmt::Display for AnsiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "the art draws nothing"),
            Self::TooWide { columns } => {
                write!(f, "{columns} columns; the format holds {}", limits::MAX_COLUMNS)
            }
            Self::TooTall { lines } => {
                write!(f, "the art runs to {lines} lines; the format holds {}", limits::MAX_LINES)
            }
        }
    }
}

impl std::error::Error for AnsiError {}

/// How to read the file, for the things the file may not say.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Width to lay the art out at. A SAUCE record overrides this when it has one.
    pub columns: u16,
}

impl Default for Options {
    fn default() -> Self {
        Options { columns: DEFAULT_COLUMNS }
    }
}

/// The metadata record some art carries in its last 128 bytes.
///
/// SAUCE is the only place an `.ans` says who drew it and how wide it is, so it is
/// worth reading rather than merely stripping — and it has to be stripped either
/// way, or its text renders as a line of garbage at the bottom of the picture.
#[derive(Debug, Default, Clone)]
struct Sauce {
    title: String,
    author: String,
    columns: Option<u16>,
    ice_colours: bool,
}

/// Split a SAUCE record and its comment block off the end of the art.
fn split_sauce(bytes: &[u8]) -> (&[u8], Option<Sauce>) {
    let Some(start) = bytes.len().checked_sub(128) else { return (bytes, None) };
    if bytes.get(start..start.saturating_add(7)) != Some(b"SAUCE00") {
        return (bytes, None);
    }
    // Fields are fixed-width and padded, with spaces in most files and with NULs
    // in some. CP437 draws a NUL as a blank, so trimming handles both.
    let field = |from: usize, len: usize| -> String {
        let begin = start.saturating_add(from);
        let end = begin.saturating_add(len).min(bytes.len());
        cp437::decode_all(bytes.get(begin..end).unwrap_or(&[])).trim_end().to_owned()
    };
    let byte_at = |offset: usize| bytes.get(start.saturating_add(offset)).copied().unwrap_or(0);
    let word_at = |offset: usize| {
        u16::from(byte_at(offset)) | (u16::from(byte_at(offset.saturating_add(1))) << 8)
    };

    // Layout: 5 id, 2 version, 35 title, 20 author, 20 group, 8 date, 4 filesize,
    // 1 datatype, 1 filetype, then TInfo1..4 as words. TInfo1 is the width.
    let comments = byte_at(104);
    let sauce = Sauce {
        title: field(7, 35),
        author: field(42, 20),
        columns: match word_at(96) {
            0 => None,
            width => Some(width),
        },
        // ANSiFlags bit 0: the file was made for a terminal where the high
        // background bit is brightness rather than blink.
        ice_colours: byte_at(105) & 1 != 0,
    };

    // A comment block sits immediately before the record: "COMNT" then 64 bytes
    // per comment. Left in, it would be drawn as art.
    let mut art_end = start;
    let block = usize::from(comments).saturating_mul(64).saturating_add(5);
    if let Some(block_start) = start.checked_sub(block) {
        if bytes.get(block_start..block_start.saturating_add(5)) == Some(b"COMNT") {
            art_end = block_start;
        }
    }
    (bytes.get(..art_end).unwrap_or(&[]), Some(sauce))
}

/// The terminal being imitated.
struct Screen {
    columns: u16,
    cells: Vec<Vec<Cell>>,
    column: usize,
    line: usize,
    saved: (usize, usize),
    fg: u8,
    bg: u8,
    bold: bool,
    /// Set once if the art ever asked for something a console cannot show.
    blink_used: bool,
    bright_background_used: bool,
    /// The lowest line the art tried to write to, when that was past the end.
    ///
    /// `row` drops those writes, so without this the art was quietly cropped:
    /// `AnsiError::TooTall` was declared, documented and unreachable, because
    /// `cells` could never grow past the limit for the check below to catch.
    /// A drawing whose bottom is missing is not something to find out later.
    too_tall: Option<usize>,
}

impl Screen {
    fn new(columns: u16) -> Self {
        Screen {
            columns,
            cells: Vec::new(),
            column: 0,
            line: 0,
            saved: (0, 0),
            fg: 7,
            bg: 0,
            bold: false,
            blink_used: false,
            bright_background_used: false,
            too_tall: None,
        }
    }

    /// Grow the grid so that `line` exists. Rows are made lazily, because art that
    /// jumps the cursor far down should not cost a screen of blanks on the way.
    fn row(&mut self, line: usize) -> Option<&mut Vec<Cell>> {
        if line >= usize::from(limits::MAX_LINES) {
            // Remembered, not just refused: the caller turns this into an
            // error rather than handing back art with the bottom cut off.
            let seen = line.saturating_add(1);
            self.too_tall = Some(self.too_tall.map_or(seen, |most| most.max(seen)));
            return None;
        }
        while self.cells.len() <= line {
            self.cells.push(vec![Cell::BLANK; usize::from(self.columns)]);
        }
        self.cells.get_mut(line)
    }

    fn put(&mut self, ch: char) {
        if self.column >= usize::from(self.columns) {
            self.column = 0;
            self.line = self.line.saturating_add(1);
        }
        let (column, line) = (self.column, self.line);
        let (fg, bg) = (self.foreground(), self.bg);
        if let Some(row) = self.row(line) {
            if let Some(cell) = row.get_mut(column) {
                *cell = Cell { ch, fg, bg };
            }
        }
        self.column = self.column.saturating_add(1);
    }

    /// Bold means bright here, as it does on every terminal this art was drawn on.
    const fn foreground(&self) -> u8 {
        if self.bold && self.fg < 8 {
            self.fg.saturating_add(8)
        } else {
            self.fg
        }
    }

    fn erase_line(&mut self, mode: u16) {
        let (column, line, columns) = (self.column, self.line, usize::from(self.columns));
        let blank = Cell { ch: ' ', fg: self.foreground(), bg: self.bg };
        let range = match mode {
            1 => 0..column.saturating_add(1).min(columns),
            2 => 0..columns,
            _ => column.min(columns)..columns,
        };
        if let Some(row) = self.row(line) {
            for index in range {
                if let Some(cell) = row.get_mut(index) {
                    *cell = blank;
                }
            }
        }
    }

    fn erase_display(&mut self, mode: u16) {
        // Mode 2 is what art uses, to start from a known state. Treating it as
        // "forget everything drawn so far" is what a terminal does; an ANSImation
        // that clears between frames therefore imports as its last frame, which is
        // the honest answer for a format with no frame boundaries. See
        // docs/ansi-support.md.
        match mode {
            2 | 3 => {
                self.cells.clear();
                self.column = 0;
                self.line = 0;
            }
            1 => {
                for line in 0..self.line {
                    if let Some(row) = self.cells.get_mut(line) {
                        row.fill(Cell::BLANK);
                    }
                }
                self.erase_line(1);
            }
            _ => {
                self.erase_line(0);
                self.cells.truncate(self.line.saturating_add(1));
            }
        }
    }

    fn sgr(&mut self, params: &[u16]) {
        if params.is_empty() {
            self.reset();
            return;
        }
        let mut index = 0;
        while let Some(&param) = params.get(index) {
            index = index.saturating_add(1);
            match param {
                0 => self.reset(),
                1 => self.bold = true,
                5 | 6 => self.blink_used = true,
                7 => core::mem::swap(&mut self.fg, &mut self.bg),
                22 => self.bold = false,
                25 => {}
                30..=37 => self.fg = param.saturating_sub(30).try_into().unwrap_or(7),
                39 => self.fg = 7,
                40..=47 => self.bg = param.saturating_sub(40).try_into().unwrap_or(0),
                49 => self.bg = 0,
                90..=97 => {
                    let base: u8 = param.saturating_sub(90).try_into().unwrap_or(7);
                    self.fg = base.saturating_add(8);
                }
                100..=107 => {
                    self.bright_background_used = true;
                    self.bg = param.saturating_sub(100).try_into().unwrap_or(0);
                }
                38 | 48 => {
                    // 38;5;N, 38;2;R;G;B and their background twins. Hand the whole
                    // run to the one place that already knows how to read it.
                    let (value, used) = extended(params.get(index..).unwrap_or(&[]));
                    index = index.saturating_add(used);
                    if let Some(colour) = value {
                        if param == 38 {
                            self.fg = colour;
                            self.bold = false;
                        } else {
                            self.bg = colour;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn reset(&mut self) {
        self.fg = 7;
        self.bg = 0;
        self.bold = false;
    }
}

/// Read a `38;`/`48;` extended colour, returning it and how many params it ate.
fn extended(rest: &[u16]) -> (Option<u8>, usize) {
    match rest.first() {
        Some(5) => {
            let text = format!("38;5;{}", rest.get(1).copied().unwrap_or(0));
            (palette::from_sgr(&text), 2)
        }
        Some(2) => {
            let value = |at: usize| rest.get(at).copied().unwrap_or(0);
            let text = format!("38;2;{};{};{}", value(1), value(2), value(3));
            (palette::from_sgr(&text), 4)
        }
        _ => (None, 1),
    }
}

/// Run an ANSI art file and return what it drew.
///
/// # Errors
///
/// If the art draws nothing, or runs past the format's canvas limits.
pub fn parse(bytes: &[u8], options: &Options) -> Result<Imported, AnsiError> {
    let (art, sauce) = split_sauce(bytes);
    let sauce = sauce.unwrap_or_default();
    let mut notes = Vec::new();

    let columns = sauce.columns.unwrap_or(options.columns).max(1);
    if columns > limits::MAX_COLUMNS {
        return Err(AnsiError::TooWide { columns });
    }
    if let Some(width) = sauce.columns {
        if width != DEFAULT_COLUMNS {
            notes.push(format!("SAUCE says the art is {width} columns wide"));
        }
    }

    let mut screen = Screen::new(columns);
    run(art, &mut screen);

    if sauce.ice_colours {
        notes.push(
            "SAUCE says iCE colours: backgrounds 8-15 are bright rather than blinking. \
             A Linux console has no bright backgrounds, so they are darkened"
                .to_owned(),
        );
    }
    if screen.blink_used {
        notes.push("the art asks for blinking text, which does not survive import".to_owned());
    }
    if screen.bright_background_used && !sauce.ice_colours {
        notes.push("the art sets bright backgrounds, which a console cannot show".to_owned());
    }

    // Trailing blank rows are an artefact of the file ending in newlines, not art.
    while screen.cells.last().is_some_and(|row| row.iter().all(|cell| *cell == Cell::BLANK)) {
        screen.cells.pop();
    }
    if screen.cells.is_empty() {
        return Err(AnsiError::Empty);
    }
    if let Some(lines) = screen.too_tall {
        return Err(AnsiError::TooTall { lines });
    }
    let lines = screen.cells.len();
    // Cannot fail: `row` refuses a line at or past `MAX_LINES`, which is well
    // inside a `u16`. Written as a conversion rather than a cast so that a
    // future change to the limit is a compile-time question.
    let lines = u16::try_from(lines).map_err(|_| AnsiError::TooTall { lines })?;

    let movie = to_movie(&screen.cells, columns, lines);
    Ok(Imported { movie, name: sauce.title, artist: sauce.author, notes })
}

/// Feed the byte stream to the screen, one character or escape at a time.
fn run(art: &[u8], screen: &mut Screen) {
    let mut index = 0;
    while let Some(&byte) = art.get(index) {
        index = index.saturating_add(1);
        match byte {
            // SUB is the DOS end-of-file marker. Everything after it is metadata
            // or padding, never art.
            0x1a => break,
            b'\n' => {
                screen.line = screen.line.saturating_add(1);
                screen.column = 0;
            }
            b'\r' => screen.column = 0,
            b'\t' => {
                let stop = screen.column.saturating_div(8).saturating_add(1).saturating_mul(8);
                screen.column = stop.min(usize::from(screen.columns));
            }
            0x1b => index = escape(art, index, screen),
            other => screen.put(cp437::decode(other)),
        }
    }
}

/// Handle one escape sequence, returning where to carry on reading.
fn escape(art: &[u8], start: usize, screen: &mut Screen) -> usize {
    if art.get(start).copied() != Some(b'[') {
        // Not CSI. Everything else an art file might carry — a charset selection,
        // a reset — changes nothing we model, so step over the introducer alone.
        return start;
    }
    let mut index = start.saturating_add(1);
    let body_start = index;
    while let Some(&byte) = art.get(index) {
        if byte.is_ascii_alphabetic() {
            break;
        }
        index = index.saturating_add(1);
    }
    let Some(&final_byte) = art.get(index) else { return index };
    let body = art.get(body_start..index).unwrap_or(&[]);
    let text = cp437::decode_all(body);
    let params: Vec<u16> = text
        .trim_start_matches(['?', '>', '='])
        .split(';')
        .map(|part| part.trim().parse::<u16>().unwrap_or(0))
        .collect();
    let first = params.first().copied().unwrap_or(0);
    let count = usize::from(first.max(1));

    match final_byte {
        b'm' => screen.sgr(&params),
        b'A' => screen.line = screen.line.saturating_sub(count),
        b'B' => screen.line = screen.line.saturating_add(count),
        b'C' => screen.column = screen.column.saturating_add(count),
        b'D' => screen.column = screen.column.saturating_sub(count),
        b'E' => {
            screen.line = screen.line.saturating_add(count);
            screen.column = 0;
        }
        b'F' => {
            screen.line = screen.line.saturating_sub(count);
            screen.column = 0;
        }
        b'G' => screen.column = count.saturating_sub(1),
        b'H' | b'f' => {
            // Both coordinates are 1-based and either may be omitted.
            let line = usize::from(params.first().copied().unwrap_or(1).max(1));
            let column = usize::from(params.get(1).copied().unwrap_or(1).max(1));
            screen.line = line.saturating_sub(1);
            screen.column = column.saturating_sub(1);
        }
        b'J' => screen.erase_display(first),
        b'K' => screen.erase_line(first),
        b's' => screen.saved = (screen.column, screen.line),
        b'u' => (screen.column, screen.line) = screen.saved,
        _ => {}
    }
    index.saturating_add(1)
}

/// One grid of cells becomes a one-frame movie.
fn to_movie(cells: &[Vec<Cell>], columns: u16, lines: u16) -> Movie {
    let palette = if cells.iter().flatten().any(|cell| cell.fg > 15 || cell.bg > 15) {
        Palette::Xterm256
    } else {
        Palette::Ansi16
    };
    let mut ops = Vec::new();
    for (line, row) in cells.iter().enumerate() {
        for (column, cell) in row.iter().enumerate() {
            let index = line.saturating_mul(usize::from(columns)).saturating_add(column);
            ops.push(Op {
                cell: u32::try_from(index).unwrap_or(0),
                ch: cell.ch,
                fg: cell.fg,
                // The console's high background bit is blink, not brightness.
                bg: palette::clamp_background(cell.bg),
            });
        }
    }
    let frame =
        Frame { delay_ms: 0, op_count: u16::try_from(ops.len()).unwrap_or(u16::MAX), op_start: 0 };
    Movie {
        columns,
        lines,
        palette,
        // A drawing: composed, meant to be seen whole, and ruined by averaging.
        scaling: Scaling::Crop,
        looping: false,
        frames: vec![frame],
        ops,
    }
}

/// A still picture as a `Grid`, for callers that want to compose rather than play.
#[must_use]
pub fn to_grid(imported: &Imported) -> Option<Grid> {
    let movie = &imported.movie;
    let mut grid = Grid::new(movie.columns, movie.lines, movie.palette)?;
    for op in &movie.ops {
        grid.set_index(op.cell as usize, Cell { ch: op.ch, fg: op.fg, bg: op.bg });
    }
    Some(grid)
}
