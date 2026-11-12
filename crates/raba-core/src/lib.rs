//! Pure core for RABA: the `.raba` format, layout, compositing and rendering.
//!
//! No I/O, no syscalls, no dependencies. Everything here is a transformation from
//! data to data, which is what makes it testable on a laptop in milliseconds rather
//! than in a virtual machine. See `docs/maintenance.md`.
//!
//! The impure parts — acquiring a terminal, the control socket, state on disk —
//! live in the `raba` binary, behind a `Surface` abstraction, and are kept small
//! enough to review line by line.

pub mod format;
pub mod render;
pub mod surface;

pub use format::{decode, encode, DecodeError, Frame, Movie, Op, Palette};
pub use render::{to_ansi, to_ansi_into, Cell, Grid};
pub use surface::{Size, Surface};
