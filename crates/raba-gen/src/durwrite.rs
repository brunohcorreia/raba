//! Writing Durdraw `.dur`, so a display manager can show the same art RABA did.
//!
//! RABA hands off to whatever comes next, and if that is Ly it plays its own `.dur`
//! animation. When the last frame of RABA's outro is the first frame of Ly's
//! background, the handoff has no seam — the picture simply stays on screen while
//! one program replaces the other. That only works if both are drawing the same
//! art, which is what this is for: `raba-gen build theme.toml --also-dur` writes
//! `theme.raba` and `theme.dur` from one source.
//!
//! Only `columns`/`lines` are written, never the `sizeX`/`sizeY` spelling as well.
//! The two mean the same thing and a reader that accepts both — ours does, because
//! real files mix them — sees a duplicate key and refuses the file. Writing one
//! name is the only way to be readable by readers that take either.
//!
//! It writes **version 8** — `colorMap[line][column]`, the orientation Durdraw
//! itself now writes. `crate::dur` reads 7 and 8 both, and the round trip through
//! here and back is checked by test.
//!
//! Nothing on the boot path writes or reads this. See `docs/dur-support.md`.

use crate::palette;
use raba_core::render::{apply, Grid};
use raba_core::{Movie, Palette};
use std::io::Write as _;

/// The `formatVersion` to write when nothing says otherwise.
///
/// Seven, not eight, and the reason is empirical rather than tidy: both `.dur`
/// files on a machine running Ly 1.4.1 — its own `example.dur` and the one a user
/// had configured — are version 7, and the point of writing `.dur` at all is that
/// a display manager can read it. Eight is available and is what Durdraw itself
/// now writes; it is one flag away.
pub const DEFAULT_VERSION: u32 = 7;

/// The versions this can write.
///
/// The difference that matters is the colour map's orientation —
/// `colorMap[column][line]` in 7 and `colorMap[line][column]` in 8 — which is the
/// same trap `crate::dur` has to navigate on the way in. The key names differ too:
/// 7 is documented as `sizeX`/`sizeY` and 8 as `columns`/`lines`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    /// See [`DEFAULT_VERSION`] for why this is the default and not the newer one.
    #[default]
    Seven,
    Eight,
}

impl Version {
    /// From the number a user typed.
    #[must_use]
    pub const fn from_number(number: u32) -> Option<Version> {
        match number {
            7 => Some(Version::Seven),
            8 => Some(Version::Eight),
            _ => None,
        }
    }

    #[must_use]
    pub const fn number(self) -> u32 {
        match self {
            Version::Seven => 7,
            Version::Eight => 8,
        }
    }
}

/// The inverse of `crate::dur`'s `CURSES_TO_ANSI`.
///
/// Derived here rather than written out, so the two cannot drift apart: the file
/// that reads the mapping and the file that writes it would otherwise be two
/// places to get red and blue the wrong way round.
fn ansi_to_curses(index: u8) -> u8 {
    const CURSES_TO_ANSI: [u8; 8] = [0, 4, 2, 6, 1, 5, 3, 7];
    let invert = |value: u8| {
        u8::try_from(CURSES_TO_ANSI.iter().position(|a| *a == value).unwrap_or(usize::from(value)))
            .unwrap_or(value)
    };
    match index {
        0..=7 => invert(index),
        8..=15 => invert(index.saturating_sub(8)).saturating_add(8),
        other => other,
    }
}

/// What to put in the file's own metadata.
#[derive(Debug, Default, Clone)]
pub struct Meta {
    pub name: String,
    pub artist: String,
    pub version: Version,
}

/// Render a movie to gzipped Durdraw JSON.
///
/// Every frame is written in full, because that is the only shape `.dur` has — it
/// stores whole screens and leaves the compression to gzip. A movie that is cheap
/// as `.raba` can therefore be large as `.dur`, which is the trade the format
/// makes and the reason `.raba` exists.
///
/// # Errors
///
/// Only if gzip fails, which in practice means the allocator did.
pub fn write(movie: &Movie, meta: &Meta) -> Result<Vec<u8>, String> {
    let json = to_json(movie, meta);
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
    encoder.finish().map_err(|e| e.to_string())
}

/// The uncompressed JSON, which is what the tests read.
#[must_use]
pub fn to_json(movie: &Movie, meta: &Meta) -> String {
    let sixteen = movie.palette == Palette::Ansi16;
    // Durdraw's framerate is global; the format has a per-frame delay in seconds
    // for anything that differs, which is where a `.raba` frame's delay_ms goes.
    let framerate = movie
        .frames
        .first()
        .map(|frame| frame.delay_ms)
        .filter(|ms| *ms > 0)
        .map_or(10.0, |ms| 1000.0 / f64::from(ms));

    let mut frames = Vec::with_capacity(movie.frames.len());
    let mut grid = Grid::new(movie.columns, movie.lines, movie.palette);
    for (number, frame) in movie.frames.iter().enumerate() {
        let Some(canvas) = grid.as_mut() else { break };
        apply(canvas, movie, frame);
        frames.push(frame_json(canvas, number, frame.delay_ms, sixteen, meta.version));
    }

    // Version 7 is documented as sizeX/sizeY and 8 as columns/lines. Only one
    // spelling is written: the two mean the same thing, and a reader that accepts
    // either — ours does, because real files mix them — sees a duplicate key.
    let (width_key, height_key) = match meta.version {
        Version::Seven => ("sizeX", "sizeY"),
        Version::Eight => ("columns", "lines"),
    };
    format!(
        "{{\"DurMovie\": {{\"formatVersion\": {}, \"colorFormat\": \"{}\", \
         \"encoding\": \"utf-8\", \"name\": {}, \"artist\": {}, \"framerate\": {framerate}, \
         \"{width_key}\": {}, \"{height_key}\": {}, \"frames\": [{}]}}}}",
        meta.version.number(),
        if sixteen { "16" } else { "256" },
        quote(&meta.name),
        quote(&meta.artist),
        movie.columns,
        movie.lines,
        frames.join(", ")
    )
}

fn frame_json(
    grid: &Grid,
    number: usize,
    delay_ms: u16,
    sixteen: bool,
    version: Version,
) -> String {
    // `contents` is always rows of text, in both versions.
    let mut contents = Vec::with_capacity(usize::from(grid.lines()));
    for line in 0..grid.lines() {
        let mut row = String::with_capacity(usize::from(grid.columns()));
        for column in 0..grid.columns() {
            row.push(grid.get(column, line).copied().unwrap_or_default().ch);
        }
        contents.push(quote(&row));
    }

    // The colour map is where the versions part company: 8 indexes it by line and
    // 7 by column, so for 7 it is the transpose of what `contents` looks like.
    let cell_colours = |column: u16, line: u16| {
        let cell = grid.get(column, line).copied().unwrap_or_default();
        format!("[{}, {}]", index(cell.fg, sixteen), index(cell.bg, sixteen))
    };
    let colours: Vec<String> = match version {
        Version::Eight => (0..grid.lines())
            .map(|line| {
                let row: Vec<String> =
                    (0..grid.columns()).map(|column| cell_colours(column, line)).collect();
                format!("[{}]", row.join(", "))
            })
            .collect(),
        Version::Seven => (0..grid.columns())
            .map(|column| {
                let col: Vec<String> =
                    (0..grid.lines()).map(|line| cell_colours(column, line)).collect();
                format!("[{}]", col.join(", "))
            })
            .collect(),
    };

    format!(
        "{{\"frameNumber\": {number}, \"delay\": {}, \"contents\": [{}], \"colorMap\": [{}]}}",
        f64::from(delay_ms) / 1000.0,
        contents.join(", "),
        colours.join(", ")
    )
}

/// A palette index as Durdraw stores it.
fn index(value: u8, sixteen: bool) -> u16 {
    let value = if sixteen { palette::downsample(value) } else { value };
    let curses = ansi_to_curses(value);
    // colorFormat "16" counts from 1, "256" from 0.
    if sixteen {
        u16::from(curses).saturating_add(1)
    } else {
        u16::from(curses)
    }
}

/// A JSON string literal.
///
/// Hand-written rather than pulled from `serde_json`'s writer because this is the
/// only escaping the output needs, and the alternative is deriving `Serialize` on
/// a parallel set of structs that would then have to be kept matching `dur.rs`.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len().saturating_add(2));
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
