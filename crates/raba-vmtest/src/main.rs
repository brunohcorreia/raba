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

mod boot_checks;
mod checks;
mod hold;
mod report;
mod shutdown;
mod vt_checks;

use report::Report;

fn main() {
    // One argument, and it is not a test run: the guest re-executes itself to get
    // a *second process* holding a terminal, which is the only way to ask what
    // `SIGKILL` leaves behind. See `hold`.
    if std::env::args().nth(1).as_deref() == Some(hold::FLAG) {
        hold::hold_a_terminal();
        return;
    }

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
    // The VT layer, which is what everything above exists to make testable.
    vt_checks::a_terminal_comes_back_quickly_after_a_switch(&mut report);
    vt_checks::a_terminal_can_be_taken_and_given_back(&mut report);
    vt_checks::the_terminal_never_leaves_text_mode(&mut report);
    vt_checks::frames_are_written_to_the_borrowed_terminal(&mut report);
    vt_checks::two_instances_do_not_take_the_same_terminal(&mut report);
    vt_checks::the_screen_being_taken_is_refused_rather_than_fought_over(&mut report);
    vt_checks::restoring_twice_is_harmless(&mut report);
    vt_checks::the_terminal_is_deallocated_afterwards(&mut report);
    vt_checks::kernel_messages_are_moved_aside_and_put_back(&mut report);
    vt_checks::being_killed_mid_frame_leaves_a_usable_terminal(&mut report);
    // M4: the daemon, driving all of the above from the real binaries.
    boot_checks::run_directory_exists(&mut report);
    boot_checks::the_daemon_draws_answers_and_hands_back(&mut report);
    boot_checks::a_timeout_reveals_the_console_without_exiting(&mut report);
    boot_checks::quit_after_deactivate_does_not_take_the_terminal_back(&mut report);
    boot_checks::a_killed_instance_is_resumed_on_the_same_terminal(&mut report);
    boot_checks::a_second_instance_yields_to_the_running_one(&mut report);
    boot_checks::a_configuration_file_chooses_the_terminal(&mut report);
    boot_checks::the_splash_leaves_when_the_screen_is_taken(&mut report);
    boot_checks::the_console_holds_the_art_and_not_a_blank_screen(&mut report);
    boot_checks::a_mistake_reaches_the_kernel_log_and_a_good_boot_does_not(&mut report);
    boot_checks::the_kill_switch_touches_nothing(&mut report);

    checks::a_deliberate_failure_is_reported(&mut report);

    report.finish();
    shutdown::power_off(report.failed());
}
