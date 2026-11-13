//! Playing a movie on a terminal.
//!
//! The same core that will drive a virtual terminal, driving stdout instead. If a
//! theme looks right here, the core is right and only the VT layer remains in doubt —
//! which is the whole reason `--preview` is a milestone and not a debug flag.

use raba_core::render::{best_fit, resamples, to_ansi_into};
use raba_core::surface::{Size, Surface};
use raba_core::{apply, Grid, Movie};
use std::io;
use std::thread::sleep;
use std::time::Duration;

/// How long to hold a frame whose `delay_ms` is 0.
///
/// The format lets a frame say "use the movie's rate", and the header has no rate
/// field — deliberately, since one number per file was a field nobody could change
/// per frame. This is that number.
pub const DEFAULT_DELAY_MS: u16 = 100;

/// Play `movie` on `surface`.
///
/// `loops` is how many times to run the animation; 0 means forever. A movie that
/// does not loop plays once regardless.
///
/// A failed write does not stop playback. Early in boot a dropped frame is worth far
/// less than the rest of the animation, and the same policy is simpler to reason
/// about than one that sometimes gives up.
pub fn play<S: Surface>(surface: &mut S, movie: &Movie, loops: u32) -> io::Result<()> {
    let Some(mut player) = Player::new(movie, surface.size()) else {
        return Ok(());
    };
    while let Some(hold) = player.step(surface, movie, loops) {
        sleep(hold);
    }
    Ok(())
}

/// One movie, one frame at a time.
///
/// [`play`] is a loop around this, and it is the smaller half: everything that
/// decides what a frame looks like is here, and what is left there is a `sleep`.
/// The split exists because `raba`(8) cannot use a function that sleeps — it has
/// a control socket to poll and signals to check between frames — and two copies
/// of frame timing and cursor parking is exactly the thing that drifts silently.
#[derive(Debug)]
pub struct Player {
    screen: Size,
    /// Whether to resample or crop, decided once for the whole movie: a picture
    /// that resampled on some frames and was cropped on others would jump.
    shading: bool,
    /// The art at its authored size, with every delta so far applied.
    current: Grid,
    /// What the terminal is showing, so the encoder emits only differences.
    shown: Grid,
    buffer: Vec<u8>,
    frame: usize,
    round: u32,
}

impl Player {
    /// Start a movie. `None` if the movie or the screen has no area.
    #[must_use]
    pub fn new(movie: &Movie, screen: Size) -> Option<Player> {
        // Art is authored at one size and played at whatever this machine has.
        // When they differ, what reaches the encoder is still always exactly
        // screen-sized, so its absolute cursor moves are always in range.
        // Drawing past the last column instead would leave the console
        // scrolled, which is the one thing a splash must not do.
        let current = Grid::new(movie.columns, movie.lines, movie.palette)?;
        let shown = Grid::new(screen.columns, screen.lines, movie.palette)?;
        Some(Player {
            screen,
            shading: resamples(movie),
            current,
            shown,
            buffer: Vec::new(),
            frame: 0,
            round: 0,
        })
    }

    /// Which frame is next.
    #[must_use]
    pub fn frame(&self) -> u32 {
        u32::try_from(self.frame).unwrap_or(u32::MAX)
    }

    /// How many complete rounds have played.
    #[must_use]
    pub const fn round(&self) -> u32 {
        self.round
    }

    /// Jump to a frame without drawing anything on the way.
    ///
    /// Frames carry deltas, so arriving at frame *n* means applying every frame
    /// before it — there is no other way to know what the canvas holds. What
    /// this skips is the *drawing*, which is what makes resuming from
    /// `/run/raba.state` a single frame of work rather than a fast-forward the
    /// user can see.
    pub fn seek(&mut self, movie: &Movie, frame: u32) {
        let target = usize::try_from(frame).unwrap_or(0).min(movie.frames.len());
        self.frame = 0;
        for index in 0..target {
            if let Some(frame) = movie.frames.get(index) {
                apply(&mut self.current, movie, frame);
            }
        }
        self.frame = target;
    }

    /// Draw the next frame and say how long to hold it.
    ///
    /// `None` when `loops` rounds are done; `0` loops means forever, and a movie
    /// that does not loop plays once regardless.
    ///
    /// A failed write does not stop playback. Early in boot a dropped frame is
    /// worth far less than the rest of the animation, and the same policy is
    /// simpler to reason about than one that sometimes gives up.
    pub fn step<S: Surface>(
        &mut self,
        surface: &mut S,
        movie: &Movie,
        loops: u32,
    ) -> Option<Duration> {
        if self.frame >= movie.frames.len() {
            // Between loops the screen already shows the last frame, and frame
            // 0 carries the whole canvas, so the next round corrects itself.
            self.frame = 0;
            self.round = self.round.saturating_add(1);
            let rounds = if movie.looping { loops } else { 1 };
            if rounds != 0 && self.round >= rounds {
                return None;
            }
        }
        let frame = *movie.frames.get(self.frame)?;
        self.frame = self.frame.saturating_add(1);

        apply(&mut self.current, movie, &frame);
        let drawn = best_fit(&self.current, self.screen, self.shading);
        to_ansi_into(&mut self.buffer, &drawn, Some(&self.shown));
        // Park the cursor out of the way in the same write, so it does not blink
        // in the middle of the animation and so nothing has to be restored if
        // this process is interrupted.
        park(&mut self.buffer, self.screen);
        let _ = surface.present(&self.buffer);
        self.shown = drawn;

        let delay = if frame.delay_ms == 0 { DEFAULT_DELAY_MS } else { frame.delay_ms };
        Some(Duration::from_millis(u64::from(delay)))
    }
}

/// `ESC [ line ; 1 H` — move to the bottom left of the screen.
///
/// The bottom left, and not the line after the art: [`best_fit`] always returns a
/// grid exactly the size of the screen, so there is no line after the art to go to.
/// An earlier version asked for one anyway and emitted a position one past the last
/// line on every single frame. Terminals clamp that, so nothing looked wrong — and
/// a splash that is one character out of range on a console it has never run on is
/// not a thing to leave resting on a clamp.
fn park(out: &mut Vec<u8>, screen: Size) {
    out.extend_from_slice(b"\x1b[");
    out.extend_from_slice(screen.lines.max(1).to_string().as_bytes());
    out.extend_from_slice(b";1H");
}

/// Clear the screen and put the cursor at the top left, so the art draws where its
/// absolute cursor moves expect it to.
pub fn clear<S: Surface>(surface: &mut S) -> io::Result<()> {
    surface.present(b"\x1b[2J\x1b[H")
}
