//! One line, in whichever log this machine actually has.
//!
//! Here rather than in `main.rs` because the boot loop has things to say
//! too — see [`boot::Boot::save`] — and a second way of saying them would
//! be a second place for the discipline in `docs/development.md` to be got
//! wrong.
//!
//! [`boot::Boot::save`]: crate::boot::Boot

/// One line about something that went wrong, in whichever log this machine
/// actually has.
///
/// stderr first, always. Under an init system that *is* the journal and there is
/// nothing more to do. When stderr is not a socket, nobody is capturing it — an
/// initramfs, where it is a file, a console or `/dev/null` — and the line is
/// repeated into `/dev/kmsg`, so `dmesg` and `journalctl -b` both have it once
/// the boot is over and somebody is asking why there was no splash.
///
/// The test is a `getsockopt` on stderr rather than a flag, because
/// `docs/compatibility.md` forbids an initramfs hook from requiring a flag that
/// the installed binary might not have, and because an fd cannot go out of date
/// the way a baked-in hook can.
///
/// Errors only, never the ordinary course of a boot: `integration/tests/distro`
/// asserts that a healthy boot leaves nothing of ours in the kernel log at all.
pub fn report(message: &dyn std::fmt::Display) {
    eprintln!("raba: {message}");
    // Two ways the message has already arrived somewhere a person will look.
    // A socket: an init system is capturing it, so it is in the journal
    // (ENOTSOCK is how the kernel says nobody is). A terminal: somebody typed
    // the command and is reading the answer — putting a typo at a prompt into
    // `dmesg` would be the opposite of leaving the system log alone.
    let stderr = std::io::stderr();
    if rustix::net::sockopt::socket_type(&stderr).is_ok() || rustix::termios::isatty(&stderr) {
        return;
    }
    // Best effort, and quiet about it: there is no /dev/kmsg in a container and
    // no permission to write it as an ordinary user, and in neither case would
    // anything be done differently.
    if let Ok(mut kmsg) = std::fs::OpenOptions::new().write(true).open("/dev/kmsg") {
        use std::io::Write as _;
        // <3> is KERN_ERR: journald files it at the priority the line deserves
        // instead of burying it among debug chatter.
        let _ = writeln!(kmsg, "<3>raba: {message}");
    }
}

/// One line about something a person might want to know, and nothing more.
///
/// stderr only. The difference from [`report`] is the kernel log, and the
/// difference in meaning is whether the boot went wrong: `dmesg` is shared with
/// everybody on the machine who is using it to debug something else, and this
/// project's promise — asserted by `crates/raba-vmtest` and by
/// `integration/tests/distro` — is that a healthy boot leaves nothing of ours
/// there at all.
///
/// A splash that could not use the resume state is the case this exists for. It
/// starts from the first frame instead of continuing, which is a fact worth
/// having in the journal and is not a fault: nothing failed, nothing is
/// degraded, and the splash is on the screen either way.
pub fn note(message: &dyn std::fmt::Display) {
    eprintln!("raba: {message}");
}
