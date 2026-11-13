//! Unit tests for [`super`].
//!
//! Its own file rather than a `mod tests` block inside the module it tests, so
//! that `scripts/coverage.sh` can leave it out: test code counted as covered
//! product code makes the only number anybody reads say the wrong thing. See
//! `docs/maintenance.md`.

// A test that cannot construct its own hostile input cannot check anything.
// The boot-path lints are about what ships; none of this does.
#![allow(clippy::expect_used, clippy::arithmetic_side_effects)]

use super::*;

#[test]
fn ordinary_text_survives_unchanged() {
    assert_eq!(sanitise("Checking filesystems", 80), "Checking filesystems");
}

#[test]
fn an_escape_sequence_leaves_no_escape_behind() {
    let hostile = "\x1b[2J\x1b[31mred";
    let clean = sanitise(hostile, 80);
    assert_eq!(clean, "[2J[31mred");
    assert!(!clean.contains('\u{1b}'));
}

/// The reason filtering `ESC` is not enough.
#[test]
fn the_single_byte_csi_is_dropped_as_well() {
    let clean = sanitise("\u{9b}5i pwned", 80);
    assert!(!clean.contains('\u{9b}'));
    assert_eq!(clean, "5i pwned");
}

#[test]
fn every_control_character_is_dropped_rather_than_spaced() {
    for code in (0..=0x1fu32).chain(0x7f..=0x9f) {
        let ch = char::from_u32(code).expect("a control codepoint");
        let clean = sanitise(&format!("a{ch}b"), 80);
        assert_eq!(clean, "ab", "{code:#04x} was not dropped");
    }
}

/// Spaces are layout and layout is a capability.
#[test]
fn a_control_character_cannot_be_used_to_push_text_sideways() {
    assert_eq!(sanitise("\x01\x01\x01login:", 80), "login:");
}

#[test]
fn text_is_truncated_to_the_canvas() {
    assert_eq!(sanitise("abcdefgh", 3), "abc");
    assert_eq!(sanitise("abc", 0), "");
}

/// Truncation counts characters, not bytes: a console cell holds one.
#[test]
fn truncation_counts_cells_and_not_bytes() {
    assert_eq!(sanitise("░░░░", 2), "░░");
}

/// A right-to-left override reverses what a reader sees without changing what
/// the string says. The console cannot draw one, so it never gets the chance.
#[test]
fn a_bidirectional_override_cannot_rearrange_the_line() {
    let clean = sanitise("boot\u{202e}txen", 80);
    assert!(!clean.contains('\u{202e}'));
    assert_eq!(clean, "boot?txen");
}

#[test]
fn a_zero_width_joiner_becomes_visible_rather_than_invisible() {
    assert_eq!(sanitise("a\u{200d}b", 80), "a?b");
}

#[test]
fn nothing_that_survives_can_mean_anything_to_a_terminal() {
    let soup: String = (0..0x3000u32).filter_map(char::from_u32).collect();
    let clean = sanitise(&soup, u16::MAX);
    assert!(clean.chars().all(|ch| !is_control(ch)));
    assert!(clean.chars().all(|ch| safety(ch) != Safety::Unsafe));
}

/// The replacement is a character the console is certain to have, or this
/// module would be substituting one unprintable thing for another.
#[test]
fn the_replacement_character_is_itself_safe() {
    assert_eq!(safety('?'), Safety::Safe);
}
