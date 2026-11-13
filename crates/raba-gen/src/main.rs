//! Theme generator and converter. Build-time only; never in an initramfs.
//!
//! The interface is `raba-gen`(1); the recipe schema is `raba-theme`(5).

use raba_core::glyph::{self, audit, Safety};
use raba_core::Movie;
use raba_gen::build::{build, spinners, Recipe};
use raba_gen::dur;
use raba_gen::generate::{generate, Kind, Spec};
use raba_gen::logos;
use raba_gen::recipe::{self, ArtSource};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// How many times `--preview` runs the animation before giving the shell back.
/// The same default as `raba --preview`, which is long enough to judge a spinner
/// and short enough not to need an interrupt.
const PREVIEW_LOOPS: u32 = 3;

const USAGE: &str = "\
raba-gen - build and convert .raba animations

usage:
    raba-gen build <recipe.toml> [-o <out.raba>] [--preview]
    raba-gen convert <art.dur> [-o <out.raba>] [--preview]
    raba-gen generate <kind> [-o <out.raba>] [--preview]
    raba-gen --check <file.raba>
    raba-gen --list-logos [pattern]

options:
    -o <path>              output file (default: the recipe's name with .raba)
    --preview              play the result here instead of writing a file
    --columns N --lines N  canvas for generate (default 80x25)
    --frames N --delay MS  length and frame time for generate
    --seed N               generator seed; the same seed gives the same file
    --palette 16|256       colour depth for generate (default 16)
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
        Some("convert") => convert_command(args.get(1..).unwrap_or(&[])),
        Some("generate") => generate_command(args.get(1..).unwrap_or(&[])),
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
    let mut preview = false;
    let mut index = 0;

    while let Some(arg) = args.get(index) {
        index = index.saturating_add(1);
        match arg.as_str() {
            "--preview" => preview = true,
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

    // A fastfetch logo brings its own slot colours. They are only used when the
    // recipe gives none: an explicit logo.colors is the author's decision and is
    // never second-guessed.
    let mut inherited: Option<Vec<u8>> = None;
    let source = match theme.art_source(recipe_path).map_err(|e| format!("{input}: {e}"))? {
        ArtSource::Inline(art) => art,
        ArtSource::File(path) => {
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?
        }
        ArtSource::Fastfetch(name) => {
            let logo = logos::find(&name).ok_or_else(|| {
                let hints = logos::suggest(&name);
                let tail = if hints.is_empty() {
                    "; try --list-logos".to_owned()
                } else {
                    format!("; did you mean {}?", hints.join(", "))
                };
                format!("{input}: no fastfetch logo called {name:?}{tail}")
            })?;
            if theme.logo.colors.is_empty() {
                inherited = Some(logo.palette_colors());
            }
            logo.art_source()
        }
    };

    // Default output next to the recipe, named after it: themes/x.toml -> themes/x.raba.
    let output = output.unwrap_or_else(|| recipe_path.with_extension("raba"));

    let mut recipe = Recipe::from(&theme);
    if let Some(colours) = inherited {
        eprintln!(
            "raba-gen: {input}: using the logo's own colours {colours:?}; \
             set logo.colors to override"
        );
        recipe.colours = colours;
    }
    let built = build(&source, &recipe).map_err(|e| format!("{input}: {e}"))?;

    for adjustment in &built.adjustments {
        eprintln!("raba-gen: {adjustment}");
    }
    // Against the colours actually used, not the recipe's: a fastfetch logo
    // brings its own, and warning about a recipe that deliberately gives none
    // would fire on every correct build.
    for slot in &built.slots_used {
        if *slot != 0 && usize::from(*slot) > recipe.colours.len() {
            eprintln!(
                "raba-gen: the art uses ${slot} and only {} colour(s) are given; \
                 it will be drawn in logo.default_color",
                recipe.colours.len()
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

    // --preview replaces the write rather than following it, as raba-gen(1) says:
    // the point is to look at a theme without leaving an artifact behind.
    if preview {
        let mut surface = raba_term::StdoutSurface::new();
        let _ = raba_term::clear(&mut surface);
        let played = raba_term::play(&mut surface, &built.movie, PREVIEW_LOOPS);
        let _ = <raba_term::StdoutSurface as raba_core::surface::Surface>::restore(&mut surface);
        played.map_err(|e| format!("{input}: {e}"))?;
        eprintln!(
            "raba-gen: {}x{}, {} frames, {} bytes if written",
            built.movie.columns,
            built.movie.lines,
            built.movie.frames.len(),
            bytes.len()
        );
        return Ok(());
    }

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

/// `convert` — import a Durdraw `.dur` and write a `.raba`.
///
/// Nothing on the boot path reads `.dur`; this is the only place gzip and JSON
/// appear, and they never leave build time. See `docs/dur-support.md`.
fn convert_command(args: &[String]) -> Result<(), String> {
    let mut input: Option<&str> = None;
    let mut output: Option<PathBuf> = None;
    let mut allow_unsafe = false;
    let mut preview = false;
    let mut index = 0;

    while let Some(arg) = args.get(index) {
        index = index.saturating_add(1);
        match arg.as_str() {
            "--preview" => preview = true,
            "--allow-unsafe-glyphs" => allow_unsafe = true,
            "-o" | "--out" => {
                let value = args.get(index).ok_or("-o needs a path")?;
                index = index.saturating_add(1);
                output = Some(PathBuf::from(value));
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option {other}\n\n{USAGE}"))
            }
            other => input = Some(other),
        }
    }

    let input = input.ok_or("convert needs a .dur file")?;
    let bytes = std::fs::read(input).map_err(|e| format!("{input}: {e}"))?;
    let imported = dur::parse(&bytes).map_err(|e| format!("{input}: {e}"))?;

    for note in &imported.notes {
        eprintln!("raba-gen: {input}: {note}");
    }
    if !imported.name.is_empty() || !imported.artist.is_empty() {
        eprintln!(
            "raba-gen: {input}: {:?} by {:?}",
            imported.name,
            if imported.artist.is_empty() { "unknown" } else { &imported.artist }
        );
    }

    // Imported art gets the same glyph check as art written here. The console does
    // not care where a character came from.
    if report_glyphs(&imported.movie) > 0 && !allow_unsafe {
        return Err(format!(
            "{input}: the art contains characters a console cannot draw; \
             pass --allow-unsafe-glyphs if this is for a terminal"
        ));
    }

    let movie = &imported.movie;

    // Warned here, where the canvas can still be changed, rather than at boot
    // where all that can happen is fitting it. 80x25 is what raba(8) assumes when
    // it cannot read the console.
    //
    // What happens there depends on the art. Shading art is resampled, which is
    // lossy but keeps the whole picture; anything else is centred and cropped,
    // which keeps the detail and loses the edges. Saying which one applies is the
    // difference between a warning somebody acts on and one they ignore.
    let fallback = raba_core::surface::Size::FALLBACK;
    let canvas = raba_core::surface::Size { columns: movie.columns, lines: movie.lines };
    if raba_core::render::crops(canvas, fallback) {
        let fate = if raba_core::render::all_shading(movie.ops.iter().map(|op| op.ch)) {
            "it is shading art, so it will be scaled down to fit"
        } else {
            "it will be centred and cropped there"
        };
        eprintln!(
            "raba-gen: {input}: canvas is {}x{}, larger than the {}x{} console \
             raba(8) assumes when it cannot read the real size; {fate} \
             (see docs/dur-support.md on size)",
            movie.columns, movie.lines, fallback.columns, fallback.lines
        );
    }

    let bytes_out = raba_core::encode(movie);

    if preview {
        let mut surface = raba_term::StdoutSurface::new();
        let _ = raba_term::clear(&mut surface);
        let played = raba_term::play(&mut surface, movie, PREVIEW_LOOPS);
        let _ = <raba_term::StdoutSurface as raba_core::surface::Surface>::restore(&mut surface);
        played.map_err(|e| format!("{input}: {e}"))?;
        return Ok(());
    }

    let output = output.unwrap_or_else(|| Path::new(input).with_extension("raba"));
    std::fs::write(&output, &bytes_out).map_err(|e| format!("{}: {e}", output.display()))?;
    println!(
        "{}: {}x{}, {} frames, {} ops, {} bytes",
        output.display(),
        movie.columns,
        movie.lines,
        movie.frames.len(),
        movie.ops.len(),
        bytes_out.len()
    );
    Ok(())
}

/// `generate` — produce an animation that has no source file.
///
/// Ly's built-in animations are procedural code rather than art, so there is
/// nothing to import. Generating them here instead of running them at boot is
/// what the project's rule asks for: authoring time may compute anything, run
/// time only plays frames.
fn generate_command(args: &[String]) -> Result<(), String> {
    let mut kind: Option<Kind> = None;
    let mut output: Option<PathBuf> = None;
    // Default to the terminal rather than to 80x25. A generated animation is
    // whatever size it is asked for, so defaulting to the fallback console size
    // and then centring that on a large terminal fills a box in the middle and
    // looks broken. An explicit --columns/--lines still wins.
    let terminal = raba_term::detect_size();
    let mut spec = Spec { columns: terminal.columns, lines: terminal.lines, ..Spec::default() };
    let mut preview = false;
    let mut index = 0;

    let number = |args: &[String], at: usize, what: &str| -> Result<u64, String> {
        args.get(at)
            .ok_or_else(|| format!("{what} needs a value"))?
            .parse::<u64>()
            .map_err(|e| format!("{what}: {e}"))
    };

    while let Some(arg) = args.get(index) {
        index = index.saturating_add(1);
        match arg.as_str() {
            "--preview" => preview = true,
            "-o" | "--out" => {
                let value = args.get(index).ok_or("-o needs a path")?;
                index = index.saturating_add(1);
                output = Some(PathBuf::from(value));
            }
            "--columns" => {
                spec.columns = u16::try_from(number(args, index, "--columns")?).unwrap_or(80);
                index = index.saturating_add(1);
            }
            "--lines" => {
                spec.lines = u16::try_from(number(args, index, "--lines")?).unwrap_or(25);
                index = index.saturating_add(1);
            }
            "--frames" => {
                spec.frames = u16::try_from(number(args, index, "--frames")?).unwrap_or(60);
                index = index.saturating_add(1);
            }
            "--delay" => {
                spec.delay_ms = u16::try_from(number(args, index, "--delay")?).unwrap_or(80);
                index = index.saturating_add(1);
            }
            "--seed" => {
                spec.seed = number(args, index, "--seed")?;
                index = index.saturating_add(1);
            }
            "--palette" => {
                spec.palette = match args.get(index).map(String::as_str) {
                    Some("16") => raba_core::Palette::Ansi16,
                    Some("256") => raba_core::Palette::Xterm256,
                    other => return Err(format!("--palette {other:?}; use 16 or 256")),
                };
                index = index.saturating_add(1);
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option {other}\n\n{USAGE}"))
            }
            other => {
                kind = Some(Kind::named(other).ok_or_else(|| {
                    format!(
                        "unknown animation {other:?}; try {}",
                        Kind::ALL.map(Kind::name).join(", ")
                    )
                })?);
            }
        }
    }

    let kind = kind.ok_or_else(|| {
        format!("generate needs an animation: {}", Kind::ALL.map(Kind::name).join(", "))
    })?;

    let movie = generate(kind, &spec);

    if !kind.loops_seamlessly() {
        eprintln!(
            "raba-gen: {} is chaotic and does not return to its first frame, so the \
             loop has a seam; see the module docs for why it is not hidden",
            kind.name()
        );
    }
    if movie.frames.len() != usize::from(spec.frames) {
        eprintln!(
            "raba-gen: {} settled into a {}-frame cycle, shorter than the {} asked for",
            kind.name(),
            movie.frames.len(),
            spec.frames
        );
    }
    if report_glyphs(&movie) > 0 {
        return Err(format!("{}: generated characters a console cannot draw", kind.name()));
    }

    let bytes = raba_core::encode(&movie);

    if preview {
        let mut surface = raba_term::StdoutSurface::new();
        let _ = raba_term::clear(&mut surface);
        let played = raba_term::play(&mut surface, &movie, PREVIEW_LOOPS);
        let _ = <raba_term::StdoutSurface as raba_core::surface::Surface>::restore(&mut surface);
        played.map_err(|e| format!("{}: {e}", kind.name()))?;
        return Ok(());
    }

    let output = output.unwrap_or_else(|| PathBuf::from(format!("{}.raba", kind.name())));
    std::fs::write(&output, &bytes).map_err(|e| format!("{}: {e}", output.display()))?;
    println!(
        "{}: {}x{}, {} frames, {} ops, {} bytes",
        output.display(),
        movie.columns,
        movie.lines,
        movie.frames.len(),
        movie.ops.len(),
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

/// List the vendored logos, saying for each whether a console can draw it.
///
/// The safety column is the point. fastfetch renders in a terminal with the
/// user's font, so a good number of its logos are braille, sextants or quadrant
/// blocks; RABA draws before `setfont` runs. Reporting it here means finding out
/// before the build instead of during a boot.
fn list_logos(pattern: Option<&str>) -> Result<(), String> {
    let matches = logos::search(pattern.unwrap_or(""));
    if matches.is_empty() {
        return Err(format!(
            "no logo matches {:?} (of {} vendored)",
            pattern.unwrap_or(""),
            logos::LOGO_COUNT
        ));
    }

    let mut usable = 0usize;
    for logo in &matches {
        let worst = logo
            .art
            .chars()
            .filter(|ch| *ch != '\n')
            .map(glyph::safety)
            .max_by_key(|safety| match safety {
                Safety::Safe => 0u8,
                Safety::Unreliable => 1,
                Safety::Unsafe => 2,
            })
            .unwrap_or(Safety::Safe);
        let note = match worst {
            Safety::Safe => {
                usable = usable.saturating_add(1);
                "console"
            }
            Safety::Unreliable => {
                usable = usable.saturating_add(1);
                "console*"
            }
            Safety::Unsafe => "terminal",
        };
        let aliases = if logo.aliases.is_empty() {
            String::new()
        } else {
            format!("  ({})", logo.aliases.join(", "))
        };
        println!("{:<10} {}{}", note, logo.name, aliases);
    }

    println!();
    println!(
        "{} of {} shown draw on a console; the rest need a terminal font.",
        usable,
        matches.len()
    );
    println!("* also needs the glyph to survive setfont; see docs/glyphs.md.");
    println!(
        "art from fastfetch {} ({}), MIT; logos are trademarks of their projects.",
        logos::FASTFETCH_TAG,
        logos::FASTFETCH_COMMIT.get(..12).unwrap_or(logos::FASTFETCH_COMMIT)
    );
    Ok(())
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
