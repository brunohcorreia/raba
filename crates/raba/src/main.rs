//! The boot splash, and the preview that proves it before there is a boot to try it on.
//!
//! # Exit codes
//!
//! `--preview` is a tool someone is watching, so a bad file is an error and exit 1.
//! The boot path is the opposite: it exits 0 on everything, because the alternative
//! to a splash is no splash, and the alternative to a boot is nothing at all. See
//! `docs/security.md`.

mod preview;
mod surface;

use raba_core::format::limits;
use raba_core::glyph::{audit, Safety};
use raba_core::surface::Surface;
use std::process::ExitCode;

const USAGE: &str = "\
raba - ASCII boot splash

usage:
    raba --preview <file.raba> [--loops <n>]

options:
    --preview <file>   play a .raba on this terminal
    --loops <n>        how many times to play it; 0 is forever (default 3)
    --version          print the version and the format version
    --help             print this
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("raba: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let mut file: Option<&str> = None;
    let mut loops: u32 = 3;
    let mut index = 0;

    while let Some(arg) = args.get(index) {
        index = index.saturating_add(1);
        match arg.as_str() {
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(());
            }
            "--version" => {
                println!(
                    "raba {} (.raba format v{})",
                    env!("CARGO_PKG_VERSION"),
                    raba_core::format::VERSION
                );
                return Ok(());
            }
            "--preview" => {
                let value = args.get(index).ok_or("--preview needs a file")?;
                index = index.saturating_add(1);
                file = Some(value);
            }
            "--loops" => {
                let value = args.get(index).ok_or("--loops needs a number")?;
                index = index.saturating_add(1);
                loops = value.parse().map_err(|_| format!("--loops: {value} is not a number"))?;
            }
            other => return Err(format!("unknown option {other}\n\n{USAGE}")),
        }
    }

    let Some(file) = file else {
        print!("{USAGE}");
        return Ok(());
    };
    preview(file, loops)
}

fn preview(path: &str, loops: u32) -> Result<(), String> {
    let bytes = read_bounded(path)?;
    let movie = raba_core::decode(&bytes).map_err(|e| format!("{path}: {e}"))?;

    // Report glyphs that will not survive a console, but play anyway: this is a
    // preview on a capable terminal, and seeing the problem is the point.
    for finding in audit(&movie) {
        let level = match finding.safety {
            Safety::Unsafe => "will not draw on a console",
            _ => "may not draw after setfont",
        };
        eprintln!("raba: {:?} ({} cells) {level}; see docs/glyphs.md", finding.ch, finding.count);
    }

    let mut surface = surface::StdoutSurface::new();
    let size = surface.size();
    if movie.columns > size.columns || movie.lines > size.lines {
        eprintln!(
            "raba: the theme is {}x{} and this terminal looks {}x{}; it will be clipped",
            movie.columns, movie.lines, size.columns, size.lines
        );
    }

    let _ = preview::clear(&mut surface);
    let result = preview::play(&mut surface, &movie, loops);
    let _ = surface.restore();
    result.map_err(|e| format!("writing to the terminal: {e}"))
}

/// Read a file, refusing one too large to be a theme before allocating for it.
///
/// The same discipline as the decoder, one layer earlier: `read` on a 4 GiB file
/// would allocate 4 GiB before the format ever got a say.
fn read_bounded(path: &str) -> Result<Vec<u8>, String> {
    let metadata = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?;
    if metadata.len() > u64::from(limits::MAX_FILE_SIZE) {
        return Err(format!(
            "{path}: {} bytes, past the {} a .raba may be",
            metadata.len(),
            limits::MAX_FILE_SIZE
        ));
    }
    std::fs::read(path).map_err(|e| format!("{path}: {e}"))
}
