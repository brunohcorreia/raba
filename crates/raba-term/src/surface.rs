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
/// Asked of the terminal with `TIOCGWINSZ`, falling back to `COLUMNS`/`LINES` and
/// then to 80x25 — the same fallback the boot path uses when the ioctl fails.
///
/// This was environment-only at first, on the reasoning in `DEPENDENCIES.md` that
/// the size was "a question it can mostly guess". That was wrong, and visibly so:
/// shells set `COLUMNS` as a shell variable and do not export it, so the guess was
/// not mostly right, it was always 80x25, and a preview on a large terminal drew
/// into a corner of it. The allowance was reserved for exactly this ioctl.
#[derive(Debug)]
pub struct StdoutSurface {
    out: Stdout,
    size: Size,
}

impl StdoutSurface {
    #[must_use]
    pub fn new() -> Self {
        StdoutSurface { out: io::stdout(), size: detect_size() }
    }
}

impl Default for StdoutSurface {
    fn default() -> Self {
        Self::new()
    }
}

/// `COLUMNS` and `LINES`, where they are set and usable.
///
/// Checked before the ioctl rather than after: a shell does not normally export
/// these, so one that *is* exported was set deliberately and should win. It is
/// also the only way to ask for a size other than the real one, which tests and
/// scripts need.
fn size_from_environment() -> (Option<u16>, Option<u16>) {
    let read = |name: &str| {
        std::env::var(name).ok().and_then(|v| v.parse::<u16>().ok()).filter(|value| *value > 0)
    };
    (read("COLUMNS"), read("LINES"))
}

/// Ask the terminal how big it is.
///
/// `None` when stdout is not a terminal at all — a pipe, a file, a test harness —
/// which is not an error and is why this is an `Option` rather than a `Result`.
fn size_from_terminal() -> Option<Size> {
    let winsize = rustix::termios::tcgetwinsize(io::stdout()).ok()?;
    // A terminal that reports zero in either axis is telling us it does not know;
    // some pseudo-terminals do this before they are fully set up.
    if winsize.ws_col == 0 || winsize.ws_row == 0 {
        return None;
    }
    Some(Size { columns: winsize.ws_col, lines: winsize.ws_row })
}

/// The size to draw at: what was asked for, else what the terminal says, else the
/// fallback the boot path uses.
///
/// Public because generating art needs it too, not just drawing it: an animation
/// built at 80x25 and then centred on a large terminal fills a box in the middle,
/// which looks exactly like the bug this function was added to fix.
#[must_use]
pub fn detect_size() -> Size {
    let (columns, lines) = size_from_environment();
    let asked = size_from_terminal().unwrap_or(Size::FALLBACK);
    Size {
        columns: columns.unwrap_or(asked.columns),
        lines: lines.unwrap_or(asked.lines),
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
