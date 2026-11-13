//! Pure core for RABA: the `.raba` format, layout, compositing and rendering.
//!
//! No I/O, no syscalls, no dependencies. Everything here is a transformation from
//! data to data, which is what makes it testable on a laptop in milliseconds rather
//! than in a virtual machine. See `docs/maintenance.md`.
//!
//! The impure parts — acquiring a terminal, the control socket, state on disk —
//! live in the `raba` binary, behind a `Surface` abstraction, and are kept small
//! enough to review line by line.

// This crate never needs unsafe: it is pure data transformation (raba-core),
// a thin wrapper over std (raba-term), or a build-time tool (raba-gen). Stated
// here rather than inherited, because a promise is worth more where it is kept.
#![forbid(unsafe_code)]

pub mod format;
pub mod glyph;
pub mod render;
pub mod surface;

#[cfg(feature = "testing")]
pub mod testing;

pub use format::{decode, encode, DecodeError, Frame, Movie, Op, Palette, Scaling};
pub use glyph::{is_safe, Safety};
pub use render::{apply, to_ansi, to_ansi_into, Cell, Grid};
pub use surface::{Size, Surface};
