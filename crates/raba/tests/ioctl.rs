//! Every console ioctl, asked of something that is not a console.
//!
//! # Why this is worth a file
//!
//! These ten wrappers are the whole of RABA's contact with the kernel's VT
//! subsystem, and `crates/raba-vmtest` exercises them against a real one — but
//! only the answers it gets there, which are all successes. The other half of
//! each wrapper had never run anywhere.
//!
//! That half is not cosmetic. Most of these ioctls fill in a struct the caller
//! then reads: `VT_GETSTATE` writes a `vt_stat`, `KDGETMODE` writes an `int`.
//! Each wrapper allocates that storage as `MaybeUninit` and may only call
//! `assume_init` if the kernel said it wrote something. A wrapper that reported
//! failure as success would hand back uninitialised memory — and the value it
//! handed back would be a terminal number this program then switches the
//! display to.
//!
//! `/dev/null` is a character device that is not a terminal, so every one of
//! these fails on it with `ENOTTY` and nothing else happens. It exists on every
//! machine a test can run on, which the alternative — a virtual terminal — does
//! not.
//!
//! Nothing here touches a real console. A test suite that called `VT_ACTIVATE`
//! would take the screen away from whoever is running it.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use raba::vt::ioctl;
use std::fs::File;

/// Something that is a file descriptor, is a character device, and is not a
/// terminal. `/dev/null` rather than a temporary file so that the kernel's
/// answer is "wrong kind of device" and not "wrong kind of file".
fn not_a_terminal() -> File {
    File::open("/dev/null").expect("/dev/null")
}

#[test]
fn every_query_refuses_rather_than_answering_about_a_device_that_is_not_a_terminal() {
    let fd = not_a_terminal();

    // Each of these would otherwise return a number this program acts on: a
    // terminal to switch to, a mode to decide whether somebody else owns the
    // screen, a console to put kernel messages back on.
    assert!(ioctl::get_state(&fd).is_err(), "VT_GETSTATE answered about /dev/null");
    assert!(ioctl::get_mode(&fd).is_err(), "KDGETMODE answered about /dev/null");
    assert!(ioctl::get_keyboard_mode(&fd).is_err(), "KDGKBMODE answered about /dev/null");
    assert!(ioctl::kmsg_redirect(&fd).is_err(), "TIOCL_GETKMSGREDIRECT answered about /dev/null");
    assert!(ioctl::open_query(&fd).is_err(), "VT_OPENQRY answered about /dev/null");
}

#[test]
fn every_command_refuses_rather_than_reporting_that_it_did_something() {
    let fd = not_a_terminal();

    // `activate` reporting success on a terminal it did not switch to is how
    // the splash ends up drawing on a screen nobody is looking at.
    assert!(ioctl::activate(&fd, 7).is_err(), "VT_ACTIVATE claimed to switch /dev/null");
    assert!(ioctl::wait_active(&fd, 7).is_err(), "VT_WAITACTIVE claimed /dev/null went active");
    assert!(ioctl::disallocate(&fd, 7).is_err(), "VT_DISALLOCATE claimed to free a terminal");
    assert!(ioctl::set_mode(&fd, ioctl::KD_TEXT).is_err(), "KDSETMODE claimed to set a mode");
    assert!(ioctl::set_kmsg_redirect(&fd, 0).is_err(), "TIOCL claimed to move kernel messages");
}

#[test]
fn the_refusal_is_the_kernels_own_and_says_which_one() {
    // `Errno` and not a flattened bool, because `vt::Error::Ioctl` carries it
    // into the journal: "VT_OPENQRY: Inappropriate ioctl for device" is a
    // sentence somebody can act on and "VT_OPENQRY failed" is not.
    let fd = not_a_terminal();
    let why = ioctl::get_state(&fd).expect_err("not a terminal");
    assert_eq!(why, rustix::io::Errno::NOTTY, "{why:?}");
    assert!(
        std::io::Error::from(why).to_string().to_ascii_lowercase().contains("ioctl"),
        "{why:?} does not render as something about an ioctl"
    );
}
