//! Text that came from somewhere else.
//!
//! Everything RABA draws is art it built itself, with one exception: `raba-ctl
//! message` puts a caller's string on the console. That string is the only path
//! from outside the process to the screen, and the program it reaches is running
//! as root with a terminal to itself.
//!
//! [security.md](../../../docs/security.md) rates it the highest-severity finding
//! in the project, for a reason worth repeating here rather than linking away: a
//! splash can draw a convincing `login:` prompt on a terminal the user trusts.
//! That is a credential harvester, and it does not need a single escape sequence
//! to work — it needs only to be allowed to print.
//!
//! So this module does not try to be clever about what is dangerous. It states
//! what is *allowed* and drops the rest.
//!
//! # Three separate jobs
//!
//! They are in one function because they have to happen in one order, and
//! splitting them would invite a caller to do two of the three.
//!
//! 1. **No control characters.** C0, `DEL` and C1. C1 matters because `0x9B` is a
//!    single-byte CSI on a console in 8-bit mode, so filtering `ESC` alone leaves
//!    a working escape channel. These are *dropped*, not turned into spaces:
//!    spaces are layout, and layout is something an attacker would like to have.
//! 2. **Nothing the console cannot draw.** A character outside the console font
//!    is replaced by the kernel with a glyph of its choosing and the codepoint is
//!    gone, so admitting them buys nothing — and it would admit the bidirectional
//!    overrides and zero-width characters that rearrange what a reader sees
//!    without changing what the string contains.
//! 3. **No wider than the canvas.** Measured in characters, because a console
//!    cell holds one.
//!
//! # What this deliberately does not do
//!
//! It does not make the message safe to *believe*. Nothing can: a root process
//! asked to print `Enter passphrase:` will print it. The access control on the
//! socket is what decides who may ask, and this is what decides what the asking
//! can turn into. Neither is allowed to be the only one. See `docs/protocol.md`.

use crate::glyph::{safety, Safety};

/// What a message is allowed to become before it is drawn.
///
/// Returns a string of at most `width` characters, every one of which a console
/// can draw and none of which can mean anything to a terminal.
///
/// Characters the console cannot draw become `?` rather than disappearing: a
/// message with a hole in it reads as a different message, and the point of
/// showing the user text is that they can act on it.
#[must_use]
pub fn sanitise(text: &str, width: u16) -> String {
    text.chars()
        .filter(|ch| !is_control(*ch))
        .map(|ch| if safety(ch) == Safety::Unsafe { '?' } else { ch })
        .take(usize::from(width))
        .collect()
}

/// Whether a character is one a terminal would read as an instruction.
///
/// C0 and `DEL` and C1, which is every codepoint below `U+00A0` that is not a
/// printable ASCII character. Written as ranges rather than as `char::
/// is_control` because that function's answer is a Unicode property and this
/// needs to be a statement about terminals — they happen to agree today, and
/// only one of them is what this module means.
#[must_use]
pub fn is_control(ch: char) -> bool {
    matches!(ch, '\0'..='\u{1f}' | '\u{7f}'..='\u{9f}')
}

#[cfg(test)]
mod tests {
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
}
