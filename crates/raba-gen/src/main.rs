//! Theme generator and converter. Build-time only; never in an initramfs.
//!
//! The interface is `raba-gen`(1); the recipe schema is `raba-theme`(5).

use raba_core::glyph::{audit, Safety};
use raba_core::Movie;
use raba_gen::build::{build, spinners, Recipe};
use raba_gen::recipe::{self, ArtSource};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
raba-gen - build and convert .raba animations

usage:
    raba-gen build <recipe.toml> [-o <out.raba>]
    raba-gen --check <file.raba>
    raba-gen --list-logos [pattern]

options:
    -o <path>              output file (default: the recipe's name with .raba)
    --allow-unsafe-glyphs  downgrade the glyph check from an error to a warning
    --version              print the version and the format version
    --help                 print this

A recipe is about ten lines of TOML and is what you contribute when you add a
theme; the .raba file is a build artifact. See man 1 raba-gen and man 5 raba-theme.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("raba-gen: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("build") => build_command(args.get(1..).unwrap_or(&[])),
        Some("--check") => check_command(args.get(1..).unwrap_or(&[])),
        Some("--list-logos") => list_logos(args.get(1).map(String::as_str)),
        Some("--version") => {
            println!(
                "raba-gen {} (.raba format v{})",
                env!("CARGO_PKG_VERSION"),
                raba_core::format::VERSION
            );
            Ok(())
        }
        Some("--help" | "-h") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command {other}\n\n{USAGE}")),
    }
}

fn build_command(args: &[String]) -> Result<(), String> {
    let mut input: Option<&str> = None;
    let mut output: Option<PathBuf> = None;
    let mut allow_unsafe = false;
    let mut index = 0;

    while let Some(arg) = args.get(index) {
        index = index.saturating_add(1);
        match arg.as_str() {
            "-o" | "--out" => {
                let value = args.get(index).ok_or("-o needs a path")?;
                index = index.saturating_add(1);
                output = Some(PathBuf::from(value));
            }
            "--allow-unsafe-glyphs" => allow_unsafe = true,
            other if other.starts_with('-') => {
                return Err(format!("unknown option {other}\n\n{USAGE}"))
            }
            other => input = Some(other),
        }
    }

    let input = input.ok_or("build needs a recipe")?;
    let recipe_path = Path::new(input);
    let text = std::fs::read_to_string(recipe_path).map_err(|e| format!("{input}: {e}"))?;
    let theme = recipe::parse(&text).map_err(|e| format!("{input}: {e}"))?;

    for note in theme.unhonoured() {
        eprintln!("raba-gen: {input}: {note}");
    }

    let source = match theme.art_source(recipe_path).map_err(|e| format!("{input}: {e}"))? {
        ArtSource::Inline(art) => art,
        ArtSource::File(path) => {
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?
        }
        ArtSource::Fastfetch(name) => {
            return Err(format!(
                "{input}: logo.source = \"fastfetch:{name}\" needs the bundled logo art, \
                 which is not vendored yet; use file: for now (see docs/milestones.md, M1)"
            ))
        }
    };

    // Default output next to the recipe, named after it: themes/x.toml -> themes/x.raba.
    let output = output.unwrap_or_else(|| recipe_path.with_extension("raba"));

    let built = build(&source, &Recipe::from(&theme)).map_err(|e| format!("{input}: {e}"))?;

    for adjustment in &built.adjustments {
        eprintln!("raba-gen: {adjustment}");
    }
    for slot in &built.slots_used {
        if *slot != 0 && usize::from(*slot) > theme.logo.colors.len() {
            eprintln!(
                "raba-gen: the art uses ${slot} and logo.colors gives {}; \
                 it will be drawn in logo.default_color",
                theme.logo.colors.len()
            );
        }
    }

    // Refusing by default, as raba-gen(1) says. The runtime degrades an unknown glyph
    // to a space because a boot splash with one wrong character beats no splash; here
    // there is somebody who can fix it, so here is where it is an error.
    if report_glyphs(&built.movie) > 0 && !allow_unsafe {
        return Err(format!(
            "{input}: the theme contains characters a console cannot draw; \
             fix them, or pass --allow-unsafe-glyphs if this theme is for a terminal"
        ));
    }

    let bytes = raba_core::encode(&built.movie);
    std::fs::write(&output, &bytes).map_err(|e| format!("{}: {e}", output.display()))?;
    println!(
        "{}: {}x{}, {} frames, {} ops, {} bytes",
        output.display(),
        built.movie.columns,
        built.movie.lines,
        built.movie.frames.len(),
        built.movie.ops.len(),
        bytes.len()
    );
    Ok(())
}

fn check_command(args: &[String]) -> Result<(), String> {
    let mut path: Option<&str> = None;
    let mut allow_unsafe = false;
    for arg in args {
        match arg.as_str() {
            "--allow-unsafe-glyphs" => allow_unsafe = true,
            other if other.starts_with('-') => return Err(format!("unknown option {other}")),
            other => path = Some(other),
        }
    }
    let path = path.ok_or("--check needs a file")?;

    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let movie = raba_core::decode(&bytes).map_err(|e| format!("{path}: {e}"))?;
    println!(
        "{path}: format v{}, {}x{}, {} frames, {} ops, {}, {}",
        raba_core::format::VERSION,
        movie.columns,
        movie.lines,
        movie.frames.len(),
        movie.ops.len(),
        if movie.looping { "loops" } else { "plays once" },
        match movie.palette {
            raba_core::Palette::Ansi16 => "16 colours",
            raba_core::Palette::Xterm256 => "256 colours",
        }
    );

    let unsafe_glyphs = report_glyphs(&movie);
    if unsafe_glyphs > 0 && !allow_unsafe {
        return Err(format!(
            "{path}: {unsafe_glyphs} character(s) a console cannot draw; \
             pass --allow-unsafe-glyphs to accept them"
        ));
    }
    Ok(())
}

fn list_logos(pattern: Option<&str>) -> Result<(), String> {
    let _ = pattern;
    Err("--list-logos needs the bundled fastfetch art, which is not vendored yet \
         (see docs/milestones.md, M1). scripts/fastfetch-logo.py extracts one in the \
         meantime"
        .to_owned())
}

/// Print the glyph audit and return how many characters are outright unsafe.
fn report_glyphs(movie: &Movie) -> usize {
    let mut unsafe_count: usize = 0;
    for finding in audit(movie) {
        let note = match finding.safety {
            Safety::Unsafe => {
                unsafe_count = unsafe_count.saturating_add(1);
                "no console font has this"
            }
            _ => "may disappear when setfont runs",
        };
        eprintln!(
            "raba-gen: {:?} in {} cell(s), first at op {}: {note} (see docs/glyphs.md)",
            finding.ch, finding.count, finding.first_op
        );
    }
    unsafe_count
}

/// Keeps the built-in spinner table referenced from the binary, so a style added to
/// `raba-theme`(5) and forgotten here shows up as dead code rather than silence.
#[allow(dead_code)]
const STYLES: &[(&str, &[&str])] = spinners::ALL;
