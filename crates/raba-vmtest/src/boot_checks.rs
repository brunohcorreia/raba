//! The daemon, end to end, on a real console.
//!
//! Everything below this runs the **real binaries** — `/raba` as a child process
//! and `/raba-ctl` as its client — rather than driving the library. That is the
//! point: the phase machine is unit-tested, the socket is unit-tested, and what
//! has never been tested until here is the three of them plus a virtual terminal
//! in one process with a clock.
//!
//! The checks are the sequences a boot actually performs, in order: come up and
//! draw, answer a client, hand the terminal back on `quit`, reveal the console
//! on a timeout, and resume after being killed.

use crate::report::Report;
use raba::vt::ioctl;
use std::fs::OpenOptions;
use std::io;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const RABA: &str = "/raba";
const CTL: &str = "/raba-ctl";
const SOCKET: &str = "/run/raba.sock";
const STATE: &str = "/run/raba.state";
const THEME: &str = "/fixtures/vmtest.raba";

fn control() -> Result<std::fs::File, String> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty0")
        .map_err(|e| format!("opening /dev/tty0: {e}"))
}

fn active(control: &std::fs::File) -> Result<u16, String> {
    Ok(ioctl::get_state(control).map_err(|e| format!("VT_GETSTATE: {e}"))?.active)
}

/// Start the daemon with the arguments a boot would use, plus whatever is asked.
fn start(extra: &[&str]) -> io::Result<Child> {
    Command::new(RABA)
        .args(["--theme", THEME, "--socket", SOCKET, "--state", STATE])
        .args(extra)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// Wait until the display is on something other than `home`, which is the only
/// evidence that the daemon is drawing rather than starting.
fn wait_for_the_splash(
    control: &std::fs::File,
    home: u16,
    child: &mut Child,
) -> Result<u16, String> {
    for _ in 0..1000u32 {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("raba exited before drawing ({status})"));
        }
        let now = active(control)?;
        if now != home {
            return Ok(now);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Err("raba never took the display".to_owned())
}

fn wait_until(what: &str, mut ready: impl FnMut() -> bool) -> Result<(), String> {
    for _ in 0..1000u32 {
        if ready() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Err(format!("timed out waiting for {what}"))
}

/// Run `raba-ctl` and give back its exit code and stdout.
fn ctl(args: &[&str]) -> Result<(i32, String), String> {
    let output = Command::new(CTL)
        .args(["--socket", SOCKET])
        .args(args)
        .output()
        .map_err(|e| format!("running raba-ctl {args:?}: {e}"))?;
    let code = output.status.code().ok_or("raba-ctl was killed")?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    Ok((code, text))
}

/// Kill and reap, so a failed check does not leave a daemon holding a terminal
/// for every check after it.
fn clean_up(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(STATE);
    let _ = std::fs::remove_file(SOCKET);
}

/// `/run` is where the socket and the state file live, and an initramfs that
/// has not got one is the usual case rather than an unusual one.
pub fn run_directory_exists(report: &mut Report) {
    report.check("/run exists and is writable", || {
        std::fs::create_dir_all("/run").map_err(|e| format!("creating /run: {e}"))?;
        std::fs::write("/run/.probe", b"x").map_err(|e| format!("writing in /run: {e}"))?;
        std::fs::remove_file("/run/.probe").map_err(|e| format!("removing: {e}"))?;
        Ok(())
    });
}

/// The whole handoff, in the order `docs/integration.md` performs it.
pub fn the_daemon_draws_answers_and_hands_back(report: &mut Report) {
    report.check("raba takes a terminal, answers raba-ctl, and gives it back on quit", || {
        let control = control()?;
        let home = active(&control)?;
        // No timeout: this check is about the `quit` path, and a timeout firing
        // in the middle of it would be testing the other one by accident.
        let mut child = start(&["--timeout", "0"]).map_err(|e| format!("starting raba: {e}"))?;

        let outcome = (|| {
            let taken = wait_for_the_splash(&control, home, &mut child)?;
            println!("# raba drew on tty{taken}, came from tty{home}");

            let (code, _) = ctl(&["ping"])?;
            if code != 0 {
                return Err(format!("ping exited {code} against a running instance"));
            }
            let (_, status) = ctl(&["status"])?;
            for field in ["protocol: 1", "phase: boot", &format!("terminal: {taken}")] {
                if !status.contains(field) {
                    return Err(format!("status has no {field:?}: {status:?}"));
                }
            }
            // The one path from outside the process to a console.
            let (code, _) = ctl(&["message", "Checking filesystems"])?;
            if code != 0 {
                return Err(format!("message exited {code}"));
            }
            // Written every frame, so by now there is one.
            if !Path::new(STATE).exists() {
                return Err("no state file while drawing".to_owned());
            }

            // `--wait` returns at EOF, which is the kernel closing the socket
            // of a process that no longer exists.
            let quitting = Instant::now();
            let (code, _) = ctl(&["quit", "--wait"])?;
            if code != 0 {
                return Err(format!("quit exited {code}"));
            }
            println!("# quit --wait returned after {:?}", quitting.elapsed());

            let status = child.wait().map_err(|e| format!("reaping: {e}"))?;
            if !status.success() {
                return Err(format!("raba exited {status}, and it exits 0 or it is a bug"));
            }
            wait_until("the display to come home", || active(&control) == Ok(home))?;
            if Path::new(STATE).exists() {
                return Err("the state file outlived the instance that wrote it".to_owned());
            }
            if Path::new(SOCKET).exists() {
                return Err("the control socket was left behind".to_owned());
            }
            Ok(())
        })();

        clean_up(&mut child);
        outcome
    });
}

/// `Boot.Timeout` reveals the console. It does **not** exit.
///
/// The distinction is the entire feature: a stalled boot should show the
/// messages that explain the stall, and exiting would leave a blank screen with
/// the explanation already scrolled past. See `man 5 raba.conf`.
pub fn a_timeout_reveals_the_console_without_exiting(report: &mut Report) {
    report.check("a timeout gives the console back and keeps running", || {
        let control = control()?;
        let home = active(&control)?;
        let mut child = start(&["--timeout", "0.4"]).map_err(|e| format!("starting raba: {e}"))?;

        let outcome = (|| {
            let taken = wait_for_the_splash(&control, home, &mut child)?;
            wait_until("the timeout to fire", || active(&control) == Ok(home))?;
            println!("# tty{taken} was handed back on timeout");

            // Still running: that is the half an `exit` would get wrong.
            if let Ok(Some(status)) = child.try_wait() {
                return Err(format!("raba exited on timeout ({status}) instead of revealing"));
            }
            let (_, status) = ctl(&["status"])?;
            if !status.contains("phase: revealed") {
                return Err(format!("phase is not revealed: {status:?}"));
            }
            // And it can be told to finish, which must not redraw anything.
            let (code, _) = ctl(&["quit", "--wait"])?;
            if code != 0 {
                return Err(format!("quit after a reveal exited {code}"));
            }
            if active(&control)? != home {
                return Err("quitting after a reveal moved the display".to_owned());
            }
            Ok(())
        })();

        clean_up(&mut child);
        outcome
    });
}

/// `deactivate` then `quit` is the display-manager handoff, and the `quit` must
/// not take the terminal back to play an outro over whoever has it now.
pub fn quit_after_deactivate_does_not_take_the_terminal_back(report: &mut Report) {
    report.check("quit after deactivate draws nothing", || {
        let control = control()?;
        let home = active(&control)?;
        let mut child = start(&["--timeout", "0", "--outro", THEME])
            .map_err(|e| format!("starting raba: {e}"))?;

        let outcome = (|| {
            wait_for_the_splash(&control, home, &mut child)?;
            let (code, _) = ctl(&["deactivate"])?;
            if code != 0 {
                return Err(format!("deactivate exited {code}"));
            }
            wait_until("the terminal to be released", || active(&control) == Ok(home))?;

            let (code, _) = ctl(&["quit", "--wait"])?;
            if code != 0 {
                return Err(format!("quit exited {code}"));
            }
            // The outro is configured and must not have played: the display
            // never went back.
            if active(&control)? != home {
                return Err("an outro was played onto a released terminal".to_owned());
            }
            Ok(())
        })();

        clean_up(&mut child);
        outcome
    });
}

/// The claim `/run/raba.state` exists to make true.
pub fn a_killed_instance_is_resumed_on_the_same_terminal(report: &mut Report) {
    report.check("a second instance adopts the dead one's terminal and frame", || {
        let control = control()?;
        let home = active(&control)?;
        let mut first = start(&["--timeout", "0"]).map_err(|e| format!("starting raba: {e}"))?;

        let outcome = (|| {
            let taken = wait_for_the_splash(&control, home, &mut first)?;
            wait_until("a state file", || Path::new(STATE).exists())?;
            // Let it get somewhere, so "resumed from the same frame" is a claim
            // with something behind it.
            std::thread::sleep(Duration::from_millis(120));

            // SIGKILL: no Drop, no restore, nothing cleaned up. Exactly the
            // switch_root case.
            first.kill().map_err(|e| format!("SIGKILL: {e}"))?;
            let _ = first.wait();

            let before = std::fs::read_to_string(STATE)
                .map_err(|e| format!("reading the state file: {e}"))?;
            let frame = field(&before, "frame")?;
            println!("# killed on tty{taken} at frame {frame}");
            // Nothing cleared the screen, and nothing switched away.
            if active(&control)? != taken {
                return Err("the display left the terminal when the instance died".to_owned());
            }

            let mut second =
                start(&["--timeout", "0"]).map_err(|e| format!("restarting raba: {e}"))?;
            let outcome = (|| {
                wait_until("the second instance", || {
                    std::fs::read_to_string(STATE).is_ok_and(|now| now != before)
                })?;
                let after = std::fs::read_to_string(STATE)
                    .map_err(|e| format!("reading the state file: {e}"))?;
                let resumed = field(&after, "terminal")?;
                if resumed != u64::from(taken) {
                    return Err(format!("resumed on tty{resumed}, not tty{taken}"));
                }
                if active(&control)? != taken {
                    return Err("the resume moved the display".to_owned());
                }
                let (_, status) = ctl(&["status"])?;
                if !status.contains(&format!("terminal: {taken}")) {
                    return Err(format!("status disagrees: {status:?}"));
                }
                let (code, _) = ctl(&["quit", "--wait"])?;
                if code != 0 {
                    return Err(format!("quit exited {code}"));
                }
                Ok(())
            })();
            clean_up(&mut second);
            outcome
        })();

        clean_up(&mut first);
        wait_until("the display to come home", || active(&control) == Ok(home))?;
        outcome
    });
}

/// A second instance started while the first is alive must leave it alone —
/// which is the difference between a *state* file and a live socket.
pub fn a_second_instance_yields_to_the_running_one(report: &mut Report) {
    report.check("a second raba sees the first and does not draw", || {
        let control = control()?;
        let home = active(&control)?;
        let mut first = start(&["--timeout", "0"]).map_err(|e| format!("starting raba: {e}"))?;

        let outcome = (|| {
            let taken = wait_for_the_splash(&control, home, &mut first)?;
            let status = Command::new(RABA)
                .args(["--theme", THEME, "--socket", SOCKET, "--state", STATE, "--timeout", "0"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|e| format!("starting the second: {e}"))?;
            // Exits, and exits 0: declining is not an error.
            if !status.success() {
                return Err(format!("the second instance exited {status}"));
            }
            if active(&control)? != taken {
                return Err("the second instance moved the display".to_owned());
            }
            let (code, _) = ctl(&["ping"])?;
            if code != 0 {
                return Err("the first instance stopped answering".to_owned());
            }
            Ok(())
        })();

        clean_up(&mut first);
        outcome
    });
}

/// One `key=value` out of a state file.
fn field(text: &str, key: &str) -> Result<u64, String> {
    text.lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .ok_or_else(|| format!("no {key} in the state file"))?
        .parse()
        .map_err(|_| format!("{key} is not a number"))
}

/// The configuration file, end to end: a `Tty=` in a file turns into frames on
/// that terminal.
///
/// `Boot.Tty` is the one setting whose effect is visible from outside the
/// process, which makes it the setting worth driving a whole file through. The
/// parser is unit-tested and `Vt::take` is not reachable from a host test, so
/// this is where the two meet.
pub fn a_configuration_file_chooses_the_terminal(report: &mut Report) {
    report.check("Boot.Tty in a file is the terminal raba draws on", || {
        let control = control()?;
        let home = active(&control)?;
        // Ask the kernel which terminal is free rather than picking one: the
        // guest's console arrangement is the kernel's business, not ours.
        let wanted = ioctl::open_query(&control)
            .map_err(|e| format!("VT_OPENQRY: {e}"))?
            .ok_or("every terminal is in use")?;

        let conf = "/run/raba-test.conf";
        std::fs::write(
            conf,
            format!("[Boot]\nTheme={THEME}\nTty={wanted}\nTimeout=0\nShowDelay=0\n"),
        )
        .map_err(|e| format!("writing {conf}: {e}"))?;

        // No `--theme` and no `--tty`: everything this draws with comes out of
        // the file.
        let mut child = Command::new(RABA)
            .args(["--config", conf, "--socket", SOCKET, "--state", STATE])
            .args(["--defaults", "/nonexistent", "--cmdline", "/nonexistent"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("starting raba: {e}"))?;

        let outcome = (|| {
            let taken = wait_for_the_splash(&control, home, &mut child)?;
            if taken != wanted {
                return Err(format!("the file said tty{wanted} and raba drew on tty{taken}"));
            }
            let (_, status) = ctl(&["status"])?;
            if !status.contains(&format!("terminal: {wanted}")) {
                return Err(format!("status disagrees with the file: {status:?}"));
            }
            let (code, _) = ctl(&["quit", "--wait"])?;
            if code != 0 {
                return Err(format!("quit exited {code}"));
            }
            wait_until("the display to come home", || active(&control) == Ok(home))
        })();

        clean_up(&mut child);
        let _ = std::fs::remove_file(conf);
        outcome
    });
}

/// `raba.enable=0`: the switch somebody flips from a bootloader when a splash is
/// the reason their machine will not boot.
///
/// Unit-tested on the host as "writes no socket"; what can only be checked here
/// is the thing they actually care about, which is that no terminal moves.
pub fn the_kill_switch_touches_nothing(report: &mut Report) {
    report.check("raba.enable=0 draws nothing and takes no terminal", || {
        let control = control()?;
        let home = active(&control)?;
        let cmdline = "/run/raba-test.cmdline";
        std::fs::write(cmdline, "root=/dev/vda quiet raba.enable=0\n")
            .map_err(|e| format!("writing {cmdline}: {e}"))?;

        let status = Command::new(RABA)
            .args(["--theme", THEME, "--socket", SOCKET, "--state", STATE])
            .args(["--cmdline", cmdline, "--defaults", "/nonexistent", "--config", "/nonexistent"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("starting raba: {e}"))?;

        let _ = std::fs::remove_file(cmdline);
        if !status.success() {
            return Err(format!("a disabled raba exited {status}, and it exits 0"));
        }
        if active(&control)? != home {
            return Err("a disabled raba moved the display".to_owned());
        }
        if Path::new(SOCKET).exists() || Path::new(STATE).exists() {
            return Err("a disabled raba left files in /run".to_owned());
        }
        Ok(())
    });
}
