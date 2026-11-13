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
//!
//! ## Why PID 1 does not run the checks itself
//!
//! It spawns a copy of itself and waits for it. Two reasons, and the second is
//! the one that forced it.
//!
//! A panic in a check, running as PID 1, is a kernel panic — `panic=-1` turns
//! that into an immediate reboot, and the host sees a machine that went away
//! mid-sentence rather than a failure. In a child it is one process dying and
//! PID 1 reporting what happened.
//!
//! And PID 1 here leaves through an I/O port write, not through `exit`, so it
//! runs no `atexit` handler and an instrumented build could never write its own
//! coverage counters. Half of tier 3a — every check that drives the `raba`
//! library directly rather than starting the binary — was therefore running and
//! being reported and showing as untested, which is the worst of the three
//! states to be in. The child exits the ordinary way and its profile lands in
//! `/` with the ones the `raba` processes left, where `profile::emit` finds it.

// This binary is PID 1 in a throwaway VM. A panic here is a kernel panic, which
// `panic=-1` turns into an immediate reboot — so panicking is not a way to report
// anything and every check returns a result instead. The lints below are relaxed
// only because `main` is allowed to be the one place that stops.
#![allow(clippy::print_stdout, clippy::print_stderr)]

mod boot_checks;
mod checks;
mod hold;
mod profile;
mod report;
mod shutdown;
mod vt_checks;

use report::Report;

/// The guest re-executes itself with this to run the checks in a child.
const CHECKS: &str = "--run-the-checks";

fn main() {
    match std::env::args().nth(1).as_deref() {
        // A *second process* holding a terminal, which is the only way to ask
        // what `SIGKILL` leaves behind. See `hold`.
        Some(hold::FLAG) => hold::hold_a_terminal(),
        // The checks. Returns normally, which is what lets an instrumented
        // build write its counters. See the module documentation.
        Some(CHECKS) => run_the_checks(),
        _ => supervise(),
    }
}

/// PID 1: mount what the checks need, run them in a child, switch the machine
/// off with the answer.
fn supervise() {
    // Reported here rather than in the child because a failure to mount fails
    // everything after it, and because the child needs `/proc` and `/dev`
    // before it can do anything at all.
    let mut report = Report::new();
    checks::mount_filesystems(&mut report);

    let mut failed = report.failed();
    if failed {
        // Nothing else can run, and the plan line still has to be printed or
        // the host sees a truncated stream and calls it a hang.
        report.finish();
    } else {
        match run_checks_in_a_child(report.count()) {
            // The child printed its own results and its own plan line: it is
            // the one that knows how many checks there were.
            Some(child_failures) => failed = child_failures > 0,
            None => {
                report.check("the checks could be started at all", || {
                    Err("the child that runs them did not report".to_owned())
                });
                report.finish();
                failed = true;
            }
        }
    }

    // Before the power-off, which is an I/O port write and not an exit: a
    // process that leaves that way runs no `atexit`, so anything still
    // unwritten would be lost with the machine. By now the child has exited
    // and its profile is sitting in `/` with the ones the `raba` processes
    // left behind.
    profile::emit();
    shutdown::power_off(failed);
}

/// Start the child and wait. Its output goes straight to the serial console —
/// a pipe would be the obvious channel and is the wrong one, because capturing
/// stdout would hold every line until the end of the run.
///
/// `None` if it could not be started or died from a signal; otherwise how many
/// checks it reported as failures.
///
/// `/proc/self/exe` rather than `/init`: the host script chooses the name.
fn run_checks_in_a_child(already_run: u32) -> Option<u32> {
    let me = std::env::current_exe().ok()?;
    let status =
        std::process::Command::new(me).arg(CHECKS).arg(already_run.to_string()).status().ok()?;
    // Exit status, not a parsed stream: the child's stdout is the report and
    // re-reading it here would mean parsing TAP to find out what TAP said.
    u32::try_from(status.code()?).ok()
}

/// The checks themselves, in a child process that exits the ordinary way.
fn run_the_checks() {
    // Where the parent's numbering left off, so the TAP stream reads as one
    // run rather than as two starting at 1.
    let already_run = std::env::args().nth(2).and_then(|n| n.parse().ok()).unwrap_or(0);
    let mut report = Report::continuing(already_run);

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
    vt_checks::relinquishing_twice_is_harmless(&mut report);
    vt_checks::adopting_a_terminal_that_is_not_on_screen_brings_the_display_back(&mut report);
    vt_checks::a_console_in_graphics_mode_is_not_resumed_onto(&mut report);
    vt_checks::the_terminal_is_deallocated_afterwards(&mut report);
    vt_checks::kernel_messages_are_moved_aside_and_put_back(&mut report);
    vt_checks::being_killed_mid_frame_leaves_a_usable_terminal(&mut report);
    // M4: the daemon, driving all of the above from the real binaries.
    boot_checks::run_directory_exists(&mut report);
    boot_checks::the_daemon_draws_answers_and_hands_back(&mut report);
    boot_checks::a_timeout_reveals_the_console_without_exiting(&mut report);
    boot_checks::quit_after_deactivate_does_not_take_the_terminal_back(&mut report);
    boot_checks::a_killed_instance_is_resumed_on_the_same_terminal(&mut report);
    boot_checks::a_terminated_instance_leaves_the_screen_as_it_is(&mut report);
    boot_checks::a_second_instance_yields_to_the_running_one(&mut report);
    boot_checks::a_configuration_file_chooses_the_terminal(&mut report);
    boot_checks::the_splash_leaves_when_the_screen_is_taken(&mut report);
    boot_checks::the_splash_leaves_when_the_terminal_goes_into_graphics_mode(&mut report);
    boot_checks::the_console_holds_the_art_and_not_a_blank_screen(&mut report);
    boot_checks::a_mistake_reaches_the_kernel_log_and_a_good_boot_does_not(&mut report);
    boot_checks::the_kill_switch_touches_nothing(&mut report);

    checks::a_deliberate_failure_is_reported(&mut report);

    report.finish();
    // `exit`, not a `return`: this runs the `atexit` handlers, which is where
    // an instrumented build writes its coverage counters, and the status is
    // the only thing the parent reads. Capped because an exit status is a
    // byte, and 255 failures is as bad as 300.
    std::process::exit(i32::from(u8::try_from(report.failures()).unwrap_or(u8::MAX)));
}
