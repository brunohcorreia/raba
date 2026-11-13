//! The virtual-terminal layer, on a real kernel.
//!
//! None of this means anything on a host. `VT_OPENQRY` on a laptop would take a
//! terminal somebody is logged into; `VT_ACTIVATE` would switch the screen out
//! from under them. In a throwaway guest with no users and no getty, both are
//! free, which is the entire reason tier 3a exists.
//!
//! The checks are arranged around the claim the module makes about itself: that
//! what it leaves behind is small, and that what it refuses is as important as
//! what it does.

use crate::report::Report;
use raba::vt::{ioctl, Vt};
use raba_core::surface::Surface as _;
use std::fs::OpenOptions;

/// A terminal is taken, it is not the one in use, and the screen follows.
pub fn a_terminal_can_be_taken_and_given_back(report: &mut Report) {
    report.check("a free terminal is acquired and the display follows it", || {
        let vt = Vt::acquire().map_err(|e| format!("acquiring: {e}"))?;
        let (number, previous) = (vt.number(), vt.previous());
        println!("# took tty{number}, came from tty{previous}");

        if number == previous {
            return Err(format!("tty{number} was already in use"));
        }
        // VT_OPENQRY must never hand back tty1: a getty lives there on a real
        // machine, and taking it is how a splash eats somebody's login prompt.
        if number <= 1 {
            return Err(format!("tty{number} is not a free terminal"));
        }
        let control = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty0")
            .map_err(|e| format!("opening /dev/tty0: {e}"))?;
        let state = ioctl::get_state(&control).map_err(|e| format!("VT_GETSTATE: {e}"))?;
        if state.active != number {
            return Err(format!("the display is on tty{}, not tty{number}", state.active));
        }
        drop(vt);

        let state = ioctl::get_state(&control).map_err(|e| format!("VT_GETSTATE: {e}"))?;
        if state.active != previous {
            return Err(format!("after restoring, the display is on tty{}", state.active));
        }
        Ok(())
    });
}

/// The terminal is in text mode throughout, which is the whole safety argument.
///
/// RABA's claim that being killed mid-frame leaves a usable machine rests on it
/// never putting a console into `KD_GRAPHICS` — because `SIGKILL` cannot be
/// caught, so no cleanup code could rescue it. A claim that load-bearing is worth
/// checking at every point in the lifecycle rather than trusting to a comment.
pub fn the_terminal_never_leaves_text_mode(report: &mut Report) {
    report.check("the console stays in text mode from start to finish", || {
        let control = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty0")
            .map_err(|e| format!("opening /dev/tty0: {e}"))?;
        let before = ioctl::get_mode(&control).map_err(|e| format!("KDGETMODE: {e}"))?;

        let mut vt = Vt::acquire().map_err(|e| format!("acquiring: {e}"))?;
        let during = vt.mode().map_err(|e| format!("KDGETMODE on the borrowed tty: {e}"))?;
        vt.present(b"\x1b[H\x1b[1;36mraba\x1b[0m").map_err(|e| format!("drawing: {e}"))?;
        let drawing = vt.mode().map_err(|e| format!("KDGETMODE while drawing: {e}"))?;
        drop(vt);

        let after = ioctl::get_mode(&control).map_err(|e| format!("KDGETMODE: {e}"))?;
        for (when, mode) in [
            ("before", before),
            ("on acquire", during),
            ("while drawing", drawing),
            ("after", after),
        ] {
            if mode != ioctl::KD_TEXT {
                return Err(format!("{when}: mode is {mode}, not KD_TEXT"));
            }
        }
        Ok(())
    });
}

/// Frames reach the borrowed terminal, at the size it reports.
pub fn frames_are_written_to_the_borrowed_terminal(report: &mut Report) {
    report.check("a frame is written to the terminal RABA took", || {
        let mut vt = Vt::acquire().map_err(|e| format!("acquiring: {e}"))?;
        let size = vt.size();
        println!("# tty{} is {}x{}", vt.number(), size.columns, size.lines);
        if size.columns == 0 || size.lines == 0 {
            return Err(format!("degenerate size {}x{}", size.columns, size.lines));
        }
        // A whole screen in one write, which is the contract `present` exists to
        // state: a console draws whatever has arrived when it is next scheduled.
        let mut frame = Vec::new();
        frame.extend_from_slice(b"\x1b[H");
        for line in 0..size.lines {
            frame.extend_from_slice(format!("\x1b[{};1H", line.saturating_add(1)).as_bytes());
            frame.extend(std::iter::repeat_n(b'#', usize::from(size.columns)));
        }
        frame.extend_from_slice(b"\x1b[0m");
        vt.present(&frame).map_err(|e| format!("presenting a full screen: {e}"))?;
        Ok(())
    });
}

/// Taking the same terminal twice does not happen.
pub fn two_instances_do_not_take_the_same_terminal(report: &mut Report) {
    report.check("a second instance gets a different terminal", || {
        let first = Vt::acquire().map_err(|e| format!("first acquire: {e}"))?;
        let second = Vt::acquire().map_err(|e| format!("second acquire: {e}"))?;
        let (a, b) = (first.number(), second.number());
        println!("# tty{a} and tty{b}");
        if a == b {
            return Err(format!("both got tty{a}"));
        }
        Ok(())
    });
}

/// RABA yields when something else owns the screen.
///
/// The case this stands for is a display server or another splash already in
/// graphics mode. Two programs fighting over one console is worse than no splash,
/// so refusing is the correct behaviour and not a limitation — and a refusal that
/// is never exercised is a refusal nobody knows works.
///
/// Simulated by putting the current terminal into graphics mode, which is a thing
/// only a throwaway machine should ever do. It is put back immediately.
pub fn the_screen_being_taken_is_refused_rather_than_fought_over(report: &mut Report) {
    report.check("RABA declines a console that is in graphics mode", || {
        let control = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty0")
            .map_err(|e| format!("opening /dev/tty0: {e}"))?;

        ioctl::set_mode(&control, ioctl::KD_GRAPHICS)
            .map_err(|e| format!("KDSETMODE(KD_GRAPHICS): {e}"))?;
        let outcome = Vt::acquire();
        // Put it back before judging, so a failed assertion does not leave the
        // guest's console in graphics mode for every check after this one.
        let restored = ioctl::set_mode(&control, ioctl::KD_TEXT);

        match outcome {
            Err(raba::vt::Error::ScreenIsTaken) => {}
            Err(other) => return Err(format!("refused, but for the wrong reason: {other}")),
            Ok(vt) => {
                let number = vt.number();
                drop(vt);
                return Err(format!("took tty{number} while the screen was in graphics mode"));
            }
        }
        restored.map_err(|e| format!("KDSETMODE(KD_TEXT): {e}"))?;
        Ok(())
    });
}

/// Restoring twice is not an error, and the second time changes nothing.
///
/// `Drop` calls `restore`, and so does every early return inside `acquire`. If
/// the second call did anything, a terminal taken by somebody else in between
/// would be switched away from — a splash that reaches out and changes a console
/// it no longer owns.
pub fn restoring_twice_is_harmless(report: &mut Report) {
    report.check("restore is idempotent", || {
        let mut vt = Vt::acquire().map_err(|e| format!("acquiring: {e}"))?;
        let previous = vt.previous();
        vt.restore().map_err(|e| format!("first restore: {e}"))?;
        vt.restore().map_err(|e| format!("second restore: {e}"))?;
        drop(vt);

        let control = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty0")
            .map_err(|e| format!("opening /dev/tty0: {e}"))?;
        let state = ioctl::get_state(&control).map_err(|e| format!("VT_GETSTATE: {e}"))?;
        if state.active != previous {
            return Err(format!("ended on tty{}, expected tty{previous}", state.active));
        }
        Ok(())
    });
}

/// The borrowed terminal is handed back, not hoarded.
///
/// A splash that leaks a terminal per run is one that stops working on a machine
/// rebooted enough times, and `VT_GETSTATE`'s allocation mask is the only place
/// that shows it.
pub fn the_terminal_is_deallocated_afterwards(report: &mut Report) {
    report.check("the borrowed terminal is given back", || {
        let control = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty0")
            .map_err(|e| format!("opening /dev/tty0: {e}"))?;
        let before = ioctl::get_state(&control).map_err(|e| format!("VT_GETSTATE: {e}"))?.state;

        let vt = Vt::acquire().map_err(|e| format!("acquiring: {e}"))?;
        let number = vt.number();
        drop(vt);

        let after = ioctl::get_state(&control).map_err(|e| format!("VT_GETSTATE: {e}"))?.state;
        println!("# allocation mask {before:#06x} -> {after:#06x}");
        let bit = 1u16.checked_shl(u32::from(number)).unwrap_or(0);
        if bit != 0 && after & bit != 0 {
            return Err(format!("tty{number} is still allocated"));
        }
        if after != before {
            return Err(format!("the allocation mask changed: {before:#06x} -> {after:#06x}"));
        }
        Ok(())
    });
}

/// A terminal switched away from comes back quickly.
///
/// `VT_DISALLOCATE` refuses a terminal that is still in use, and the surprise is
/// *when*: an open and close with no switch in between frees it in microseconds,
/// but after a switch the kernel releases it a moment later rather than during
/// the switch. `Vt::restore` therefore waits, and this is where the number it
/// waits for stops being a guess — if a kernel ever makes that window long, a
/// splash would hang at exactly the moment it is handing the machine over.
pub fn a_terminal_comes_back_quickly_after_a_switch(report: &mut Report) {
    report.check("a terminal switched away from is returnable within a millisecond or two", || {
        let control = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty0")
            .map_err(|e| format!("opening /dev/tty0: {e}"))?;
        let previous = ioctl::get_state(&control).map_err(|e| format!("VT_GETSTATE: {e}"))?.active;
        let number = ioctl::open_query(&control)
            .map_err(|e| format!("VT_OPENQRY: {e}"))?
            .ok_or("no free terminal")?;

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(format!("/dev/tty{number}"))
            .map_err(|e| format!("opening /dev/tty{number}: {e}"))?;
        ioctl::activate(&control, number).map_err(|e| format!("VT_ACTIVATE: {e}"))?;
        ioctl::wait_active(&control, number).map_err(|e| format!("VT_WAITACTIVE: {e}"))?;
        ioctl::activate(&control, previous).map_err(|e| format!("VT_ACTIVATE back: {e}"))?;
        ioctl::wait_active(&control, previous).map_err(|e| format!("VT_WAITACTIVE back: {e}"))?;
        drop(file);

        let start = std::time::Instant::now();
        for _ in 0..200u32 {
            if ioctl::disallocate(&control, number).is_ok() {
                let waited = start.elapsed();
                println!("# tty{number} came back after {waited:?}");
                // The budget in `raba::vt` is 50ms. Measured at ~600us; anything
                // near the budget means the assumption behind it has changed.
                if waited > std::time::Duration::from_millis(10) {
                    return Err(format!("took {waited:?}, which is close to RABA's budget"));
                }
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_micros(500));
        }
        Err(format!("still busy after {:?}", start.elapsed()))
    });
}
