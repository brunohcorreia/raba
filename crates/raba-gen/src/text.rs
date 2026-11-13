//! Importing plain text: a `.txt` file, as it is, in one colour.
//!
//! The simplest importer there is, and worth having precisely because it is: a
//! figlet banner, a hand-drawn sign, the output of `cowsay`, anything already
//! sitting in a file. No markers, no escapes, no metadata.
//!
//! ## Why `$1` means nothing here
//!
//! Theme art uses `$1`..`$9` to mark colour changes, and that is the right thing
//! for art written *for* RABA. It is the wrong thing for a file written for
//! something else, where a dollar sign is a dollar sign — a price, a shell prompt,
//! a variable in a code listing. So `convert` takes text literally and `build`
//! reads markers, and which you get follows from which command you ran rather than
//! from anything hidden in the file. See `crate::art` for the other one.

use crate::cp437;
use crate::import::Imported;
use raba_core::format::{limits, Palette, Scaling};

use raba_core::{Frame, Movie, Op};
use std::fmt;

/// Where a tab stops, counting from the left margin.
const TAB_WIDTH: usize = 8;

/// What `raba-gen convert` could not do with a text file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextError {
    /// Nothing but whitespace.
    Empty,
    TooWide {
        columns: usize,
    },
    TooTall {
        lines: usize,
    },
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "the file has no text in it"),
            Self::TooWide { columns } => {
                write!(
                    f,
                    "the longest line is {columns} characters; the format holds {}",
                    limits::MAX_COLUMNS
                )
            }
            Self::TooTall { lines } => {
                write!(f, "{lines} lines; the format holds {}", limits::MAX_LINES)
            }
        }
    }
}

impl std::error::Error for TextError {}

/// Colours to draw the text in.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub fg: u8,
    pub bg: u8,
}

impl Default for Options {
    fn default() -> Self {
        // The console's own default grey on its own black, so imported text looks
        // like text rather than like a choice somebody made.
        Options { fg: 7, bg: 0 }
    }
}

/// Decode the bytes, preferring UTF-8 and falling back to CP437.
///
/// Both are real: a `.txt` written today is UTF-8, and a `.nfo` from a release
/// group is CP437 and will decode as UTF-8 only by accident. Trying UTF-8 first
/// and falling back is unambiguous, because CP437 has no invalid bytes — so the
/// fallback can never itself fail, and valid UTF-8 is never mistaken for CP437.
#[must_use]
pub fn decode(bytes: &[u8]) -> (String, bool) {
    match core::str::from_utf8(bytes) {
        Ok(text) => (text.trim_start_matches('\u{feff}').to_owned(), true),
        Err(_) => (cp437::decode_all(bytes), false),
    }
}

/// Convert plain text to a one-frame movie.
///
/// # Errors
///
/// If the text is empty, or does not fit the format's canvas.
pub fn parse(bytes: &[u8], options: &Options) -> Result<Imported, TextError> {
    let (text, utf8) = decode(bytes);
    let mut notes = Vec::new();
    if !utf8 {
        notes.push("not valid UTF-8, so it was read as CP437".to_owned());
    }

    let mut rows: Vec<Vec<char>> =
        text.split('\n').map(|line| expand_tabs(line.strip_suffix('\r').unwrap_or(line))).collect();
    // A file ending in a newline is not a file with a blank last line.
    while rows.last().is_some_and(|row| row.iter().all(|ch| *ch == ' ')) {
        rows.pop();
    }
    if rows.is_empty() {
        return Err(TextError::Empty);
    }

    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns == 0 {
        return Err(TextError::Empty);
    }
    if columns > usize::from(limits::MAX_COLUMNS) {
        return Err(TextError::TooWide { columns });
    }
    if rows.len() > usize::from(limits::MAX_LINES) {
        return Err(TextError::TooTall { lines: rows.len() });
    }
    let (Ok(columns), Ok(lines)) = (u16::try_from(columns), u16::try_from(rows.len())) else {
        return Err(TextError::TooWide { columns });
    };

    let mut ops = Vec::new();
    for (line, row) in rows.iter().enumerate() {
        for column in 0..usize::from(columns) {
            let ch = row.get(column).copied().unwrap_or(' ');
            let index = line.saturating_mul(usize::from(columns)).saturating_add(column);
            ops.push(Op {
                cell: u32::try_from(index).unwrap_or(0),
                ch,
                fg: options.fg,
                bg: options.bg,
            });
        }
    }

    let palette =
        if options.fg > 15 || options.bg > 15 { Palette::Xterm256 } else { Palette::Ansi16 };
    let frame =
        Frame { delay_ms: 0, op_count: u16::try_from(ops.len()).unwrap_or(u16::MAX), op_start: 0 };
    let movie = Movie {
        columns,
        lines,
        palette,
        // Letters. Halving one does not give half a letter.
        scaling: Scaling::Crop,
        looping: false,
        frames: vec![frame],
        ops,
    };
    Ok(Imported { movie, name: String::new(), artist: String::new(), notes })
}

/// Expand tabs to the next multiple of [`TAB_WIDTH`].
///
/// Counted in characters rather than bytes, which is the whole reason this is not
/// a one-line `replace`: a tab after an accented letter stops in the same place as
/// a tab after an unaccented one.
fn expand_tabs(line: &str) -> Vec<char> {
    let mut out = Vec::new();
    for ch in line.chars() {
        if ch == '\t' {
            let stop =
                out.len().saturating_div(TAB_WIDTH).saturating_add(1).saturating_mul(TAB_WIDTH);
            while out.len() < stop {
                out.push(' ');
            }
        } else {
            out.push(ch);
        }
    }
    out
}
