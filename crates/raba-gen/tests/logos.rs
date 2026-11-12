//! The vendored fastfetch logo collection.
//!
//! These tests are what stands between a fastfetch update and a silent change to
//! every theme built from `fastfetch:` art. Re-run them after
//! `scripts/vendor-fastfetch-logos.sh`.

// Panicking is how a test reports; the arithmetic is on lengths that cannot wrap.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic
)]

use raba_core::glyph::{safety, Safety};
use raba_gen::art;
use raba_gen::logos::{self, Logo, LOGOS, LOGO_COUNT};
use std::path::{Path, PathBuf};

fn themes_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes")
}

/// The printable shape of a piece of art, with colour markers removed. Two arts
/// that agree here draw the same picture whatever their colours.
fn shape(source: &str) -> String {
    art::parse(source)
        .map(|parsed| {
            let columns = usize::from(parsed.columns());
            let mut out = String::new();
            for (index, cell) in parsed.cells().iter().enumerate() {
                if index > 0 && columns > 0 && index % columns == 0 {
                    out.push('\n');
                }
                out.push(cell.ch);
            }
            // Trailing blanks are an artefact of squaring the canvas off, not
            // part of the picture.
            out.lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim_end().to_owned()
        })
        .unwrap_or_default()
}

#[test]
fn the_collection_is_not_empty_and_the_count_is_real() {
    assert_eq!(LOGOS.len(), LOGO_COUNT);
    // Through LOGOS.len() rather than the constant, which clippy folds away.
    assert!(LOGOS.len() > 500, "{} logos; the vendored set is around 545", LOGOS.len());
    assert!(!logos::FASTFETCH_COMMIT.is_empty(), "the pinned commit is not recorded");
    assert_eq!(logos::FASTFETCH_COMMIT.len(), 40, "not a full git hash");
}

#[test]
fn every_vendored_logo_parses() {
    // The one that matters. fastfetch's parser is lenient about a bare `$` and
    // RABA's is not, so every logo goes through Logo::art_source; if that
    // conversion is wrong, `fastfetch:NAME` fails for art that --list-logos has
    // already told the author is fine.
    let mut failures = Vec::new();
    for logo in LOGOS {
        if let Err(error) = art::parse(&logo.art_source()) {
            failures.push(format!("{}: {error}", logo.name));
        }
    }
    assert!(failures.is_empty(), "{} logos do not parse: {:?}", failures.len(), &failures[..failures.len().min(5)]);
}

#[test]
fn the_dollar_dialect_is_converted_rather_than_rejected() {
    // Debian draws itself out of dollar signs and relies on fastfetch reading a
    // `$` before a letter as a literal. Twenty-six logos do.
    let debian = logos::find("Debian").expect("Debian");
    assert!(debian.art.contains("$P"), "upstream art no longer has a bare dollar");
    assert!(debian.art_source().contains("$$P"), "the bare dollar was not escaped");
    // Parsing is the real check: "contains" cannot tell $$P from $P, since the
    // escaped form contains the bare one as a substring.
    assert!(art::parse(debian.art).is_err(), "upstream art should be rejected as-is");
    assert!(art::parse(&debian.art_source()).is_ok(), "converted art should parse");

    let relying: usize = LOGOS.iter().filter(|l| l.art_source() != l.art).count();
    assert!(relying >= 20, "only {relying} logos need the conversion; expected around 26");
}

#[test]
fn a_marker_and_an_escape_both_survive_conversion() {
    let converted = Logo { name: "t", aliases: &[], art: "$1a$$b$c", colors: &[] }.art_source();
    assert_eq!(converted, "$1a$$b$$c", "markers and escapes kept, bare dollar escaped");
}

#[test]
fn fastfetch_renders_logos_bold_so_base_colours_come_back_bright() {
    // Checked against real output, not assumed: fastfetch emits ESC[1m before
    // the colour. Reading 31 as index 1 would ship every logo a shade dark.
    let debian = logos::find("Debian").expect("Debian");
    assert_eq!(debian.colors, &["31", "37"], "upstream's declared colours");
    assert_eq!(debian.palette_colors(), vec![9, 15], "what a console shows");
}

#[test]
fn bright_and_explicit_colours_are_left_alone() {
    // Bold brightens the eight base colours. It does not brighten 90-97, which
    // are already bright, and it does not touch 256-colour or RGB values.
    let bright = Logo { name: "t", aliases: &[], art: "", colors: &["91", "38;5;149"] };
    assert_eq!(bright.palette_colors(), vec![9, 149]);
}

#[test]
fn find_is_case_insensitive_and_knows_aliases() {
    let by_name = logos::find("arch").expect("arch");
    let by_case = logos::find("ARCH").expect("ARCH");
    let by_alias = logos::find("archmerge").expect("archmerge");
    assert!(std::ptr::eq(by_name, by_case));
    assert!(std::ptr::eq(by_name, by_alias));
}

#[test]
fn an_unknown_name_is_none_rather_than_a_fallback() {
    // fastfetch answers an unknown name with the running distribution's logo,
    // which is a plausible-looking wrong answer. This is the whole reason the
    // extractor grew a guard; the library must not reintroduce it.
    assert!(logos::find("bazzite").is_none());
    assert!(logos::find("").is_none());
}

#[test]
fn an_unknown_name_gets_suggestions_when_there_are_any() {
    let hints = logos::suggest("ubunt");
    assert!(hints.iter().any(|h| h.eq_ignore_ascii_case("Ubuntu")), "{hints:?}");
    assert!(hints.len() <= 8, "too many to print");
    assert!(logos::suggest("").is_empty());
}

#[test]
fn search_matches_names_and_aliases_and_an_empty_pattern_matches_all() {
    assert_eq!(logos::search("").len(), LOGO_COUNT);
    assert!(logos::search("archmerge").iter().any(|l| l.name == "arch"));
    assert!(logos::search("zzzznosuch").is_empty());

    // raba-gen(1) documents `|` as alternation and uses `void|artix` as its
    // example. That example has to work.
    let either = logos::search("void|artix");
    assert!(either.iter().any(|l| l.name.eq_ignore_ascii_case("void")), "{either:?}");
    assert!(either.iter().any(|l| l.name.eq_ignore_ascii_case("artix")), "{either:?}");
    assert!(either.len() < logos::search("").len(), "alternation matched everything");
}

#[test]
fn most_logos_draw_on_a_console_and_some_do_not() {
    // --list-logos promises this split. If every logo were safe the column would
    // be noise; if none were, the collection would be useless at boot.
    let unsafe_count = LOGOS
        .iter()
        .filter(|l| l.art.chars().any(|ch| ch != '\n' && safety(ch) == Safety::Unsafe))
        .count();
    assert!(unsafe_count > 10, "only {unsafe_count} need a terminal font; the check looks broken");
    assert!(
        unsafe_count < LOGO_COUNT / 2,
        "{unsafe_count} of {LOGO_COUNT} unusable at boot; something is wrong"
    );
}

#[test]
fn the_shipped_art_matches_what_upstream_actually_has() {
    // Cross-check of two independent paths to the same picture: these themes
    // were extracted by rendering fastfetch and reading the output back, and the
    // vendored art is upstream's source. They should draw the same thing.
    //
    // The exceptions are the hand edits, each recorded in its own recipe. Listing
    // them here means a hand edit cannot be lost in a regeneration without a test
    // going red.
    let hand_edited = ["linuxmint", "garuda", "mageia"];

    let mut compared = 0usize;
    let mut differing = Vec::new();
    for entry in std::fs::read_dir(themes_directory()).expect("themes/") {
        let path = entry.expect("entry").path();
        if path.extension().is_none_or(|e| e != "toml") {
            continue;
        }
        let stem = path.file_stem().expect("stem").to_string_lossy().into_owned();
        let recipe = std::fs::read_to_string(&path).expect("recipe");

        // The extractor records the upstream name only when it differs.
        let upstream = recipe
            .lines()
            .find_map(|line| line.split_once("fastfetch calls this logo '"))
            .and_then(|(_, tail)| tail.split_once('\''))
            .map_or_else(|| stem.clone(), |(name, _)| name.to_owned());

        let Some(logo) = logos::find(&upstream) else {
            panic!("{stem}: no vendored logo called {upstream:?}");
        };
        let art_path = path.with_extension("txt");
        let Ok(shipped) = std::fs::read_to_string(&art_path) else {
            continue;
        };

        compared = compared.saturating_add(1);
        let same = shape(&shipped) == shape(&logo.art_source());
        if hand_edited.contains(&stem.as_str()) {
            assert!(!same, "{stem} is listed as hand-edited but matches upstream; drop it from the list");
        } else if !same {
            differing.push(stem);
        }
    }

    assert!(compared > 40, "only {compared} themes compared");
    assert!(differing.is_empty(), "art differs from upstream without a recorded hand edit: {differing:?}");
}
