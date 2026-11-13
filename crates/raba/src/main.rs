//! The boot splash, and the preview that proves it before there is a boot to try it on.
//!
//! # Exit codes
//!
//! `--preview` is a tool someone is watching, so a bad file is an error and exit 1.
//! The boot path is the opposite: it exits 0 on everything, because the alternative
//! to a splash is no splash, and the alternative to a boot is nothing at all. See
//! `docs/security.md`.

use raba_core::format::limits;
use raba_core::glyph::{audit, Safety};
use raba_core::surface::Surface;
use std::process::ExitCode;

const USAGE: &str = "\
raba - ASCII boot splash

usage:
    raba --theme <file.raba> [options]      draw on a virtual terminal
    raba --preview <file.raba> [--loops n]  draw on this terminal

preview:
    --preview <file>       play a .raba on this terminal
    --loops <n>            how many times to play it; 0 is forever (default 3)

boot:
    --theme <file>         the animation to loop while the system boots
    --outro <file>         played once on quit; absent means no outro
    --show-delay <secs>    wait this long before drawing anything (default 0)
    --timeout <secs>       reveal the console after this long; 0 disables
                           (default 30)
    --max-duration <secs>  hard ceiling on the outro (default 1.5)
    --socket <path>        control socket (default /run/raba.sock)
    --state <path>         resume file (default /run/raba.state)

    --version              print the version and the format version
    --help                 print this

the boot options are named after raba.conf(5) and will be read from it once
there is a configuration parser; until then they are given here.
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

/// Seconds, as `raba.conf`(5) spells them: a number, possibly fractional.
///
/// `Duration::from_secs_f64` panics on a negative or a NaN, and this is a
/// number from a command line — so the fallible spelling, and the error goes to
/// the user rather than to a backtrace.
fn seconds(value: &str, what: &str) -> Result<std::time::Duration, String> {
    let parsed: f64 = value.parse().map_err(|_| format!("{what}: {value} is not a number"))?;
    std::time::Duration::try_from_secs_f64(parsed)
        .map_err(|_| format!("{what}: {value} is not a length of time"))
}

fn run(args: &[String]) -> Result<(), String> {
    let mut file: Option<&str> = None;
    let mut loops: u32 = 3;
    let mut settings = raba::boot::Settings::default();
    let mut theme: Option<&str> = None;
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
            "--theme" => {
                let value = args.get(index).ok_or("--theme needs a file")?;
                index = index.saturating_add(1);
                theme = Some(value);
            }
            "--outro" => {
                let value = args.get(index).ok_or("--outro needs a file")?;
                index = index.saturating_add(1);
                settings.outro = Some(value.into());
            }
            "--show-delay" => {
                let value = args.get(index).ok_or("--show-delay needs a number")?;
                index = index.saturating_add(1);
                settings.show_delay = seconds(value, "--show-delay")?;
            }
            "--timeout" => {
                let value = args.get(index).ok_or("--timeout needs a number")?;
                index = index.saturating_add(1);
                let length = seconds(value, "--timeout")?;
                // Zero disables it, which `raba.conf`(5) documents and does not
                // recommend: a boot that stalls then shows nothing for ever.
                settings.timeout = (!length.is_zero()).then_some(length);
            }
            "--max-duration" => {
                let value = args.get(index).ok_or("--max-duration needs a number")?;
                index = index.saturating_add(1);
                settings.max_duration = seconds(value, "--max-duration")?;
            }
            "--socket" => {
                let value = args.get(index).ok_or("--socket needs a path")?;
                index = index.saturating_add(1);
                settings.socket = value.into();
                settings.places.socket = value.into();
            }
            "--state" => {
                let value = args.get(index).ok_or("--state needs a path")?;
                index = index.saturating_add(1);
                settings.state = value.into();
            }
            other => return Err(format!("unknown option {other}\n\n{USAGE}")),
        }
    }

    if let Some(theme) = theme {
        if file.is_some() {
            return Err("--preview and --theme are different programs".to_owned());
        }
        settings.theme = theme.into();
        boot(&settings);
        return Ok(());
    }
    let Some(file) = file else {
        print!("{USAGE}");
        return Ok(());
    };
    preview(file, loops)
}

/// The boot path, which exits 0 whatever happened.
///
/// Every outcome here is a reason not to draw, and none of them is worth
/// stopping a boot over: the alternative to a splash is no splash, and the
/// alternative to a boot is nothing at all. The reason goes to stderr, which
/// early in boot is the console log. See `man 8 raba` on exit status.
fn boot(settings: &raba::boot::Settings) {
    let outcome = raba::boot::run(settings);
    if !matches!(outcome, raba::boot::Outcome::Finished) {
        eprintln!("raba: {outcome}");
    }
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

    let mut surface = raba_term::StdoutSurface::new();
    let size = surface.size();
    if movie.columns > size.columns || movie.lines > size.lines {
        eprintln!(
            "raba: the theme is {}x{} and this terminal looks {}x{}; it will be clipped",
            movie.columns, movie.lines, size.columns, size.lines
        );
    }

    let _ = raba_term::clear(&mut surface);
    let result = raba_term::play(&mut surface, &movie, loops);
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
