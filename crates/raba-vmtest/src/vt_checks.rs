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

/// Kernel messages are moved off the splash, and put back exactly.
///
/// `TIOCL_SETKMSGREDIRECT` is a single global in the kernel rather than a
/// property of a file descriptor, so "put it back" means "put back the value
/// that was there", not "set it to zero". Checking the restore is the half that
/// matters: setting it wrong makes an untidy splash, and leaving it set wrong
/// makes a machine where kernel messages print somewhere nobody is looking,
/// which is the kind of thing found months later by somebody debugging a disk.
///
/// This would fail on a kernel built without `CONFIG_VT_CONSOLE`, where the
/// redirect is a no-op that always reads back `0`. The guest's kernel is the
/// host's, so that is a real thing to see one day rather than a hypothetical —
/// and failing is right: RABA tolerates it, but a harness that shrugged at it
/// would also shrug at the redirect being genuinely broken.
pub fn kernel_messages_are_moved_aside_and_put_back(report: &mut Report) {
    report.check("kernel messages move off the splash and go back where they were", || {
        let control = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty0")
            .map_err(|e| format!("opening /dev/tty0: {e}"))?;
        let before = ioctl::kmsg_redirect(&control).map_err(|e| format!("TIOCL_GET: {e}"))?;

        // The pair is exercised directly before it is exercised through `Vt`,
        // because `Vt` swallows a failure here by design — a splash must draw
        // even when it may not move the messages — and a check that went through
        // it would report "it did not move them" for a reason it cannot see.
        ioctl::set_kmsg_redirect(&control, 1).map_err(|e| format!("TIOCL_SETKMSGREDIRECT: {e}"))?;
        let probe = ioctl::kmsg_redirect(&control).map_err(|e| format!("TIOCL_GET: {e}"))?;
        if probe != 1 {
            return Err(format!("set the redirect to tty1 and read back tty{probe}"));
        }
        ioctl::set_kmsg_redirect(&control, before).map_err(|e| format!("TIOCL_SET back: {e}"))?;

        let vt = Vt::acquire().map_err(|e| format!("acquiring: {e}"))?;
        let (number, previous) = (vt.number(), vt.previous());
        let during = ioctl::kmsg_redirect(&control).map_err(|e| format!("TIOCL_GET: {e}"))?;
        println!("# kmsg redirect {before} -> {during} (drawing on tty{number})");

        if u16::from(during) != previous {
            drop(vt);
            return Err(format!(
                "messages go to tty{during}, but the user came from tty{previous}"
            ));
        }
        if u16::from(during) == number {
            drop(vt);
            return Err(format!("messages go to tty{number}, which is the splash"));
        }
        drop(vt);

        let after = ioctl::kmsg_redirect(&control).map_err(|e| format!("TIOCL_GET: {e}"))?;
        if after != before {
            return Err(format!("left the redirect on tty{after}, not tty{before}"));
        }
        Ok(())
    });
}

/// The one that matters: `SIGKILL` mid-frame leaves a machine somebody can use.
///
/// Nothing in RABA runs after a `SIGKILL`. There is no handler, no `Drop`, no
/// atexit — the process stops between two bytes of a frame and the kernel is left
/// holding whatever it was holding. So this check does not test cleanup code; it
/// tests that there was never anything that *needed* cleaning up, which is the
/// claim `raba::vt` makes about itself in its module documentation.
///
/// "Usable" is spelled out rather than assumed, because the ways a console dies
/// are specific: a screen in graphics mode showing nothing, a keyboard in `K_RAW`
/// that types nothing, a VT switch that silently does not happen, and a terminal
/// the kernel will never hand back. Each is asserted here on a console whose
/// owner was killed a moment ago.
pub fn being_killed_mid_frame_leaves_a_usable_terminal(report: &mut Report) {
    report.check("a terminal whose owner was SIGKILLed mid-frame is still usable", || {
        let control = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty0")
            .map_err(|e| format!("opening /dev/tty0: {e}"))?;
        let home = ioctl::get_state(&control).map_err(|e| format!("VT_GETSTATE: {e}"))?.active;
        let kmsg = ioctl::kmsg_redirect(&control).map_err(|e| format!("TIOCL_GET: {e}"))?;
        let keyboard_before =
            ioctl::get_keyboard_mode(&control).map_err(|e| format!("KDGKBMODE: {e}"))?;

        // `/proc/self/exe`, not `/init`: the guest is installed under whatever
        // name the host script chose, and a hardcoded path here would turn a
        // rename into a mysterious failure of the most important check.
        let me = std::env::current_exe().map_err(|e| format!("finding this binary: {e}"))?;
        let mut child = std::process::Command::new(&me)
            .arg(crate::hold::FLAG)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("spawning the victim: {e}"))?;

        // Wait for it to be *on screen*, which is the only evidence that it is
        // mid-frame rather than mid-startup. Polling the display beats a sleep:
        // a sleep that is too short kills a process that has not drawn yet, and
        // the check would then pass for the wrong reason.
        let mut taken = home;
        for _ in 0..500u32 {
            if let Ok(Some(status)) = child.try_wait() {
                return Err(format!("the victim exited before drawing ({status})"));
            }
            let active =
                ioctl::get_state(&control).map_err(|e| format!("VT_GETSTATE: {e}"))?.active;
            if active != home {
                taken = active;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        if taken == home {
            let _ = child.kill();
            let _ = child.wait();
            return Err("the victim never took the display".to_owned());
        }

        // `Child::kill` is SIGKILL on Unix, which is the signal with no handler
        // and no grace period. The process is drawing a frame as it arrives.
        child.kill().map_err(|e| format!("SIGKILL: {e}"))?;
        let status = child.wait().map_err(|e| format!("reaping: {e}"))?;
        println!("# victim held tty{taken}, killed: {status}");

        // Everything from here is asked of a console nobody owns any more.
        let orphan = OpenOptions::new()
            .read(true)
            .write(true)
            .open(format!("/dev/tty{taken}"))
            .map_err(|e| format!("the dead process's tty{taken} will not open: {e}"))?;

        let mode = ioctl::get_mode(&orphan).map_err(|e| format!("KDGETMODE: {e}"))?;
        if mode != ioctl::KD_TEXT {
            return Err(format!("tty{taken} is in mode {mode}, not KD_TEXT: the screen is dead"));
        }
        let keyboard = ioctl::get_keyboard_mode(&orphan).map_err(|e| format!("KDGKBMODE: {e}"))?;
        if keyboard != keyboard_before {
            return Err(format!(
                "the keyboard mode went from {keyboard_before} to {keyboard}; \
                 RABA is supposed never to touch it"
            ));
        }
        if !ioctl::is_typable(keyboard) {
            return Err(format!("the keyboard is in mode {keyboard}: nobody can type"));
        }
        // It still draws. A console that passes its ioctls and swallows writes is
        // the same thing as a dead one from in front of the machine.
        {
            use std::io::Write as _;
            let mut orphan = &orphan;
            orphan
                .write_all(b"\x1b[0m\x1b[H\x1b[2Jraba: the owner of this terminal was killed\r\n")
                .map_err(|e| format!("writing to tty{taken}: {e}"))?;
        }

        // And the user can get away from it, which is the thing they will
        // actually try. A switch that is merely queued and never happens is the
        // failure this waits for rather than assumes.
        ioctl::activate(&control, home)
            .map_err(|e| format!("VT_ACTIVATE back to tty{home}: {e}"))?;
        ioctl::wait_active(&control, home)
            .map_err(|e| format!("the display will not leave tty{taken}: {e}"))?;

        // The dead process's redirect is still in place, and that is the designed
        // outcome rather than a leak: it points at the terminal the user is on.
        let stranded = ioctl::kmsg_redirect(&control).map_err(|e| format!("TIOCL_GET: {e}"))?;
        if u16::from(stranded) != home {
            return Err(format!(
                "kernel messages were left pointing at tty{stranded}, not at tty{home} \
                 where the user is"
            ));
        }

        // Put the guest back the way it was, so the checks after this one are not
        // reading this one's wreckage.
        drop(orphan);
        let returned = {
            let mut outcome = Err("still busy".to_owned());
            for _ in 0..200u32 {
                if ioctl::disallocate(&control, taken).is_ok() {
                    outcome = Ok(());
                    break;
                }
                std::thread::sleep(std::time::Duration::from_micros(500));
            }
            outcome
        };
        returned.map_err(|e| format!("tty{taken} could not be reclaimed: {e}"))?;
        ioctl::set_kmsg_redirect(&control, kmsg).map_err(|e| format!("TIOCL_SET: {e}"))?;
        Ok(())
    });
}
