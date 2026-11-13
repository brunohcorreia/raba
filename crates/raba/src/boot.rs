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
use crate::signals::{Signal, Signals};
use crate::state::{self, State};
use crate::vt::Vt;
use raba_core::surface::{Size, Surface as _};
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
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Finished => write!(f, "finished"),
            Self::Yielded(why) => write!(f, "not drawing: {why}"),
            Self::NoTerminal(why) => write!(f, "not drawing: {why}"),
            Self::NoTheme(why) => write!(f, "not drawing: {why}"),
        }
    }
}

/// Read a theme, refusing one too large to be one before allocating for it.
fn read_movie(path: &std::path::Path) -> Result<Movie, String> {
    let shown = path.display();
    let metadata = std::fs::metadata(path).map_err(|e| format!("{shown}: {e}"))?;
    if metadata.len() > u64::from(raba_core::format::limits::MAX_FILE_SIZE) {
        return Err(format!("{shown}: larger than a .raba may be"));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{shown}: {e}"))?;
    raba_core::decode(&bytes).map_err(|e| format!("{shown}: {e}"))
}

/// Run until something says to stop.
pub fn run(settings: &Settings) -> Outcome {
    if let Some(conflict) = conflict::detect(&settings.places) {
        return Outcome::Yielded(conflict);
    }
    let boot = match read_movie(&settings.theme) {
        Ok(movie) => movie,
        Err(why) => return Outcome::NoTheme(why),
    };
    let frames = u32::try_from(boot.frames.len()).unwrap_or(u32::MAX);
    let resume = state::load(&settings.state, frames, state::now_ms()).unwrap_or(None);

    Session::new(settings, boot, resume).run()
}

/// Everything the loop needs, so that the loop itself reads as a list of
/// questions rather than as a pile of local variables.
struct Session<'a> {
    settings: &'a Settings,
    boot: Movie,
    /// Loaded on the way into the outro, not before: on most boots it is never
    /// needed, and reading it early is a file read in the window where a boot is
    /// slowest.
    outro: Option<Movie>,
    life: Lifecycle,
    vt: Option<Vt>,
    player: Option<Player>,
    signals: Option<Signals>,
    listener: Option<Listener>,
    /// Connections from `quit --wait`, held open so that the client sees EOF
    /// when this process stops existing. Dropping them is the reply.
    waiters: Vec<Connection>,
    /// A sanitised `message`, redrawn under the animation on every frame.
    message: Option<String>,
    resume: Option<State>,
    started: Instant,
    outro_started: Option<Instant>,
    next_frame: Instant,
}

impl<'a> Session<'a> {
    fn new(settings: &'a Settings, boot: Movie, resume: Option<State>) -> Session<'a> {
        // A resume has no handoff to play an outro for: the screen it would
        // animate away from is one the user has been looking at for a while.
        // `docs/coexistence.md` is explicit about it.
        let outro_wanted = settings.outro.is_some() && resume.is_none();
        Session {
            settings,
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
                self.advance(match signal {
                    Signal::Terminate => Event::Terminate,
                    Signal::Quit => Event::Quit,
                });
                continue;
            }
            self.serve();
            self.deadlines();
            if let Some(outcome) = self.draw() {
                return outcome;
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
            Phase::Boot => self.player = None,
            Phase::Outro => {
                self.outro_started = Some(Instant::now());
                self.player = None;
            }
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
                let width = self.vt.as_ref().map_or(Size::FALLBACK, Vt::size).columns;
                self.message = Some(raba_core::sanitise(&text, width));
                (Reply::Ok, None)
            }
            // `Progress.Source=ctl` is configuration, and configuration is not
            // read yet. Refused with the reason rather than accepted and
            // ignored, which would be a client that thinks it is working.
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
            ("terminal", self.vt.as_ref().map_or(0, Vt::number).to_string()),
            ("previous", self.vt.as_ref().map_or(0, Vt::previous).to_string()),
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
        let vt = self.vt.as_mut()?;
        if self.player.is_none() {
            let mut player = Player::new(movie, vt.size())?;
            // The resumed frame, used once and then forgotten: the next release
            // and reacquire is not a resume and starts from the top.
            if let Some(state) = self.resume.take() {
                player.seek(movie, state.frame);
            }
            self.player = Some(player);
        }
        let player = self.player.as_mut()?;
        // `0` loops: the boot animation repeats until something says otherwise,
        // and the outro is bounded by `MaxDuration` rather than by a count.
        match player.step(vt, movie, if self.outro_started.is_some() { 1 } else { 0 }) {
            Some(hold) => self.next_frame = Instant::now().checked_add(hold)?,
            None => {
                self.advance(Event::OutroEnded);
                return None;
            }
        }
        self.show_message();
        self.save();
        None
    }

    /// Adopt the terminal from the state file, or take a fresh one.
    fn take_terminal(&mut self) -> Result<(), crate::vt::Error> {
        let vt = match self.resume.as_ref() {
            // Nothing is cleared on this path, which is the point: the screen is
            // still showing the frame the dead instance left, so continuing from
            // the next one is a pause rather than a flash of black.
            Some(state) => Vt::adopt(state.terminal, state.previous)?,
            None => {
                let mut vt = Vt::acquire()?;
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
        let _ = state::save(&self.settings.state, &state);
    }

    /// Everything that has to be true after RABA stops.
    fn finish(&mut self) {
        self.release();
        // Removed rather than left: a state file outliving the instance that
        // wrote it is a later instance resuming an animation that already
        // finished. The freshness check would catch it; not writing a trap is
        // better than catching one.
        state::discard(&self.settings.state);
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
