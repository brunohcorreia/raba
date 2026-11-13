//! Tier 3a: the `/init` of a one-second virtual machine.
//!
//! This is PID 1 of a QEMU guest that has a real Linux kernel, a real VGA console
//! and real ioctls, and no disk, no distribution and no init system. It mounts
//! what it needs, runs its checks, reports them on the serial line, and switches
//! the machine off.
//!
//! **It exists before the code it is meant to test**, which is the point. The VT
//! layer is the part of RABA where a mistake hangs somebody's boot, and the rule
//! this project set itself is that the harness is built and proven first. So the
//! checks here start by proving the harness: that a guest boots, that its output
//! reaches the host, that a failure is reported as a failure rather than as a
//! silent pass, and that the environment the VT layer will need is actually there.
//!
//! See `docs/development.md` on the tiers, and `scripts/vm-test.sh` for the host
//! side.
//!
//! ## Why there is no test framework here
//!
//! `cargo test` needs a filesystem, a loader and a process model. This has 20 MB
//! of RAM, no `/tmp`, and is itself PID 1. A harness small enough to read in one
//! sitting is worth more here than one with features.

// This binary is PID 1 in a throwaway VM. A panic here is a kernel panic, which
// `panic=-1` turns into an immediate reboot — so panicking is not a way to report
// anything and every check returns a result instead. The lints below are relaxed
// only because `main` is allowed to be the one place that stops.
#![allow(clippy::print_stdout, clippy::print_stderr)]

mod checks;
mod report;
mod shutdown;

use report::Report;

fn main() {
    let mut report = Report::new();

    // Nothing works before this: no /proc means no /proc/consoles, and no
    // devtmpfs means no /dev/tty0 to open. A failure here fails everything after
    // it, so it is reported as a check rather than silently assumed.
    checks::mount_filesystems(&mut report);
    checks::kernel_is_what_we_asked_for(&mut report);
    checks::the_console_devices_exist(&mut report);
    checks::a_virtual_terminal_is_present(&mut report);
    checks::the_terminal_is_backed_by_a_real_driver(&mut report);
    checks::the_console_reports_a_size(&mut report);
    checks::writing_to_the_console_works(&mut report);
    checks::the_real_binary_runs_against_a_console(&mut report);
    checks::a_deliberate_failure_is_reported(&mut report);

    report.finish();
    shutdown::power_off(report.failed());
}
