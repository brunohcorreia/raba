//! The surface `raba-gen --preview` draws on.
//!
//! There is nothing here about what it puts on the screen — that is the
//! renderer's own suite, which asserts against a terminal model. This is about
//! the two things a surface attached to a real, possibly absent terminal has to
//! get right: the size it reports when nobody will tell it one, and that the
//! two ways of making it are the same way.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use raba_core::surface::Surface;
use raba_term::surface::StdoutSurface;

#[test]
fn a_surface_with_no_terminal_behind_it_still_has_a_usable_size() {
    // Under `cargo test` stdout is a pipe, so `TIOCGWINSZ` fails and there is
    // nothing to measure. 80x25 is the documented fallback and it is the same
    // one the boot path uses; zero in either axis would divide by zero in the
    // layout, and a huge guess would compose a screen nobody can see.
    let surface = StdoutSurface::new();
    let size = surface.size();
    assert!(size.columns > 0 && size.lines > 0, "{size:?}");
    assert!(size.columns <= 1024 && size.lines <= 1024, "{size:?}: past what a Grid can be");
}

#[test]
fn the_default_surface_is_the_one_new_makes() {
    // `Default` is here because clippy asks for it next to a no-argument
    // `new`, and an impl nobody calls is an impl nobody has checked. If it
    // ever stops agreeing with `new` — a default size baked in, say — a
    // preview would be drawn at a size the terminal never reported.
    assert_eq!(StdoutSurface::default().size(), StdoutSurface::new().size());
}
