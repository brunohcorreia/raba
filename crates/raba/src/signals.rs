//! Signals, without a signal handler.
//!
//! `std` has no signal API, so this is why RABA's dependency budget went from
//! one direct crate to two. `libc` was the cheaper of the two candidates by a
//! long way: `signal-hook` would have brought `signal-hook-registry`, `arc-swap`
//! **and** `libc` anyway, where `libc` on its own has no transitive dependencies
//! and adds no bytes at all — the boot binary is statically linked against musl,
//! so the libc these bindings bind to is already in the file.
//!
//! # No handler, which is the point
//!
//! The usual shape is a handler that sets an `AtomicBool` and a loop that reads
//! it, and every line of that is a compromise with async-signal-safety. The
//! shape here is different: the signals are **blocked**, so they are never
//! delivered and no handler ever runs, and the frame loop asks the kernel once a
//! frame whether one is pending. `sigtimedwait` with a zero timeout is a poll,
//! exactly like the socket accept next to it in the same loop.
//!
//! Nothing runs asynchronously, nothing is reentrant, and a signal is handled at
//! a frame boundary — which is where RABA wants to handle it anyway, because the
//! terminal is in a known state there.
//!
//! # The escape hatch
//!
//! Blocking `SIGTERM` means the default disposition never runs, so a RABA whose
//! loop was somehow stuck would ignore it. For a program whose first rule is not
//! to break a boot, that is not acceptable on its own.
//!
//! So the block is lifted the moment the first signal arrives. The first
//! `SIGTERM` is handled gracefully; a **second** one is delivered the ordinary
//! way and kills the process outright. Anyone holding down `Ctrl-C`, or any init
//! system that escalates, gets what they expect, and nothing has to go right
//! inside RABA for it to work.

#![allow(
    unsafe_code,
    reason = "libc's signal functions are the second and last unsafe in the project"
)]

use std::io;

/// What arrived. Only the signals RABA has an opinion about are blocked at all;
/// everything else keeps its default behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// `SIGTERM` or `SIGINT`: stop now, tidily.
    Terminate,
    /// `SIGUSR1`: start the outro, as though `raba-ctl quit` had been called.
    /// Documented in `man 8 raba`.
    Quit,
}

/// The signals RABA takes over, and nothing else.
const WATCHED: [i32; 3] = [libc::SIGTERM, libc::SIGINT, libc::SIGUSR1];

/// A poll-able source of signals.
#[derive(Debug)]
pub struct Signals {
    /// Whether the signals are still blocked. Goes false after the first one,
    /// so the next is fatal. See the module documentation.
    armed: bool,
}

impl Signals {
    /// Block the watched signals so they can be polled for instead of delivered.
    ///
    /// # Errors
    ///
    /// If `sigprocmask` fails, which on Linux means the arguments were wrong —
    /// a bug here, not a condition. The caller runs without signal handling
    /// rather than refusing to start.
    pub fn arm() -> io::Result<Signals> {
        // SAFETY: `set` is a `sigset_t` this function owns and initialises with
        // `sigemptyset` before anything reads it; `sigaddset` and `sigprocmask`
        // take pointers to it that do not outlive this call. The null `oldset`
        // is explicitly allowed by `sigprocmask(2)`.
        let ok = unsafe {
            let mut set = std::mem::MaybeUninit::<libc::sigset_t>::uninit();
            if libc::sigemptyset(set.as_mut_ptr()) != 0 {
                return Err(io::Error::last_os_error());
            }
            let mut set = set.assume_init();
            for signal in WATCHED {
                if libc::sigaddset(&raw mut set, signal) != 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            libc::sigprocmask(libc::SIG_BLOCK, &raw const set, std::ptr::null_mut())
        };
        if ok != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Signals { armed: true })
    }

    /// Whether a signal is pending, without waiting for one.
    ///
    /// Returns `None` when nothing has arrived, which is the answer on nearly
    /// every frame. After the first signal the block is lifted, so a second one
    /// is delivered normally and ends the process.
    pub fn poll(&mut self) -> Option<Signal> {
        if !self.armed {
            return None;
        }
        // SAFETY: both pointers are to storage this function owns. `sigtimedwait`
        // writes a `siginfo_t` through the first and reads a `timespec` through
        // the second; a zero timeout makes it a poll rather than a wait, which
        // is what keeps this callable from inside a frame loop.
        let number = unsafe {
            let mut set = std::mem::MaybeUninit::<libc::sigset_t>::uninit();
            if libc::sigemptyset(set.as_mut_ptr()) != 0 {
                return None;
            }
            let mut set = set.assume_init();
            for signal in WATCHED {
                if libc::sigaddset(&raw mut set, signal) != 0 {
                    return None;
                }
            }
            let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::uninit();
            let nothing = libc::timespec { tv_sec: 0, tv_nsec: 0 };
            libc::sigtimedwait(&raw const set, info.as_mut_ptr(), &raw const nothing)
        };
        let signal = match number {
            libc::SIGTERM | libc::SIGINT => Signal::Terminate,
            libc::SIGUSR1 => Signal::Quit,
            // `-1` with `EAGAIN`: nothing pending, which is the usual answer.
            _ => return None,
        };
        self.disarm();
        Some(signal)
    }

    /// Put the signals back to their default behaviour.
    ///
    /// Called after the first one arrives, so the second is fatal, and on the
    /// way out so that nothing inherits a mask it did not ask for.
    pub fn disarm(&mut self) {
        if !self.armed {
            return;
        }
        self.armed = false;
        // SAFETY: as `arm`, with `SIG_UNBLOCK` in place of `SIG_BLOCK`.
        let _ = unsafe {
            let mut set = std::mem::MaybeUninit::<libc::sigset_t>::uninit();
            if libc::sigemptyset(set.as_mut_ptr()) != 0 {
                return;
            }
            let mut set = set.assume_init();
            for signal in WATCHED {
                if libc::sigaddset(&raw mut set, signal) != 0 {
                    return;
                }
            }
            libc::sigprocmask(libc::SIG_UNBLOCK, &raw const set, std::ptr::null_mut())
        };
    }

    /// Whether the signals are still blocked.
    #[must_use]
    pub const fn armed(&self) -> bool {
        self.armed
    }
}

impl Drop for Signals {
    fn drop(&mut self) {
        self.disarm();
    }
}
