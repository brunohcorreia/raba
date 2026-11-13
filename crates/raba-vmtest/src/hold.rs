//! A second process, so that `SIGKILL` has something to kill.
//!
//! The claim M3 rests on is that RABA killed mid-frame leaves a usable terminal.
//! It cannot be checked in-process: `SIGKILL` cannot be caught, so a test that
//! sent it to itself would have nobody left to report the answer. The guest
//! therefore re-executes itself with [`FLAG`], and that child takes a terminal,
//! draws, and waits to be killed.
//!
//! It deliberately does **nothing else**. No signal handler, no cleanup, no
//! graceful anything — the whole point is to find out what the kernel is left
//! holding when a process that owns a console simply stops existing.

use raba::vt::Vt;
use raba_core::surface::Surface as _;
use std::time::Duration;

/// The argument that turns this binary into the victim rather than the harness.
pub const FLAG: &str = "--hold-a-terminal";

/// Take a terminal, draw on it, and wait to be killed.
///
/// Returns on its own only if it could not get a terminal, which the parent sees
/// as the child exiting early rather than as a hang.
pub fn hold_a_terminal() {
    let Ok(mut vt) = Vt::acquire() else {
        return;
    };
    let size = vt.size();

    // A frame mid-write is the interesting moment, so the loop keeps drawing
    // rather than drawing once and idling. Everything it emits is ordinary text
    // and SGR: if the terminal is unusable afterwards it will not be because of
    // what was on it.
    loop {
        let mut frame = Vec::new();
        frame.extend_from_slice(b"\x1b[H");
        for line in 0..size.lines {
            frame.extend_from_slice(
                format!("\x1b[{};1H\x1b[33m", line.saturating_add(1)).as_bytes(),
            );
            frame.extend(std::iter::repeat_n(b'=', usize::from(size.columns)));
        }
        if vt.present(&frame).is_err() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
