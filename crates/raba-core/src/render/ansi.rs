//! Grid to terminal bytes, drawing only what changed.
//!
//! # One write per frame
//!
//! This module produces a complete frame into one buffer, for the caller to hand to
//! [`Surface::present`](crate::surface::Surface::present) in a single write. A
//! renderer that emits escape sequences incrementally tears and flickers on a
//! console, because the terminal draws whatever has arrived by the time it is
//! scheduled. The contract is the reason `present` is not called `write`.
//!
//! # Stateless between calls
//!
//! Each call assumes nothing about the terminal except that `prev` is on screen: the
//! cursor position is unknown, no colour is in effect, and the output ends with a
//! reset. So the function is pure in `(grid, prev)`, which is what makes the central
//! property testable:
//!
//! > applying `to_ansi(grid, prev)` to `prev` yields `grid`
//!
//! It costs a handful of bytes per frame. Redraw minimisation is easy to get subtly
//! wrong, and carrying terminal state across calls is how it gets wrong.
//!
//! # One strategy, deliberately
//!
//! Moves are absolute `CUP`, except when the cursor is already in the right place,
//! which covers the common case of a contiguous run. `CUF` would save about three
//! bytes on a scattered frame; every extra strategy is another way for the redraw
//! logic to be wrong. Revisit if frame size ever becomes a measured problem rather
//! than an imagined one.

use super::{Cell, Grid};
use crate::format::Palette;

/// Render a frame, allocating a buffer for it.
///
/// Convenience for tests and tools. The boot path uses [`to_ansi_into`] with a
/// buffer it reuses, so a frame costs no allocation.
#[must_use]
pub fn to_ansi(grid: &Grid, prev: Option<&Grid>) -> Vec<u8> {
    let mut out = Vec::new();
    to_ansi_into(&mut out, grid, prev);
    out
}

/// Render a frame into `out`, which is cleared first.
///
/// `prev` is what is currently on screen. Cells equal to their counterpart in `prev`
/// are not redrawn. Pass `None` for a full redraw, which is also what happens
/// automatically if `prev` does not match `grid` in size or palette — a mismatch
/// means the two are not describing the same screen, and drawing the difference
/// between them would be meaningless.
pub fn to_ansi_into(out: &mut Vec<u8>, grid: &Grid, prev: Option<&Grid>) {
    out.clear();

    let prev = prev.filter(|p| {
        p.columns() == grid.columns() && p.lines() == grid.lines() && p.palette() == grid.palette()
    });

    // Both unknown at entry, by the contract above.
    let mut cursor: Option<(u16, u16)> = None;
    let mut fg: Option<u8> = None;
    let mut bg: Option<u8> = None;
    let mut drew_anything = false;

    for line in 0..grid.lines() {
        for column in 0..grid.columns() {
            let Some(cell) = grid.get(column, line) else {
                continue;
            };
            if let Some(prev) = prev {
                if prev.get(column, line) == Some(cell) {
                    continue;
                }
            }

            if cursor != Some((column, line)) {
                push_cup(out, column, line);
            }
            push_sgr(out, grid.palette(), cell, &mut fg, &mut bg);
            push_glyph(out, cell.ch);
            drew_anything = true;

            // After the last column the cursor's position is genuinely ambiguous:
            // terminals differ on whether autowrap has happened yet, and DEC's
            // deferred wrap means it is neither here nor on the next line until
            // another character arrives. Forget it and move absolutely. One escape
            // per row, in exchange for never being wrong about it.
            cursor = match column.checked_add(1) {
                Some(next) if next < grid.columns() => Some((next, line)),
                _ => None,
            };
        }
    }

    // Leave no colour in effect, so the next frame starts from the same known state
    // and an interrupted animation does not tint whatever is printed next.
    if drew_anything {
        out.extend_from_slice(b"\x1b[0m");
    }
}

/// `ESC [ line+1 ; column+1 H` — absolute cursor position, 1-based.
fn push_cup(out: &mut Vec<u8>, column: u16, line: u16) {
    out.extend_from_slice(b"\x1b[");
    push_decimal(out, line.saturating_add(1));
    out.push(b';');
    push_decimal(out, column.saturating_add(1));
    out.push(b'H');
}

/// Emit only the colour that changed, or nothing if neither did.
fn push_sgr(
    out: &mut Vec<u8>,
    palette: Palette,
    cell: &Cell,
    fg: &mut Option<u8>,
    bg: &mut Option<u8>,
) {
    let need_fg = *fg != Some(cell.fg);
    let need_bg = *bg != Some(cell.bg);
    if !need_fg && !need_bg {
        return;
    }

    out.extend_from_slice(b"\x1b[");
    if need_fg {
        push_colour(out, palette, cell.fg, Layer::Foreground);
        *fg = Some(cell.fg);
    }
    if need_bg {
        if need_fg {
            out.push(b';');
        }
        push_colour(out, palette, cell.bg, Layer::Background);
        *bg = Some(cell.bg);
    }
    out.push(b'm');
}

enum Layer {
    Foreground,
    Background,
}

/// SGR parameters for one colour.
///
/// For [`Palette::Ansi16`] the low eight are `30`/`40` and the high eight are the
/// `aixterm` `90`/`100` brights. Note that a Linux console reads a bright background
/// as blink, not as a colour — clamping belongs in `raba-gen`, which knows the
/// target, not here, which only encodes what it is given.
fn push_colour(out: &mut Vec<u8>, palette: Palette, index: u8, layer: Layer) {
    match palette {
        Palette::Ansi16 => {
            let index = index & 0x0F;
            let base: u16 = match (index < 8, layer) {
                (true, Layer::Foreground) => 30,
                (true, Layer::Background) => 40,
                (false, Layer::Foreground) => 82, // 90 - 8
                (false, Layer::Background) => 92, // 100 - 8
            };
            push_decimal(out, base.saturating_add(u16::from(index)));
        }
        Palette::Xterm256 => {
            out.extend_from_slice(match layer {
                Layer::Foreground => b"38;5;",
                Layer::Background => b"48;5;",
            });
            push_decimal(out, u16::from(index));
        }
    }
}

/// Write one character, substituting a space for anything that could be read as a
/// command rather than as content.
///
/// The last line of defence against escape injection, placed here because it is the
/// single point every character passes through on its way to a terminal. C0, DEL and
/// C1 all matter: C1 `0x9B` is a single-byte CSI on a terminal in 8-bit mode, so
/// filtering `ESC` alone is not enough.
///
/// `raba-gen`'s glyph validator rejects these at authoring time with a diagnostic.
/// This catches the file that did not come from `raba-gen`.
fn push_glyph(out: &mut Vec<u8>, ch: char) {
    let safe = match ch {
        '\0'..='\u{1f}' | '\u{7f}'..='\u{9f}' => ' ',
        other => other,
    };
    let mut buf = [0u8; 4];
    out.extend_from_slice(safe.encode_utf8(&mut buf).as_bytes());
}

/// Decimal digits of `value`, without `core::fmt`.
///
/// `write!` would pull in the formatting machinery for the sake of three digits, in a
/// binary with a size gate. The `checked_div`/`checked_rem` are for the lint, not for
/// a real case: the divisor is a literal.
fn push_decimal(out: &mut Vec<u8>, value: u16) {
    if value == 0 {
        out.push(b'0');
        return;
    }
    let mut digits = [0u8; 5];
    let mut len = 0usize;
    let mut rest = value;
    while rest > 0 {
        let digit = u8::try_from(rest.checked_rem(10).unwrap_or(0)).unwrap_or(0);
        if let Some(slot) = digits.get_mut(len) {
            *slot = b'0'.saturating_add(digit);
        }
        len = len.saturating_add(1);
        rest = rest.checked_div(10).unwrap_or(0);
    }
    for index in (0..len).rev() {
        if let Some(byte) = digits.get(index) {
            out.push(*byte);
        }
    }
}
