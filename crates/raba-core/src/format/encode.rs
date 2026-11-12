//! Encoding. The mirror of [`super::decode`], and deliberately dull.
//!
//! Used by `raba-gen` and by tests. The boot binary only ever decodes.

use super::crc::crc32;
use super::{Movie, FRAME_SIZE, HEADER_SIZE, MAGIC, OP_SIZE, VERSION};

/// Encode a movie to bytes.
///
/// Produces a file that [`super::decode`] accepts, provided the movie is itself
/// well formed — ops within the canvas, frame ranges within `ops`. `raba-gen` is
/// responsible for that; this function does not re-derive it, because an encoder
/// that silently corrects its input hides bugs in the generator.
#[must_use]
pub fn encode(movie: &Movie) -> Vec<u8> {
    let frame_count = movie.frames.len();
    let op_count = movie.ops.len();
    // Saturating throughout: a movie too large to describe produces a file the
    // decoder rejects, which is better than a panic and better than a silent wrap.
    // `raba-gen` validates before reaching here.
    let payload_len =
        frame_count.saturating_mul(FRAME_SIZE).saturating_add(op_count.saturating_mul(OP_SIZE));
    let total = HEADER_SIZE.saturating_add(payload_len);

    let mut out = Vec::with_capacity(total);
    let mut payload = Vec::with_capacity(payload_len);
    for frame in &movie.frames {
        payload.extend_from_slice(&frame.delay_ms.to_le_bytes());
        payload.extend_from_slice(&frame.op_count.to_le_bytes());
        payload.extend_from_slice(&frame.op_start.to_le_bytes());
    }
    for op in &movie.ops {
        payload.extend_from_slice(&op.cell.to_le_bytes());
        payload.extend_from_slice(&u32::from(op.ch).to_le_bytes());
        payload.push(op.fg);
        payload.push(op.bg);
        payload.extend_from_slice(&0u16.to_le_bytes()); // reserved
    }

    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(u16::from(movie.looping)).to_le_bytes());
    out.extend_from_slice(&movie.columns.to_le_bytes());
    out.extend_from_slice(&movie.lines.to_le_bytes());
    out.extend_from_slice(&u32::try_from(frame_count).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(&u32::try_from(total).unwrap_or(u32::MAX).to_le_bytes());
    out.push(movie.palette.to_byte());
    out.extend_from_slice(&[0, 0, 0]); // reserved
    out.extend_from_slice(&u32::try_from(op_count).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(&crc32(&payload).to_le_bytes());

    out.extend_from_slice(&payload);
    out
}
