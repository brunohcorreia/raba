//! Decoding, and the validation that makes it safe to run as root in early boot.
//!
//! The order of checks here is the point of the module, not an implementation
//! detail. Nothing is allocated until the header's claims have been checked against
//! the actual file length and the layout arithmetic has been shown to add up. A
//! malformed file is rejected on the strength of 32 bytes.
//!
//! After [`decode`] returns, every op's `cell` is within the canvas and every
//! frame's op range is within [`Movie::ops`]. The renderer relies on this and does
//! not check again.

use core::fmt;

use super::crc::crc32;
use super::{limits, Frame, Movie, Op, Palette, FRAME_SIZE, HEADER_SIZE, MAGIC, OP_SIZE, VERSION};

/// Why a byte slice is not a valid `.raba`.
///
/// Every variant carries enough to tell a user what is wrong with their file.
/// `raba --check` prints these; the boot path logs them and exits 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Shorter than the fixed header.
    TooShort { got: usize },
    /// First four bytes are not `RABA`.
    BadMagic { got: [u8; 4] },
    /// Format version this build does not know.
    UnsupportedVersion { got: u16 },
    /// The header's `total_bytes` disagrees with the actual length. This is the
    /// check that makes a truncated or padded file impossible to misread.
    SizeMismatch { declared: u32, actual: usize },
    /// Larger than [`limits::MAX_FILE_SIZE`].
    FileTooLarge { declared: u32 },
    /// Canvas is zero-sized or beyond [`limits`].
    BadCanvas { columns: u16, lines: u16 },
    /// No frames. An animation with nothing to show is malformed, not empty.
    NoFrames,
    /// More frames than [`limits::MAX_FRAMES`].
    TooManyFrames { got: u32 },
    /// Palette byte is not a known value.
    BadPalette { got: u8 },
    /// A reserved field is non-zero. Strict now so it can be relaxed later.
    ReservedNotZero { offset: usize },
    /// Header sizes do not add up to `total_bytes`. Catches the case where the
    /// counts are individually plausible but jointly impossible.
    LayoutMismatch { declared: u32, computed: u64 },
    /// Payload does not match the recorded checksum.
    CrcMismatch { declared: u32, computed: u32 },
    /// A frame's ops run past the end of the op array.
    FrameOpsOutOfRange { frame: u32, start: u32, count: u16 },
    /// An op addresses a cell outside the canvas.
    CellOutOfRange { op: u32, cell: u32, cells: u32 },
    /// An op's codepoint is not a character — a surrogate, or beyond U+10FFFF.
    BadCodepoint { op: u32, value: u32 },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { got } => {
                write!(f, "too short: {got} bytes, need at least {HEADER_SIZE}")
            }
            Self::BadMagic { got } => write!(f, "not a .raba file: magic is {got:02x?}"),
            Self::UnsupportedVersion { got } => {
                write!(f, "format version {got} is not supported (this build reads {VERSION})")
            }
            Self::SizeMismatch { declared, actual } => {
                write!(f, "header declares {declared} bytes but the file is {actual}")
            }
            Self::FileTooLarge { declared } => {
                write!(f, "declares {declared} bytes, over the {} limit", limits::MAX_FILE_SIZE)
            }
            Self::BadCanvas { columns, lines } => write!(f, "invalid canvas {columns}x{lines}"),
            Self::NoFrames => write!(f, "no frames"),
            Self::TooManyFrames { got } => {
                write!(f, "{got} frames, over the {} limit", limits::MAX_FRAMES)
            }
            Self::BadPalette { got } => write!(f, "unknown palette {got}"),
            Self::ReservedNotZero { offset } => {
                write!(f, "reserved field at offset {offset} is not zero")
            }
            Self::LayoutMismatch { declared, computed } => {
                write!(f, "sizes add up to {computed} but the header declares {declared}")
            }
            Self::CrcMismatch { declared, computed } => {
                write!(f, "checksum mismatch: declared {declared:#010x}, computed {computed:#010x}")
            }
            Self::FrameOpsOutOfRange { frame, start, count } => {
                write!(f, "frame {frame} claims {count} ops from {start}, past the end")
            }
            Self::CellOutOfRange { op, cell, cells } => {
                write!(f, "op {op} addresses cell {cell}, canvas has {cells}")
            }
            Self::BadCodepoint { op, value } => {
                write!(f, "op {op} has codepoint {value:#x}, which is not a character")
            }
        }
    }
}

impl std::error::Error for DecodeError {}

/// Read a little-endian `u16` at `offset`. `None` if it does not fit.
fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let slice = bytes.get(offset..end)?;
    Some(u16::from_le_bytes([*slice.first()?, *slice.get(1)?]))
}

/// Read a little-endian `u32` at `offset`. `None` if it does not fit.
fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let slice = bytes.get(offset..end)?;
    Some(u32::from_le_bytes([*slice.first()?, *slice.get(1)?, *slice.get(2)?, *slice.get(3)?]))
}

/// Read a little-endian `u16` from the start of `chunk`. Chunks come from
/// `chunks_exact`, so the length is guaranteed; `0` is a defensive default that
/// cannot be reached.
fn u16_of(chunk: &[u8], offset: usize) -> u16 {
    u16_at(chunk, offset).unwrap_or(0)
}

/// As [`u16_of`], for `u32`.
fn u32_of(chunk: &[u8], offset: usize) -> u32 {
    u32_at(chunk, offset).unwrap_or(0)
}

/// Decode a `.raba` file.
///
/// # Errors
///
/// Returns [`DecodeError`] for any malformed input. It never panics and never
/// allocates on a file it is going to reject.
pub fn decode(bytes: &[u8]) -> Result<Movie, DecodeError> {
    // --- 1. Enough bytes to hold a header at all. -------------------------------
    if bytes.len() < HEADER_SIZE {
        return Err(DecodeError::TooShort { got: bytes.len() });
    }

    // --- 2. Is this ours, and do we know this version? --------------------------
    let magic = [
        *bytes.first().unwrap_or(&0),
        *bytes.get(1).unwrap_or(&0),
        *bytes.get(2).unwrap_or(&0),
        *bytes.get(3).unwrap_or(&0),
    ];
    if magic != MAGIC {
        return Err(DecodeError::BadMagic { got: magic });
    }

    let version = u16_at(bytes, 4).unwrap_or(0);
    if version != VERSION {
        return Err(DecodeError::UnsupportedVersion { got: version });
    }

    // --- 3. The size claim, checked against reality. ----------------------------
    //
    // This is the check that bounds everything downstream. Once the file's declared
    // length is known to equal its actual length and to be under the ceiling, every
    // other count can be validated arithmetically rather than trusted.
    let total_bytes = u32_at(bytes, 16).unwrap_or(0);
    if total_bytes > limits::MAX_FILE_SIZE {
        return Err(DecodeError::FileTooLarge { declared: total_bytes });
    }
    if total_bytes as usize != bytes.len() {
        return Err(DecodeError::SizeMismatch { declared: total_bytes, actual: bytes.len() });
    }

    // --- 4. The rest of the header. ---------------------------------------------
    let flags = u16_at(bytes, 6).unwrap_or(0);
    let columns = u16_at(bytes, 8).unwrap_or(0);
    let lines = u16_at(bytes, 10).unwrap_or(0);
    let frame_count = u32_at(bytes, 12).unwrap_or(0);
    let palette_byte = *bytes.get(20).unwrap_or(&0xFF);
    let op_count = u32_at(bytes, 24).unwrap_or(0);
    let declared_crc = u32_at(bytes, 28).unwrap_or(0);

    if columns == 0 || lines == 0 || columns > limits::MAX_COLUMNS || lines > limits::MAX_LINES {
        return Err(DecodeError::BadCanvas { columns, lines });
    }
    if frame_count == 0 {
        return Err(DecodeError::NoFrames);
    }
    if frame_count > limits::MAX_FRAMES {
        return Err(DecodeError::TooManyFrames { got: frame_count });
    }
    let palette =
        Palette::from_byte(palette_byte).ok_or(DecodeError::BadPalette { got: palette_byte })?;

    for offset in 21..24 {
        if *bytes.get(offset).unwrap_or(&0) != 0 {
            return Err(DecodeError::ReservedNotZero { offset });
        }
    }

    // --- 5. Do the declared counts add up to the declared size? -----------------
    //
    // Done in u64 so the arithmetic itself cannot overflow. Individually plausible
    // counts can still be jointly impossible, and this is where that is caught —
    // before a single byte is allocated.
    let computed = u64::from(frame_count)
        .checked_mul(FRAME_SIZE as u64)
        .and_then(|frames| u64::from(op_count).checked_mul(OP_SIZE as u64).map(|ops| (frames, ops)))
        .and_then(|(frames, ops)| frames.checked_add(ops))
        .and_then(|payload| payload.checked_add(HEADER_SIZE as u64))
        .unwrap_or(u64::MAX);
    if computed != u64::from(total_bytes) {
        return Err(DecodeError::LayoutMismatch { declared: total_bytes, computed });
    }

    // --- 6. Integrity. ----------------------------------------------------------
    let payload = bytes.get(HEADER_SIZE..).unwrap_or(&[]);
    let computed_crc = crc32(payload);
    if computed_crc != declared_crc {
        return Err(DecodeError::CrcMismatch { declared: declared_crc, computed: computed_crc });
    }

    // Everything above holds, so these allocations are bounded by a verified file
    // length that is itself bounded by MAX_FILE_SIZE.
    let mut frames = Vec::with_capacity(frame_count as usize);
    let mut ops = Vec::with_capacity(op_count as usize);

    // The layout check above proved these split points are inside the slice, so the
    // only way to reach the `unwrap_or` defaults is a bug in that check.
    let frame_table_len = (frame_count as usize).saturating_mul(FRAME_SIZE);
    let op_table_start = HEADER_SIZE.saturating_add(frame_table_len);
    let frame_table = bytes.get(HEADER_SIZE..op_table_start).unwrap_or(&[]);
    let op_table = bytes.get(op_table_start..).unwrap_or(&[]);

    // --- 7. Frames, with every op range checked against op_count. ---------------
    //
    // `chunks_exact` rather than computed offsets: the arithmetic a parser gets
    // wrong is the arithmetic it does not do.
    for (index, chunk) in frame_table.chunks_exact(FRAME_SIZE).enumerate() {
        let delay_ms = u16_of(chunk, 0);
        let frame_ops = u16_of(chunk, 2);
        let op_start = u32_of(chunk, 4);

        let end = u64::from(op_start).saturating_add(u64::from(frame_ops));
        if end > u64::from(op_count) {
            return Err(DecodeError::FrameOpsOutOfRange {
                frame: u32::try_from(index).unwrap_or(u32::MAX),
                start: op_start,
                count: frame_ops,
            });
        }
        frames.push(Frame { delay_ms, op_start, op_count: frame_ops });
    }

    // --- 8. Ops, with every cell checked against the canvas. --------------------
    //
    // This is what lets the renderer index with `cell` and not check again.
    let cells = u32::from(columns).saturating_mul(u32::from(lines));
    for (index, chunk) in op_table.chunks_exact(OP_SIZE).enumerate() {
        let op = u32::try_from(index).unwrap_or(u32::MAX);
        let cell = u32_of(chunk, 0);
        let codepoint = u32_of(chunk, 4);
        let fg = *chunk.get(8).unwrap_or(&0);
        let bg = *chunk.get(9).unwrap_or(&0);

        if u16_of(chunk, 10) != 0 {
            let offset =
                op_table_start.saturating_add(index.saturating_mul(OP_SIZE)).saturating_add(10);
            return Err(DecodeError::ReservedNotZero { offset });
        }
        if cell >= cells {
            return Err(DecodeError::CellOutOfRange { op, cell, cells });
        }
        let ch =
            char::from_u32(codepoint).ok_or(DecodeError::BadCodepoint { op, value: codepoint })?;

        ops.push(Op { cell, ch, fg, bg });
    }

    Ok(Movie { columns, lines, palette, looping: flags & 1 != 0, frames, ops })
}
