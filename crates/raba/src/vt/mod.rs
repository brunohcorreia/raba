//! Owning a virtual terminal, and giving it back.
//!
//! This is the part of RABA where a mistake does not look like a bug report. A
//! console left in the wrong mode is a machine whose owner sees nothing after
//! boot and cannot type; the project's first rule is that nothing it does may stop
//! a machine booting, and this module is where that rule is kept or broken.
//!
//! # The shape of the guarantee
//!
//! Almost everything here is about what RABA **does not do**.
//!
//! - **It never sets `KD_GRAPHICS`.** A process killed while a console is in
//!   graphics mode leaves a machine with no visible console and no way back, and
//!   `SIGKILL` cannot be caught, so no amount of cleanup code would help. RABA
//!   draws characters, so text mode is all it ever needs: "killed mid-frame leaves
//!   a usable terminal" is true by construction rather than by careful unwinding.
//! - **It never touches the keyboard mode.** Same argument. A console in `K_RAW`
//!   with nobody left to read from it is a machine you cannot type on.
//! - **It draws on a terminal of its own**, from [`ioctl::open_query`], and never
//!   on the one somebody is using. So anything it does leave wrong is confined to
//!   a terminal that is about to be given back, and the terminal the user was on
//!   is untouched apart from being switched away from.
//!
//! The one thing it does reach outside its own terminal for is where the kernel
//! prints. `TIOCL_SETKMSGREDIRECT` is a single global in the kernel, not a
//! property of a file descriptor, so RABA reads the old value and puts that exact
//! value back — and points it at the terminal the user came from, so that if RABA
//! is killed and never restores it, the messages are waiting on the terminal the
//! user lands on rather than lost or aimed at a terminal that no longer exists.
//!
//! What is left to restore is therefore small: which terminal is displayed, where
//! kernel messages print, and the terminal we borrowed. [`Vt::restore`] does both, is safe to call twice, and
//! carries on when a step fails — a restore that gives up halfway is exactly how a
//! console ends up stranded.
//!
//! # What it refuses
//!
//! Yielding is a feature. If another splash or a display server already owns the
//! screen ([`ioctl::KD_GRAPHICS`]), or there is no free terminal, RABA does not
//! draw. Two programs fighting over one console is worse than no splash at all.
//!
//! See `docs/early-boot.md` and `docs/coexistence.md`.

pub mod ioctl;

use raba_core::surface::{Size, Surface};
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, Write as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::time::{Duration, Instant};

/// The terminal used to ask about and switch terminals.
///
/// `/dev/tty0` is "whichever terminal is current", which is what the VT ioctls
/// want. It is not `/dev/console`: that may be a serial line, and asking a serial
/// line to allocate a virtual terminal is a category error.
const CONTROL: &str = "/dev/tty0";

/// Open a console without adopting it as a controlling terminal.
///
/// `O_NOCTTY` is not a detail. RABA runs as PID 1 in an initramfs, which makes it
/// a session leader with no controlling terminal — and a session leader that opens
/// a terminal without this flag *acquires* it. Two consequences, both bad: the
/// splash starts receiving the terminal's signals, and the kernel keeps a
/// reference that makes `VT_DISALLOCATE` refuse with `EBUSY`, so the terminal is
/// never handed back.
const O_NOCTTY: i32 = 0o400;

/// How long to keep asking for the borrowed terminal back.
///
/// `VT_DISALLOCATE` refuses a terminal that is still in use, and after a switch
/// away from one the kernel releases it a moment later rather than during the
/// switch — measured at roughly 600 microseconds on a 7.3 kernel, where an open
/// and close with no switch in between frees it in 50. So the first attempt
/// essentially always fails and the second essentially always succeeds.
///
/// Fifty milliseconds is a ceiling nothing is expected to reach, and it is spent
/// after the display is already back where it belongs: the only thing still
/// waiting is the terminal's few kilobytes. `crates/raba-vmtest` measures the real
/// number on every run, so this budget stops being a guess the moment a kernel
/// changes it.
const RETURN_BUDGET: Duration = Duration::from_millis(50);
const RETURN_INTERVAL: Duration = Duration::from_micros(500);

/// Hand a terminal back, waiting out the kernel's brief hold on it.
fn give_back<F: rustix::fd::AsFd>(control: F, number: u16) -> Result<(), rustix::io::Errno> {
    let deadline = Instant::now().checked_add(RETURN_BUDGET);
    loop {
        match ioctl::disallocate(&control, number) {
            Ok(()) => return Ok(()),
            Err(rustix::io::Errno::BUSY) => {}
            Err(other) => return Err(other),
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(rustix::io::Errno::BUSY);
        }
        std::thread::sleep(RETURN_INTERVAL);
    }
}

/// Move kernel messages off the terminal RABA is about to draw on.
///
/// Returns what the redirect was set to, and `None` when nothing was changed and
/// so nothing needs putting back.
///
/// `aside` is the terminal the user was on, which is the only sensible target:
/// it exists, RABA never deallocates it, and it is where the user ends up — so
/// an `EXT4-fs error` that printed while the splash was up is still on screen
/// when they get there. Aiming them at the borrowed terminal instead would hide
/// them behind a terminal that is about to cease to exist.
///
/// Every failure is silent and harmless. Without `CAP_SYS_ADMIN` the messages
/// simply stay where they were, which costs a tidy splash and nothing else; a
/// splash that refused to draw over it would be trading the whole feature for
/// one of its details.
fn move_kernel_messages(control: &File, aside: u16) -> Option<u8> {
    // The kernel's field is one byte, and 0 is not a terminal — it is the
    // "follow the display" default, which is the thing being moved away from.
    let aside = u8::try_from(aside).ok().filter(|number| *number > 0)?;
    let before = ioctl::kmsg_redirect(control).ok()?;
    if before == aside {
        return None;
    }
    ioctl::set_kmsg_redirect(control, aside).ok()?;
    Some(before)
}

fn open_console(path: &str) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        // 0o400 on Linux. Spelled out rather than pulled from a crate so the one
        // flag this depends on is visible next to the reason it is needed.
        .custom_flags(O_NOCTTY)
        .open(path)
}

/// Why RABA is not drawing.
#[derive(Debug)]
pub enum Error {
    /// There is no VT subsystem to talk to, or we may not open it. On a machine
    /// booted with `console=ttyS0` alone this is the normal case, not a fault.
    NoConsole(io::Error),
    /// Something already owns the screen in graphics mode.
    ScreenIsTaken,
    /// Every virtual terminal is in use.
    NoFreeTerminal,
    /// An ioctl failed.
    Ioctl(&'static str, rustix::io::Errno),
    /// The terminal was allocated but could not be opened.
    Open(u16, io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoConsole(e) => write!(f, "no virtual terminal available ({CONTROL}: {e})"),
            Self::ScreenIsTaken => {
                write!(f, "the console is in graphics mode; something else owns the screen")
            }
            Self::NoFreeTerminal => write!(f, "every virtual terminal is in use"),
            Self::Ioctl(name, errno) => write!(f, "{name}: {errno}"),
            Self::Open(number, e) => write!(f, "/dev/tty{number}: {e}"),
        }
    }
}

impl std::error::Error for Error {}

/// A virtual terminal RABA has borrowed.
#[derive(Debug)]
pub struct Vt {
    /// `/dev/tty0`, kept open for the duration: the switching ioctls go here.
    control: File,
    /// `/dev/tty<number>`, what frames are written to.
    terminal: File,
    number: u16,
    /// The terminal that was displayed when we arrived, to go back to.
    previous: u16,
    /// Where kernel messages printed before we moved them, if we moved them.
    /// `None` means there is nothing to put back — either the redirect already
    /// pointed where we wanted it, or we were not allowed to change it.
    kmsg: Option<u8>,
    restored: bool,
}

impl Vt {
    /// Take a free terminal and switch the display to it.
    ///
    /// # Errors
    ///
    /// Every failure here means "do not draw", and none of them is worth stopping
    /// a boot over. The caller's job is to report and carry on.
    pub fn acquire() -> Result<Vt, Error> {
        Vt::start(None)
    }

    /// Take the terminal the configuration named, whatever is on it.
    ///
    /// `Boot.Tty=n`, and `raba.tty=n` on the kernel command line. Unlike
    /// [`acquire`](Self::acquire) this can land on a terminal somebody is
    /// using — which is what asking for one by number means, and why `auto` is
    /// the default.
    ///
    /// # Errors
    ///
    /// The same as [`acquire`](Self::acquire), minus [`NoFreeTerminal`] — no
    /// terminal is allocated — plus the terminal simply not opening.
    ///
    /// [`NoFreeTerminal`]: Error::NoFreeTerminal
    pub fn take(number: u16) -> Result<Vt, Error> {
        Vt::start(Some(number))
    }

    /// Both of the above. `wanted` is `None` for "any free one".
    fn start(wanted: Option<u16>) -> Result<Vt, Error> {
        let control = open_console(CONTROL).map_err(Error::NoConsole)?;

        // Asked before anything is changed. A console in graphics mode belongs to
        // something else, and the polite thing — also the only thing that cannot
        // make the screen worse — is to leave.
        let mode = ioctl::get_mode(&control).map_err(|e| Error::Ioctl("KDGETMODE", e))?;
        if mode == ioctl::KD_GRAPHICS {
            return Err(Error::ScreenIsTaken);
        }

        let state = ioctl::get_state(&control).map_err(|e| Error::Ioctl("VT_GETSTATE", e))?;
        let previous = state.active;

        let number = match wanted {
            Some(number) => number,
            None => ioctl::open_query(&control)
                .map_err(|e| Error::Ioctl("VT_OPENQRY", e))?
                .ok_or(Error::NoFreeTerminal)?,
        };

        let path = format!("/dev/tty{number}");
        let terminal = open_console(&path).map_err(|e| Error::Open(number, e))?;

        // Before the switch, not after: between `VT_ACTIVATE` and this call the
        // display would already be ours, and a message arriving in that window
        // would print on top of the first frame.
        //
        // `Boot.Tty` can name the terminal that is already displayed, and then
        // there is nowhere to move the messages *to*: aside is where the user
        // is, and the user is here. `0` means "do not move them", and they print
        // over the art exactly as they would with no splash installed. That is
        // what `quiet loglevel=3` is for, and `man 5 raba.conf` says so under
        // `Boot.Tty`.
        let aside = if number == previous { 0 } else { previous };
        let kmsg = move_kernel_messages(&control, aside);

        let mut vt = Vt { control, terminal, number, previous, kmsg, restored: false };

        // From here on a failure has to undo what has already been done, which is
        // what `restore` is for. It is why the struct is built before the switch.
        if let Err(errno) = ioctl::activate(&vt.control, number) {
            let _ = vt.restore();
            return Err(Error::Ioctl("VT_ACTIVATE", errno));
        }
        if let Err(errno) = ioctl::wait_active(&vt.control, number) {
            let _ = vt.restore();
            return Err(Error::Ioctl("VT_WAITACTIVE", errno));
        }
        Ok(vt)
    }

    /// Take over a terminal a previous instance was already using.
    ///
    /// The resume path. `/run/raba.state` names a terminal and a frame; this is
    /// the half that picks the terminal back up, and the point of it is that
    /// **nothing is cleared**. The screen is still showing the last frame the
    /// dead instance drew, so continuing from the next one looks like a pause
    /// rather than a flash of black. Acquiring a fresh terminal instead would
    /// make every `switch_root` visible.
    ///
    /// # Errors
    ///
    /// The same reasons as [`acquire`](Self::acquire), minus the ones that only
    /// apply to allocating — there is nothing to allocate.
    pub fn adopt(terminal: u16, previous: u16) -> Result<Vt, Error> {
        let control = open_console(CONTROL).map_err(Error::NoConsole)?;

        let mode = ioctl::get_mode(&control).map_err(|e| Error::Ioctl("KDGETMODE", e))?;
        if mode == ioctl::KD_GRAPHICS {
            return Err(Error::ScreenIsTaken);
        }

        let path = format!("/dev/tty{terminal}");
        let file = open_console(&path).map_err(|e| Error::Open(terminal, e))?;

        // Kernel messages were moved aside by the instance that died, and there
        // is no way left to find out what the redirect was before that — the
        // process that knew is gone. If it still points where we would have put
        // it, take it as ours and put back the kernel's default on the way out.
        // `0` is a guess, and it is the only defensible one: it is what the
        // kernel boots with and what anything that has not touched it has.
        let kmsg = ioctl::kmsg_redirect(&control)
            .ok()
            .filter(|current| u16::from(*current) == previous)
            .map(|_| 0u8);

        let mut vt =
            Vt { control, terminal: file, number: terminal, previous, kmsg, restored: false };

        // Usually a no-op: the display is already here, which is why the screen
        // still shows the last frame. It is asked for anyway, because "usually"
        // is not a thing to leave a boot resting on.
        let state = ioctl::get_state(&vt.control).map_err(|e| Error::Ioctl("VT_GETSTATE", e))?;
        if state.active != terminal {
            if let Err(errno) = ioctl::activate(&vt.control, terminal) {
                let _ = vt.restore();
                return Err(Error::Ioctl("VT_ACTIVATE", errno));
            }
            if let Err(errno) = ioctl::wait_active(&vt.control, terminal) {
                let _ = vt.restore();
                return Err(Error::Ioctl("VT_WAITACTIVE", errno));
            }
        }
        Ok(vt)
    }

    /// Which terminal was taken. `/dev/tty<this>`.
    #[must_use]
    pub fn number(&self) -> u16 {
        self.number
    }

    /// Which terminal was displayed before, and will be again.
    #[must_use]
    pub fn previous(&self) -> u16 {
        self.previous
    }

    /// Whether the terminal is in text mode, which it should never not be.
    ///
    /// Exposed for the virtual-machine tests, which assert it across the whole
    /// lifecycle: the claim that RABA never leaves text mode is worth checking
    /// rather than asserting in a comment.
    ///
    /// # Errors
    ///
    /// If `KDGETMODE` fails on the borrowed terminal.
    pub fn mode(&self) -> Result<i32, Error> {
        ioctl::get_mode(&self.terminal).map_err(|e| Error::Ioctl("KDGETMODE", e))
    }

    /// Has something else taken the screen?
    ///
    /// Two ways for that to be true, and a splash has to notice both:
    ///
    /// - the display moved to another terminal. A display manager that takes a
    ///   VT of its own, a compositor started from a login on tty1, or a person
    ///   pressing Ctrl+Alt+F2 because the boot looks stuck.
    /// - *our* terminal went into graphics mode. That is what a compositor
    ///   taking the terminal we are on looks like — there is no switch to see,
    ///   the mode under us changes. `docs/coexistence.md` uses the same check
    ///   to decide not to start.
    ///
    /// This is how the splash ends on a machine with no display manager and no
    /// way to call `raba-ctl`: the control socket is root-only on purpose, and
    /// widening it so a user session could say "quit" would trade the most
    /// sensitive boundary in the program for a convenience the kernel already
    /// provides. Nothing here is specific to systemd, to Wayland or to any
    /// display manager, which is the whole reason it is this check and not an
    /// integration.
    ///
    /// An ioctl that fails answers `None`: a question that cannot be asked is
    /// not evidence that the screen was taken, and the clock is the backstop.
    #[must_use]
    pub fn displaced(&self) -> Option<Displaced> {
        if ioctl::get_mode(&self.terminal).is_ok_and(|mode| mode == ioctl::KD_GRAPHICS) {
            return Some(Displaced::Graphics);
        }
        match ioctl::get_state(&self.control) {
            Ok(state) if state.active != self.number => Some(Displaced::Switched(state.active)),
            _ => None,
        }
    }

    /// Give the terminal up without taking the screen back.
    ///
    /// The counterpart to [`restore`](Vt::restore) for the case [`displaced`]
    /// reports: something else is on the screen now, and every step `restore`
    /// takes to put the display back would take it away from whatever just
    /// arrived. So no `VT_ACTIVATE`, no mode change, no escape sequence written
    /// to a terminal that may be mid-handover.
    ///
    /// Two things still happen, because they are ours to undo: kernel messages
    /// go back where they were printing, and a terminal we opened and nobody
    /// took is handed back to the kernel. A terminal that went into graphics
    /// mode is *not* handed back — somebody is using it.
    ///
    /// [`displaced`]: Vt::displaced
    pub fn relinquish(&mut self, why: Displaced) -> io::Result<()> {
        if self.restored {
            return Ok(());
        }
        self.restored = true;

        let mut first: Option<io::Error> = None;
        let mut note = |result: Result<(), rustix::io::Errno>| {
            if let Err(errno) = result {
                if first.is_none() {
                    first = Some(io::Error::from(errno));
                }
            }
        };

        if let Some(before) = self.kmsg.take() {
            note(ioctl::set_kmsg_redirect(&self.control, before));
        }
        if let Ok(null) = File::open("/dev/null") {
            drop(std::mem::replace(&mut self.terminal, null));
        }
        if matches!(why, Displaced::Switched(_)) {
            note(give_back(&self.control, self.number));
        }

        match first {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

/// Why the screen stopped being ours. See [`Vt::displaced`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Displaced {
    /// The display is on another terminal now, this one.
    Switched(u16),
    /// Our own terminal is in graphics mode: something is drawing on it that is
    /// not us.
    Graphics,
}

impl fmt::Display for Displaced {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Switched(active) => write!(f, "the display moved to tty{active}"),
            Self::Graphics => write!(f, "the terminal went into graphics mode"),
        }
    }
}

impl Surface for Vt {
    fn size(&self) -> Size {
        // The console's real size, which is the screen divided by the loaded
        // font's cell. A console that will not say falls back to 80x25 rather
        // than to nothing: see docs/console-size.md.
        rustix::termios::tcgetwinsize(&self.terminal)
            .ok()
            .filter(|winsize| winsize.ws_col > 0 && winsize.ws_row > 0)
            .map_or(Size::FALLBACK, |winsize| Size {
                columns: winsize.ws_col,
                lines: winsize.ws_row,
            })
    }

    fn present(&mut self, frame: &[u8]) -> io::Result<()> {
        // One write, as the trait requires: a console draws whatever has arrived
        // when it is next scheduled, so a frame delivered in pieces tears.
        self.terminal.write_all(frame)?;
        self.terminal.flush()
    }

    fn restore(&mut self) -> io::Result<()> {
        if self.restored {
            return Ok(());
        }
        self.restored = true;

        // Every step is attempted even when an earlier one failed, and the first
        // error is what gets reported. A restore that returns early is how a
        // console is left on a terminal nobody can see.
        let mut first: Option<io::Error> = None;
        let mut note = |result: Result<(), rustix::io::Errno>| {
            if let Err(errno) = result {
                if first.is_none() {
                    first = Some(io::Error::from(errno));
                }
            }
        };

        // Attributes first, while the terminal is still ours and still displayed:
        // reset colours, and show the cursor in case it was hidden.
        let _ = self.terminal.write_all(b"\x1b[0m\x1b[?25h");
        let _ = self.terminal.flush();

        note(ioctl::activate(&self.control, self.previous));
        note(ioctl::wait_active(&self.control, self.previous));

        // After the switch, never before. The saved value is often 0, which means
        // "print on whichever terminal is displayed" — putting that back while the
        // display is still ours aims the next message at the splash.
        if let Some(before) = self.kmsg.take() {
            note(ioctl::set_kmsg_redirect(&self.control, before));
        }

        // `VT_DISALLOCATE` refuses a terminal that is still open, so the handle
        // goes first. Replacing it keeps `self` whole rather than needing an
        // `Option` everywhere else.
        if let Ok(null) = File::open("/dev/null") {
            let open = std::mem::replace(&mut self.terminal, null);
            drop(open);
        }
        // Last, and the only step whose failure is not worth much: by now the
        // display is back where it belongs and the person in front of the machine
        // sees the right thing. A terminal that could not be returned costs a few
        // kilobytes until the next boot.
        note(give_back(&self.control, self.number));

        match first {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl Drop for Vt {
    /// The ordinary exit path, and the one that covers an early `return`.
    ///
    /// Not the only one that matters and not the one to rely on: `panic = "abort"`
    /// in release means a panic never gets here, and `SIGKILL` never gets
    /// anywhere. Those are handled by not needing cleanup — see the module
    /// documentation on why nothing is left in a state that needs undoing.
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
