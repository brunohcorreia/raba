//! Every theme in `themes/`, checked the way `raba-gen` checks it.
//!
//! The directory is read at test time rather than listed here, so a theme added by
//! someone who has never opened this file is covered the moment they add it. That is
//! the point: "add a distribution" is meant to be a contained contribution that needs
//! no Rust, and this is what makes it safe to accept one.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use raba_core::glyph::{audit, Safety};
use raba_core::{decode, encode, Movie};
use raba_gen::build::{build, Recipe};
use raba_gen::recipe::{self, ArtSource};
use std::path::{Path, PathBuf};

/// The size `raba`(8) falls back to when it cannot read the console. A theme that
/// does not fit it is clipped on exactly the machines least able to say why.
const FALLBACK: (u16, u16) = (80, 25);

fn themes_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes")
}

/// Every recipe in `themes/`, as (path, built movie).
fn built() -> Vec<(PathBuf, Movie)> {
    let directory = themes_directory();
    let mut recipes: Vec<PathBuf> = std::fs::read_dir(&directory)
        .unwrap_or_else(|e| panic!("{}: {e}", directory.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "toml"))
        .collect();
    recipes.sort();

    assert!(!recipes.is_empty(), "no themes in {}", directory.display());

    recipes
        .into_iter()
        .map(|path| {
            let name = path.display().to_string();
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
            let theme = recipe::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"));

            assert!(theme.unhonoured().is_empty(), "{name}: {:?}", theme.unhonoured());

            let art = match theme.art_source(&path).unwrap_or_else(|e| panic!("{name}: {e}")) {
                ArtSource::Inline(art) => art,
                ArtSource::File(art_path) => std::fs::read_to_string(&art_path)
                    .unwrap_or_else(|e| panic!("{name}: {}: {e}", art_path.display())),
                ArtSource::Fastfetch(logo) => {
                    panic!("{name}: fastfetch:{logo} needs vendored art (issue 1)")
                }
            };

            let built =
                build(&art, &Recipe::from(&theme)).unwrap_or_else(|e| panic!("{name}: {e}"));
            (path, built.movie)
        })
        .collect()
}

#[test]
fn every_theme_builds() {
    let themes = built();
    println!("{} themes", themes.len());
    for (path, movie) in &themes {
        println!(
            "  {:<24} {}x{}, {} frames",
            path.file_stem().unwrap_or_default().to_string_lossy(),
            movie.columns,
            movie.lines,
            movie.frames.len()
        );
    }
}

#[test]
fn every_theme_draws_on_any_console() {
    // The check `raba-gen` refuses a build over. Several fastfetch logos are drawn
    // with quadrant or sextant blocks that no console font has -- the current NixOS
    // one is -- so this is not a formality.
    for (path, movie) in built() {
        let unsafe_glyphs: Vec<char> = audit(&movie)
            .into_iter()
            .filter(|finding| finding.safety == Safety::Unsafe)
            .map(|finding| finding.ch)
            .collect();
        assert!(unsafe_glyphs.is_empty(), "{}: {unsafe_glyphs:?}", path.display());
    }
}

#[test]
fn every_theme_draws_after_setfont_too() {
    // Stricter than the build, which only warns about these. A shipped theme that
    // looks right for the first second of boot and wrong afterwards is a bug report
    // nobody can diagnose.
    for (path, movie) in built() {
        let fragile: Vec<char> = audit(&movie)
            .into_iter()
            .filter(|finding| finding.safety == Safety::Unreliable)
            .map(|finding| finding.ch)
            .collect();
        assert!(
            fragile.is_empty(),
            "{}: {fragile:?} are in the kernel's built-in font and not in every font \
             that replaces it; see docs/glyphs.md",
            path.display()
        );
    }
}

#[test]
fn every_theme_fits_the_fallback_console() {
    for (path, movie) in built() {
        assert!(
            movie.columns <= FALLBACK.0 && movie.lines <= FALLBACK.1,
            "{}: {}x{} does not fit {}x{}",
            path.display(),
            movie.columns,
            movie.lines,
            FALLBACK.0,
            FALLBACK.1
        );
    }
}

#[test]
fn every_theme_survives_the_format() {
    for (path, movie) in built() {
        let bytes = encode(&movie);
        assert_eq!(decode(&bytes), Ok(movie), "{}", path.display());
    }
}

#[test]
fn every_theme_uses_colours_the_console_has() {
    // Backgrounds stay in 0-7 because the high bit there is blink rather than
    // brightness, and foregrounds stay in 0-15 because that is the palette.
    for (path, movie) in built() {
        assert!(movie.ops.iter().all(|op| op.fg <= 15 && op.bg <= 7), "{}", path.display());
    }
}

#[test]
fn every_theme_animates() {
    // A recipe with the spinner section deleted by accident builds happily and sits
    // there. That is a legitimate theme, but not one of these.
    for (path, movie) in built() {
        assert!(movie.frames.len() > 1, "{} has nothing moving", path.display());
        assert!(movie.looping, "{} does not repeat", path.display());
    }
}

#[test]
fn every_theme_is_mostly_one_frame() {
    // The delta encoding doing its job: frame 0 is the canvas and the rest is the
    // spinner. If this grows, a theme has started costing a canvas per frame.
    for (path, movie) in built() {
        let canvas = u32::from(movie.columns) * u32::from(movie.lines);
        assert_eq!(u32::from(movie.frames[0].op_count), canvas, "{}", path.display());
        for frame in &movie.frames[1..] {
            assert!(frame.op_count <= 4, "{}: {} ops in a frame", path.display(), frame.op_count);
        }
    }
}

#[test]
fn every_theme_has_its_art_next_to_it() {
    // A recipe pointing outside themes/ would build here and break in a package.
    let directory = themes_directory().canonicalize().unwrap();
    for entry in std::fs::read_dir(&directory).unwrap().filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "toml") {
            let theme = recipe::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
            if let Ok(ArtSource::File(art)) = theme.art_source(&path) {
                let art = art.canonicalize().unwrap_or_else(|e| panic!("{}: {e}", art.display()));
                assert!(art.starts_with(&directory), "{} is outside themes/", art.display());
            }
        }
    }
}
