//! The `.raba` animation format.
//!
//! A fixed-layout binary format. The design rule, from `docs/format.md`:
//!
//! > Total memory is derivable from the header, and a malformed file is rejected
//! > before allocation rather than after.
//!
//! This is why the format is ours rather than Durdraw's `.dur`. A gzipped JSON file
//! can decompress to gigabytes; early in boot, as root, with no swap, that is an
//! unbootable machine. Here the file states its own size in a fixed header, that
//! claim is checked against reality, and every subsequent length is derived from it.
//!
//! # Layout
//!
//! Little-endian throughout.
//!
//! ```text
//! header, 32 bytes
//!    0  4  magic        "RABA"
//!    4  2  version      u16
//!    6  2  flags        u16   bit 0: loop
//!    8  2  columns      u16
//!   10  2  lines        u16
//!   12  4  frame_count  u32
//!   16  4  total_bytes  u32   must equal the file length
//!   20  1  palette      u8    0 = ANSI 16, 1 = xterm-256
//!   21  1  scaling      u8    0 = auto, 1 = resample, 2 = crop
//!   22  2  reserved     must be zero
//!   24  4  op_count     u32   total across all frames
//!   28  4  crc32        u32   over everything after the header
//!
//! frame table, frame_count entries of 8 bytes
//!    0  2  delay_ms     u16   0 means use the movie frame rate
//!    2  2  op_count     u16
//!    4  4  op_start     u32   index into the op array
//!
//! ops, op_count entries of 12 bytes
//!    0  4  cell         u32   line * columns + column
//!    4  4  codepoint    u32
//!    8  1  fg           u8
//!    9  1  bg           u8
//!   10  2  reserved     must be zero
//! ```
//!
//! Frame 0 carries a full canvas. Later frames carry only changed cells, which is
//! what makes a spinner over a static logo cost a few cells per frame instead of a
//! whole canvas.

mod crc;
mod decode;
mod encode;

pub use crc::crc32;
pub use decode::{decode, DecodeError};
pub use encode::encode;

/// Magic bytes at offset 0.
pub const MAGIC: [u8; 4] = *b"RABA";

/// The only format version this build reads or writes.
pub const VERSION: u16 = 1;

/// Size of the fixed header, in bytes.
pub const HEADER_SIZE: usize = 32;

/// Size of one frame table entry, in bytes.
pub const FRAME_SIZE: usize = 8;

/// Size of one op, in bytes.
pub const OP_SIZE: usize = 12;

/// Ceilings. These exist so a malformed header is rejected arithmetically, before
/// anything is allocated. They are deliberately generous against real themes and
/// deliberately small against a machine's memory.
pub mod limits {
    /// 16 MiB. The master bound: every other length is derived from the file size,
    /// so capping this caps allocation.
    pub const MAX_FILE_SIZE: u32 = 16 * 1024 * 1024;
    /// A console is rarely wider than 500 cells. 1024 is generous.
    pub const MAX_COLUMNS: u16 = 1024;
    /// Likewise.
    pub const MAX_LINES: u16 = 1024;
    /// What a screen may hold. Not the same bound as the two above.
    ///
    /// Those bound a canvas, which is written to a file. A console is not: its
    /// size is the panel divided by the loaded font — see `docs/console-size.md`
    /// — and may exceed 1024 in either axis. What has to be bounded there is the
    /// allocation, so the cap is on total cells, set at the area of the largest
    /// canvas the format allows. [`Grid::screen`](crate::Grid::screen) applies it.
    pub const MAX_SCREEN_CELLS: usize = MAX_COLUMNS as usize * MAX_LINES as usize;
    /// At 10 fps this is nearly two hours of animation.
    pub const MAX_FRAMES: u32 = 65_535;
}

/// What to do when the console is not the size the art was authored for.
///
/// The question is not answerable from the bytes alone, which is why it is stored.
/// Resampling averages the coverage of a block of cells into one; that is right for
/// a picture whose characters mean *intensity* and wrong for one whose characters
/// mean *things*. Both can be drawn in the same seven characters:
///
/// - a fire, or a video converted by `video-to-dur`, means intensity — `▓` is
///   "more than `▒`", and the average of a block is a true statement about it
/// - a Game of Life board does not — `█` is a live cell and `░` is its trail, and
///   the average of four cells is not three quarters of a glider
///
/// [`Scaling::Auto`] guesses from the alphabet, which is the best a converter can
/// do for art somebody else authored. Anything RABA generates itself says outright.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scaling {
    /// Resample if every character is on the shading ramp, otherwise crop.
    #[default]
    Auto,
    /// Always resample. The art means coverage.
    Resample,
    /// Never resample: centre, and crop what does not fit.
    Crop,
}

impl Scaling {
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        match self {
            Scaling::Auto => 0,
            Scaling::Resample => 1,
            Scaling::Crop => 2,
        }
    }

    #[must_use]
    pub const fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Scaling::Auto),
            1 => Some(Scaling::Resample),
            2 => Some(Scaling::Crop),
            _ => None,
        }
    }
}

/// Which palette the colour indices in [`Op`] refer to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Palette {
    /// Indices 0-15. What a Linux virtual console can actually display.
    Ansi16,
    /// Indices 0-255, xterm-256color. Downsampled by `raba-gen` for console targets.
    Xterm256,
}

impl Palette {
    /// Highest valid colour index.
    ///
    /// Decoding checks every `fg` and `bg` against this, so a renderer never has to
    /// decide what an out-of-range index means. There is no good answer to that
    /// question — masking the high bits and clamping to the default are equally
    /// arbitrary — so the file is rejected instead of guessed at.
    #[must_use]
    pub fn max_index(self) -> u8 {
        match self {
            Palette::Ansi16 => 15,
            Palette::Xterm256 => 255,
        }
    }

    fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(Palette::Ansi16),
            1 => Some(Palette::Xterm256),
            _ => None,
        }
    }

    fn to_byte(self) -> u8 {
        match self {
            Palette::Ansi16 => 0,
            Palette::Xterm256 => 1,
        }
    }
}

/// A single cell write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Op {
    /// `line * columns + column`. Validated against the canvas at decode time, so a
    /// renderer may index with it without checking again.
    pub cell: u32,
    /// The character to draw.
    pub ch: char,
    /// Foreground colour index into the movie's [`Palette`]. Validated against
    /// [`Palette::max_index`] at decode time.
    pub fg: u8,
    /// Background colour index.
    pub bg: u8,
}

/// One frame: how long to hold it, and which ops produce it from the previous frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    /// How long to hold this frame. 0 means use the movie's frame rate.
    pub delay_ms: u16,
    /// Index of this frame's first op in [`Movie::ops`].
    pub op_start: u32,
    /// How many ops this frame has.
    pub op_count: u16,
}

/// A decoded animation.
///
/// Ops are held in one flat vector and frames reference ranges within it. This
/// mirrors the file layout exactly, which keeps encode and decode symmetrical and
/// makes the round-trip property obvious. It is also one allocation rather than one
/// per frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Movie {
    pub columns: u16,
    pub lines: u16,
    pub palette: Palette,
    /// What to do when the console is not the size this was authored for.
    pub scaling: Scaling,
    /// Whether playback repeats. A boot animation loops; an outro does not.
    pub looping: bool,
    pub frames: Vec<Frame>,
    pub ops: Vec<Op>,
}

impl Movie {
    /// Total cells in the canvas.
    ///
    /// Cannot overflow even for a hand-built movie: `u16::MAX * u16::MAX` is under
    /// `u32::MAX`. `saturating_mul` states that rather than relying on the reader
    /// to check the arithmetic.
    #[must_use]
    pub fn cell_count(&self) -> u32 {
        u32::from(self.columns).saturating_mul(u32::from(self.lines))
    }

    /// The ops belonging to `frame`.
    ///
    /// Returns an empty slice rather than panicking if the range is somehow out of
    /// bounds. Decoding validates every range, so that cannot happen for a decoded
    /// movie — but this is on the boot path, and a wrong pixel beats a dead machine.
    #[must_use]
    pub fn frame_ops(&self, frame: &Frame) -> &[Op] {
        let start = frame.op_start as usize;
        let end = start.saturating_add(frame.op_count as usize);
        self.ops.get(start..end).unwrap_or(&[])
    }
}
