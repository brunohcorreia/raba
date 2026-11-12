//! Surfaces that own a real terminal.
//!
//! Only [`StdoutSurface`] so far. `VtSurface`, the one that takes over a virtual
//! terminal with ioctls, arrives in M3 behind the VM harness — see
//! `docs/milestones.md` for why it is built last and tested first.

use raba_core::surface::{Size, Surface};
use std::io::{self, Stdout, Write};

/// Draws to stdout, for `--preview`.
///
/// # Size
///
/// Reading the real terminal size needs `TIOCGWINSZ`, which needs a syscall wrapper,
/// which is the one dependency `raba` is allowed and has not spent yet. Until the VT
/// work needs that ioctl anyway, this reads `COLUMNS` and `LINES` if the shell
/// exported them and otherwise assumes 80x25 — the same fallback the boot path will
/// use when the ioctl fails. See `DEPENDENCIES.md`.
#[derive(Debug)]
pub struct StdoutSurface {
    out: Stdout,
    size: Size,
}

impl StdoutSurface {
    #[must_use]
    pub fn new() -> Self {
        StdoutSurface { out: io::stdout(), size: size_from_environment() }
    }
}

impl Default for StdoutSurface {
    fn default() -> Self {
        Self::new()
    }
}

fn size_from_environment() -> Size {
    let read = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<u16>().ok());
    Size {
        columns: read("COLUMNS").filter(|&c| c > 0).unwrap_or(Size::FALLBACK.columns),
        lines: read("LINES").filter(|&l| l > 0).unwrap_or(Size::FALLBACK.lines),
    }
}

impl Surface for StdoutSurface {
    fn size(&self) -> Size {
        self.size
    }

    fn present(&mut self, frame: &[u8]) -> io::Result<()> {
        // One write, then one flush. Handing the terminal a frame in pieces is what
        // makes an animation tear; see `raba_core::render` on why the method is
        // called `present`.
        let mut handle = self.out.lock();
        handle.write_all(frame)?;
        handle.flush()
    }

    fn restore(&mut self) -> io::Result<()> {
        // Reset colours and leave the cursor on a fresh line, so a shell prompt does
        // not land on top of the last frame.
        let mut handle = self.out.lock();
        handle.write_all(b"\x1b[0m\n")?;
        handle.flush()
    }
}
