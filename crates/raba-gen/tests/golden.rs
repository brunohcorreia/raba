//! Golden renders of every shipped theme.
//!
//! A theme is built, drawn through the real renderer, and read back with the
//! terminal model into a grid; that grid is what is recorded. So a change to the
//! composer, the layout, the colour fitting or the ANSI encoder that alters what
//! somebody sees has to be acknowledged in a diff rather than discovered on a
//! console.
//!
//! The grid is golden, not the byte stream. An optimisation that emits shorter
//! escape sequences for the same picture should not churn fifty files, and a
//! diff of characters and colour indices is one a person can actually read.
//!
//! Run `RABA_BLESS=1 cargo test -p raba-gen --test golden` to re-record after a
//! change you meant to make, then read the diff before committing it.

// Panicking is how a test reports. The arithmetic is on grid indices that the
// format has already bounded.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic
)]

use raba_core::render::to_ansi;
use raba_core::{apply, decode, encode, Grid, Movie};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

#[path = "../../raba-core/tests/terminal/mod.rs"]
mod terminal;

fn themes_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes")
}

fn golden_directory() -> PathBuf {
    themes_directory().join("golden")
}

/// Build every theme the same way `scripts/build-themes.sh` does, through the
/// format, so what is recorded is what a `.raba` file actually plays rather than
/// what the builder happened to hold in memory.
fn themes() -> Vec<(String, Movie)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(themes_directory()).expect("themes/") {
        let path = entry.expect("entry").path();
        if path.extension().is_none_or(|e| e != "toml") {
            continue;
        }
        let name = path.file_stem().expect("stem").to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path).expect("recipe");
        let theme = raba_gen::recipe::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        let source = match theme.art_source(&path).unwrap_or_else(|e| panic!("{name}: {e}")) {
            raba_gen::recipe::ArtSource::Inline(art) => art,
            raba_gen::recipe::ArtSource::File(p) => {
                std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{name}: {e}"))
            }
            raba_gen::recipe::ArtSource::Fastfetch(logo) => raba_gen::logos::find(&logo)
                .unwrap_or_else(|| panic!("{name}: no logo {logo:?}"))
                .art_source(),
        };
        let built = raba_gen::build::build(&source, &raba_gen::build::Recipe::from(&theme))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let movie = decode(&encode(&built.movie)).unwrap_or_else(|e| panic!("{name}: {e}"));
        out.push((name, movie));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(out.len() > 40, "only {} themes found", out.len());
    out
}

/// One cell's colour as a single character, so a colour plane lines up under the
/// character plane and a shifted cell is visible as a shifted column.
fn nibble(value: u8) -> char {
    char::from_digit(u32::from(value), 16).unwrap_or('?')
}

fn plane(grid: &Grid, pick: fn(&raba_core::render::Cell) -> u8) -> String {
    let mut out = String::new();
    for line in 0..grid.lines() {
        for column in 0..grid.columns() {
            out.push(grid.get(column, line).map_or('?', |cell| nibble(pick(cell))));
        }
        out.push('\n');
    }
    out
}

/// Draw the movie through the renderer and read it back off a modelled terminal.
///
/// This is the part that makes the test worth having: it exercises `to_ansi`,
/// including the cursor arithmetic and the deferred-wrap handling, rather than
/// recording the grid the composer produced.
fn render(movie: &Movie) -> String {
    let mut composed = Grid::new(movie.columns, movie.lines, movie.palette).expect("grid");
    let mut shown = composed.clone();
    let mut screen = terminal::Terminal::blank(movie.columns, movie.lines, movie.palette);

    let mut out = String::new();
    let _ = writeln!(
        out,
        "# {}x{}, {} frames, {}, {}",
        movie.columns,
        movie.lines,
        movie.frames.len(),
        if movie.looping { "loops" } else { "plays once" },
        match movie.palette {
            raba_core::Palette::Ansi16 => "16 colours",
            raba_core::Palette::Xterm256 => "256 colours",
        }
    );

    for (index, frame) in movie.frames.iter().enumerate() {
        apply(&mut composed, movie, frame);
        let bytes = to_ansi(&composed, Some(&shown));
        screen.apply(&bytes);
        shown.clone_from(&composed);

        if index == 0 {
            let first = screen.screen();
            let _ = writeln!(out, "\n## frame 0: characters");
            let mut chars = String::new();
            for line in 0..first.lines() {
                for column in 0..first.columns() {
                    chars.push(first.get(column, line).map_or('?', |cell| cell.ch));
                }
                chars.push('\n');
            }
            out.push_str(&chars);
            let _ = writeln!(out, "## frame 0: foreground");
            out.push_str(&plane(first, |cell| cell.fg));
            let _ = writeln!(out, "## frame 0: background");
            out.push_str(&plane(first, |cell| cell.bg));
        }
    }

    // The frames after the first are a handful of cells each. Record what each one
    // changes, which is both smaller and more legible than another full canvas.
    let _ = writeln!(out, "\n## later frames");
    let mut previous = Grid::new(movie.columns, movie.lines, movie.palette).expect("grid");
    let mut current = previous.clone();
    for (index, frame) in movie.frames.iter().enumerate() {
        apply(&mut current, movie, frame);
        if index > 0 {
            let mut changes = Vec::new();
            for line in 0..current.lines() {
                for column in 0..current.columns() {
                    let (now, before) = (current.get(column, line), previous.get(column, line));
                    if now != before {
                        if let Some(cell) = now {
                            changes.push(format!(
                                "({column},{line}) {:?} fg={} bg={}",
                                cell.ch, cell.fg, cell.bg
                            ));
                        }
                    }
                }
            }
            let _ = writeln!(out, "frame {index}: {}", changes.join("  "));
        }
        previous.clone_from(&current);
    }

    // The modelled screen must end up showing the last frame. If it does not, the
    // renderer and the composer disagree and the golden above is of the wrong thing.
    assert_eq!(screen.screen(), &composed, "the modelled terminal drifted from the composed grid");
    out
}

#[test]
fn every_theme_matches_its_golden_render() {
    let bless = std::env::var_os("RABA_BLESS").is_some();
    let directory = golden_directory();
    if bless {
        std::fs::create_dir_all(&directory).expect("golden/");
    }

    let mut stale = Vec::new();
    let mut missing = Vec::new();
    let mut changed = Vec::new();

    for (name, movie) in themes() {
        let rendered = render(&movie);
        let path = directory.join(format!("{name}.txt"));

        if bless {
            std::fs::write(&path, &rendered).expect("write golden");
            continue;
        }

        match std::fs::read_to_string(&path) {
            Err(_) => missing.push(name),
            Ok(recorded) if recorded != rendered => {
                // Report the first differing line; the whole canvas is too much
                // for a panic message and the file is right there to diff.
                let at = recorded
                    .lines()
                    .zip(rendered.lines())
                    .position(|(a, b)| a != b)
                    .map_or_else(|| "length".to_owned(), |n| format!("line {}", n + 1));
                changed.push(format!("{name} (first difference at {at})"));
            }
            Ok(_) => {}
        }
    }

    if bless {
        // Recording leaves nothing behind for a theme that has been deleted.
        for entry in std::fs::read_dir(&directory).expect("golden/") {
            let path = entry.expect("entry").path();
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned());
            if let Some(stem) = stem {
                if !themes_directory().join(format!("{stem}.toml")).exists() {
                    stale.push(stem);
                }
            }
        }
        assert!(stale.is_empty(), "golden files with no theme: {stale:?}");
        return;
    }

    assert!(
        missing.is_empty(),
        "no golden render for {missing:?}; run RABA_BLESS=1 cargo test -p raba-gen --test golden"
    );
    assert!(
        changed.is_empty(),
        "{} theme(s) render differently: {:?}\n\
         If the change was intended, re-record with RABA_BLESS=1 and read the diff.",
        changed.len(),
        changed
    );
}
