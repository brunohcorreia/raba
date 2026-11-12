//! Importing Durdraw `.dur`.
//!
//! `.dur` is gzipped JSON holding frames of text and a colour map. Nothing on the
//! boot path reads it: `raba-gen convert` turns it into `.raba` once, at build
//! time, and `raba`(8) only ever plays the result. That is why a gzip decoder is
//! allowed here and nowhere else. See `docs/dur-support.md`.

use crate::palette;
use raba_core::format::{limits, Palette};
use raba_core::render::Cell;
use raba_core::{Frame, Grid, Movie, Op};
use serde::Deserialize;
use std::fmt;
use std::io::Read as _;

/// Durdraw's indices 0–15 are in **curses** order, not ANSI order: 1 is blue, 4 is
/// red, 3 is cyan, 6 is yellow. Copying them across unchanged swaps red and blue
/// in every imported file, which looks like an art problem rather than a bug.
/// Indices 16–255 are the standard xterm cube and need no remapping.
const CURSES_TO_ANSI: [u8; 8] = [0, 4, 2, 6, 1, 5, 3, 7];

/// Milliseconds from a floating-point duration, clamped into what a frame holds.
fn millis(value: f64, fallback: u16) -> u16 {
    if !value.is_finite() || value <= 0.0 {
        return fallback;
    }
    // Walked up rather than cast down: no float-to-integer conversion happens at
    // all, so there is nothing to truncate and nothing to distrust.
    let rounded = value.round();
    (1..=u16::MAX).rev().find(|ms| f64::from(*ms) <= rounded).unwrap_or(1)
}

/// What `raba-gen convert` could not do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DurError {
    Gzip(String),
    Json(String),
    UnsupportedVersion(u32),
    UnsupportedColorFormat(String),
    UnsupportedEncoding(String),
    NoFrames,
    EmptyCanvas,
    CanvasTooLarge { columns: u16, lines: u16 },
    TooManyFrames(usize),
    FrameTooBig { frame: usize, ops: usize },
    ColourMapMismatch { frame: usize, got: usize, columns: u16, lines: u16 },
}

impl fmt::Display for DurError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gzip(e) => write!(f, "not readable as gzip: {e}"),
            Self::Json(e) => write!(f, "not readable as Durdraw JSON: {e}"),
            Self::UnsupportedVersion(v) => {
                write!(f, "formatVersion {v}; this converter knows 7 and 8")
            }
            Self::UnsupportedColorFormat(c) => write!(
                f,
                "colorFormat {c:?}; only \"16\" and \"256\" convert (see docs/dur-support.md)"
            ),
            Self::UnsupportedEncoding(e) => {
                write!(f, "encoding {e:?}; only utf-8 is supported so far")
            }
            Self::NoFrames => write!(f, "no frames"),
            Self::EmptyCanvas => write!(f, "canvas is zero cells"),
            Self::CanvasTooLarge { columns, lines } => write!(
                f,
                "canvas {columns}x{lines} exceeds the format's {}x{}",
                limits::MAX_COLUMNS,
                limits::MAX_LINES
            ),
            Self::TooManyFrames(n) => {
                write!(f, "{n} frames; the format holds {}", limits::MAX_FRAMES)
            }
            Self::FrameTooBig { frame, ops } => write!(
                f,
                "frame {frame} changes {ops} cells; one frame holds {} \
                 (shrink the canvas; see docs/dur-support.md on size)",
                u32::from(u16::MAX)
            ),
            Self::ColourMapMismatch { frame, got, columns, lines } => write!(
                f,
                "frame {frame}: colour map is {got} wide, which is neither \
                 {columns} columns nor {lines} lines"
            ),
        }
    }
}

impl std::error::Error for DurError {}

#[derive(Deserialize)]
struct DurFile {
    #[serde(rename = "DurMovie")]
    movie: DurMovie,
}

#[derive(Deserialize)]
struct DurMovie {
    #[serde(rename = "formatVersion")]
    format_version: u32,
    #[serde(rename = "colorFormat")]
    color_format: String,
    encoding: Option<String>,
    name: Option<String>,
    artist: Option<String>,
    framerate: Option<f64>,
    // Version 7 is documented as sizeX/sizeY and version 8 as columns/lines, but
    // real files do not keep to it: the one this was first tested on declares
    // version 7 and uses columns/lines. Accept either spelling from either.
    #[serde(alias = "sizeX")]
    columns: u16,
    #[serde(alias = "sizeY")]
    lines: u16,
    frames: Vec<DurFrame>,
}

#[derive(Deserialize)]
struct DurFrame {
    delay: Option<f64>,
    contents: Vec<String>,
    #[serde(rename = "colorMap")]
    color_map: Vec<Vec<Vec<i64>>>,
}

/// A converted file, with what the converter had to decide along the way.
#[derive(Debug)]
pub struct Imported {
    pub movie: Movie,
    pub name: String,
    pub artist: String,
    /// Anything a person should know: a guessed orientation, a clamped colour.
    pub notes: Vec<String>,
}

/// Durdraw index to a palette index.
fn colour(raw: i64, sixteen: bool) -> u8 {
    // colorFormat "16" counts from 1; "256" counts from 0.
    let shifted = if sixteen { raw.saturating_sub(1) } else { raw };
    let index = u8::try_from(shifted.clamp(0, 255)).unwrap_or(0);
    let remap = |i: u8| CURSES_TO_ANSI.get(usize::from(i)).copied().unwrap_or(i);
    match index {
        0..=7 => remap(index),
        8..=15 => remap(index.saturating_sub(8)).saturating_add(8),
        other => other,
    }
}

/// Decompress if gzipped, pass through if not.
///
/// Durdraw always writes gzip, but a file that has been through a pipe or an
/// editor may arrive plain, and refusing that would be unhelpful for no reason.
fn decompress(bytes: &[u8]) -> Result<Vec<u8>, DurError> {
    if bytes.first().copied() != Some(0x1f) || bytes.get(1).copied() != Some(0x8b) {
        return Ok(bytes.to_vec());
    }
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut out)
        .map_err(|e| DurError::Gzip(e.to_string()))?;
    Ok(out)
}

/// Convert a `.dur` file to a `Movie`.
pub fn parse(bytes: &[u8]) -> Result<Imported, DurError> {
    let raw = decompress(bytes)?;
    let file: DurFile =
        serde_json::from_slice(&raw).map_err(|e| DurError::Json(e.to_string()))?;
    let dur = file.movie;
    let mut notes = Vec::new();

    if !matches!(dur.format_version, 7 | 8) {
        return Err(DurError::UnsupportedVersion(dur.format_version));
    }
    let sixteen = match dur.color_format.as_str() {
        "16" => true,
        "256" => false,
        other => return Err(DurError::UnsupportedColorFormat(other.to_owned())),
    };
    if let Some(encoding) = &dur.encoding {
        if !encoding.eq_ignore_ascii_case("utf-8") && !encoding.eq_ignore_ascii_case("utf8") {
            return Err(DurError::UnsupportedEncoding(encoding.clone()));
        }
    }
    if dur.frames.is_empty() {
        return Err(DurError::NoFrames);
    }
    if dur.columns == 0 || dur.lines == 0 {
        return Err(DurError::EmptyCanvas);
    }
    if dur.columns > limits::MAX_COLUMNS || dur.lines > limits::MAX_LINES {
        return Err(DurError::CanvasTooLarge { columns: dur.columns, lines: dur.lines });
    }
    if dur.frames.len() > usize::try_from(limits::MAX_FRAMES).unwrap_or(usize::MAX) {
        return Err(DurError::TooManyFrames(dur.frames.len()));
    }

    // 256-colour art keeps its palette; 16-colour art says so in the header.
    let palette = if sixteen { Palette::Ansi16 } else { Palette::Xterm256 };

    // Durdraw's framerate is frames per second; a frame's own delay is in seconds
    // and overrides it. See docs/dur-support.md.
    let default_delay = dur
        .framerate
        .filter(|rate| *rate > 0.0)
        .map_or(100u16, |rate| millis(1000.0_f64 / rate, 100));

    let mut ops: Vec<Op> = Vec::new();
    let mut frames: Vec<Frame> = Vec::new();
    let mut previous: Option<Grid> = None;

    for (index, frame) in dur.frames.iter().enumerate() {
        // Version 7 transposes the colour map relative to the contents and version
        // 8 does not, but the version is not reliable on its own — so take the
        // shape's word for it when the shape is unambiguous, and the version's
        // only when the canvas is square.
        let width = frame.color_map.len();
        let transposed = if width == usize::from(dur.columns) && width != usize::from(dur.lines) {
            true
        } else if width == usize::from(dur.lines) && width != usize::from(dur.columns) {
            false
        } else if width == usize::from(dur.columns) {
            // Square canvas: nothing to go on but the version.
            if index == 0 && dur.columns == dur.lines {
                notes.push(format!(
                    "square canvas, so the colour map orientation comes from \
                     formatVersion {} rather than its shape",
                    dur.format_version
                ));
            }
            dur.format_version == 7
        } else {
            return Err(DurError::ColourMapMismatch {
                frame: index,
                got: width,
                columns: dur.columns,
                lines: dur.lines,
            });
        };

        if index == 0 && transposed != (dur.format_version == 7) {
            notes.push(format!(
                "colour map is laid out {}, which is not what formatVersion {} \
                 specifies; followed the data",
                if transposed { "[column][line]" } else { "[line][column]" },
                dur.format_version
            ));
        }

        let mut grid =
            Grid::new(dur.columns, dur.lines, palette).ok_or(DurError::EmptyCanvas)?;
        for line in 0..dur.lines {
            let text: Vec<char> = frame
                .contents
                .get(usize::from(line))
                .map(|row| row.chars().collect())
                .unwrap_or_default();
            for column in 0..dur.columns {
                let ch = text.get(usize::from(column)).copied().unwrap_or(' ');
                let pair = if transposed {
                    frame.color_map.get(usize::from(column)).and_then(|c| c.get(usize::from(line)))
                } else {
                    frame.color_map.get(usize::from(line)).and_then(|l| l.get(usize::from(column)))
                };
                let fg = pair.and_then(|p| p.first()).copied().unwrap_or(7);
                let bg = pair.and_then(|p| p.get(1)).copied().unwrap_or(0);
                let mut cell = Cell {
                    ch,
                    fg: colour(fg, sixteen),
                    // A console's high background bit is blink, not brightness.
                    bg: palette::clamp_background(colour(bg, sixteen)),
                };
                if palette == Palette::Ansi16 {
                    cell.fg = palette::downsample(cell.fg);
                }
                grid.set(column, line, cell);
            }
        }

        // Frame 0 carries the whole canvas; the rest carry only what moved.
        let start = ops.len();
        for (cell_index, cell) in grid.cells().iter().enumerate() {
            let unchanged = previous
                .as_ref()
                .and_then(|p| p.cells().get(cell_index))
                .is_some_and(|before| before == cell);
            if unchanged {
                continue;
            }
            ops.push(Op {
                cell: u32::try_from(cell_index).unwrap_or(u32::MAX),
                ch: cell.ch,
                fg: cell.fg,
                bg: cell.bg,
            });
        }
        let count = ops.len().saturating_sub(start);
        if count > usize::from(u16::MAX) {
            return Err(DurError::FrameTooBig { frame: index, ops: count });
        }

        let delay = frame
            .delay
            .filter(|d| *d > 0.0)
            .map_or(default_delay, |seconds| millis(seconds * 1000.0, default_delay));

        frames.push(Frame {
            op_start: u32::try_from(start).unwrap_or(u32::MAX),
            op_count: u16::try_from(count).unwrap_or(u16::MAX),
            delay_ms: delay,
        });
        previous = Some(grid);
    }

    Ok(Imported {
        movie: Movie {
            columns: dur.columns,
            lines: dur.lines,
            palette,
            looping: true,
            frames,
            ops,
        },
        name: dur.name.unwrap_or_default(),
        artist: dur.artist.unwrap_or_default(),
        notes,
    })
}
