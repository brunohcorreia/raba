//! The daemon: one loop, three sources of events, and a terminal.
//!
//! Everything M4 built is pure or nearly so — the phase machine decides, the
//! wire module parses, the state module validates. This is the only part that
//! has a clock, a socket and a console at once, and it is kept small for that
//! reason: it does no deciding of its own.
//!
//! # One loop, polled
//!
//! ```text
//! every few milliseconds:
//!     a signal?        -> an Event
//!     a command?       -> an Event and a reply
//!     a deadline?      -> an Event
//!     feed the Events to the Lifecycle
//!     if the phase changed, acquire or release the terminal
//!     if a frame is due and we are drawing, draw it
//!     write /run/raba.state
//! ```
//!
//! No threads and no async runtime. Signals are blocked and polled with
//! `sigtimedwait`, the listener is non-blocking, and the only thing that sleeps
//! is the gap between frames — which is sliced, so a `quit` is acted on within
//! [`SLICE`] rather than at the end of a frame that may be holding for a second.
//! A control socket is exactly the shape of problem people reach for an async
//! runtime to solve, and it turns out to be a `for` loop.
//!
//! # Resume, and why there is no "was I killed" branch
//!
//! A fresh `/run/raba.state` means the terminal in it is adopted and the
//! animation continues from the frame in it. No state file means a terminal is
//! acquired and frame 0 is drawn. Those are the same code path with a different
//! starting point, which is what makes being killed a non-event rather than a
//! case to handle. See `docs/early-boot.md`.

use crate::conflict::{self, Conflict};
use crate::control::wire::{Command, Rejection, Reply};
use crate::control::{Connection, Listener};
use crate::lifecycle::{Event, Lifecycle, Phase};
use crate::report::{note, report};
use crate::signals::{Signal, Signals};
use crate::state::{self, State};
use crate::vt::{Console, Displaced, Kernel, Screen};
use raba_core::surface::{Size, Surface};
use raba_core::Movie;
use raba_term::Player;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How long the loop sleeps between polls.
///
/// The delay between acting on a `quit` and starting to act on it. Short enough
/// that nobody notices, long enough that the loop is not a spin: at 10ms a frame
/// held for a second costs a hundred wakeups, which on a machine that is busy
/// booting is nothing.
pub const SLICE: Duration = Duration::from_millis(10);

/// How late a frame may be before the splash decides it is broken. See
/// [`Session::stalled`].
const STALL_GRACE: Duration = Duration::from_secs(5);

/// What `raba` was asked to do. The names are `raba.conf`(5)'s.
#[derive(Debug, Clone)]
pub struct Settings {
    pub theme: PathBuf,
    /// `Outro.Theme`. `None` means `Outro.Enable=false`.
    pub outro: Option<PathBuf>,
    /// `Boot.ShowDelay`.
    pub show_delay: Duration,
    /// `Boot.Timeout`. `None` is `0`, which disables it and is not recommended.
    pub timeout: Option<Duration>,
    /// `Outro.MaxDuration`. A hard ceiling: without one, a long outro becomes
    /// the boot delay this project exists to avoid.
    pub max_duration: Duration,
    /// `Boot.Tty`. `None` is `auto`: a free terminal from `VT_OPENQRY`.
    pub tty: Option<u16>,
    pub socket: PathBuf,
    pub state: PathBuf,
    pub places: conflict::Where,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: PathBuf::new(),
            outro: None,
            show_delay: Duration::ZERO,
            timeout: Some(Duration::from_secs(30)),
            max_duration: Duration::from_millis(1500),
            tty: None,
            socket: PathBuf::from(crate::control::SOCKET),
            state: PathBuf::from(state::PATH),
            places: conflict::Where::default(),
        }
    }
}

/// How it ended. Every one of these is an exit code of 0; see `man 8 raba`.
#[derive(Debug)]
pub enum Outcome {
    /// Drew, and gave the terminal back.
    Finished,
    /// Something else owns the screen.
    Yielded(Conflict),
    /// There is no virtual terminal to draw on. Normal on a machine booted with
    /// `console=ttyS0` alone.
    NoTerminal(crate::vt::Error),
    /// The theme could not be read or decoded.
    NoTheme(String),
    /// The art and this console cannot be put together — a console far larger
    /// than any grid may be, a size the kernel reports as nonsense, a clock that
    /// will not add up.
    ///
    /// Its own outcome because the alternative, found on a Debian machine where
    /// the splash stopped after two frames and held the screen for the rest of
    /// the boot, is for `draw` to answer "nothing to report, carry on" and keep
    /// doing that forever: alive, holding a terminal, answering the socket, and
    /// never drawing again. Silence is the one thing a boot splash must not do
    /// when it has somebody's screen.
    CannotDraw(String),
    /// Something else took the screen while we were drawing, and we left it
    /// alone. See [`Vt::displaced`](crate::vt::Vt::displaced).
    Displaced(Displaced),
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Finished => write!(f, "finished"),
            Self::Yielded(why) => write!(f, "not drawing: {why}"),
            Self::NoTerminal(why) => write!(f, "not drawing: {why}"),
            Self::NoTheme(why) => write!(f, "not drawing: {why}"),
            Self::CannotDraw(why) => write!(f, "stopped drawing: {why}"),
            Self::Displaced(why) => write!(f, "stopped: {why}"),
        }
    }
}

/// Read a theme, refusing one too large to be one before allocating for it.
///
/// # Errors
///
/// Anything that means there is no animation: absent, too large, not a `.raba`.
/// Public because `raba --check` answers the same question the daemon does and
/// has to answer it the same way.
pub fn read_movie(path: &std::path::Path) -> Result<Movie, String> {
    let shown = path.display();
    let metadata = std::fs::metadata(path).map_err(|e| format!("{shown}: {e}"))?;
    if metadata.len() > u64::from(raba_core::format::limits::MAX_FILE_SIZE) {
        return Err(format!("{shown}: larger than a .raba may be"));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{shown}: {e}"))?;
    raba_core::decode(&bytes).map_err(|e| format!("{shown}: {e}"))
}

/// Run until something says to stop, on this machine's real consoles.
pub fn run(settings: &Settings) -> Outcome {
    run_on(settings, Kernel)
}

/// Run until something says to stop, drawing on whatever `console` hands out.
///
/// The entry point the tests use. `run` is this with [`Kernel`], and there is no
/// other implementation of [`Console`] in the shipped binary — see
/// `crates/raba/src/vt/screen.rs` for why the seam is here and not elsewhere.
pub fn run_on<C: Console>(settings: &Settings, console: C) -> Outcome {
    if let Some(conflict) = conflict::detect(&settings.places) {
        return Outcome::Yielded(conflict);
    }
    let boot = match read_movie(&settings.theme) {
        Ok(movie) => movie,
        Err(why) => return Outcome::NoTheme(why),
    };
    let frames = u32::try_from(boot.frames.len()).unwrap_or(u32::MAX);
    let resume = match state::load(&settings.state, frames, state::now_ms()) {
        Ok(resume) => resume,
        Err(why) => {
            // `state::load` documents this as reported, and for a long time it
            // was not: `unwrap_or(None)` threw away the one sentence that
            // explains why a splash started over at frame 0 instead of
            // continuing where the last instance left off. The ordinary boot
            // has no state file at all and says nothing; this runs only when
            // there was one and it could not be used, which is never ordinary.
            // `note`, not `report`: the splash draws either way, so this is
            // news and not a fault, and the kernel log is shared with everybody
            // on the machine who is using it to debug something else.
            note(&format!("{}: ignoring the resume state: {why}", settings.state.display()));
            None
        }
    };

    Session::new(settings, console, boot, resume).run()
}

/// Everything the loop needs, so that the loop itself reads as a list of
/// questions rather than as a pile of local variables.
struct Session<'a, C: Console> {
    settings: &'a Settings,
    /// Where a terminal comes from. Held rather than called statically so that
    /// a test can hand the loop a console that fails, or one that reports a
    /// different size on every frame.
    console: C,
    boot: Movie,
    /// Loaded on the way into the outro, not before: on most boots it is never
    /// needed, and reading it early is a file read in the window where a boot is
    /// slowest.
    outro: Option<Movie>,
    life: Lifecycle,
    vt: Option<C::Screen>,
    player: Option<Player>,
    signals: Option<Signals>,
    listener: Option<Listener>,
    /// Connections from `quit --wait`, held open so that the client sees EOF
    /// when this process stops existing. Dropping them is the reply.
    waiters: Vec<Connection>,
    /// A sanitised `message`, redrawn under the animation on every frame.
    message: Option<String>,
    resume: Option<State>,
    /// A termination signal arrived. On a systemd machine that means
    /// `switch_root`, which is an interruption and not an ending: see
    /// [`Session::finish`].
    interrupted: bool,
    /// Whether the resume file has already been complained about once.
    ///
    /// Saving it is best effort and happens on every frame, so a path that
    /// cannot be written is a failure sixty times a second and must never be
    /// said sixty times. It is said once: something is at a path RABA owns in
    /// `/run`, and the person looking at a splash that does not survive
    /// `switch_root` deserves the sentence that explains it. See
    /// `docs/security.md` findings 3 and 4 for why that path is interesting.
    save_complained: bool,
    /// The console size the current player was built for. A console that
    /// changes size under us is a console that was replaced, and everything
    /// drawn before that is gone.
    screen: Option<Size>,
    started: Instant,
    outro_started: Option<Instant>,
    next_frame: Instant,
}

impl<'a, C: Console> Session<'a, C> {
    fn new(
        settings: &'a Settings,
        console: C,
        boot: Movie,
        resume: Option<State>,
    ) -> Session<'a, C> {
        // A resume has no handoff to play an outro for: the screen it would
        // animate away from is one the user has been looking at for a while.
        // `docs/coexistence.md` is explicit about it.
        let outro_wanted = settings.outro.is_some() && resume.is_none();
        Session {
            settings,
            console,
            boot,
            outro: None,
            life: Lifecycle::new(outro_wanted),
            vt: None,
            player: None,
            signals: Signals::arm().ok(),
            listener: Listener::bind(&settings.socket).ok(),
            waiters: Vec::new(),
            message: None,
            resume,
            interrupted: false,
            save_complained: false,
            screen: None,
            started: Instant::now(),
            outro_started: None,
            next_frame: Instant::now(),
        }
    }

    fn run(mut self) -> Outcome {
        // A resumed instance was already on screen a moment ago, so the show
        // delay has long since passed and waiting again would blank nothing and
        // help nobody.
        if self.resume.is_some() {
            self.advance(Event::ShowDelayElapsed);
        }
        while !self.life.phase().is_done() {
            if let Some(signal) = self.signals.as_mut().and_then(Signals::poll) {
                let event = match signal {
                    Signal::Terminate => {
                        self.interrupted = true;
                        Event::Terminate
                    }
                    Signal::Quit => Event::Quit,
                };
                self.advance(event);
                continue;
            }
            self.serve();
            self.deadlines();
            if let Some(why) = self.displaced() {
                return self.leave(why);
            }
            if let Some(outcome) = self.draw() {
                return outcome;
            }
            if self.stalled() {
                return self.give_up("a frame was due and none was drawn");
            }
            std::thread::sleep(SLICE);
        }
        self.finish();
        Outcome::Finished
    }

    /// Feed one event to the phase machine and act on what it decides.
    fn advance(&mut self, event: Event) {
        let Some(phase) = self.life.on(event) else {
            return;
        };
        match phase {
            // Acquiring is deferred to `draw`, which is the only place that
            // knows whether it succeeded and the only place that needs it.
            Phase::Boot => {
                self.player = None;
                // The frame clock starts when the drawing does. Without this a
                // long `Boot.ShowDelay` leaves `next_frame` seconds in the past
                // at the moment the splash begins, which is indistinguishable
                // from a splash that has stopped drawing — see `stalled`.
                self.next_frame = Instant::now();
            }
            Phase::Outro => {
                self.outro_started = Some(Instant::now());
                self.player = None;
                // Read here and not at startup, which is what the field's
                // documentation has always said and what nothing did: `outro`
                // stayed `None` for the life of the process, `draw` fell
                // through to `unwrap_or(&self.boot)`, and a configured
                // `Outro.Theme` silently played the boot animation once more.
                // Nothing failed, nothing was logged, and no test looked —
                // this phase could not be entered outside a virtual machine
                // until `vt::Console` existed.
                if let Some(path) = self.settings.outro.as_ref() {
                    match read_movie(path) {
                        Ok(movie) => self.outro = Some(movie),
                        // `note`, not `report`: the boot animation plays in its
                        // place, the splash still ends properly, and the only
                        // loss is the handoff looking less deliberate.
                        Err(why) => note(&format!("ignoring the outro: {why}")),
                    }
                }
            }
            // A termination signal is `switch_root`, and there the terminal
            // must be left exactly as it stands for the instance that comes
            // next: `docs/early-boot.md` says the screen holds the last frame
            // "because nothing cleared it", and [`Session::finish`] is the one
            // place that chooses between the two endings.
            //
            // Releasing here made that choice first and silently. The display
            // went back, the console flashed, the terminal was deallocated —
            // and then `finish` found `self.vt` already gone, so it saved no
            // state and had nothing to hand over. The whole resume path across
            // `switch_root` was dead, and every test that covered it killed the
            // process with `SIGKILL`, which skips this line.
            Phase::Done if self.interrupted => {}
            // The timeout case and the deactivate case are the same action and
            // different reasons: give the console back. For a timeout that is
            // the whole feature — `raba` does not exit, it *reveals*, so a
            // stalled boot shows the messages that explain the stall rather
            // than a blank screen. See `man 5 raba.conf` on `Boot.Timeout`.
            Phase::Deactivated | Phase::Revealed | Phase::Done => self.release(),
            Phase::Waiting => {}
        }
    }

    /// Give the terminal back, keeping everything else.
    /// Has the screen stopped being ours? Asked once per slice, which is two
    /// ioctls every ten milliseconds on a machine that is otherwise waiting.
    fn displaced(&self) -> Option<Displaced> {
        self.vt.as_ref().and_then(Screen::displaced)
    }

    /// Stop, having been displaced: no outro, no switching back.
    ///
    /// An outro would be drawn on a terminal nobody is looking at, and the
    /// switch back that ends a normal run would take the screen away from
    /// whatever just took it. Everything else a clean exit does still happens —
    /// the state file goes, the waiters get their EOF — because a `quit --wait`
    /// blocked on this process is owed an answer either way.
    fn leave(&mut self, why: Displaced) -> Outcome {
        self.player = None;
        if let Some(mut vt) = self.vt.take() {
            let _ = vt.relinquish(why);
        }
        state::discard(&self.settings.state);
        self.listener = None;
        if let Some(signals) = self.signals.as_mut() {
            signals.disarm();
        }
        self.waiters.clear();
        Outcome::Displaced(why)
    }

    fn release(&mut self) {
        self.player = None;
        if let Some(mut vt) = self.vt.take() {
            let _ = vt.restore();
        }
    }

    /// Answer whatever is on the socket. At most one command per pass, because
    /// a client that could queue a hundred could stop the animation.
    fn serve(&mut self) {
        let Some(listener) = self.listener.as_ref() else {
            return;
        };
        let Some(mut connection) = listener.accept() else {
            return;
        };
        let Ok(outcome) = connection.request() else {
            return;
        };
        let answer = self.answer(outcome);
        let _ = connection.reply(&answer.reply);
        // Held rather than dropped: the client is waiting for EOF, and EOF is
        // what the kernel sends when this process stops existing.
        if answer.wait {
            self.waiters.push(connection);
        }
        if let Some(event) = answer.event {
            self.advance(event);
        }
    }

    fn answer(&mut self, outcome: Result<Command, Rejection>) -> Answer {
        let command = match outcome {
            Ok(command) => command,
            Err(why) => return Answer::just(Reply::Failed(why)),
        };
        let (reply, event) = match command {
            Command::Ping => (Reply::Ok, None),
            Command::Status => (Reply::Report(self.status()), None),
            Command::Quit { wait } => {
                return Answer { reply: Reply::Ok, event: Some(Event::Quit), wait }
            }
            Command::Deactivate => (Reply::Ok, Some(Event::Deactivate)),
            Command::Reactivate => (Reply::Ok, Some(Event::Reactivate)),
            Command::Message(text) => {
                if !self.life.phase().drawing() {
                    return Answer::just(Reply::Failed(Rejection::NotDrawing));
                }
                // The only path from outside this process to a terminal, and the
                // only place that is true. See `raba_core::message`.
                let width = self.vt.as_ref().map_or(Size::FALLBACK, Surface::size).columns;
                self.message = Some(raba_core::sanitise(&text, width));
                (Reply::Ok, None)
            }
            // The configuration is read now, and `Progress.Enable` is on the
            // list of keys it parses and does not act on: there is no progress
            // indicator to set. Refused with the reason rather than accepted
            // and ignored, which would be a client that thinks it is working.
            // See `config::NOT_HONOURED`.
            Command::Progress { .. } => (Reply::Failed(Rejection::NotConfigured), None),
        };
        Answer { reply, event, wait: false }
    }

    fn status(&self) -> Vec<(&'static str, String)> {
        let uptime = self.started.elapsed().as_millis();
        vec![
            ("protocol", crate::control::VERSION.to_string()),
            ("version", env!("CARGO_PKG_VERSION").to_owned()),
            ("phase", self.life.phase().name().to_owned()),
            ("terminal", self.vt.as_ref().map_or(0, Screen::number).to_string()),
            ("previous", self.vt.as_ref().map_or(0, Screen::previous).to_string()),
            ("theme", self.settings.theme.display().to_string()),
            ("frame", self.player.as_ref().map_or(0, Player::frame).to_string()),
            ("uptime_ms", uptime.to_string()),
        ]
    }

    /// The events that come from a clock rather than from anybody.
    fn deadlines(&mut self) {
        let elapsed = self.started.elapsed();
        if self.life.phase() == Phase::Waiting && elapsed >= self.settings.show_delay {
            self.advance(Event::ShowDelayElapsed);
        }
        if let Some(timeout) = self.settings.timeout {
            if elapsed >= timeout {
                self.advance(Event::TimeoutElapsed);
            }
        }
        // The outro's own ceiling, which is why `Boot.Timeout` is not allowed to
        // fire in this phase: a one-second animation does not get judged by a
        // thirty-second rule.
        if let Some(started) = self.outro_started {
            if started.elapsed() >= self.settings.max_duration {
                self.advance(Event::OutroEnded);
            }
        }
    }

    /// Draw a frame, if one is due and there is somewhere to draw it.
    ///
    /// Returns an [`Outcome`] only when there is no terminal at all, which ends
    /// the run: there is nothing to wait for and nothing to come back to.
    fn draw(&mut self) -> Option<Outcome> {
        if !self.life.phase().drawing() || Instant::now() < self.next_frame {
            return None;
        }
        if self.vt.is_none() {
            match self.take_terminal() {
                Ok(()) => {}
                Err(error) => return Some(Outcome::NoTerminal(error)),
            }
        }
        let movie = match self.life.phase() {
            Phase::Outro => self.outro.as_ref().unwrap_or(&self.boot),
            _ => &self.boot,
        };
        let Some(vt) = self.vt.as_mut() else {
            return Some(
                self.give_up("the terminal went away between taking it and drawing on it"),
            );
        };

        // Has the console changed under us? Early in a boot it can: with no
        // graphics driver loaded yet the kernel binds `dummycon`, which answers
        // every ioctl, says 80x25 and shows nothing, and fbcon replaces it the
        // moment the driver arrives — with a blank screen at a different size.
        // Deltas written after that land on a picture that is not there.
        //
        // The size changing is the signal, and `Player::resize` is the repair:
        // it forgets what is on screen without touching where the animation has
        // got to, so the next frame is written out in full and nothing
        // restarts.
        let size = vt.size();
        if self.screen != Some(size) {
            self.screen = Some(size);
            if let Some(player) = self.player.as_mut() {
                if player.resize(movie, size).is_none() {
                    let why = format!(
                        "the console became {}x{}, which is not a size art can be laid out at",
                        size.columns, size.lines
                    );
                    return Some(self.give_up(&why));
                }
            }
        }

        if self.player.is_none() {
            let Some(mut player) = Player::new(movie, size) else {
                let why = format!(
                    "a {}x{} movie cannot be laid out on a {}x{} console",
                    movie.columns, movie.lines, size.columns, size.lines
                );
                return Some(self.give_up(&why));
            };
            // The resumed frame, used once and then forgotten: the next release
            // and reacquire is not a resume and starts from the top.
            if let Some(state) = self.resume.take() {
                player.seek(movie, state.frame);
            }
            self.player = Some(player);
        }
        let Some(player) = self.player.as_mut() else {
            return Some(self.give_up("there is no player, one line after making one"));
        };
        // `0` loops: the boot animation repeats until something says otherwise,
        // and the outro is bounded by `MaxDuration` rather than by a count.
        match player.step(vt, movie, if self.outro_started.is_some() { 1 } else { 0 }) {
            Some(hold) => {
                let Some(next) = Instant::now().checked_add(hold) else {
                    return Some(
                        self.give_up("the frame delay does not fit in this machine's clock"),
                    );
                };
                self.next_frame = next;
            }
            None => {
                // The movie has nothing more to play. In the outro that is the
                // end of the run and the phase machine knows what to do with
                // it. In the boot phase it means a movie that does not loop has
                // run out, and there is no transition for `OutroEnded` there —
                // so `advance` would do nothing, `next_frame` would stay in the
                // past, and this loop would spend the rest of the boot waking
                // up every ten milliseconds to be told there is nothing to draw,
                // holding a terminal the whole time.
                if self.life.phase() == Phase::Outro {
                    self.advance(Event::OutroEnded);
                    return None;
                }
                return Some(self.give_up("the animation ran out of frames and does not loop"));
            }
        }
        self.show_message();
        self.save();
        None
    }

    /// A frame was due and is very late.
    ///
    /// Measured against when the next frame was *due* rather than against a
    /// fixed interval, because a theme is allowed to hold one frame for a
    /// minute and that is not a stall. Five seconds past due is not a slow
    /// machine; it is this program broken.
    ///
    /// The guard exists because the alternative has happened: a wedged loop,
    /// alive, answering the control socket, holding a virtual terminal and
    /// drawing nothing for the rest of the boot. Whatever the next cause of
    /// that turns out to be, it ends here — with the screen handed back and a
    /// line in the log — instead of with somebody staring at a frozen picture.
    fn stalled(&self) -> bool {
        self.life.phase().drawing()
            && self
                .next_frame
                .checked_add(STALL_GRACE)
                .is_some_and(|deadline| Instant::now() > deadline)
    }

    /// Stop, and say why.
    ///
    /// Everything a clean ending does — the terminal goes back, the state file
    /// goes, a `quit --wait` gets its EOF — because the console is worth more to
    /// the person in front of it than a frozen picture is, and because a boot
    /// that stops being drawn on must not also be a boot that hangs.
    fn give_up(&mut self, why: &str) -> Outcome {
        self.finish();
        Outcome::CannotDraw(why.to_owned())
    }

    /// Adopt the terminal from the state file, or take a fresh one.
    fn take_terminal(&mut self) -> Result<(), crate::vt::Error> {
        let vt = match self.resume.as_ref() {
            // Nothing is cleared on this path, which is the point: the screen is
            // still showing the frame the dead instance left, so continuing from
            // the next one is a pause rather than a flash of black.
            Some(state) => self.console.adopt(state.terminal, state.previous)?,
            None => {
                let mut vt = match self.settings.tty {
                    // Asked for by name, and taken whatever is on it: that is
                    // what naming one means. `Vt::acquire` is the other case.
                    Some(number) => self.console.take(number)?,
                    None => self.console.acquire()?,
                };
                // Start from a known screen, and hide the cursor so it does not
                // blink in the middle of the art. `Vt::restore` shows it again
                // on every exit path it can reach.
                let _ = vt.present(b"\x1b[2J\x1b[H\x1b[?25l");
                vt
            }
        };
        self.vt = Some(vt);
        Ok(())
    }

    /// Redraw the message under the animation.
    ///
    /// Every frame, because the frame that was just drawn wrote over wherever it
    /// was. One line of a console is a few dozen bytes and this is not a path
    /// that runs when nobody has sent a message.
    fn show_message(&mut self) {
        let (Some(text), Some(vt)) = (self.message.as_ref(), self.vt.as_mut()) else {
            return;
        };
        let size = vt.size();
        let line = size.lines.max(1);
        let column = size
            .columns
            .saturating_sub(u16::try_from(text.chars().count()).unwrap_or(size.columns))
            / 2;
        let mut out = Vec::with_capacity(text.len().saturating_add(24));
        out.extend_from_slice(
            format!("\x1b[{line};{}H\x1b[0m", column.saturating_add(1)).as_bytes(),
        );
        out.extend_from_slice(text.as_bytes());
        out.extend_from_slice(format!("\x1b[{line};1H").as_bytes());
        let _ = vt.present(&out);
    }

    fn save(&mut self) {
        let (Some(vt), Some(player)) = (self.vt.as_ref(), self.player.as_ref()) else {
            return;
        };
        let state = State {
            terminal: vt.number(),
            previous: vt.previous(),
            frame: player.frame(),
            phase: self.life.phase(),
            theme: self.settings.theme.clone(),
            written_ms: state::now_ms(),
        };
        if let Err(error) = state::save(&self.settings.state, &state) {
            if !self.save_complained {
                self.save_complained = true;
                // No path of our own in front: `error` already names the file
                // that refused, which on a failure is as often the staging
                // name as the state file itself.
                report(&format!("could not write the resume state: {error}"));
            }
        }
    }

    /// Everything that has to be true after RABA stops.
    fn finish(&mut self) {
        // Two endings, and they are not the same thing.
        //
        // Interrupted: a termination signal, which on a systemd machine is
        // `switch_root` and nothing else. The animation is not over, this
        // process is. So the state is written down — terminal and frame, saved
        // before anything is released, because releasing is what destroys both
        // — and the terminal is left exactly as it stands, art and all, for
        // whoever starts next. Restoring it here would hand back a console,
        // flash a boot log, and make the successor take it again: three visible
        // events in the gap this whole design exists to close.
        //
        // Finished: the splash is over, somebody asked for the screen, and a
        // state file outliving the instance that wrote it is a later instance
        // resuming an animation that already ended. The freshness check would
        // catch it; not writing a trap is better than catching one.
        if self.interrupted {
            self.save();
            if let Some(mut vt) = self.vt.take() {
                vt.forget();
            }
        } else {
            self.release();
            state::discard(&self.settings.state);
        }
        self.listener = None;
        if let Some(signals) = self.signals.as_mut() {
            signals.disarm();
        }
        // Last: dropping these is the EOF that `quit --wait` is waiting for, and
        // it should mean "the terminal is back", not "the command was read".
        self.waiters.clear();
    }
}

/// What answering one command produced: something to send, possibly something
/// for the phase machine, and whether the client is waiting for EOF.
struct Answer {
    reply: Reply,
    event: Option<Event>,
    /// `quit --wait`. The connection is kept until the process ends; see
    /// [`Session::finish`].
    wait: bool,
}

impl Answer {
    const fn just(reply: Reply) -> Answer {
        Answer { reply, event: None, wait: false }
    }
}
