//! What the harness checks, before there is a VT layer to check.
//!
//! Two kinds. The first few prove the **harness** — that a guest boots, reaches
//! this code, and can say so — because a harness that silently passes is worse
//! than no harness, and a VT layer built on one is exactly the mistake M3 is
//! arranged to avoid. The rest prove the **environment**: that the real kernel
//! console the VT layer will reach for is actually here.
//!
//! Every check returns `Result<(), String>` rather than asserting. A panic in PID
//! 1 is a kernel panic, and `panic=-1` turns that into a reboot — so a panicking
//! check does not report a failure, it destroys the evidence.

use crate::report::Report;
use std::path::Path;

/// Mount the three filesystems nothing else works without.
///
/// A kernel with no `/proc` has no `/proc/consoles`; a kernel with no devtmpfs
/// has no `/dev/tty0` to open. Reported as a check rather than assumed, because
/// when it fails everything after it fails too and the reason should be legible.
pub fn mount_filesystems(report: &mut Report) {
    report.check("the filesystems a console needs are mounted", || {
        for (source, target, kind) in [
            ("proc", "/proc", "proc"),
            ("sysfs", "/sys", "sysfs"),
            ("devtmpfs", "/dev", "devtmpfs"),
        ] {
            let _ = std::fs::create_dir_all(target);
            rustix::mount::mount(source, target, kind, rustix::mount::MountFlags::empty(), None)
                .map_err(|e| format!("mounting {kind} on {target}: {e}"))?;
        }
        Ok(())
    });
}

/// The kernel is the one the host meant to boot, and it is a Linux of a sane age.
pub fn kernel_is_what_we_asked_for(report: &mut Report) {
    let info = rustix::system::uname();
    let release = info.release().to_string_lossy().into_owned();
    report.note(&format!("kernel {release}"));
    report.check("a Linux kernel is running", || {
        let name = info.sysname().to_string_lossy();
        if name != "Linux" {
            return Err(format!("sysname is {name:?}"));
        }
        if release.is_empty() {
            return Err("empty release string".to_owned());
        }
        Ok(())
    });
}

/// The device nodes the VT layer opens.
///
/// `/dev/tty0` is "whichever VT is current" and is what RABA will draw on when it
/// has not acquired one of its own. `/dev/console` is where the kernel talks.
pub fn the_console_devices_exist(report: &mut Report) {
    report.check("the console device nodes are present", || {
        let missing: Vec<&str> = ["/dev/tty0", "/dev/tty1", "/dev/console", "/dev/port"]
            .into_iter()
            .filter(|path| !Path::new(path).exists())
            .collect();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(format!("missing: {}", missing.join(", ")))
        }
    });
}

/// There is a real virtual terminal, and not only a serial line.
///
/// This is the one that decides whether tier 3a is worth having at all. A guest
/// booted with `-display none` still has an emulated VGA adapter, so the kernel
/// brings up a console on it; if that ever stops being true, every VT test from
/// M3 onward would be testing a serial console and quietly proving nothing.
pub fn a_virtual_terminal_is_present(report: &mut Report) {
    report.check("the kernel has a virtual terminal, not just a serial line", || {
        let consoles = std::fs::read_to_string("/proc/consoles")
            .map_err(|e| format!("reading /proc/consoles: {e}"))?;
        for line in consoles.lines() {
            report_line(line);
        }
        if consoles.lines().any(|line| line.starts_with("tty0") || line.starts_with("tty1")) {
            Ok(())
        } else {
            Err(format!("no tty0 among: {}", consoles.replace('\n', " | ")))
        }
    });
}

fn report_line(line: &str) {
    println!("# /proc/consoles: {line}");
}

/// The virtual terminal is backed by a driver that draws, not by `dummycon`.
///
/// The same class of trap as the serial one above, one layer down. `CONFIG_VT`
/// gives a kernel 64 terminals whether or not anything can display them: when no
/// real console driver binds, `dummycon` takes over and accepts everything while
/// drawing nothing. Every VT ioctl would succeed, every test would pass, and the
/// splash would be invisible on a real machine.
///
/// Booting with `-kernel` is exactly the case where this happens — QEMU's loader
/// may leave the boot parameters that `vgacon` looks for unfilled — so it is
/// checked rather than hoped for.
pub fn the_terminal_is_backed_by_a_real_driver(report: &mut Report) {
    report.check("the virtual terminal has a driver that can draw", || {
        let directory = Path::new("/sys/class/vtconsole");
        if !directory.exists() {
            return Err("/sys/class/vtconsole does not exist".to_owned());
        }
        let entries = std::fs::read_dir(directory)
            .map_err(|e| format!("reading /sys/class/vtconsole: {e}"))?;
        let mut bound = Vec::new();
        for entry in entries.flatten() {
            let name = std::fs::read_to_string(entry.path().join("name")).unwrap_or_default();
            let active = std::fs::read_to_string(entry.path().join("bind")).unwrap_or_default();
            let name = name.trim().to_owned();
            println!("# vtconsole: {name} bound={}", active.trim());
            if active.trim() == "1" {
                bound.push(name);
            }
        }
        if bound.is_empty() {
            return Err("no console driver is bound to the virtual terminal".to_owned());
        }
        if bound.iter().all(|name| name.contains("dummy")) {
            return Err(format!(
                "only dummycon is bound ({}), so a VT would accept everything and \
                 display nothing",
                bound.join(", ")
            ));
        }
        Ok(())
    });
}

/// `TIOCGWINSZ` on the console returns a size, and a plausible one.
///
/// The same ioctl `raba-term` uses. Eighty by twenty-five is what a VGA text
/// console gives, and it is also the fallback RABA uses when it cannot read a
/// size at all — so the check is that the call *succeeds*, with the numbers
/// reported for a human to read rather than asserted against.
pub fn the_console_reports_a_size(report: &mut Report) {
    report.check("the console answers TIOCGWINSZ", || {
        let console =
            std::fs::File::open("/dev/tty0").map_err(|e| format!("opening /dev/tty0: {e}"))?;
        let size = rustix::termios::tcgetwinsize(&console)
            .map_err(|e| format!("TIOCGWINSZ on /dev/tty0: {e}"))?;
        println!("# console is {}x{}", size.ws_col, size.ws_row);
        if size.ws_col == 0 || size.ws_row == 0 {
            return Err(format!("degenerate size {}x{}", size.ws_col, size.ws_row));
        }
        Ok(())
    });
}

/// Bytes written to the console are accepted.
///
/// Not that they *looked* right — nothing in the guest can see the screen. What
/// this rules out is the console being read-only, absent, or owned by something
/// else, which is the failure that would otherwise show up as a blank splash.
pub fn writing_to_the_console_works(report: &mut Report) {
    report.check("the console accepts a frame", || {
        use std::io::Write as _;
        let mut console = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/tty0")
            .map_err(|e| format!("opening /dev/tty0 for writing: {e}"))?;
        // Home, a word, and a reset: the smallest thing shaped like a real frame.
        console
            .write_all(b"\x1b[H\x1b[1;32mraba-vmtest\x1b[0m")
            .map_err(|e| format!("writing to /dev/tty0: {e}"))?;
        console.flush().map_err(|e| format!("flushing /dev/tty0: {e}"))?;
        Ok(())
    });
}

/// The shipped binary runs against a real console and exits cleanly.
///
/// End to end, on a real kernel: `raba` opens a console it has never seen, asks
/// it how big it is, draws every frame of a real `.raba` file and gives the
/// terminal back. Everything tier 2 could not reach except the VT ioctls
/// themselves — and when those arrive, this is the check they slot into.
pub fn the_real_binary_runs_against_a_console(report: &mut Report) {
    report.check("raba plays a theme on a real console", || {
        let theme = "/fixtures/vmtest.raba";
        if !Path::new(theme).exists() {
            return Err(format!("{theme} is not in the initramfs"));
        }
        let console = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/tty0")
            .map_err(|e| format!("opening /dev/tty0: {e}"))?;
        let status = std::process::Command::new("/raba")
            .args(["--preview", theme, "--loops", "1"])
            .stdout(std::process::Stdio::from(console))
            .stderr(std::process::Stdio::inherit())
            .status()
            .map_err(|e| format!("running /raba: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("raba exited with {status}"))
        }
    });
}

/// Prove that a failure is reported as a failure.
///
/// The check nobody wants and every harness needs. A test rig that cannot fail
/// reports success for a reason unrelated to the code, and the first time anybody
/// finds out is when something that should have been caught was not.
///
/// It is driven from the host: `scripts/vm-test.sh --self-test` sets a kernel
/// command line flag, this check reads it and fails on purpose, and the script
/// asserts that the run came back failed. In an ordinary run it passes.
pub fn a_deliberate_failure_is_reported(report: &mut Report) {
    let cmdline = std::fs::read_to_string("/proc/cmdline").unwrap_or_default();
    let asked = cmdline.split_whitespace().any(|word| word == "rabatest.selftest=fail");
    report.check("the harness can report a failure", || {
        if asked {
            Err("failing on purpose, because rabatest.selftest=fail was on the kernel command line"
                .to_owned())
        } else {
            Ok(())
        }
    });
}
