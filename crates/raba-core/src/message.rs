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
mod tests;
