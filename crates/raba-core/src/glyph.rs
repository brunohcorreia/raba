//! Which characters a Linux console can actually draw.
//!
//! # The question this answers
//!
//! The console holds 256 or 512 glyphs, inherited from VGA hardware. A character
//! outside the loaded font is replaced and the kernel discards the original
//! codepoint, so there is no runtime recovery and no error — the art is simply wrong
//! on the user's screen and right on the author's terminal.
//!
//! The usual framing, "which characters are in the console font", is the wrong
//! question for RABA, because **RABA draws before anyone knows which font that is.**
//! The splash starts in the initramfs; `systemd-vconsole-setup` (or the distribution's
//! equivalent) runs `setfont` later, and may replace the font underneath a running
//! animation. So the only defensible rule is the intersection:
//!
//! > Safe means present in the kernel's built-in font **and** in the fonts a
//! > distribution is likely to load over it.
//!
//! The built-in font is CP437. The common replacements — `lat2-16`, `lat9w-16`,
//! `ter-116n` — keep ASCII and the drawing characters and differ about everything
//! else. That intersection is ASCII, the CP437 block elements, and the CP437 box
//! drawing set, which is exactly what [`Safety::Safe`] admits.
//!
//! # Three levels, not a boolean
//!
//! [`Safety::Unreliable`] is the useful part. CP437's accented letters, Greek and
//! mathematical symbols are in the built-in font and absent from several of the fonts
//! that replace it, so they work until the moment `setfont` runs and then stop. A
//! boolean would have to call that either safe, which is a lie, or unsafe, which
//! would reject half the logos that render perfectly well on most machines. The
//! author decides; this reports.
//!
//! # Why this is a warning and not a decode error
//!
//! The decoder rejects a colour index outside its palette, and does not reject an
//! unsafe glyph. The difference is whether a correct fallback exists. For a colour
//! there is none — masking and clamping are equally arbitrary inventions. For a glyph
//! there is: draw a space, which is what the renderer does for control characters.
//! A boot splash with one wrong character beats no boot splash.

use crate::format::Movie;
use std::collections::BTreeMap;

/// How much a character can be trusted on a console.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Safety {
    /// Present in every font the console plausibly has when RABA draws.
    Safe,
    /// In the kernel's built-in font, missing from some fonts that replace it.
    /// Works on most machines, and stops working when `setfont` runs on the rest.
    Unreliable,
    /// Not in a console font. Braille, arrows, emoji, the quadrant and sextant
    /// blocks that terminal-based art tools like to use.
    Unsafe,
}

/// Block elements and box drawing present in CP437, and therefore in essentially
/// every console font. Sorted; [`is_sorted`] is asserted in the tests, because a
/// binary search over an unsorted table fails silently and intermittently.
const SAFE_DRAWING: &[char] = &[
    '\u{2500}', '\u{2502}', '\u{250c}', '\u{2510}', '\u{2514}', '\u{2518}', '\u{251c}', '\u{2524}',
    '\u{252c}', '\u{2534}', '\u{253c}', '\u{2550}', '\u{2551}', '\u{2552}', '\u{2553}', '\u{2554}',
    '\u{2555}', '\u{2556}', '\u{2557}', '\u{2558}', '\u{2559}', '\u{255a}', '\u{255b}', '\u{255c}',
    '\u{255d}', '\u{255e}', '\u{255f}', '\u{2560}', '\u{2561}', '\u{2562}', '\u{2563}', '\u{2564}',
    '\u{2565}', '\u{2566}', '\u{2567}', '\u{2568}', '\u{2569}', '\u{256a}', '\u{256b}', '\u{256c}',
    '\u{2580}', '\u{2584}', '\u{2588}', '\u{258c}', '\u{2590}', '\u{2591}', '\u{2592}', '\u{2593}',
    '\u{25a0}',
];

/// The rest of CP437: currency, punctuation, Greek, mathematics. In the built-in
/// font, gone the moment a Latin-2 or Terminus font is loaded.
const CP437_REMAINDER: &[char] = &[
    '\u{00a0}', '\u{00a1}', '\u{00a2}', '\u{00a3}', '\u{00a5}', '\u{00aa}', '\u{00ab}', '\u{00ac}',
    '\u{00ba}', '\u{00bb}', '\u{00bc}', '\u{00bd}', '\u{00bf}', '\u{0192}', '\u{0393}', '\u{0398}',
    '\u{03a3}', '\u{03a6}', '\u{03a9}', '\u{03b1}', '\u{03b4}', '\u{03b5}', '\u{03c0}', '\u{03c3}',
    '\u{03c4}', '\u{03c6}', '\u{207f}', '\u{20a7}', '\u{2219}', '\u{221a}', '\u{221e}', '\u{2229}',
    '\u{2248}', '\u{2261}', '\u{2264}', '\u{2265}', '\u{2310}', '\u{2320}', '\u{2321}',
];

/// Classify one character.
#[must_use]
pub fn safety(ch: char) -> Safety {
    match ch {
        // Not characters at all as far as a terminal is concerned. The renderer
        // substitutes a space; this is where an author finds out why.
        '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}' => Safety::Unsafe,
        ' '..='~' => Safety::Safe,
        _ if SAFE_DRAWING.binary_search(&ch).is_ok() => Safety::Safe,
        _ if CP437_REMAINDER.binary_search(&ch).is_ok() => Safety::Unreliable,
        // Latin-1 and Latin-2, letters and punctuation alike: in some console fonts
        // and not others, and which ones depends on the distribution's locale
        // settings rather than on anything RABA can see. This range subsumes CP437's
        // own Latin-1 selection, degree and plus-minus included.
        '\u{a0}'..='\u{17f}' => Safety::Unreliable,
        _ => Safety::Unsafe,
    }
}

/// Whether a character will draw correctly whatever font the console has.
#[must_use]
pub fn is_safe(ch: char) -> bool {
    safety(ch) == Safety::Safe
}

/// One character that will not draw reliably, and where it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Finding {
    pub ch: char,
    pub safety: Safety,
    /// Index of the first op using it, so a tool can point at a place in the art.
    pub first_op: u32,
    /// How many ops use it. A logo drawn entirely in braille is one finding with a
    /// large count, not four hundred findings.
    pub count: u32,
}

/// Every character in `movie` that will not draw reliably.
///
/// Grouped by character and ordered worst-first, then by where it first appears, so
/// the output is deterministic and the first line is the one worth fixing. An empty
/// result means the movie draws correctly on any console.
#[must_use]
pub fn audit(movie: &Movie) -> Vec<Finding> {
    let mut seen: BTreeMap<char, Finding> = BTreeMap::new();

    for (index, op) in movie.ops.iter().enumerate() {
        let safety = safety(op.ch);
        if safety == Safety::Safe {
            continue;
        }
        let op_index = u32::try_from(index).unwrap_or(u32::MAX);
        seen.entry(op.ch)
            .and_modify(|finding| finding.count = finding.count.saturating_add(1))
            .or_insert(Finding { ch: op.ch, safety, first_op: op_index, count: 1 });
    }

    let mut findings: Vec<Finding> = seen.into_values().collect();
    findings.sort_by(|a, b| {
        b.safety.cmp(&a.safety).then(a.first_op.cmp(&b.first_op)).then(a.ch.cmp(&b.ch))
    });
    findings
}
