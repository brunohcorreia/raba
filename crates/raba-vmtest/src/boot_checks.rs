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
use std::os::unix::fs::OpenOptionsExt as _;
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
/// Let the instance finish, then make sure it is gone.
///
/// The grace period is not politeness. `quit --wait` returns on EOF, and EOF
/// arrives when the listener is dropped at the top of `Session::finish` —
/// before the process has finished restoring the terminal and before it has
/// returned from `main`. This used to send `SIGKILL` on the next line, which
/// means every check that ended with a clean `quit` was in fact racing the
/// tidy-up it had just asked for.
///
/// It also cost the coverage run every profile but a handful: an instrumented
/// binary writes its counters from an `atexit` handler, and `SIGKILL` cannot
/// be caught. `Vt::adopt` and `Vt::take` read 0% while the checks that use
/// them were passing. See `crates/raba-vmtest/src/profile.rs`.
fn clean_up(child: &mut Child) {
    for _ in 0..200u32 {
        match child.try_wait() {
            Ok(Some(_)) => break,
            // Only a process that is still here after two seconds is one that
            // is stuck, and a stuck splash is what SIGKILL is for.
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => break,
        }
    }
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
/// The splash leaves when something else takes the screen.
///
/// This is the whole handoff story on a machine with no display manager: the
/// control socket is root-only, a compositor in a user session cannot say
/// `quit`, and the clock is a backstop rather than an answer. So `raba` watches
/// the screen it was given, and when the display moves it stops — without
/// switching anything back, because the switch back would take the screen from
/// whatever just took it.
///
/// The third terminal is the point of the check. Switching *home* would prove
/// nothing: a normal restore ends there too. Switching somewhere else means the
/// display can only still be there if `raba` left it alone.
pub fn the_splash_leaves_when_the_screen_is_taken(report: &mut Report) {
    report.check("raba stops when the display moves, and does not take it back", || {
        let control = control()?;
        let home = active(&control)?;
        let mut child = start(&["--timeout", "0"]).map_err(|e| format!("starting raba: {e}"))?;

        let outcome = (|| {
            let taken = wait_for_the_splash(&control, home, &mut child)?;

            // Somewhere that is neither ours nor home, the way a compositor
            // starting on a terminal of its own would be.
            let elsewhere =
                (1..=12u16).find(|n| *n != taken && *n != home).ok_or("no third terminal")?;
            ioctl::activate(&control, elsewhere).map_err(|e| format!("VT_ACTIVATE: {e}"))?;
            ioctl::wait_active(&control, elsewhere).map_err(|e| format!("VT_WAITACTIVE: {e}"))?;
            println!(
                "# home is tty{home}, raba took tty{taken}, the display moved to tty{elsewhere}"
            );

            // No quit, no signal, nobody asking: it has to notice by itself.
            let left = Instant::now();
            wait_until("raba to notice and exit", || matches!(child.try_wait(), Ok(Some(_))))?;
            println!("# raba exited {:?} after the display moved", left.elapsed());

            let status = child.wait().map_err(|e| format!("reaping: {e}"))?;
            if !status.success() {
                return Err(format!("raba exited {status}, and it exits 0 or it is a bug"));
            }
            let now = active(&control)?;
            if now != elsewhere {
                return Err(format!("the display was pulled back to tty{now} from tty{elsewhere}"));
            }
            if Path::new(STATE).exists() {
                return Err("the state file outlived the instance that wrote it".to_owned());
            }
            if Path::new(SOCKET).exists() {
                return Err("the control socket was left behind".to_owned());
            }
            Ok(())
        })();

        clean_up(&mut child);
        // Put the display back for the checks after this one.
        let _ = ioctl::activate(&control, home);
        let _ = ioctl::wait_active(&control, home);
        outcome
    });
}

/// Everything in the kernel's ring buffer, oldest first.
///
/// Each open of `/dev/kmsg` starts at the oldest record it still holds, so this
/// is the whole buffer and not a tail. Non-blocking, because a reader that has
/// caught up blocks forever otherwise.
fn kernel_log() -> Result<String, String> {
    use std::io::Read as _;
    let file = OpenOptions::new()
        .read(true)
        // O_NONBLOCK. A constant rather than a dependency: this crate exists
        // only inside the test VM and has no business pulling in libc for one
        // number that Linux has never changed.
        .custom_flags(0o4000)
        .open("/dev/kmsg")
        .map_err(|e| format!("opening /dev/kmsg: {e}"))?;
    // One `read` per record, which is how this device works: it is not a stream
    // and `read_to_string` on it either blocks or throws away what it collected
    // when the buffer runs out. EAGAIN *is* the end of the buffer here, and the
    // ordinary ending of this loop.
    let mut file = file;
    let mut text = String::new();
    let mut record = [0u8; 8192];
    loop {
        match file.read(&mut record) {
            Ok(0) | Err(_) => break,
            Ok(read) => text.push_str(&String::from_utf8_lossy(record.get(..read).unwrap_or(&[]))),
        }
    }
    if text.is_empty() {
        return Err("/dev/kmsg read back empty, which no running kernel does".to_owned());
    }
    Ok(text)
}

fn ours(log: &str) -> usize {
    log.lines().filter(|line| line.contains("raba: ")).count()
}

/// What is on the console is the art.
///
/// Every other check here asks the kernel *about* the terminal — which one is
/// active, what mode it is in, who has it. None of them asks what is on it, and
/// that turns out to be a different question: a tier 3b run found a machine
/// where RABA held the screen for three and a half seconds with the display
/// switched to its terminal, and the console was blank the whole time.
///
/// `/dev/vcsN` is the kernel's own copy of the text on virtual console *N* —
/// rows times columns of characters, no escape sequences, no scrollback. It is
/// the one answer that does not depend on a video mode, a framebuffer driver,
/// or an emulator's idea of what the screen looks like.
pub fn the_console_holds_the_art_and_not_a_blank_screen(report: &mut Report) {
    report.check("the characters on the console are the ones that were drawn", || {
        let control = control()?;
        let home = active(&control)?;
        let mut child = start(&["--timeout", "0"]).map_err(|e| format!("starting raba: {e}"))?;

        let outcome = (|| {
            let taken = wait_for_the_splash(&control, home, &mut child)?;
            // A frame or two of grace: the switch happens before the first
            // write lands, and this is a race the real thing also has.
            std::thread::sleep(Duration::from_millis(250));

            let path = format!("/dev/vcs{taken}");
            let screen = std::fs::read(&path).map_err(|e| format!("reading {path}: {e}"))?;
            let ink = screen.iter().filter(|byte| byte.is_ascii_graphic()).count();
            println!("# {path}: {} bytes, {ink} of them ink", screen.len());
            if ink < 100 {
                return Err(format!(
                    "the console RABA is drawing on holds {ink} printable characters"
                ));
            }
            Ok(())
        })();

        let _ = child.kill();
        let _ = child.wait();
        let _ = Command::new(CTL).arg("quit").status();
        outcome
    });
}

/// A person's mistake reaches the kernel log, and a healthy boot does not.
///
/// Two halves of one promise, and neither is worth much without the other. The
/// splash is allowed one line when something is wrong, because in an initramfs
/// there is no journal yet and `/run/raba.early.log` is not somewhere anybody
/// thinks to look. It is allowed *nothing* when all is well: a boot splash that
/// writes to `dmesg` every time it runs is a boot splash that has made the
/// system log worse for everybody who was using it to debug something else.
pub fn a_mistake_reaches_the_kernel_log_and_a_good_boot_does_not(report: &mut Report) {
    report.check("a broken configuration leaves exactly one line in dmesg", || {
        let conf = "/run/raba-test-broken.conf";
        std::fs::write(conf, "[Boot]\nTheme=nosuchtheme\n")
            .map_err(|e| format!("writing {conf}: {e}"))?;
        let before = ours(&kernel_log()?);

        // stderr to a file: not a socket, not a terminal, which is what an
        // initramfs looks like and is the only case that should reach kmsg.
        let log = std::fs::File::create("/run/raba-test-stderr.log")
            .map_err(|e| format!("creating the stderr file: {e}"))?;
        let status = Command::new(RABA)
            .args(["--socket", SOCKET, "--state", STATE])
            .args(["--config", conf, "--defaults", "/nonexistent", "--themes", "/nonexistent"])
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .status()
            .map_err(|e| format!("starting raba: {e}"))?;
        if !status.success() {
            return Err(format!("a missing theme exited {status}, and it exits 0"));
        }

        let said = std::fs::read_to_string("/run/raba-test-stderr.log").unwrap_or_default();
        if !said.contains("no theme found") {
            return Err(format!("stderr did not say what it looked for: {said:?}"));
        }
        let after = ours(&kernel_log()?);
        match after.checked_sub(before) {
            Some(1) => {}
            Some(0) => return Err("nothing reached the kernel log".to_owned()),
            Some(many) => return Err(format!("{many} lines reached the kernel log, not one")),
            None => return Err("the kernel log lost records mid-check".to_owned()),
        }

        // And now the other half: a boot that works says nothing at all.
        let before = ours(&kernel_log()?);
        let control = control()?;
        let home = active(&control)?;
        let mut child = start(&["--timeout", "1"]).map_err(|e| format!("starting raba: {e}"))?;
        let taken = wait_for_the_splash(&control, home, &mut child)?;
        let _ = Command::new(CTL).args(["quit", "--wait"]).status();
        let _ = child.wait();
        println!("# drew on tty{taken} and said nothing");
        let after = ours(&kernel_log()?);
        if after != before {
            let log = kernel_log()?;
            let added: Vec<&str> = log
                .lines()
                .filter(|line| line.contains("raba: "))
                .rev()
                .take(after.saturating_sub(before))
                .map(str::trim)
                .collect();
            return Err(format!("a healthy splash wrote to the kernel log: {added:?}"));
        }
        let _ = std::fs::remove_file(conf);
        Ok(())
    });
}

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

/// `SIGTERM` is `switch_root`, and the whole handoff turns on it.
///
/// systemd kills everything in the initramfs before pivoting to the real root.
/// RABA blocks the signal, acts on it at a frame boundary, writes down where it
/// was, and — this is the part — **does not hand the console back**. The art
/// stays on the screen it was drawn on, the display stays where it is, and the
/// instance started after the pivot adopts both. That is what makes the pivot a
/// pause instead of a flash of black, and it is promised in `docs/early-boot.md`
/// and in `integration/systemd/raba-initrd.service`.
///
/// A unit test proves it against a fake console. This proves it against a real
/// one, which is the only place "the screen still has the art on it" means
/// anything.
pub fn a_terminated_instance_leaves_the_screen_as_it_is(report: &mut Report) {
    report.check("SIGTERM hands the terminal over without clearing it", || {
        let control = control()?;
        let home = active(&control)?;
        let mut first = start(&["--timeout", "0"]).map_err(|e| format!("starting raba: {e}"))?;

        let outcome = (|| {
            let taken = wait_for_the_splash(&control, home, &mut first)?;
            wait_until("a state file", || Path::new(STATE).exists())?;
            std::thread::sleep(Duration::from_millis(120));

            let pid = rustix::process::Pid::from_raw(
                i32::try_from(first.id()).map_err(|_| "a pid that does not fit")?,
            )
            .ok_or("a pid of zero")?;
            rustix::process::kill_process(pid, rustix::process::Signal::TERM)
                .map_err(|e| format!("SIGTERM: {e}"))?;

            // It exits of its own accord, and exits 0: a splash that fails on
            // the way out of an initramfs is a failed unit in the journal of
            // every boot.
            wait_until("raba to act on SIGTERM", || matches!(first.try_wait(), Ok(Some(_))))?;
            let status = first.wait().map_err(|e| format!("reaping: {e}"))?;
            if !status.success() {
                return Err(format!("raba exited {status} on SIGTERM"));
            }

            // Not handed back. Both halves matter: the display is still on the
            // splash's terminal, and the terminal is still in text mode with
            // the art on it rather than cleared.
            let now = active(&control)?;
            if now != taken {
                return Err(format!("the display went home to tty{now} instead of staying"));
            }
            let text = std::fs::read_to_string(STATE)
                .map_err(|e| format!("the successor has nothing to resume from: {e}"))?;
            if !text.contains(&format!("terminal={taken}")) {
                return Err(format!("the state file does not name tty{taken}: {text:?}"));
            }
            println!("# SIGTERM on tty{taken}, state left behind: {} bytes", text.len());

            // And the successor picks it up, on the same terminal, with the
            // screen never having changed hands.
            let mut second =
                start(&["--timeout", "0"]).map_err(|e| format!("restarting raba: {e}"))?;
            let outcome = (|| {
                wait_until("the successor", || {
                    std::fs::read_to_string(STATE).is_ok_and(|now| now != text)
                })?;
                let resumed =
                    field(&std::fs::read_to_string(STATE).unwrap_or_default(), "terminal")?;
                if resumed != u64::from(taken) {
                    return Err(format!("the successor took tty{resumed}, not tty{taken}"));
                }
                if active(&control)? != taken {
                    return Err("the handoff moved the display".to_owned());
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

/// A compositor taking the same terminal, which is how this ends on a machine
/// with no display manager.
///
/// Switching the display elsewhere is one way to lose the screen and is checked
/// above. The other is a program putting *this* terminal into graphics mode —
/// what a Wayland compositor started on the console does, and the case the
/// project cares most about getting right, since holding the screen from boot
/// until the compositor's first frame is the whole point. RABA has to notice,
/// stop drawing, and leave the mode alone: a splash that writes text at a
/// terminal in `KD_GRAPHICS` corrupts whatever is being scanned out of it.
pub fn the_splash_leaves_when_the_terminal_goes_into_graphics_mode(report: &mut Report) {
    report.check("raba stops when its own terminal enters graphics mode", || {
        let control = control()?;
        let home = active(&control)?;
        let mut child = start(&["--timeout", "0"]).map_err(|e| format!("starting raba: {e}"))?;

        let outcome = (|| {
            let taken = wait_for_the_splash(&control, home, &mut child)?;
            let screen = OpenOptions::new()
                .read(true)
                .write(true)
                .open(format!("/dev/tty{taken}"))
                .map_err(|e| format!("opening tty{taken}: {e}"))?;

            ioctl::set_mode(&screen, ioctl::KD_GRAPHICS)
                .map_err(|e| format!("KDSETMODE KD_GRAPHICS: {e}"))?;
            println!("# tty{taken} put into graphics mode under raba");

            let noticed = Instant::now();
            wait_until("raba to notice graphics mode", || matches!(child.try_wait(), Ok(Some(_))))?;
            println!("# raba exited {:?} after the mode changed", noticed.elapsed());

            let status = child.wait().map_err(|e| format!("reaping: {e}"))?;
            if !status.success() {
                return Err(format!("raba exited {status}, and it exits 0 or it is a bug"));
            }
            // It must not have put the terminal back into text mode on the way
            // out: the compositor owns that now.
            let mode = ioctl::get_mode(&screen).map_err(|e| format!("KDGETMODE: {e}"))?;
            if mode != ioctl::KD_GRAPHICS {
                return Err("raba pulled the terminal back into text mode".to_owned());
            }
            if active(&control)? != taken {
                return Err("raba moved the display away from the new owner".to_owned());
            }

            // Tidy up after the compositor this check is pretending to be.
            ioctl::set_mode(&screen, ioctl::KD_TEXT)
                .map_err(|e| format!("KDSETMODE KD_TEXT: {e}"))?;
            Ok(())
        })();

        clean_up(&mut child);
        let _ = ioctl::activate(&control, home);
        let _ = ioctl::wait_active(&control, home);
        outcome
    });
}
