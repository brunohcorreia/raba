//! The fastfetch logo collection, vendored at a pinned commit.
//!
//! fastfetch compiles its logos into its binary rather than shipping them as
//! data, so `vendor/fastfetch-logos` holds a copy taken at a recorded commit and
//! `build.rs` turns it into the table below. Updating is deliberate: change the
//! two constants in `scripts/vendor-fastfetch-logos.sh`, run it, regenerate the
//! manifest, and read the diff. See `docs/risks.md`.
//!
//! The art is MIT, from fastfetch. The logos themselves are trademarks of the
//! distributions they belong to; see `docs/packaging.md`.
//!
//! # Not every logo can be used
//!
//! fastfetch renders in a terminal with whatever font the user has. RABA draws on
//! a Linux console before `setfont` runs, which has a few hundred glyphs. A good
//! number of these logos are braille, sextants or quadrant blocks and simply
//! cannot be drawn there — `raba_core::glyph` is what says which, and
//! `--list-logos` reports it so the answer arrives before the build rather than
//! during a boot.

use crate::palette;

/// One logo: the name fastfetch knows it by, its aliases, its art verbatim from
/// upstream, and the default colour of each `$N` slot as SGR parameters.
///
/// Both `art` and `colors` are upstream's bytes, not RABA's reading of them. Use
/// [`Logo::art_source`] and [`Logo::palette_colors`] to get values this crate can
/// act on; each applies one documented difference between the two projects.
pub struct Logo {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub art: &'static str,
    pub colors: &'static [&'static str],
}

// Written by hand rather than derived: the art is a few kilobytes of string and
// deriving Debug would dump all of it into every error message that mentions a
// logo. The size is the part anyone debugging actually wants.
impl core::fmt::Debug for Logo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Logo")
            .field("name", &self.name)
            .field("aliases", &self.aliases)
            .field("art_bytes", &self.art.len())
            .field("colors", &self.colors)
            .finish()
    }
}

include!(concat!(env!("OUT_DIR"), "/fastfetch_logos.rs"));

impl Logo {
    /// Whether this logo answers to `name`, case-insensitively, by its own name
    /// or any alias. fastfetch itself is case-insensitive here.
    #[must_use]
    pub fn answers_to(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
            || self.aliases.iter().any(|alias| alias.eq_ignore_ascii_case(name))
    }

    /// The art, in the dialect [`crate::art::parse`] accepts.
    ///
    /// fastfetch's parser is lenient: a `$` that is not followed by a digit or
    /// another `$` is simply a dollar sign. Twenty-six of the vendored logos
    /// depend on that — Debian draws itself out of dollar signs and contains
    /// `,$P"'`. RABA's parser is strict instead, because a stray `$a` in
    /// hand-written art is a typo worth catching rather than rendering.
    ///
    /// Both positions are right for their own input, so the conversion happens
    /// here, at the one boundary between the two formats.
    #[must_use]
    pub fn art_source(&self) -> String {
        let mut out = String::with_capacity(self.art.len());
        let mut chars = self.art.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch != '$' {
                out.push(ch);
                continue;
            }
            match chars.peek() {
                // A real marker, or an escape upstream already wrote. Pass through.
                Some(next) if next.is_ascii_digit() || *next == '$' => {
                    out.push('$');
                    out.push(*next);
                    let _ = chars.next();
                }
                // A bare dollar: upstream means the character. Escape it.
                _ => out.push_str("$$"),
            }
        }
        out
    }

    /// The default slot colours as palette indices, as a console actually shows
    /// them, with anything unreadable dropped rather than guessed at.
    ///
    /// fastfetch prints its logos **bold**, which was checked against the real
    /// output rather than assumed: `ESC[1m ESC[31m`. On a Linux console bold
    /// turns the eight base colours into their bright halves, so upstream's `31`
    /// is index 9 on screen, not 1, and `39` — the default foreground, grey — is
    /// 15. Taking the numbers at face value would ship every logo a shade dark.
    ///
    /// Indices 90–97 are already bright and 256-colour and RGB values are
    /// explicit, so bold leaves all of those alone.
    #[must_use]
    pub fn palette_colors(&self) -> Vec<u8> {
        self.colors
            .iter()
            .filter_map(|sgr| palette::from_sgr(sgr))
            .map(|index| if index < 8 { index.saturating_add(8) } else { index })
            .collect()
    }

    /// Every name this logo answers to, its own first.
    pub fn names(&self) -> impl Iterator<Item = &'static str> {
        std::iter::once(self.name).chain(self.aliases.iter().copied())
    }
}

/// Look a logo up by name or alias, case-insensitively.
///
/// Unlike fastfetch, an unknown name is `None` rather than a silent fallback to
/// the running distribution's logo. That fallback is a real trap: asking for a
/// logo that does not exist produces a plausible-looking wrong answer.
#[must_use]
pub fn find(name: &str) -> Option<&'static Logo> {
    LOGOS.iter().find(|logo| logo.answers_to(name))
}

/// Every logo whose name or an alias contains `pattern`, case-insensitively.
/// An empty pattern matches everything.
///
/// `|` separates alternatives, so `void|artix` finds both. That is the extent of
/// the pattern language: `raba-gen`(1) documents this one form, and a real regex
/// engine would be a dependency bought for a list command.
#[must_use]
pub fn search(pattern: &str) -> Vec<&'static Logo> {
    let needles: Vec<String> = pattern
        .split('|')
        .map(|part| part.trim().to_lowercase())
        .filter(|part| !part.is_empty())
        .collect();
    LOGOS
        .iter()
        .filter(|logo| {
            needles.is_empty()
                || logo
                    .names()
                    .any(|name| needles.iter().any(|n| name.to_lowercase().contains(n)))
        })
        .collect()
}

/// Names close enough to `name` to be worth printing when it is not found.
/// Substring matches in either direction, which catches the common cases: a
/// missing suffix, a guessed one, and a wrong capitalisation.
#[must_use]
pub fn suggest(name: &str) -> Vec<&'static str> {
    let needle = name.to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<&'static str> = LOGOS
        .iter()
        .flat_map(Logo::names)
        .filter(|candidate| {
            let lower = candidate.to_lowercase();
            lower.contains(&needle) || needle.contains(&lower)
        })
        .collect();
    out.sort_unstable_by_key(|n| (n.len(), *n));
    out.truncate(8);
    out
}
