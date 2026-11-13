//! When RABA draws, and when it stops.
//!
//! Pure. No clock, no terminal, no socket — the caller turns elapsed time and
//! socket commands into [`Event`]s and this decides what they mean. That split is
//! deliberate: the interesting failures in a boot splash's lifecycle are decisions
//! made in the wrong state, and a decision table that needs a virtual machine to
//! exercise is a decision table nobody exercises.
//!
//! # The states
//!
//! Five, and each one is a thing the user can see:
//!
//! | phase | on screen |
//! |---|---|
//! | [`Phase::Waiting`] | whatever was there; `Boot.ShowDelay` has not elapsed |
//! | [`Phase::Boot`] | the boot animation, looping |
//! | [`Phase::Outro`] | the outro, once, bounded by `Outro.MaxDuration` |
//! | [`Phase::Deactivated`] | the console, because a client asked for it back |
//! | [`Phase::Revealed`] | the console, because `Boot.Timeout` elapsed |
//!
//! Plus [`Phase::Done`], which is not a thing the user sees because the process is
//! gone. `Deactivated` and `Revealed` behave identically and are separate anyway:
//! `status` reports which, and "the splash let go" and "the boot is taking too
//! long" are the two most different things that can be happening on a machine
//! showing the same screen.
//!
//! # The two transitions that are the point of the file
//!
//! **`quit` after `deactivate` must not play the outro.** That sequence is the
//! documented handoff — `raba-ctl deactivate`, display manager starts and takes
//! the terminal, `raba-ctl quit --wait` — and an outro there would re-acquire a
//! terminal that now belongs to somebody else and draw over them. The splash would
//! be fighting the thing it just got out of the way of. See `docs/integration.md`.
//!
//! **`Boot.Timeout` must not fire during the outro.** The outro has its own
//! ceiling, `Outro.MaxDuration`, measured in a second or so. If the boot timeout
//! could land in that window it would turn a successful handoff into a "the boot
//! has stalled" reveal, at the exact moment everything was going right.

/// What RABA is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Before `Boot.ShowDelay`. Nothing has been acquired and nothing drawn, so a
    /// machine that boots faster than the delay shows no splash at all — which is
    /// the entire purpose of the setting.
    Waiting,
    /// The boot animation, looping until something says otherwise.
    Boot,
    /// The outro, playing once on the way to a display manager.
    Outro,
    /// Not drawing, terminal released, still listening. A client asked.
    Deactivated,
    /// Not drawing, terminal released, still listening. `Boot.Timeout` elapsed
    /// and the console was given back so the user can see why the boot stalled.
    Revealed,
    /// Over. The caller restores and exits.
    Done,
}

impl Phase {
    /// Whether frames should be going to a terminal in this phase.
    #[must_use]
    pub const fn drawing(self) -> bool {
        matches!(self, Phase::Boot | Phase::Outro)
    }

    /// Whether RABA should still be holding a terminal.
    ///
    /// The same answer as [`drawing`](Self::drawing) today, and a separate
    /// question on purpose: "has nothing to draw" and "has given the console
    /// back" come apart the moment anything buffers a frame.
    #[must_use]
    pub const fn holds_terminal(self) -> bool {
        self.drawing()
    }

    #[must_use]
    pub const fn is_done(self) -> bool {
        matches!(self, Phase::Done)
    }

    /// The word `status` reports. Part of the wire protocol — see
    /// `docs/protocol.md` — so these strings are API and not labels.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Phase::Waiting => "waiting",
            Phase::Boot => "boot",
            Phase::Outro => "outro",
            Phase::Deactivated => "deactivated",
            Phase::Revealed => "revealed",
            Phase::Done => "done",
        }
    }
}

/// Something that happened. Time and sockets become these and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// `Boot.ShowDelay` has elapsed; it is time to draw.
    ShowDelayElapsed,
    /// `Boot.Timeout` has elapsed with no `quit`.
    TimeoutElapsed,
    /// The outro ran out of frames, or hit `Outro.MaxDuration`.
    OutroEnded,
    /// `raba-ctl quit`, or `SIGUSR1`.
    Quit,
    /// `raba-ctl deactivate`.
    Deactivate,
    /// `raba-ctl reactivate`.
    Reactivate,
    /// `SIGTERM` or `SIGINT`. Not negotiable and not a phase change to argue
    /// with: whatever is happening, stop.
    Terminate,
}

/// The phase machine.
#[derive(Debug, Clone, Copy)]
pub struct Lifecycle {
    phase: Phase,
    outro: bool,
}

impl Lifecycle {
    /// Start, in [`Phase::Waiting`].
    ///
    /// `outro` is `Outro.Enable`, and is also how the resume path says no: an
    /// instance that adopted a running animation has no handoff to play an outro
    /// for, and `docs/coexistence.md` is explicit that it should not.
    #[must_use]
    pub const fn new(outro: bool) -> Self {
        Lifecycle { phase: Phase::Waiting, outro }
    }

    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// Feed in an event. Returns the new phase, or `None` if nothing changed.
    ///
    /// An event that means nothing in the current phase is ignored rather than
    /// rejected. There is no error path here on purpose: every caller of this is
    /// something that must not stop a boot, and "`reactivate` while already
    /// drawing" is a client being redundant, not a client being wrong.
    pub fn on(&mut self, event: Event) -> Option<Phase> {
        let next = self.next(event)?;
        if next == self.phase {
            return None;
        }
        self.phase = next;
        Some(next)
    }

    fn next(&self, event: Event) -> Option<Phase> {
        // Absorbing, and first, so nothing below has to remember to check.
        if self.phase.is_done() {
            return None;
        }
        // Unconditional, for the same reason: a signal is not a request.
        if event == Event::Terminate {
            return Some(Phase::Done);
        }
        match (self.phase, event) {
            // Nothing drawn yet. `quit` here is not "finish gracefully", it is
            // "you were never on screen" — and an outro for an animation nobody
            // saw is pure boot delay.
            (Phase::Waiting, Event::ShowDelayElapsed) => Some(Phase::Boot),
            (Phase::Waiting, Event::Quit) => Some(Phase::Done),
            (Phase::Waiting, Event::Deactivate) => Some(Phase::Deactivated),
            (Phase::Waiting, Event::TimeoutElapsed) => Some(Phase::Revealed),

            (Phase::Boot, Event::Quit) => Some(if self.outro { Phase::Outro } else { Phase::Done }),
            (Phase::Boot, Event::TimeoutElapsed) => Some(Phase::Revealed),
            (Phase::Boot, Event::Deactivate) => Some(Phase::Deactivated),

            (Phase::Outro, Event::OutroEnded) => Some(Phase::Done),
            // A client that wants the terminal back mid-outro gets it. We were
            // leaving anyway; finishing the animation is not worth making them
            // wait for a terminal they have asked for twice.
            (Phase::Outro, Event::Deactivate) => Some(Phase::Done),

            // Released, by either route. `quit` here must not draw: the terminal
            // is somebody else's now, and taking it back to play an outro is the
            // splash fighting the display manager it just made way for.
            (Phase::Deactivated | Phase::Revealed, Event::Quit) => Some(Phase::Done),
            (Phase::Deactivated | Phase::Revealed, Event::Reactivate) => Some(Phase::Boot),

            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
