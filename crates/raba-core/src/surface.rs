//! The boundary between the pure core and anything that owns a screen.
//!
//! Everything above this trait is a function from data to data and is tested on a
//! laptop in milliseconds. Everything below it is syscalls, and is kept small enough
//! to review line by line. See `docs/maintenance.md`.
//!
//! Three implementations are planned, and the useful consequence is that `--preview`
//! is not a debug feature bolted on: it is the same core driving a different
//! surface. If preview looks right, the core is right, and only the VT layer
//! remains in doubt.
//!
//! | implementation | where | what it owns |
//! |---|---|---|
//! | `VtSurface` | `raba` | a real virtual terminal, via ioctls |
//! | `StdoutSurface` | `raba` | stdout, for `--preview` |
//! | [`BufferSurface`] | here, behind `testing` | nothing |

use std::io;

/// A terminal's dimensions in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub columns: u16,
    pub lines: u16,
}

impl Size {
    /// The classic console, and the fallback when the real size cannot be read.
    pub const FALLBACK: Size = Size { columns: 80, lines: 25 };
}

/// Something that can display a frame and give the terminal back.
pub trait Surface {
    /// The drawable area. May change between frames if the console is resized.
    fn size(&self) -> Size;

    /// Display one complete frame.
    ///
    /// **`frame` must be written in a single operation.** It is not called `write`
    /// because `write` invites incremental calls, and a renderer that emits escape
    /// sequences piecemeal tears and flickers: the terminal draws whatever has
    /// arrived by the time it is next scheduled, which is halfway through a frame.
    ///
    /// [`to_ansi_into`](crate::render::to_ansi_into) produces exactly one frame per
    /// call for this reason.
    fn present(&mut self, frame: &[u8]) -> io::Result<()>;

    /// Return the terminal to the state it was found in.
    ///
    /// Must be safe to call more than once, and must do as much as it can when part
    /// of it fails: a surface that gives up halfway through restoring is how a
    /// console is left in `KD_GRAPHICS` with no way back. See `docs/early-boot.md`.
    fn restore(&mut self) -> io::Result<()>;
}

#[cfg(feature = "testing")]
mod buffer;
#[cfg(feature = "testing")]
pub use buffer::BufferSurface;
