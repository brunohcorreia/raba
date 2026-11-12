//! The glyph validator: what a console can draw, and what only looks like it can.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use raba_core::glyph::{audit, safety, Safety};
use raba_core::{Frame, Movie, Op, Palette};

fn movie_of(chars: &[char]) -> Movie {
    let ops: Vec<Op> = chars.iter().map(|&ch| Op { cell: 0, ch, fg: 7, bg: 0 }).collect();
    Movie {
        columns: 8,
        lines: 2,
        palette: Palette::Ansi16,
        looping: true,
        frames: vec![Frame { delay_ms: 100, op_start: 0, op_count: ops.len() as u16 }],
        ops,
    }
}

#[test]
fn printable_ascii_is_safe() {
    for codepoint in 0x20u32..=0x7e {
        let ch = char::from_u32(codepoint).unwrap();
        assert_eq!(safety(ch), Safety::Safe, "{ch:?} is ASCII");
    }
}

#[test]
fn the_blocks_and_box_drawing_are_safe() {
    // The characters ASCII art is actually made of. All CP437, all present in the
    // Latin-2 and Terminus fonts that replace it.
    for ch in "█▀▄▌▐░▒▓■─│┌┐└┘├┤┬┴┼═║╔╗╚╝╠╣╦╩╬".chars()
    {
        assert_eq!(safety(ch), Safety::Safe, "{ch:?}");
    }
}

#[test]
fn the_shading_ramp_is_safe() {
    // donut.c's ramp, which `docs/3d-logos.md` says is the one 3D shading mode that
    // could survive a console. Worth asserting rather than assuming.
    for ch in ".,-~:;=!*#$@".chars() {
        assert_eq!(safety(ch), Safety::Safe, "{ch:?}");
    }
}

#[test]
fn quadrant_and_sextant_blocks_are_unsafe() {
    // Terminal art tools reach for these constantly and they are not in CP437.
    // Sextants are Symbols for Legacy Computing, added to Unicode in 2019.
    for ch in "▘▝▖▗▚▞▞".chars() {
        assert_eq!(safety(ch), Safety::Unsafe, "quadrant {ch:?}");
    }
    for ch in ['\u{1fb00}', '\u{1fb3b}', '\u{1fb8b}'] {
        assert_eq!(safety(ch), Safety::Unsafe, "sextant {ch:?}");
    }
}

#[test]
fn braille_is_unsafe() {
    // The common surprise, and the reason the braille spinner idea died: at 256
    // codepoints it would eat half the console's glyph budget, which is why
    // distributions ship it as a separate console font package.
    for ch in "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏".chars() {
        assert_eq!(safety(ch), Safety::Unsafe, "{ch:?}");
    }
}

#[test]
fn arrows_emoji_and_cjk_are_unsafe() {
    for ch in "→←↑↓🚀🐧漢字".chars() {
        assert_eq!(safety(ch), Safety::Unsafe, "{ch:?}");
    }
}

#[test]
fn control_characters_are_unsafe() {
    // Including C1: 0x9b is a single-byte CSI on a terminal in 8-bit mode.
    for ch in ['\0', '\u{1b}', '\n', '\r', '\u{7f}', '\u{9b}'] {
        assert_eq!(safety(ch), Safety::Unsafe, "{:#06x}", ch as u32);
    }
}

#[test]
fn accented_latin_is_unreliable_not_unsafe() {
    // These draw on most machines and stop drawing when setfont runs with a font
    // that drops them. Calling that "safe" would be a lie and calling it "unsafe"
    // would reject half the logos that work fine.
    for ch in "éàüñçÄÖßåø".chars() {
        assert_eq!(safety(ch), Safety::Unreliable, "{ch:?}");
    }
}

#[test]
fn cp437_greek_and_mathematics_are_unreliable() {
    // In the kernel's built-in font, gone the moment lat2-16 or Terminus loads.
    for ch in "αΓπΣΩ∞≡±≥≤÷√∩".chars() {
        assert_eq!(safety(ch), Safety::Unreliable, "{ch:?}");
    }
}

#[test]
fn the_lookup_tables_are_sorted() {
    // A binary search over an unsorted table does not fail loudly. It reports the
    // wrong answer for some entries and the right one for others, which would show
    // up as an intermittently wrong verdict rather than as a test failure.
    let drawing: Vec<char> = "─│┌┐└┘├┤┬┴┼═║╒╓╔╕╖╗╘╙╚╛╜╝╞╟╠╡╢╣╤╥╦╧╨╩╪╫╬▀▄█▌▐░▒▓■".chars().collect();
    let mut sorted = drawing.clone();
    sorted.sort_unstable();
    assert_eq!(drawing, sorted, "the safe table in glyph.rs must stay sorted");
    for ch in drawing {
        assert_eq!(safety(ch), Safety::Safe, "{ch:?} must be found by the search");
    }
}

#[test]
fn a_clean_movie_has_nothing_to_report() {
    assert!(audit(&movie_of(&"RABA 1.0 █▀▄".chars().collect::<Vec<_>>())).is_empty());
}

#[test]
fn findings_are_grouped_by_character() {
    let movie = movie_of(&['⠋', 'a', '⠋', '⠋', 'é']);
    let findings = audit(&movie);
    assert_eq!(findings.len(), 2, "two distinct problem characters, not four ops");

    // Worst first: a character that cannot draw at all outranks one that might.
    assert_eq!(findings[0].ch, '⠋');
    assert_eq!(findings[0].safety, Safety::Unsafe);
    assert_eq!(findings[0].count, 3);
    assert_eq!(findings[0].first_op, 0);

    assert_eq!(findings[1].ch, 'é');
    assert_eq!(findings[1].safety, Safety::Unreliable);
    assert_eq!(findings[1].count, 1);
    assert_eq!(findings[1].first_op, 4);
}

#[test]
fn equally_bad_characters_are_ordered_by_where_they_appear() {
    // So the first line of a report points at the first thing to go and fix.
    let findings = audit(&movie_of(&['→', '⠋', '→']));
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].ch, '→');
    assert_eq!(findings[1].ch, '⠋');
}

#[test]
fn auditing_is_deterministic() {
    let movie = movie_of(&['⠋', '→', 'é', '🚀', '⠋', 'α']);
    assert_eq!(audit(&movie), audit(&movie));
}
