//! `/run/raba.state`, and why being killed stops being an event.
//!
//! RABA starts in the initramfs and has to survive `switch_root`, which systemd
//! describes as "a killing spree of all running processes". Plymouth survives it
//! with `KillMode=none`, `SendSIGKILL=no`, a cgroup it lives outside of and a
//! dedicated re-rooting unit — all of it systemd-specific, for a requirement that
//! is not.
//!
//! The alternative is to stop trying to survive. The state RABA needs to continue
//! is five fields; written to a tmpfs that `/run` carries across `switch_root`,
//! they turn "did we die" into a question with the same answer either way:
//!
//! ```text
//! start
//!   ├─ a fresh state file ──► adopt that terminal, continue from that frame
//!   └─ no state file ───────► acquire a terminal, draw frame 0
//! ```
//!
//! Survival becomes an optimisation. If RABA lives through the switch, nothing
//! happens. If it is killed — by `switch_root`, by the OOM killer, by an init
//! system nobody anticipated — the next instance picks up where it stopped, and
//! the screen held the last frame in the meantime because nothing cleared it. A
//! brief pause rather than a flash of black. See `docs/early-boot.md`.
//!
//! # Reading it is not trusting it
//!
//! `security.md` finding 3: whoever can write this file chooses which terminal a
//! root process draws on. `/run` is root-only this early, so the exposure is
//! small — which is a reason to be careful rather than a reason to skip it.
//!
//! Every field is checked against something outside the file: the terminal
//! against the kernel's range, the frame against the theme that was actually
//! loaded, the timestamp against the clock. A file that fails any check is
//! **deleted and treated as a fresh start**, never partially believed. There is
//! no path where half a state file becomes half a resume.
//!
//! And `O_NOFOLLOW` on both ends, for finding 4: a symlink planted at this path
//! is a root process writing wherever the symlink points.

use crate::lifecycle::Phase;
use std::fmt::Write as _;
use std::io::{self, Read as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

/// Where it lives. See `man 8 raba`.
pub const PATH: &str = "/run/raba.state";

/// First line, so a file that is not ours is rejected before it is parsed.
const MAGIC: &str = "raba-state 1";

/// `O_NOFOLLOW` on Linux. Spelled out next to the reason, as `O_NOCTTY` is in
/// [`crate::vt`].
const O_NOFOLLOW: i32 = 0o400_000;

/// How old a state file may be and still be resumed from.
///
/// It is written every frame, so a fresh one is milliseconds old. This is the
/// gap a `switch_root` is allowed to take — generous, because the cost of being
/// wrong in this direction is a brief pause and the cost of being wrong in the
/// other is a splash that appears on a terminal somebody is now logged into.
pub const FRESH: u64 = 10_000;

/// The most a state file may be. Ours are under 200 bytes.
const LIMIT: usize = 4096;

/// The kernel's `MAX_NR_CONSOLES`. A terminal number outside this is not a
/// terminal, whatever the file says.
pub const MAX_TERMINAL: u16 = 63;

/// What one instance leaves for the next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// The terminal being drawn on, to be adopted rather than re-acquired.
    pub terminal: u16,
    /// The terminal to switch back to at the end.
    pub previous: u16,
    /// Where in the animation we were.
    pub frame: u32,
    pub phase: Phase,
    pub theme: PathBuf,
    /// Milliseconds since boot, from `CLOCK_BOOTTIME`.
    ///
    /// Since *boot*, not since the epoch: a wall clock early in boot is whatever
    /// the RTC said, and may jump by years the moment NTP starts. A monotonic
    /// clock that counts from power-on cannot be dragged backwards into making a
    /// stale file look fresh.
    pub written_ms: u64,
}

/// Why a state file was not used. Each of these deletes the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invalid {
    NotOurs,
    Missing(&'static str),
    Unreadable(&'static str),
    /// Outside `1..=63`, so not a terminal the kernel has.
    Terminal(u16),
    /// Past the end of the theme that was actually loaded.
    Frame {
        frame: u32,
        frames: u32,
    },
    /// Older than [`FRESH`], or claiming to be from the future.
    Stale {
        age_ms: u64,
    },
    /// Not an absolute path, so not something to open as root.
    Theme,
}

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotOurs => write!(f, "not a raba state file"),
            Self::Missing(key) => write!(f, "no {key}"),
            Self::Unreadable(key) => write!(f, "{key} is not readable as a value"),
            Self::Terminal(n) => write!(f, "tty{n} is outside 1..={MAX_TERMINAL}"),
            Self::Frame { frame, frames } => {
                write!(f, "frame {frame} of a theme with {frames}")
            }
            Self::Stale { age_ms } => write!(f, "written {age_ms}ms ago"),
            Self::Theme => write!(f, "theme is not an absolute path"),
        }
    }
}

/// Milliseconds since the machine powered on.
#[must_use]
pub fn now_ms() -> u64 {
    let time = rustix::time::clock_gettime(rustix::time::ClockId::Boottime);
    let seconds = u64::try_from(time.tv_sec).unwrap_or(0);
    let millis = u64::try_from(time.tv_nsec).unwrap_or(0) / 1_000_000;
    seconds.saturating_mul(1000).saturating_add(millis)
}

impl State {
    /// The file's contents.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(128);
        out.push_str(MAGIC);
        out.push('\n');
        // `write!` to a String cannot fail; the results are discarded rather
        // than unwrapped so that this stays panic-free by construction.
        let _ = writeln!(out, "terminal={}", self.terminal);
        let _ = writeln!(out, "previous={}", self.previous);
        let _ = writeln!(out, "frame={}", self.frame);
        let _ = writeln!(out, "phase={}", self.phase.name());
        let _ = writeln!(out, "written_ms={}", self.written_ms);
        // Last, because it is the only field that could contain anything
        // surprising, and because a parser that has already read everything
        // else cannot be confused by it.
        let _ = writeln!(out, "theme={}", self.theme.display());
        out
    }

    /// Parse and validate in one step, against the world outside the file.
    ///
    /// # Errors
    ///
    /// Any field that is missing, unparseable, or disagrees with the kernel, the
    /// clock or the loaded theme. The caller deletes the file and starts fresh;
    /// see [`load`].
    pub fn parse(text: &str, frames: u32, now: u64) -> Result<State, Invalid> {
        let mut lines = text.lines();
        if lines.next() != Some(MAGIC) {
            return Err(Invalid::NotOurs);
        }

        let mut terminal = None;
        let mut previous = None;
        let mut frame = None;
        let mut phase = None;
        let mut written_ms = None;
        let mut theme = None;

        for line in lines {
            let Some((key, value)) = line.split_once('=') else {
                // A line with no `=` is ignored rather than fatal, which is the
                // same rule as an unknown key: a future version may write
                // something this one has no opinion about.
                continue;
            };
            match key {
                "terminal" => terminal = Some(value.parse::<u16>().ok()),
                "previous" => previous = Some(value.parse::<u16>().ok()),
                "frame" => frame = Some(value.parse::<u32>().ok()),
                "written_ms" => written_ms = Some(value.parse::<u64>().ok()),
                "phase" => phase = Some(phase_named(value)),
                "theme" => theme = Some(Some(PathBuf::from(value))),
                _ => {}
            }
        }

        let terminal = want(terminal, "terminal")?;
        let previous = want(previous, "previous")?;
        let frame = want(frame, "frame")?;
        let phase = want(phase, "phase")?;
        let written_ms = want(written_ms, "written_ms")?;
        let theme: PathBuf = want(theme, "theme")?;

        // Checked against the kernel, the theme and the clock — never against
        // the file, which is the thing that is not trusted.
        for number in [terminal, previous] {
            if number == 0 || number > MAX_TERMINAL {
                return Err(Invalid::Terminal(number));
            }
        }
        if frame >= frames {
            return Err(Invalid::Frame { frame, frames });
        }
        if !theme.is_absolute() {
            return Err(Invalid::Theme);
        }
        // A file claiming to be from the future is as wrong as one too old, and
        // is what a state file from a *previous* boot would look like if `/run`
        // ever turned out not to be a tmpfs.
        let age = now.checked_sub(written_ms).ok_or(Invalid::Stale { age_ms: 0 })?;
        if age > FRESH {
            return Err(Invalid::Stale { age_ms: age });
        }

        Ok(State { terminal, previous, frame, phase, theme, written_ms })
    }
}

fn want<T>(field: Option<Option<T>>, key: &'static str) -> Result<T, Invalid> {
    match field {
        None => Err(Invalid::Missing(key)),
        Some(None) => Err(Invalid::Unreadable(key)),
        Some(Some(value)) => Ok(value),
    }
}

fn phase_named(name: &str) -> Option<Phase> {
    [Phase::Waiting, Phase::Boot, Phase::Outro, Phase::Deactivated, Phase::Revealed, Phase::Done]
        .into_iter()
        .find(|phase| phase.name() == name)
}

/// Write the state, atomically, without following a symlink.
///
/// Atomic because it happens on every frame and a torn file read by the next
/// instance is exactly the failure this whole mechanism exists to avoid: write
/// to a staging name, then `rename`, which is indivisible.
///
/// # Errors
///
/// If `/run` is not writable, or the staging path exists as something we did not
/// create. The caller carries on without a state file: not being resumable is
/// worse than not drawing, and not drawing is already not worth stopping a boot.
pub fn save(path: impl AsRef<Path>, state: &State) -> io::Result<()> {
    crate::write::replace(path.as_ref(), state.render().as_bytes())
}

/// Read the state, if there is one worth having.
///
/// Returns `Ok(None)` when there is no file, which is the ordinary first boot.
/// A file that exists and fails any check is **deleted** and reported as the
/// reason, so the next start is a clean one rather than a repeat of the same
/// rejection.
///
/// # Errors
///
/// The [`Invalid`] that made it unusable. The caller logs it and starts fresh.
pub fn load(path: impl AsRef<Path>, frames: u32, now: u64) -> Result<Option<State>, Invalid> {
    let path = path.as_ref();
    let file = match std::fs::OpenOptions::new().read(true).custom_flags(O_NOFOLLOW).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        // `ELOOP` lands here: the path is a symlink, which we will not follow
        // and will not delete, because deleting it is also acting on somebody
        // else's choice of path.
        Err(_) => return Err(Invalid::NotOurs),
    };

    // A state file is seven short lines. Anything larger is not one — and
    // reading it unbounded would be the only unbounded read in the program.
    //
    // One byte past the limit is read on purpose: a file that *has* a byte there
    // is refused outright rather than parsed from a prefix, because a prefix of
    // something enormous that happens to start with a valid state is exactly the
    // file an attacker gets to choose.
    let mut text = String::new();
    let read = file.take(LIMIT.saturating_add(1) as u64).read_to_string(&mut text);
    if read.is_err() || text.len() > LIMIT {
        discard(path);
        return Err(Invalid::NotOurs);
    }
    match State::parse(&text, frames, now) {
        Ok(state) => Ok(Some(state)),
        Err(why) => {
            discard(path);
            Err(why)
        }
    }
}

/// Remove a state file we are not going to use.
///
/// Separate and named, because "delete it" is a decision and not a detail: a
/// rejected file that stays on disk is a rejection repeated on every start.
pub fn discard(path: impl AsRef<Path>) {
    let _ = std::fs::remove_file(path.as_ref());
}
