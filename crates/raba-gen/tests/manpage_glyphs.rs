//! Claims the documentation makes about glyphs, checked against the validator.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use raba_core::glyph::{safety, Safety};

#[test]
fn the_bar_spinner_is_not_cp437() {
    // raba-theme(5) called these "CP437 blocks". CP437 has only the halves, the
    // quarters and the shades -- the eighths are not in it, so a bar spinner built
    // from them is a spinner nobody can see on a console.
    for ch in "▁▂▃▅▆▇".chars() {
        assert_eq!(safety(ch), Safety::Unsafe, "{ch:?} is not in CP437");
    }
    // These two are, which is what makes the claim plausible enough to have been
    // written down.
    for ch in "▄█".chars() {
        assert_eq!(safety(ch), Safety::Safe, "{ch:?}");
    }
}

#[test]
fn the_arc_spinner_is_not_console_safe_either() {
    for ch in "◜◝◞◟".chars() {
        assert_eq!(safety(ch), Safety::Unsafe, "{ch:?}");
    }
}
