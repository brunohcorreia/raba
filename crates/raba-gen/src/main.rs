//! Theme generator and converter. Build-time only; never in an initramfs.

use raba_core::format::Palette;
use raba_core::glyph::{audit, Safety};
use raba_gen::build::{build, Recipe, SpinnerSpec};
use raba_gen::compose::spinners;
use std::process::ExitCode;

const USAGE: &str = "\
raba-gen - build .raba animations

usage:
    raba-gen build <art.txt> -o <out.raba> [options]
    raba-gen check <file.raba>

build options:
    -o, --out <file>      where to write (required)
    --colours <a,b,c>     foregrounds for colour slots $1, $2, ... (default: none)
    --default-fg <n>      colour for art outside any slot (default 7)
    --bg <n>              background colour (default 0)
    --palette <16|256>    which palette the indices mean (default 16)
    --spinner <name>      ascii, dots, blocks, or none (default ascii)
    --spinner-fg <n>      spinner colour (default: --default-fg)
    --spinner-at <c,l>    place it explicitly instead of centred below the art
    --delay <ms>          how long to hold each frame (default 100)
    --no-loop             play once rather than repeating

check options:
    --allow-unsafe-glyphs  report characters a console cannot draw, but succeed
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
        Some("check") => check_command(args.get(1..).unwrap_or(&[])),
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
    let mut recipe = Recipe::default();
    let mut input: Option<&str> = None;
    let mut output: Option<&str> = None;
    let mut spinner_name = "ascii".to_owned();
    let mut spinner_fg: Option<u8> = None;
    let mut spinner_at: Option<(u16, u16)> = None;
    let mut index = 0;

    while let Some(arg) = args.get(index) {
        index = index.saturating_add(1);
        let mut value = || -> Result<&str, String> {
            let got = args.get(index).ok_or_else(|| format!("{arg} needs a value"))?;
            index = index.saturating_add(1);
            Ok(got.as_str())
        };
        match arg.as_str() {
            "-o" | "--out" => output = Some(value()?),
            "--colours" | "--colors" => recipe.colours = parse_list(value()?)?,
            "--default-fg" => recipe.default_fg = parse_u8(value()?)?,
            "--bg" => recipe.bg = parse_u8(value()?)?,
            "--palette" => {
                recipe.palette = match value()? {
                    "16" => Palette::Ansi16,
                    "256" => Palette::Xterm256,
                    other => return Err(format!("--palette: expected 16 or 256, got {other}")),
                }
            }
            "--spinner" => spinner_name = value()?.to_owned(),
            "--spinner-fg" => spinner_fg = Some(parse_u8(value()?)?),
            "--spinner-at" => {
                let pair = parse_list(value()?)?;
                let [column, line] = pair.as_slice() else {
                    return Err("--spinner-at: expected COLUMN,LINE".to_owned());
                };
                spinner_at = Some((u16::from(*column), u16::from(*line)));
            }
            "--delay" => {
                recipe.delay_ms =
                    value()?.parse().map_err(|_| "--delay: expected milliseconds".to_owned())?;
            }
            "--no-loop" => recipe.looping = false,
            other if other.starts_with('-') => {
                return Err(format!("unknown option {other}\n\n{USAGE}"))
            }
            _ => input = Some(arg.as_str()),
        }
    }

    let input = input.ok_or("build needs an art file")?;
    let output = output.ok_or("build needs --out")?;

    recipe.spinner = if spinner_name == "none" {
        None
    } else {
        let fg = spinner_fg.unwrap_or(recipe.default_fg);
        let mut spec = SpinnerSpec::named(&spinner_name, fg).ok_or_else(|| {
            let known: Vec<&str> = spinners::ALL.iter().map(|(name, _)| *name).collect();
            format!("unknown spinner {spinner_name}; try one of {} or none", known.join(", "))
        })?;
        spec.at = spinner_at;
        Some(spec)
    };

    let source = std::fs::read_to_string(input).map_err(|e| format!("{input}: {e}"))?;
    let built = build(&source, &recipe).map_err(|e| format!("{input}: {e}"))?;

    for slot in &built.slots_used {
        if *slot != 0 && usize::from(*slot) > recipe.colours.len() {
            eprintln!(
                "raba-gen: the art uses ${slot} and --colours gives {}; \
                 it will be drawn in the default",
                recipe.colours.len()
            );
        }
    }
    report_glyphs(&built.movie);

    let bytes = raba_core::encode(&built.movie);
    std::fs::write(output, &bytes).map_err(|e| format!("{output}: {e}"))?;
    println!(
        "{output}: {}x{}, {} frames, {} ops, {} bytes",
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
    let path = path.ok_or("check needs a file")?;

    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let movie = raba_core::decode(&bytes).map_err(|e| format!("{path}: {e}"))?;
    println!(
        "{path}: {}x{}, {} frames, {} ops, {}, {}",
        movie.columns,
        movie.lines,
        movie.frames.len(),
        movie.ops.len(),
        if movie.looping { "loops" } else { "plays once" },
        match movie.palette {
            Palette::Ansi16 => "16 colours",
            Palette::Xterm256 => "256 colours",
        }
    );

    let unsafe_glyphs = report_glyphs(&movie);
    if unsafe_glyphs > 0 && !allow_unsafe {
        return Err(format!(
            "{unsafe_glyphs} character(s) a console cannot draw; \
             pass --allow-unsafe-glyphs to accept them"
        ));
    }
    Ok(())
}

/// Print the glyph audit and return how many characters are outright unsafe.
fn report_glyphs(movie: &raba_core::Movie) -> usize {
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

fn parse_u8(value: &str) -> Result<u8, String> {
    value.parse().map_err(|_| format!("{value} is not a colour index (0-255)"))
}

fn parse_list(value: &str) -> Result<Vec<u8>, String> {
    value.split(',').map(str::trim).filter(|s| !s.is_empty()).map(parse_u8).collect()
}
