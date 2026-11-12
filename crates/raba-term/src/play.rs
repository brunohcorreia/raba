//! Playing a movie on a terminal.
//!
//! The same core that will drive a virtual terminal, driving stdout instead. If a
//! theme looks right here, the core is right and only the VT layer remains in doubt —
//! which is the whole reason `--preview` is a milestone and not a debug flag.

use raba_core::render::to_ansi_into;
use raba_core::surface::Surface;
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
    let Some(mut shown) = Grid::new(movie.columns, movie.lines, movie.palette) else {
        return Ok(());
    };
    let mut current = shown.clone();
    let mut buffer: Vec<u8> = Vec::new();

    let rounds = if movie.looping { loops } else { 1 };
    let mut round: u32 = 0;

    loop {
        for frame in &movie.frames {
            apply(&mut current, movie, frame);
            to_ansi_into(&mut buffer, &current, Some(&shown));
            // Park the cursor below the art in the same write, so it does not blink
            // in the middle of the animation and so nothing has to be restored if
            // this process is interrupted.
            park_below(&mut buffer, movie.lines);
            let _ = surface.present(&buffer);
            shown.clone_from(&current);

            let delay = if frame.delay_ms == 0 { DEFAULT_DELAY_MS } else { frame.delay_ms };
            sleep(Duration::from_millis(u64::from(delay)));
        }

        round = round.saturating_add(1);
        if rounds != 0 && round >= rounds {
            return Ok(());
        }
        // Between loops the screen already shows the last frame, and frame 0 carries
        // the whole canvas, so the next round corrects everything by itself.
    }
}

/// `ESC [ line ; 1 H` — move to the first column of the line after the art.
fn park_below(out: &mut Vec<u8>, lines: u16) {
    out.extend_from_slice(b"\x1b[");
    out.extend_from_slice(lines.saturating_add(1).to_string().as_bytes());
    out.extend_from_slice(b";1H");
}

/// Clear the screen and put the cursor at the top left, so the art draws where its
/// absolute cursor moves expect it to.
pub fn clear<S: Surface>(surface: &mut S) -> io::Result<()> {
    surface.present(b"\x1b[2J\x1b[H")
}
