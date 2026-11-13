//! Noticing that something else owns the screen.
//!
//! The real check is not here. It is in [`crate::vt`]: if the terminal RABA
//! would use is already in `KD_GRAPHICS`, something owns it, and RABA yields
//! without needing to know what. That catches every competitor including the
//! ones nobody has written yet, and it is what detects the specific failure this
//! project is afraid of — a `VT_WAITACTIVE` that hangs against a terminal stuck
//! in graphics mode.
//!
//! What is here is the **named** checks, and they exist for one reason: to turn
//! "the terminal is busy" into "Plymouth is running". A log line a user can act
//! on is worth a few lines of code. See `docs/coexistence.md`.
//!
//! # In the binary, not in the service files
//!
//! It is tempting to express this per init system — a systemd `Condition`, an
//! OpenRC `start_pre`, a guard at the top of a runit `run` script. That is six
//! implementations of one rule, five of which will drift.
//!
//! So the check runs here, before the terminal is touched, on every start,
//! whoever started it. Every init system gets it for free and none of them need
//! to know it exists. The systemd units still carry
//! `ConditionPathExists=!/run/plymouth.pid`, because a skipped unit reads
//! usefully in `systemctl status` — a diagnostic nicety on top of the real
//! check, not a substitute for it.
//!
//! # Runtime detection is the backstop, not the plan
//!
//! A configuration that gets this far is one the initramfs hook should have
//! refused at build time, where the user is at their keyboard and can read the
//! error. By the time this fires they are looking at a boot.

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

/// Who else is already here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conflict {
    /// `/run/plymouth.pid` exists.
    PlymouthPidFile(Option<u32>),
    /// A process named `plymouthd` is running, pid file or not.
    Plymouthd(u32),
    /// Another `raba` answered on the control socket.
    ///
    /// Not the same as finding a *state* file: a state file means an instance
    /// that is gone and is to be resumed from, and this means one that is still
    /// there and is to be left alone.
    Raba,
}

impl std::fmt::Display for Conflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PlymouthPidFile(Some(pid)) => {
                write!(f, "Plymouth is running (pid {pid}); see docs/coexistence.md")
            }
            Self::PlymouthPidFile(None) => {
                write!(f, "Plymouth left a pid file; see docs/coexistence.md")
            }
            Self::Plymouthd(pid) => {
                write!(f, "plymouthd is running (pid {pid}); see docs/coexistence.md")
            }
            Self::Raba => write!(f, "another raba is already running"),
        }
    }
}

/// Where to look. Parameterised so the check is testable without a Plymouth.
#[derive(Debug, Clone)]
pub struct Where {
    pub plymouth_pid: PathBuf,
    pub proc: PathBuf,
    pub socket: PathBuf,
}

impl Default for Where {
    fn default() -> Self {
        Where {
            plymouth_pid: PathBuf::from("/run/plymouth.pid"),
            proc: PathBuf::from("/proc"),
            socket: PathBuf::from(crate::control::SOCKET),
        }
    }
}

/// Is somebody else already drawing?
///
/// Cheap: one `stat`, one directory read, one connect. It runs once per start,
/// before anything is acquired.
#[must_use]
pub fn detect(places: &Where) -> Option<Conflict> {
    if let Ok(text) = std::fs::read_to_string(&places.plymouth_pid) {
        return Some(Conflict::PlymouthPidFile(text.trim().parse().ok()));
    }
    // A pid file is the usual sign and not the only one: plymouthd can be
    // started by hand, and a crashed one can leave no file at all.
    if let Some(pid) = named_process(&places.proc, "plymouthd") {
        return Some(Conflict::Plymouthd(pid));
    }
    if answers(&places.socket) {
        return Some(Conflict::Raba);
    }
    None
}

/// The pid of a running process with this name, if there is one.
fn named_process(proc: &Path, name: &str) -> Option<u32> {
    let entries = std::fs::read_dir(proc).ok()?;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(pid) = file_name.to_str().and_then(|text| text.parse::<u32>().ok()) else {
            continue;
        };
        // `comm` rather than `cmdline`: it is the name the kernel knows the
        // process by, it is one short line, and it is not something the process
        // can set to whatever it likes as easily as `argv[0]`.
        let Ok(comm) = std::fs::read_to_string(entry.path().join("comm")) else {
            continue;
        };
        if comm.trim() == name {
            return Some(pid);
        }
    }
    None
}

/// Whether a live `raba` answers on the control socket.
///
/// A socket file left by a dead instance gives `ECONNREFUSED`, and a socket that
/// accepts but says nothing is not one of ours — so the test is an actual `ping`
/// and an actual `+OK`, not the existence of a file.
fn answers(socket: &Path) -> bool {
    let Ok(mut stream) = std::os::unix::net::UnixStream::connect(socket) else {
        return false;
    };
    let timeout = std::time::Duration::from_millis(200);
    if stream.set_read_timeout(Some(timeout)).is_err() || stream.write_all(b"ping\n").is_err() {
        return false;
    }
    let mut reply = [0u8; 3];
    stream.read_exact(&mut reply).is_ok() && &reply == b"+OK"
}
