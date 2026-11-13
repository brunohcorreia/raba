//! Fitting colours to what a console can show.
//!
//! Two separate problems, often confused:
//!
//! 1. **256 to 16.** Art drawn in xterm-256 colours has to become sixteen indices,
//!    because that is what a Linux console has.
//! 2. **Backgrounds to eight.** On a console the high background bit drives blink,
//!    not brightness. A background of 9 is not bright red; it is red text that
//!    flashes.
//!
//! # Whose sixteen colours
//!
//! xterm and the Linux console disagree about what colour 4 is — xterm renders blue
//! as `0,0,238`, the console as `0,0,170`, and so on through the table. RABA draws on
//! a console, so the console's palette is the one to fit against. Matching against
//! xterm's would quietly shift every hue on the machine the theme is actually for.

/// The Linux console's default palette, inherited from VGA.
///
/// `console_codes(4)` lets a program redefine these with `ESC ] P`, which is a thing
/// RABA may do one day for colour effects. Until then, this is what a user sees.
pub const CONSOLE_PALETTE: [(u8, u8, u8); 16] = [
    (0, 0, 0),       // 0  black
    (170, 0, 0),     // 1  red
    (0, 170, 0),     // 2  green
    (170, 85, 0),    // 3  brown
    (0, 0, 170),     // 4  blue
    (170, 0, 170),   // 5  magenta
    (0, 170, 170),   // 6  cyan
    (170, 170, 170), // 7  light grey
    (85, 85, 85),    // 8  dark grey
    (255, 85, 85),   // 9  light red
    (85, 255, 85),   // 10 light green
    (255, 255, 85),  // 11 yellow
    (85, 85, 255),   // 12 light blue
    (255, 85, 255),  // 13 light magenta
    (85, 255, 255),  // 14 light cyan
    (255, 255, 255), // 15 white
];

/// The colour an xterm-256 index stands for.
///
/// The palette is three regions: the sixteen system colours, a 6x6x6 cube, and a
/// 24-step grey ramp. The cube's levels are not evenly spaced — the first step jumps
/// to 95 and the rest go in 40s — which is a detail that quietly ruins a conversion
/// that assumes `level * 51`.
#[must_use]
pub fn rgb_of_xterm(index: u8) -> (u8, u8, u8) {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    match index {
        0..=15 => CONSOLE_PALETTE.get(usize::from(index)).copied().unwrap_or((0, 0, 0)),
        16..=231 => {
            let offset = u16::from(index).saturating_sub(16);
            let red = offset.checked_div(36).unwrap_or(0);
            let green = offset.checked_rem(36).unwrap_or(0).checked_div(6).unwrap_or(0);
            let blue = offset.checked_rem(6).unwrap_or(0);
            let level = |value: u16| LEVELS.get(usize::from(value)).copied().unwrap_or(0);
            (level(red), level(green), level(blue))
        }
        _ => {
            // 232..=255: grey from 8 to 238 in steps of 10.
            let step = u16::from(index).saturating_sub(232);
            let value = step.saturating_mul(10).saturating_add(8);
            let value = u8::try_from(value).unwrap_or(255);
            (value, value, value)
        }
    }
}

/// The console colour closest to an RGB triple.
///
/// Distance is the "redmean" approximation rather than plain Euclidean: weighting the
/// channels by where in the red range the comparison sits is a cheap stand-in for
/// perceptual distance, and the difference shows up exactly where it matters here —
/// plain Euclidean likes to call a mid blue "dark grey".
#[must_use]
pub fn nearest_console_colour(rgb: (u8, u8, u8)) -> u8 {
    let mut best = 0u8;
    let mut best_distance = u64::MAX;
    for (index, candidate) in CONSOLE_PALETTE.iter().enumerate() {
        let distance = redmean(rgb, *candidate);
        if distance < best_distance {
            best_distance = distance;
            best = u8::try_from(index).unwrap_or(0);
        }
    }
    best
}

/// The weights are `2 + mean/256`, `4` and `2 + (255 - mean)/256`, scaled by 256 so
/// the whole thing stays in integers.
///
/// The scaling is the point, not an optimisation: `mean` is at most 255, so
/// `mean / 256` in integer arithmetic is always zero and the formula silently
/// collapses to a fixed `(2, 4, 2)` weighting that is not redmean at all. It still
/// produces plausible-looking answers, which is what makes it worth a comment.
fn redmean(a: (u8, u8, u8), b: (u8, u8, u8)) -> u64 {
    let mean = (u64::from(a.0).saturating_add(u64::from(b.0))).checked_div(2).unwrap_or(0);
    let square = |x: u8, y: u8| {
        let diff = i64::from(x).saturating_sub(i64::from(y));
        u64::try_from(diff.saturating_mul(diff)).unwrap_or(0)
    };
    let red = (512u64.saturating_add(mean)).saturating_mul(square(a.0, b.0));
    let green = 1024u64.saturating_mul(square(a.1, b.1));
    let blue = (767u64.saturating_sub(mean)).saturating_mul(square(a.2, b.2));
    red.saturating_add(green).saturating_add(blue)
}

/// An xterm-256 index as the nearest console colour.
#[must_use]
pub fn downsample(index: u8) -> u8 {
    if index < 16 {
        return index;
    }
    nearest_console_colour(rgb_of_xterm(index))
}

/// A background index a console can actually show as a colour.
///
/// Indices 8 to 15 are mapped to their dim counterparts rather than to black: the hue
/// is what the author chose, and the brightness is the part the hardware will not
/// give. Returning the same hue one shade down is the closest thing to what was
/// asked for.
///
/// **Only those eight.** The rule being applied is a fact about the 16-colour
/// attribute byte, where the high background bit is blink rather than brightness.
/// It says nothing about the xterm-256 cube, where 240 is an ordinary grey and not
/// a bright anything — so 16 and above pass through untouched. Folding them in as
/// well turned every 256-colour background into one of the first eight, silently:
/// `(240 - 8) & 7` is 0, so a grey background imported as black.
#[must_use]
pub fn clamp_background(index: u8) -> u8 {
    if (8..16).contains(&index) {
        index.saturating_sub(8)
    } else {
        index
    }
}

/// Read an SGR foreground parameter the way fastfetch writes it.
///
/// fastfetch stores a logo's default colour per `$N` slot as the literal SGR
/// parameters it would print: `"34"`, `"90"`, `"38;5;149"`, `"38;2;198;151;66"`.
/// The first three forms have a palette index already; the RGB form does not, and
/// is fitted to the console here because the `.raba` format carries no RGB.
///
/// A 256-colour index is returned unchanged rather than downsampled, so that a
/// theme declaring `canvas.palette = 256` keeps it. `Recipe::normalise` does the
/// downsampling when the target is sixteen colours, and reports it.
///
/// `39` is "default foreground", which on a console is grey — 7, not "no colour".
/// Reading it as absent is what shipped MX and elementary in the wrong colour.
pub fn from_sgr(params: &str) -> Option<u8> {
    let mut parts = params.split(';');
    let first: u16 = parts.next()?.trim().parse().ok()?;
    match first {
        30..=37 => u8::try_from(first.checked_sub(30)?).ok(),
        39 => Some(7),
        90..=97 => u8::try_from(first.checked_sub(90)?.checked_add(8)?).ok(),
        38 => match parts.next()?.trim() {
            "5" => parts.next()?.trim().parse::<u8>().ok(),
            "2" => {
                let red = parts.next()?.trim().parse::<u8>().ok()?;
                let green = parts.next()?.trim().parse::<u8>().ok()?;
                let blue = parts.next()?.trim().parse::<u8>().ok()?;
                Some(nearest_console_colour((red, green, blue)))
            }
            _ => None,
        },
        _ => None,
    }
}
