//! The boot loop, on a laptop.
//!
//! This is the program. Everything else in the tree decides something — the
//! phase machine, the wire parser, the state validator — and each of those is
//! tested on its own. This is the one place where a clock, a socket and a
//! console meet, and until [`raba::vt::Console`] existed it could not be run
//! outside a virtual machine: `Session` held a `Vt`, a `Vt` comes from
//! `VT_OPENQRY`, and `VT_OPENQRY` needs root and a console that `cargo test`
//! does not have.
//!
//! So it was exercised only by the QEMU tier, which boots a real kernel and
//! asserts thirty things about the result. That tier is the only way to know the
//! ioctls are right and it stays. What it cannot do is reach inside: it says the
//! splash drew and gave the terminal back, not that the branch for *a console
//! that changes size between two frames* was ever taken. Those branches are the
//! reason this file exists, and most of them are reached here by handing the
//! loop a console that misbehaves on purpose.
//!
//! The fake is deliberately dumb — it records and it answers — so that a test
//! failing means the loop is wrong, not that the double is.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic, clippy::arithmetic_side_effects)]
#![allow(
    unsafe_code,
    reason = "one `pthread_kill`, to deliver SIGTERM to the thread that blocked it"
)]

use raba::boot::{run_on, Outcome, Settings};
use raba::conflict;
use raba::control::wire::MAX_REQUEST;
use raba::state;
use raba::vt::{Console, Displaced, Error, Screen};
use raba_core::surface::{Size, Surface};
use raba_core::{Cell, Grid, Movie};
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

// --- the double ------------------------------------------------------------

/// How the loop asked for a terminal. Three situations, not three options:
/// see [`raba::vt::Console`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asked {
    Adopt(u16, u16),
    Take(u16),
    Acquire,
}

/// Everything the loop did to a console, in order.
#[derive(Debug, Default)]
struct Log {
    asked: Vec<Asked>,
    frames: Vec<Vec<u8>>,
    restored: u32,
    relinquished: Vec<Displaced>,
    forgotten: u32,
}

/// A console that hands out screens and remembers what was done to them.
///
/// Cloning shares the record, so a test keeps a handle after the loop has taken
/// ownership of its copy.
#[derive(Debug, Clone)]
struct Fake {
    log: Arc<Mutex<Log>>,
    /// What `size()` answers. Changing it mid-run is a console being replaced
    /// under the splash, which is what happens when fbcon takes over from
    /// dummycon a second into a real boot.
    columns: Arc<AtomicU16>,
    lines: Arc<AtomicU16>,
    /// What `displaced()` answers: `0` nothing, `1` graphics, anything else the
    /// terminal the display moved to.
    displaced: Arc<AtomicU16>,
    /// Refuse to hand out a screen at all.
    refuse: Arc<AtomicBool>,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            log: Arc::new(Mutex::new(Log::default())),
            columns: Arc::new(AtomicU16::new(80)),
            lines: Arc::new(AtomicU16::new(25)),
            displaced: Arc::new(AtomicU16::new(0)),
            refuse: Arc::new(AtomicBool::new(false)),
        }
    }

    fn refusing() -> Fake {
        let fake = Fake::new();
        fake.refuse.store(true, Ordering::SeqCst);
        fake
    }

    fn resize(&self, columns: u16, lines: u16) {
        self.columns.store(columns, Ordering::SeqCst);
        self.lines.store(lines, Ordering::SeqCst);
    }

    fn log(&self) -> std::sync::MutexGuard<'_, Log> {
        self.log.lock().expect("the log is not poisoned")
    }

    /// Everything written to the screen so far, concatenated.
    fn written(&self) -> Vec<u8> {
        self.log().frames.concat()
    }

    fn hand_out(&mut self, how: Asked) -> Result<FakeScreen, Error> {
        self.log().asked.push(how);
        if self.refuse.load(Ordering::SeqCst) {
            return Err(Error::NoFreeTerminal);
        }
        let (number, previous) = match how {
            Asked::Adopt(terminal, previous) => (terminal, previous),
            Asked::Take(number) => (number, 1),
            Asked::Acquire => (7, 1),
        };
        Ok(FakeScreen { shared: self.clone(), number, previous })
    }
}

impl Console for Fake {
    type Screen = FakeScreen;

    fn adopt(&mut self, terminal: u16, previous: u16) -> Result<FakeScreen, Error> {
        self.hand_out(Asked::Adopt(terminal, previous))
    }

    fn take(&mut self, number: u16) -> Result<FakeScreen, Error> {
        self.hand_out(Asked::Take(number))
    }

    fn acquire(&mut self) -> Result<FakeScreen, Error> {
        self.hand_out(Asked::Acquire)
    }
}

#[derive(Debug)]
struct FakeScreen {
    shared: Fake,
    number: u16,
    previous: u16,
}

impl Surface for FakeScreen {
    fn size(&self) -> Size {
        Size {
            columns: self.shared.columns.load(Ordering::SeqCst),
            lines: self.shared.lines.load(Ordering::SeqCst),
        }
    }

    fn present(&mut self, frame: &[u8]) -> std::io::Result<()> {
        self.shared.log().frames.push(frame.to_vec());
        Ok(())
    }

    fn restore(&mut self) -> std::io::Result<()> {
        self.shared.log().restored += 1;
        Ok(())
    }
}

impl Screen for FakeScreen {
    fn number(&self) -> u16 {
        self.number
    }

    fn previous(&self) -> u16 {
        self.previous
    }

    fn displaced(&self) -> Option<Displaced> {
        match self.shared.displaced.load(Ordering::SeqCst) {
            0 => None,
            1 => Some(Displaced::Graphics),
            other => Some(Displaced::Switched(other)),
        }
    }

    fn relinquish(&mut self, why: Displaced) -> std::io::Result<()> {
        self.shared.log().relinquished.push(why);
        Ok(())
    }

    fn forget(&mut self) {
        self.shared.log().forgotten += 1;
    }
}

// --- scaffolding -----------------------------------------------------------

/// A directory of our own, removed afterwards, with settings pointed into it.
struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Scratch {
        let mut dir = std::env::temp_dir();
        dir.push(format!("raba-boot-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch directory");
        // An empty `/proc` so the conflict check finds no plymouthd and does not
        // walk the real one, which would be both slow and a test that passes or
        // fails depending on what the developer happens to be running.
        std::fs::create_dir_all(dir.join("proc")).expect("a fake /proc");
        Scratch { dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// Settings with a two-frame looping theme already written, and every path
    /// inside the scratch directory.
    fn settings(&self) -> Settings {
        self.settings_for(&two_frames())
    }

    fn settings_for(&self, movie: &Movie) -> Settings {
        let theme = self.path("theme.raba");
        std::fs::write(&theme, raba_core::encode(movie)).expect("writing the theme");
        Settings {
            theme,
            outro: None,
            show_delay: Duration::ZERO,
            // Long enough that no test hits it by accident; the ones about the
            // timeout set their own.
            timeout: Some(Duration::from_secs(600)),
            max_duration: Duration::from_millis(200),
            tty: None,
            socket: self.path("raba.sock"),
            state: self.path("raba.state"),
            places: conflict::Where {
                plymouth_pid: self.path("plymouth.pid"),
                proc: self.path("proc"),
                socket: self.path("raba.sock"),
            },
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A movie with two visibly different frames, so that "it drew again" and "it
/// drew the same thing twice" are distinguishable in the bytes.
fn two_frames() -> Movie {
    frames_of(&['A', 'B'])
}

fn frames_of(characters: &[char]) -> Movie {
    let mut ops = Vec::new();
    let mut frames = Vec::new();
    for ch in characters {
        let mut grid = Grid::new(4, 2, raba_core::Palette::Ansi16).expect("a grid");
        for line in 0..2 {
            for column in 0..4 {
                grid.set(column, line, Cell { ch: *ch, fg: 7, bg: 0 });
            }
        }
        let start = ops.len();
        for (index, cell) in grid.cells().iter().enumerate() {
            ops.push(raba_core::Op {
                cell: u32::try_from(index).unwrap(),
                ch: cell.ch,
                fg: cell.fg,
                bg: cell.bg,
            });
        }
        frames.push(raba_core::Frame {
            delay_ms: 10,
            op_start: u32::try_from(start).unwrap(),
            op_count: u16::try_from(ops.len() - start).unwrap(),
        });
    }
    Movie {
        columns: 4,
        lines: 2,
        palette: raba_core::Palette::Ansi16,
        scaling: raba_core::Scaling::Crop,
        looping: true,
        frames,
        ops,
    }
}

/// A running loop, and the way to ask it things.
struct Running {
    outcome: mpsc::Receiver<Outcome>,
    socket: PathBuf,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Running {
    /// Start the loop on a thread and wait until its socket answers, so that a
    /// command sent next cannot race the bind.
    fn start(settings: Settings, console: Fake) -> Running {
        let socket = settings.socket.clone();
        let (send, outcome) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _ = send.send(run_on(&settings, console));
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if UnixStream::connect(&socket).is_ok() {
                return Running { outcome, socket, thread: Some(thread) };
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("the loop never started listening on {}", socket.display());
    }

    /// Send one command and return the whole reply, EOF-terminated.
    fn ask(&self, request: &str) -> String {
        assert!(request.len() < MAX_REQUEST, "the test's own request is over the limit");
        let mut stream = UnixStream::connect(&self.socket).expect("connecting");
        writeln!(stream, "{request}").expect("sending");
        stream.flush().expect("flushing");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("a read timeout, so a wedged loop fails the test instead of hanging it");
        let mut reply = String::new();
        let mut reader = BufReader::new(stream);
        match request {
            // Both of these are answered and then followed by more, so reading
            // to EOF is the only way to get all of it.
            "status" | "quit --wait" => {
                reader.read_to_string(&mut reply).expect("reading the reply");
            }
            _ => {
                reader.read_line(&mut reply).expect("reading the reply");
            }
        }
        reply
    }

    /// One field of `status`.
    fn status(&self, key: &str) -> String {
        let report = self.ask("status");
        report
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{key}: ")))
            .unwrap_or_else(|| panic!("no {key} in {report:?}"))
            .to_owned()
    }

    fn finish(mut self) -> Outcome {
        let outcome = self
            .outcome
            .recv_timeout(Duration::from_secs(10))
            .expect("the loop ended within ten seconds");
        if let Some(thread) = self.thread.take() {
            thread.join().expect("the loop thread did not panic");
        }
        outcome
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        // A test that failed mid-way must not leave a thread drawing forever
        // into a scratch directory that is about to be deleted.
        if let Some(thread) = self.thread.take() {
            let _ = UnixStream::connect(&self.socket)
                .and_then(|mut stream| stream.write_all(b"quit\n"));
            let _ = thread.join();
        }
    }
}

/// Wait for something to become true, or fail the test saying what did not.
fn until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if ready() {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("timed out waiting for {what}");
}

// --- before a terminal is asked for ----------------------------------------

#[test]
fn a_theme_that_is_not_there_ends_the_run_before_any_terminal_is_asked_for() {
    let scratch = Scratch::new("no-theme");
    let mut settings = scratch.settings();
    settings.theme = scratch.path("absent.raba");
    let console = Fake::new();

    let outcome = run_on(&settings, console.clone());

    assert!(matches!(outcome, Outcome::NoTheme(_)), "{outcome:?}");
    assert!(
        console.log().asked.is_empty(),
        "a splash with nothing to draw must not take somebody's console first"
    );
}

#[test]
fn a_theme_that_is_not_a_raba_file_ends_the_run_with_the_decoder_s_reason() {
    let scratch = Scratch::new("bad-theme");
    let settings = scratch.settings();
    std::fs::write(&settings.theme, b"this is not a movie").expect("writing rubbish");

    let outcome = run_on(&settings, Fake::new());

    match outcome {
        Outcome::NoTheme(why) => assert!(
            why.contains(&settings.theme.display().to_string()),
            "the reason should name the file: {why}"
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn plymouth_already_running_yields_before_any_terminal_is_asked_for() {
    let scratch = Scratch::new("yield");
    let settings = scratch.settings();
    std::fs::write(&settings.places.plymouth_pid, "1234\n").expect("a pid file");
    let console = Fake::new();

    let outcome = run_on(&settings, console.clone());

    assert!(matches!(outcome, Outcome::Yielded(_)), "{outcome:?}");
    assert!(console.log().asked.is_empty(), "yielding means touching nothing");
}

#[test]
fn a_console_that_will_not_be_taken_ends_the_run_and_says_which_terminal_was_wanted() {
    let scratch = Scratch::new("no-terminal");
    let settings = scratch.settings();
    let console = Fake::refusing();

    let outcome = run_on(&settings, console.clone());

    assert!(matches!(outcome, Outcome::NoTerminal(_)), "{outcome:?}");
    assert_eq!(console.log().asked, vec![Asked::Acquire]);
    assert!(console.log().frames.is_empty(), "nothing can be drawn without a terminal");
}

// --- how a terminal is asked for -------------------------------------------

#[test]
fn an_ordinary_boot_acquires_a_free_terminal_and_draws_on_it() {
    let scratch = Scratch::new("ordinary");
    let settings = scratch.settings();
    let console = Fake::new();
    let running = Running::start(settings, console.clone());

    until("a frame", || !console.log().frames.is_empty());
    assert_eq!(console.log().asked, vec![Asked::Acquire]);

    assert_eq!(running.ask("quit"), "+OK\n");
    assert!(matches!(running.finish(), Outcome::Finished));
    assert_eq!(console.log().restored, 1, "the terminal goes back exactly once");
}

#[test]
fn boot_tty_takes_that_terminal_by_name_rather_than_asking_for_a_free_one() {
    let scratch = Scratch::new("named-tty");
    let mut settings = scratch.settings();
    settings.tty = Some(9);
    let console = Fake::new();
    let running = Running::start(settings, console.clone());

    until("a frame", || !console.log().frames.is_empty());
    assert_eq!(console.log().asked, vec![Asked::Take(9)]);
    assert_eq!(running.status("terminal"), "9");

    running.ask("quit");
    running.finish();
}

#[test]
fn a_fresh_state_file_adopts_its_terminal_and_continues_from_its_frame() {
    let scratch = Scratch::new("adopt");
    let settings = scratch.settings();
    state::save(
        &settings.state,
        &state::State {
            terminal: 5,
            previous: 2,
            frame: 1,
            phase: raba::lifecycle::Phase::Boot,
            theme: settings.theme.clone(),
            written_ms: state::now_ms(),
        },
    )
    .expect("writing a resume state");

    let console = Fake::new();
    let running = Running::start(settings, console.clone());

    until("a frame", || !console.log().frames.is_empty());
    assert_eq!(console.log().asked, vec![Asked::Adopt(5, 2)]);
    assert_eq!(running.status("previous"), "2");

    // Adopting must not clear: the screen is still showing the frame the dead
    // instance left, and wiping it is the flash of black this whole path exists
    // to avoid.
    let written = console.written();
    assert!(
        !written.starts_with(b"\x1b[2J"),
        "an adopted terminal was cleared: {:?}",
        String::from_utf8_lossy(written.get(..40).unwrap_or(&written))
    );

    running.ask("quit");
    running.finish();
}

#[test]
fn a_fresh_boot_clears_the_terminal_and_hides_the_cursor_before_drawing() {
    let scratch = Scratch::new("clear");
    let settings = scratch.settings();
    let console = Fake::new();
    let running = Running::start(settings, console.clone());

    until("a frame", || !console.log().frames.is_empty());
    assert_eq!(
        console.log().frames.first().map(Vec::as_slice),
        Some(b"\x1b[2J\x1b[H\x1b[?25l".as_slice()),
        "a borrowed terminal starts from a known screen with no cursor blinking in the art"
    );

    running.ask("quit");
    running.finish();
}

/// Not a check this program makes, and the test is here to say so on purpose.
///
/// `State` carries the theme path, and nothing compares it with the configured
/// one. That is deliberate: the resume exists to span `switch_root`, where the
/// initramfs instance and the real-root instance resolve their theme
/// independently, and refusing to continue because two strings differ would put
/// back the flash of black the design exists to remove. What bounds the risk is
/// the other check — `state::load` refuses a frame past the end of the theme
/// that was *actually* loaded — so the worst case is a frame index that means
/// something else in an animation of the same length, which the next full frame
/// paints over.
#[test]
fn a_state_file_naming_another_theme_is_still_adopted_because_the_path_is_not_the_identity() {
    let scratch = Scratch::new("other-theme");
    let settings = scratch.settings();
    state::save(
        &settings.state,
        &state::State {
            terminal: 5,
            previous: 2,
            frame: 1,
            phase: raba::lifecycle::Phase::Boot,
            theme: scratch.path("a-different-theme.raba"),
            written_ms: state::now_ms(),
        },
    )
    .expect("writing a resume state");

    let console = Fake::new();
    let running = Running::start(settings, console.clone());

    until("a frame", || !console.log().frames.is_empty());
    assert_eq!(
        console.log().asked,
        vec![Asked::Adopt(5, 2)],
        "the theme path is recorded, not enforced; see this test's documentation"
    );

    running.ask("quit");
    running.finish();
}

// --- the clock --------------------------------------------------------------

#[test]
fn the_show_delay_takes_no_terminal_and_draws_nothing_until_it_has_passed() {
    let scratch = Scratch::new("show-delay");
    let mut settings = scratch.settings();
    settings.show_delay = Duration::from_millis(400);
    let console = Fake::new();
    let running = Running::start(settings, console.clone());

    // Halfway through the delay: a machine that finishes booting before this
    // point must have seen no splash at all, which is the entire feature.
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(running.status("phase"), "waiting");
    assert!(console.log().asked.is_empty(), "a terminal was taken during the show delay");
    assert!(console.log().frames.is_empty(), "something was drawn during the show delay");

    until("the delay to pass and a frame to arrive", || !console.log().frames.is_empty());
    assert_eq!(console.log().asked, vec![Asked::Acquire]);

    running.ask("quit");
    running.finish();
}

#[test]
fn the_timeout_gives_the_console_back_and_keeps_answering() {
    let scratch = Scratch::new("timeout");
    let mut settings = scratch.settings();
    settings.timeout = Some(Duration::from_millis(120));
    let console = Fake::new();
    let running = Running::start(settings, console.clone());

    until("the timeout", || console.log().restored > 0);
    // Not an exit. `raba` reveals: the console goes back so the user can read
    // whatever stalled the boot, and the process stays up to be told to quit.
    assert_eq!(running.status("phase"), "revealed");
    assert_eq!(running.status("terminal"), "0", "a revealed splash holds no terminal");

    let drawn = console.log().frames.len();
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(console.log().frames.len(), drawn, "a revealed splash kept drawing");

    running.ask("quit");
    assert!(matches!(running.finish(), Outcome::Finished));
}

#[test]
fn the_outro_plays_once_and_is_bounded_by_its_own_ceiling() {
    let scratch = Scratch::new("outro");
    let mut settings = scratch.settings();
    let outro = scratch.path("outro.raba");
    // A looping movie as the outro, so that only `Outro.MaxDuration` can end it:
    // if the ceiling were not enforced this test would hang rather than pass.
    std::fs::write(&outro, raba_core::encode(&frames_of(&['X', 'Y']))).expect("writing the outro");
    settings.outro = Some(outro);
    settings.max_duration = Duration::from_millis(150);
    let console = Fake::new();
    let running = Running::start(settings, console.clone());

    until("a boot frame", || !console.log().frames.is_empty());
    assert_eq!(running.ask("quit"), "+OK\n");

    let outcome = running.finish();
    assert!(matches!(outcome, Outcome::Finished), "{outcome:?}");
    let written = console.written();
    assert!(written.windows(1).any(|w| w == b"X"), "the outro never drew a frame of its own");
    assert_eq!(console.log().restored, 1);
}

#[test]
fn a_resumed_instance_plays_no_outro_because_there_is_no_handoff_to_animate() {
    let scratch = Scratch::new("resume-no-outro");
    let mut settings = scratch.settings();
    let outro = scratch.path("outro.raba");
    std::fs::write(&outro, raba_core::encode(&frames_of(&['X']))).expect("writing the outro");
    settings.outro = Some(outro);
    state::save(
        &settings.state,
        &state::State {
            terminal: 5,
            previous: 2,
            frame: 0,
            phase: raba::lifecycle::Phase::Boot,
            theme: settings.theme.clone(),
            written_ms: state::now_ms(),
        },
    )
    .expect("writing a resume state");

    let console = Fake::new();
    let running = Running::start(settings, console.clone());
    until("a frame", || !console.log().frames.is_empty());
    running.ask("quit");
    running.finish();

    let written = console.written();
    assert!(
        !written.windows(1).any(|w| w == b"X"),
        "a resumed splash animated away from a screen the user had been looking at"
    );
}

// --- the socket -------------------------------------------------------------

#[test]
fn deactivate_gives_the_console_back_and_reactivate_takes_a_fresh_one() {
    let scratch = Scratch::new("deactivate");
    let settings = scratch.settings();
    let console = Fake::new();
    let running = Running::start(settings, console.clone());

    until("a frame", || !console.log().frames.is_empty());
    assert_eq!(running.ask("deactivate"), "+OK\n");
    until("the console to go back", || console.log().restored == 1);
    assert_eq!(running.status("phase"), "deactivated");

    assert_eq!(running.ask("reactivate"), "+OK\n");
    until("a second terminal", || console.log().asked.len() == 2);
    assert_eq!(console.log().asked, vec![Asked::Acquire, Asked::Acquire]);

    running.ask("quit");
    running.finish();
    assert_eq!(console.log().restored, 2, "both terminals were given back");
}

#[test]
fn a_message_is_drawn_under_the_animation_and_refused_when_there_is_nothing_to_draw_on() {
    let scratch = Scratch::new("message");
    let settings = scratch.settings();
    let console = Fake::new();
    let running = Running::start(settings, console.clone());

    until("a frame", || !console.log().frames.is_empty());
    assert_eq!(running.ask("message unlocking"), "+OK\n");
    until("the message on the screen", || console.written().windows(9).any(|w| w == b"unlocking"));

    running.ask("deactivate");
    until("the console to go back", || console.log().restored == 1);
    assert_eq!(
        running.ask("message unlocking"),
        "-ERR not drawing\n",
        "a message accepted while nothing is on screen is a client that thinks it worked"
    );

    running.ask("quit");
    running.finish();
}

#[test]
fn progress_is_refused_with_a_reason_rather_than_accepted_and_ignored() {
    let scratch = Scratch::new("progress");
    let settings = scratch.settings();
    let console = Fake::new();
    let running = Running::start(settings, console.clone());

    until("a frame", || !console.log().frames.is_empty());
    assert_eq!(running.ask("progress 0.5"), "-ERR not configured\n");

    running.ask("quit");
    running.finish();
}

#[test]
fn a_command_that_is_not_one_is_refused_without_disturbing_the_animation() {
    let scratch = Scratch::new("nonsense");
    let settings = scratch.settings();
    let console = Fake::new();
    let running = Running::start(settings, console.clone());

    until("a frame", || !console.log().frames.is_empty());
    let reply = running.ask("defenestrate");
    assert!(reply.starts_with("-ERR "), "{reply:?}");

    let before = console.log().frames.len();
    until("the animation to carry on", || console.log().frames.len() > before);

    running.ask("quit");
    running.finish();
}

#[test]
fn status_reports_the_protocol_the_phase_and_the_terminal_in_use() {
    let scratch = Scratch::new("status");
    let settings = scratch.settings();
    let console = Fake::new();
    let running = Running::start(settings, console.clone());

    until("a frame", || !console.log().frames.is_empty());
    assert_eq!(running.status("protocol"), "1");
    assert_eq!(running.status("phase"), "boot");
    assert_eq!(running.status("terminal"), "7");
    assert_eq!(running.status("version"), env!("CARGO_PKG_VERSION"));

    running.ask("quit");
    running.finish();
}

#[test]
fn quit_wait_gets_its_eof_only_once_the_run_is_over() {
    let scratch = Scratch::new("quit-wait");
    let settings = scratch.settings();
    let console = Fake::new();
    let running = Running::start(settings, console.clone());

    until("a frame", || !console.log().frames.is_empty());
    // Reading to EOF is the whole point: the reply arrives first, and the
    // close arrives when the process is done with the console.
    let reply = running.ask("quit --wait");
    assert_eq!(reply, "+OK\n");
    assert_eq!(console.log().restored, 1, "EOF arrived before the terminal went back");

    assert!(matches!(running.finish(), Outcome::Finished));
}

// --- the console misbehaving ------------------------------------------------

#[test]
fn a_display_that_moves_away_ends_the_run_without_taking_the_screen_back() {
    let scratch = Scratch::new("switched");
    let settings = scratch.settings();
    let console = Fake::new();
    let running = Running::start(settings.clone(), console.clone());

    until("a frame", || !console.log().frames.is_empty());
    console.displaced.store(3, Ordering::SeqCst);

    let outcome = running.finish();
    assert!(matches!(outcome, Outcome::Displaced(Displaced::Switched(3))), "{outcome:?}");
    assert_eq!(console.log().relinquished, vec![Displaced::Switched(3)]);
    assert_eq!(
        console.log().restored,
        0,
        "restoring would have dragged the display back off whatever just took it"
    );
    assert!(!settings.state.exists(), "the resume state outlived the instance that wrote it");
}

#[test]
fn a_terminal_that_goes_into_graphics_mode_ends_the_run_the_same_way() {
    let scratch = Scratch::new("graphics");
    let settings = scratch.settings();
    let console = Fake::new();
    let running = Running::start(settings, console.clone());

    until("a frame", || !console.log().frames.is_empty());
    console.displaced.store(1, Ordering::SeqCst);

    let outcome = running.finish();
    assert!(matches!(outcome, Outcome::Displaced(Displaced::Graphics)), "{outcome:?}");
    assert_eq!(console.log().relinquished, vec![Displaced::Graphics]);
    assert_eq!(console.log().restored, 0);
}

#[test]
fn a_console_that_changes_size_is_redrawn_in_full_without_restarting_the_animation() {
    let scratch = Scratch::new("resize");
    let settings = scratch.settings();
    let console = Fake::new();
    let running = Running::start(settings, console.clone());

    // Let the animation get past its first frame, so that "the frame number did
    // not go back to zero" is a claim with something behind it.
    until("the animation to be under way", || console.log().frames.len() > 3);
    let before: u32 = running.status("frame").parse().expect("a frame number");
    assert!(before > 0, "the animation never advanced, so this test proves nothing");

    // This is dummycon being replaced by fbcon a second into a real boot: the
    // console is a different size and everything written before it is gone.
    console.resize(100, 30);

    let writes = console.log().frames.len();
    until("frames at the new size", || console.log().frames.len() > writes + 4);
    let after: u32 = running.status("frame").parse().expect("a frame number");
    assert!(
        after >= before,
        "the animation restarted on a resize: frame went from {before} to {after}"
    );

    running.ask("quit");
    running.finish();
}

/// `raba_core::format::limits::MAX_COLUMNS` is 1024, and a console larger than
/// a grid may be is the condition [`Outcome::CannotDraw`] documents first.
#[test]
fn a_console_with_no_usable_size_stops_with_a_reason_and_gives_the_terminal_back() {
    let scratch = Scratch::new("impossible-size");
    let settings = scratch.settings();
    let console = Fake::new();
    console.resize(2000, 2000);

    // Run it here rather than on a thread: this one ends before a client could
    // connect, and waiting for a socket that is already closed is not a test.
    let outcome = run_on(&settings, console.clone());

    match outcome {
        Outcome::CannotDraw(why) => assert!(
            why.contains("cannot be laid out"),
            "the reason should say what could not be done: {why}"
        ),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        console.log().restored,
        1,
        "a splash that stops drawing still gives the console back"
    );
}

#[test]
fn a_console_that_becomes_an_impossible_size_stops_rather_than_holding_a_frozen_screen() {
    let scratch = Scratch::new("impossible-resize");
    let settings = scratch.settings();
    let console = Fake::new();
    let running = Running::start(settings, console.clone());

    until("a frame", || !console.log().frames.is_empty());
    console.resize(2000, 2000);

    let outcome = running.finish();
    match outcome {
        Outcome::CannotDraw(why) => assert!(
            why.contains("which is not a size art can be laid out at"),
            "the reason should name the size: {why}"
        ),
        other => panic!("{other:?}"),
    }
    assert_eq!(console.log().restored, 1);
}

#[test]
fn an_animation_that_runs_out_of_frames_and_does_not_loop_stops_instead_of_going_silent() {
    let scratch = Scratch::new("ran-out");
    let mut movie = frames_of(&['A']);
    movie.looping = false;
    let settings = scratch.settings_for(&movie);
    let console = Fake::new();

    let outcome = run_on(&settings, console.clone());

    match outcome {
        Outcome::CannotDraw(why) => {
            assert_eq!(why, "the animation ran out of frames and does not loop")
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(console.log().restored, 1, "the console came back rather than being held");
}

// --- the state file ---------------------------------------------------------

#[test]
fn the_state_file_names_the_terminal_and_frame_while_drawing_and_is_gone_afterwards() {
    let scratch = Scratch::new("state");
    let settings = scratch.settings();
    let console = Fake::new();
    let running = Running::start(settings.clone(), console.clone());

    until("a state file", || settings.state.exists());
    let text = std::fs::read_to_string(&settings.state).expect("reading the state");
    assert!(text.contains("terminal=7"), "{text}");
    assert!(text.contains("previous=1"), "{text}");

    running.ask("quit");
    running.finish();
    assert!(
        !settings.state.exists(),
        "a state file outliving its instance is a later boot resuming an animation that ended"
    );
}

#[test]
fn a_state_file_that_cannot_be_written_is_complained_about_once_and_does_not_stop_the_splash() {
    let scratch = Scratch::new("state-unwritable");
    let mut settings = scratch.settings();
    // A directory where the state file should be: every save fails, on every
    // frame, for the whole run.
    settings.state = scratch.path("state-is-a-directory");
    std::fs::create_dir_all(&settings.state).expect("a directory in the way");
    let console = Fake::new();
    let running = Running::start(settings, console.clone());

    until("several frames", || console.log().frames.len() > 5);
    assert_eq!(running.status("phase"), "boot", "a failed save stopped the splash");

    running.ask("quit");
    assert!(matches!(running.finish(), Outcome::Finished));
}

#[test]
fn a_state_file_from_another_boot_is_ignored_and_the_splash_starts_from_the_top() {
    let scratch = Scratch::new("stale-state");
    let settings = scratch.settings();
    std::fs::write(&settings.state, "raba-state 1\nterminal 5\n").expect("a truncated state");
    let console = Fake::new();
    let running = Running::start(settings, console.clone());

    until("a frame", || !console.log().frames.is_empty());
    assert_eq!(
        console.log().asked,
        vec![Asked::Acquire],
        "a state file that does not parse must not be adopted"
    );

    running.ask("quit");
    running.finish();
}

// --- being killed -----------------------------------------------------------

#[test]
fn a_termination_signal_leaves_the_screen_exactly_as_it_is_for_whoever_comes_next() {
    use std::os::unix::thread::JoinHandleExt as _;

    let scratch = Scratch::new("terminate");
    let settings = scratch.settings();
    let console = Fake::new();
    let mut running = Running::start(settings.clone(), console.clone());

    until("a frame", || !console.log().frames.is_empty());

    // `pthread_kill` and not `kill`: the loop thread is the one that blocked
    // these signals, and a process-directed SIGTERM would be delivered to some
    // other thread of the test binary — which has not blocked it, and would die.
    let thread = running.thread.take().expect("the loop is running");
    // SAFETY: `as_pthread_t` is the id of a thread that is still running — the
    // handle is held right here — and `SIGTERM` is one of the three signals
    // `Signals::arm` blocked on it, so it is queued rather than delivered.
    let sent = unsafe { libc::pthread_kill(thread.as_pthread_t(), libc::SIGTERM) };
    assert_eq!(sent, 0, "sending SIGTERM to the loop thread");

    let outcome = running
        .outcome
        .recv_timeout(Duration::from_secs(10))
        .expect("the loop ended within ten seconds");
    thread.join().expect("the loop thread did not panic");

    assert!(matches!(outcome, Outcome::Finished), "{outcome:?}");
    // This is `switch_root`, not the end of the animation. The art stays on the
    // screen, the display stays where it is, and the successor adopts both.
    assert_eq!(console.log().forgotten, 1, "the terminal was not left for the successor");
    assert_eq!(console.log().restored, 0, "the console was handed back mid-animation");
    assert!(settings.state.exists(), "nothing was left for the successor to resume from");

    let text = std::fs::read_to_string(&settings.state).expect("reading the state");
    assert!(text.contains("terminal=7"), "{text}");
}

#[test]
fn a_run_that_never_drew_still_leaves_no_state_behind() {
    let scratch = Scratch::new("never-drew");
    let mut settings = scratch.settings();
    settings.show_delay = Duration::from_secs(600);
    let console = Fake::new();
    let running = Running::start(settings.clone(), console.clone());

    assert_eq!(running.ask("quit"), "+OK\n");
    assert!(matches!(running.finish(), Outcome::Finished));

    assert!(console.log().asked.is_empty(), "a splash nobody saw took a terminal anyway");
    assert!(!settings.state.exists());
}

/// Not a behaviour of the loop: a check that the double is not quietly lying.
///
/// Every assertion above about `restored` or `relinquished` is worthless if the
/// fake records them in the wrong place, and this is cheaper than finding that
/// out from a test that passes for the wrong reason.
#[test]
fn the_double_records_what_is_done_to_it() {
    let mut console = Fake::new();
    let mut screen = console.acquire().expect("a screen");

    assert_eq!(screen.size(), Size { columns: 80, lines: 25 });
    assert_eq!(Screen::number(&screen), 7);
    assert_eq!(Screen::previous(&screen), 1);
    assert_eq!(Screen::displaced(&screen), None);

    screen.present(b"hello").expect("presenting");
    screen.restore().expect("restoring");
    screen.relinquish(Displaced::Graphics).expect("relinquishing");
    screen.forget();

    let log = console.log();
    assert_eq!(log.asked, vec![Asked::Acquire]);
    assert_eq!(log.frames, vec![b"hello".to_vec()]);
    assert_eq!(log.restored, 1);
    assert_eq!(log.relinquished, vec![Displaced::Graphics]);
    assert_eq!(log.forgotten, 1);
}

/// Why the splash is not drawing, as the line that reaches `dmesg`.
///
/// On a machine booted with `console=ttyS0` alone, `NoConsole` is the normal
/// case and not a fault — so the sentence has to be readable by somebody who
/// went looking for a splash that never appeared, and has to name the file or
/// the ioctl that answered. Every arm of this was unexercised.
#[test]
fn every_reason_there_is_no_terminal_says_which_one_and_why() {
    let cases: Vec<(Error, Vec<&str>)> = vec![
        (
            Error::NoConsole(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            vec!["/dev/tty0", "no virtual terminal"],
        ),
        (Error::ScreenIsTaken, vec!["graphics mode"]),
        (Error::NoFreeTerminal, vec!["every virtual terminal"]),
        (Error::Ioctl("VT_OPENQRY", rustix::io::Errno::NOTTY), vec!["VT_OPENQRY"]),
        (Error::Open(7, std::io::Error::from(std::io::ErrorKind::NotFound)), vec!["/dev/tty7"]),
    ];
    for (error, wanted) in cases {
        let said = error.to_string();
        for fragment in wanted {
            assert!(said.contains(fragment), "{error:?} says {said:?}, without {fragment:?}");
        }
        assert!(
            std::error::Error::source(&error).is_none() || !said.is_empty(),
            "{error:?} has no message"
        );
    }
}

/// The two ways the screen stops being ours, as the line that explains a splash
/// that ended early.
#[test]
fn both_ways_of_being_displaced_name_what_happened() {
    assert!(Displaced::Switched(4).to_string().contains("tty4"));
    assert!(Displaced::Graphics.to_string().contains("graphics mode"));
}

/// Every answer this program can give about a boot, as the line that goes in
/// the log.
///
/// The one thing anybody has to debug a splash that did not appear is this
/// sentence in `journalctl -b`. "not drawing" and "stopped drawing" are
/// different problems — the first never took a screen, the second took one and
/// let go — and a line that does not say which sends the reader to the wrong
/// half of the system.
#[test]
fn every_outcome_says_whether_it_ever_drew_and_why_it_stopped() {
    let cases: Vec<(Outcome, Vec<&str>)> = vec![
        (Outcome::Finished, vec!["finished"]),
        (Outcome::Yielded(conflict::Conflict::Plymouthd(42)), vec!["not drawing", "plymouth"]),
        (Outcome::NoTerminal(Error::NoFreeTerminal), vec!["not drawing", "every virtual terminal"]),
        (
            Outcome::NoTheme("/usr/share/raba/arch.raba: No such file".to_owned()),
            vec!["not drawing", "arch.raba"],
        ),
        (
            Outcome::CannotDraw("the console reports 0 columns".to_owned()),
            vec!["stopped drawing", "0 columns"],
        ),
        (Outcome::Displaced(Displaced::Switched(2)), vec!["stopped", "tty2"]),
    ];
    for (outcome, wanted) in cases {
        let said = outcome.to_string();
        for fragment in wanted {
            assert!(said.contains(fragment), "{outcome:?} says {said:?}, without {fragment:?}");
        }
    }
}

#[test]
fn a_theme_larger_than_a_raba_may_be_is_refused_without_being_read_into_memory() {
    // The guard is on the metadata and not on the bytes, which is the point:
    // this file is 17 MB and reading it to find that out is the thing being
    // avoided. Sparse, so the test costs no disk.
    let scratch = Scratch::new("huge");
    let huge = scratch.path("huge.raba");
    let file = std::fs::File::create(&huge).expect("creating");
    file.set_len(17 * 1024 * 1024).expect("a sparse file");
    drop(file);

    let why = raba::boot::read_movie(&huge).expect_err("a 17 MB theme is refused");
    assert!(why.contains("huge.raba"), "{why}");
    assert!(why.contains("larger than"), "{why}");
}

#[test]
fn sigusr1_starts_the_outro_the_same_way_the_quit_command_does() {
    use std::os::unix::thread::JoinHandleExt as _;

    // Documented in man 8 raba, for a boot script with no socket client: the
    // signal and `raba-ctl quit` must mean the same thing, or the documented
    // one is the one nobody tests.
    let scratch = Scratch::new("sigusr1");
    let mut settings = scratch.settings();
    let outro = scratch.path("outro.raba");
    std::fs::write(&outro, raba_core::encode(&frames_of(&['X', 'Y']))).expect("writing the outro");
    settings.outro = Some(outro);
    settings.max_duration = Duration::from_millis(150);
    let console = Fake::new();
    let mut running = Running::start(settings, console.clone());

    until("a boot frame", || !console.log().frames.is_empty());

    let thread = running.thread.take().expect("the loop is running");
    // SAFETY: as in the SIGTERM test above — a live thread id, and a signal
    // `Signals::arm` blocked on that thread, so it queues rather than kills.
    let sent = unsafe { libc::pthread_kill(thread.as_pthread_t(), libc::SIGUSR1) };
    assert_eq!(sent, 0, "sending SIGUSR1 to the loop thread");

    let outcome = running
        .outcome
        .recv_timeout(Duration::from_secs(10))
        .expect("the loop ended within ten seconds");
    thread.join().expect("the loop thread did not panic");

    assert!(matches!(outcome, Outcome::Finished), "{outcome:?}");
    let written = console.written();
    assert!(written.windows(1).any(|w| w == b"X"), "SIGUSR1 did not start the outro");
    // And unlike SIGTERM, this is the end of the animation and not a handoff:
    // the console goes back.
    assert_eq!(console.log().restored, 1, "the console was not handed back");
}

#[test]
fn an_outro_that_cannot_be_read_is_skipped_rather_than_taking_the_boot_down_with_it() {
    // The outro is decoration at the very end of a boot. A theme that is
    // missing stops the splash before it starts, because there is nothing to
    // draw; an *outro* that is missing costs an animation nobody will miss,
    // and refusing to finish over it would hold the screen.
    let scratch = Scratch::new("outro-missing");
    let mut settings = scratch.settings();
    settings.outro = Some(scratch.path("not-here.raba"));
    let console = Fake::new();
    let running = Running::start(settings, console.clone());

    until("a boot frame", || !console.log().frames.is_empty());
    assert_eq!(running.ask("quit"), "+OK\n");

    let outcome = running.finish();
    assert!(matches!(outcome, Outcome::Finished), "{outcome:?}");
    assert_eq!(console.log().restored, 1, "the console was not handed back");
}

#[test]
fn an_outro_that_is_not_a_raba_file_is_skipped_the_same_way() {
    let scratch = Scratch::new("outro-junk");
    let mut settings = scratch.settings();
    let outro = scratch.path("outro.raba");
    std::fs::write(&outro, b"this is not a theme").expect("writing junk");
    settings.outro = Some(outro);
    let console = Fake::new();
    let running = Running::start(settings, console.clone());

    until("a boot frame", || !console.log().frames.is_empty());
    running.ask("quit");

    let outcome = running.finish();
    assert!(matches!(outcome, Outcome::Finished), "{outcome:?}");
    assert_eq!(console.log().restored, 1);
}

#[test]
fn an_outro_that_would_outlast_its_ceiling_is_cut_off_by_it() {
    // `the_outro_plays_once_and_is_bounded_by_its_own_ceiling` above proves the
    // ordinary case, where a short looping outro finishes on its own. This is
    // the case the ceiling exists for: one frame, held for ten seconds. Without
    // `Outro.MaxDuration` the handoff would hold the screen for all of it, and
    // `raba-ctl quit --wait` — which a display manager's ExecStartPost blocks
    // on — would not return until it was over.
    let scratch = Scratch::new("outro-ceiling");
    let mut settings = scratch.settings();
    let outro = scratch.path("outro.raba");
    let mut slow = frames_of(&['X']);
    slow.looping = false;
    if let Some(frame) = slow.frames.first_mut() {
        frame.delay_ms = 10_000;
    }
    std::fs::write(&outro, raba_core::encode(&slow)).expect("writing the outro");
    settings.outro = Some(outro);
    settings.max_duration = Duration::from_millis(120);

    let console = Fake::new();
    let running = Running::start(settings, console.clone());
    until("a boot frame", || !console.log().frames.is_empty());

    let asked = std::time::Instant::now();
    assert_eq!(running.ask("quit"), "+OK\n");
    let outcome = running.finish();
    let took = asked.elapsed();

    assert!(matches!(outcome, Outcome::Finished), "{outcome:?}");
    assert!(
        took < Duration::from_secs(5),
        "the ten-second frame was waited out: quit took {took:?}"
    );
    assert!(
        console.written().windows(1).any(|w| w == b"X"),
        "the outro was cut off before it drew anything at all"
    );
    assert_eq!(console.log().restored, 1, "the console was not handed back");
}
