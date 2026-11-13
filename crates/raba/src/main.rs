//! The boot splash, and the preview that proves it before there is a boot to try it on.
//!
//! # Three programs in one binary
//!
//! `--preview` is a tool someone is watching, so a bad file is an error and exit 1.
//! `--check` is the same: it is a question, and the answer is the exit code. The boot
//! path is the opposite — it exits 0 on everything, because the alternative to a splash
//! is no splash, and the alternative to a boot is nothing at all. See `docs/security.md`.
//!
//! # Where the settings come from
//!
//! `raba.conf`(5), via [`raba::config`], which layers the distribution's defaults under
//! the user's file under the kernel command line. The flags here sit on top of all of
//! it and exist for two callers: a test that has to point every path somewhere else,
//! and a person trying something without editing `/etc` and rebuilding an initramfs.

use raba::config;
use raba::report::report;
use raba_core::format::limits;
use raba_core::glyph::{audit, Safety};
use raba_core::surface::Surface;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str = "\
raba - ASCII boot splash

usage:
    raba [options]                          draw on a virtual terminal
    raba --preview <file.raba> [--loops n]  draw on this terminal
    raba --check                            validate, draw nothing, report

the boot settings come from raba.conf(5) and the kernel command line; the
options below override them and are for testing and for trying something
without rebuilding an initramfs.

preview:
    --preview <file>       play a .raba on this terminal
    --loops <n>            how many times to play it; 0 is forever (default 3)

boot:
    --theme <file>         the animation to loop while the system boots
    --outro <file>         played once on quit
    --tty <n|auto>         Boot.Tty: the terminal to draw on
    --show-delay <secs>    wait this long before drawing anything
    --timeout <secs>       reveal the console after this long; 0 disables
    --max-duration <secs>  hard ceiling on the outro

paths (testing):
    --config <file>        instead of /etc/raba/raba.conf
    --defaults <file>      instead of /usr/share/raba/raba.defaults
    --themes <dir>         instead of /usr/share/raba/themes
    --cmdline <file>       instead of /proc/cmdline
    --os-release <file>    instead of /etc/os-release
    --socket <path>        control socket (default /run/raba.sock)
    --state <path>         resume file (default /run/raba.state)

    --version              print the version and the format version
    --help                 print this
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(message) => {
            report(&message);
            ExitCode::FAILURE
        }
    }
}

/// Everything the command line can say. Nothing is decided here: an override is
/// `None` until somebody gives it, because "the flag was not given" and "the
/// flag was given the same value the file has" have to stay different.
#[derive(Default)]
struct Options {
    preview: Option<String>,
    loops: Option<u32>,
    check: bool,
    places: Places,
    theme: Option<PathBuf>,
    outro: Option<PathBuf>,
    tty: Option<config::Tty>,
    show_delay: Option<Duration>,
    timeout: Option<Option<Duration>>,
    max_duration: Option<Duration>,
    socket: Option<PathBuf>,
    state: Option<PathBuf>,
}

/// [`config::Places`], but every field optional, for the same reason.
#[derive(Default)]
struct Places {
    defaults: Option<PathBuf>,
    config: Option<PathBuf>,
    themes: Option<PathBuf>,
    cmdline: Option<PathBuf>,
    os_release: Option<PathBuf>,
}

impl Places {
    fn resolve(&self) -> config::Places {
        let standard = config::Places::default();
        config::Places {
            defaults: self.defaults.clone().unwrap_or(standard.defaults),
            config: self.config.clone().unwrap_or(standard.config),
            themes: self.themes.clone().unwrap_or(standard.themes),
            cmdline: self.cmdline.clone().unwrap_or(standard.cmdline),
            os_release: self.os_release.clone().unwrap_or(standard.os_release),
        }
    }
}

/// Seconds on a command line, with the complaint aimed at the flag that carried
/// them. The parsing itself is `raba.conf`(5)'s, so that `--timeout 1.5` and
/// `Timeout=1.5` cannot come to mean different things.
fn seconds(value: &str, what: &str) -> Result<Duration, String> {
    config::seconds(value).map_err(|why| format!("{what}: {why}"))
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let mut options = Options::default();
    let mut index = 0;

    // A flag and its value, so that the twenty arms below are one line each.
    macro_rules! value {
        ($flag:literal, $want:literal) => {{
            let value = args.get(index).ok_or(concat!($flag, " needs ", $want))?;
            index = index.saturating_add(1);
            value.as_str()
        }};
    }

    while let Some(arg) = args.get(index) {
        index = index.saturating_add(1);
        match arg.as_str() {
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(ExitCode::SUCCESS);
            }
            "--version" => {
                println!(
                    "raba {} (.raba format v{})",
                    env!("CARGO_PKG_VERSION"),
                    raba_core::format::VERSION
                );
                return Ok(ExitCode::SUCCESS);
            }
            "--check" => options.check = true,
            "--preview" => options.preview = Some(value!("--preview", "a file").to_owned()),
            "--loops" => {
                let value = value!("--loops", "a number");
                let count =
                    value.parse().map_err(|_| format!("--loops: {value} is not a number"))?;
                options.loops = Some(count);
            }
            "--theme" => options.theme = Some(PathBuf::from(value!("--theme", "a file"))),
            "--outro" => options.outro = Some(PathBuf::from(value!("--outro", "a file"))),
            "--tty" => {
                let value = value!("--tty", "a terminal number");
                options.tty =
                    Some(config::parse_tty(value).map_err(|why| format!("--tty: {why}"))?);
            }
            "--show-delay" => {
                options.show_delay =
                    Some(seconds(value!("--show-delay", "a number"), "--show-delay")?);
            }
            "--timeout" => {
                let length = seconds(value!("--timeout", "a number"), "--timeout")?;
                // Zero disables it, which `raba.conf`(5) documents and does not
                // recommend: a boot that stalls then shows nothing for ever.
                options.timeout = Some((!length.is_zero()).then_some(length));
            }
            "--max-duration" => {
                options.max_duration =
                    Some(seconds(value!("--max-duration", "a number"), "--max-duration")?);
            }
            "--config" => options.places.config = Some(PathBuf::from(value!("--config", "a file"))),
            "--defaults" => {
                options.places.defaults = Some(PathBuf::from(value!("--defaults", "a file")));
            }
            "--themes" => {
                options.places.themes = Some(PathBuf::from(value!("--themes", "a directory")));
            }
            "--cmdline" => {
                options.places.cmdline = Some(PathBuf::from(value!("--cmdline", "a file")));
            }
            "--os-release" => {
                options.places.os_release = Some(PathBuf::from(value!("--os-release", "a file")));
            }
            "--socket" => options.socket = Some(PathBuf::from(value!("--socket", "a path"))),
            "--state" => options.state = Some(PathBuf::from(value!("--state", "a path"))),
            other => return Err(format!("unknown option {other}\n\n{USAGE}")),
        }
    }

    if options.preview.is_some() && options.check {
        return Err("--preview and --check are different questions".to_owned());
    }
    if let Some(file) = options.preview.as_deref() {
        preview(file, options.loops.unwrap_or(3))?;
        return Ok(ExitCode::SUCCESS);
    }
    let startup = startup(&options);
    if options.check {
        return Ok(check(&startup));
    }
    boot(&startup);
    Ok(ExitCode::SUCCESS)
}

/// Read the configuration, then let the command line have the last word.
fn startup(options: &Options) -> config::Startup {
    let mut startup = config::start(&options.places.resolve());
    let settings = &mut startup.settings;
    if let Some(theme) = options.theme.clone() {
        settings.theme = theme;
    }
    if let Some(outro) = options.outro.clone() {
        settings.outro = Some(outro);
    }
    if let Some(tty) = options.tty {
        settings.tty = match tty {
            config::Tty::Auto => None,
            config::Tty::Fixed(number) => Some(number),
        };
    }
    if let Some(delay) = options.show_delay {
        settings.show_delay = delay;
    }
    if let Some(timeout) = options.timeout {
        settings.timeout = timeout;
    }
    if let Some(ceiling) = options.max_duration {
        settings.max_duration = ceiling;
    }
    if let Some(socket) = options.socket.clone() {
        settings.socket = socket.clone();
        // The same path twice because they are two uses of it: the one this
        // instance binds, and the one it connects to in order to find out
        // whether there is already an instance. See `conflict::detect`.
        settings.places.socket = socket;
    }
    if let Some(state) = options.state.clone() {
        settings.state = state;
    }
    startup
}

/// `--check`: the strict reading of the same files the boot path reads loosely.
///
/// Exits non-zero on anything the boot path would have stepped over, which is
/// the whole point of it existing — the lenient parser is safe precisely because
/// there is a strict one you can run while you are sitting at the machine.
fn check(startup: &config::Startup) -> ExitCode {
    print!("{}", startup.report());
    let mut ok = startup.ok();
    if !startup.settings.theme.as_os_str().is_empty() {
        match raba::boot::read_movie(&startup.settings.theme) {
            Ok(movie) => {
                println!("theme: {} frames, {}x{}", movie.frames.len(), movie.columns, movie.lines);
                for finding in audit(&movie) {
                    if finding.safety == Safety::Unsafe {
                        println!(
                            "  {:?} ({} cells) will not draw on a console; see docs/glyphs.md",
                            finding.ch, finding.count
                        );
                        ok = false;
                    }
                }
            }
            Err(why) => {
                println!("theme: {why}");
                ok = false;
            }
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The boot path, which exits 0 whatever happened.
///
/// Every outcome here is a reason not to draw, and none of them is worth
/// stopping a boot over: the alternative to a splash is no splash, and the
/// alternative to a boot is nothing at all. The reason goes to stderr, which
/// early in boot is the console log. See `man 8 raba` on exit status.
fn boot(startup: &config::Startup) {
    complain(startup);
    // The kill switch, and the first thing checked after the configuration is
    // read, because reading it is how the switch is found. Nothing after this
    // line touches a terminal.
    if !startup.enabled {
        return;
    }
    // `raba.debug=1`: say what would have happened, and do none of it. The same
    // report `--check` prints, so that the thing on a screen and the thing in a
    // log cannot drift apart.
    if startup.debug {
        let report = startup.report();
        eprint!("{report}");
        let _ = raba::write::replace(&startup.config.log.file, report.as_bytes());
        return;
    }
    // Caught here rather than in the boot path, because this is the one reason
    // for not drawing that is almost always somebody's *mistake* — a theme name
    // with a typo in it, a themes directory that was never installed, an
    // initramfs built before the art package was. The boot path would report it
    // as the empty path it is ("not drawing: : No such file or directory"),
    // which tells the person nothing at all. Here the list of what was looked
    // for still exists, so the message can be the list.
    if startup.settings.theme.as_os_str().is_empty() {
        // Always a list: `config::candidates` cannot return an empty one, so
        // there is no "no theme is configured" case to handle separately. There
        // was a branch for it, and nothing could reach it.
        let looked: Vec<String> =
            startup.tried.iter().map(|path| path.display().to_string()).collect();
        report(&format!("not drawing: no theme found; looked for {}", looked.join(", ")));
        return;
    }
    let outcome = raba::boot::run(&startup.settings);
    match outcome {
        raba::boot::Outcome::Finished => {}
        // Quiet on the console, on purpose: the message would be printed over
        // whatever just took the screen. Written down all the same — an exit
        // that leaves no trace is an exit nobody can debug, and "the splash
        // stopped early" is exactly the report that needs one.
        ref displaced @ raba::boot::Outcome::Displaced(_) => {
            let line = format!("raba: {displaced}\n");
            let _ = raba::write::replace(&startup.config.log.file, line.as_bytes());
        }
        ref other => report(other),
    }
}

/// Configuration problems, on the console log, bounded.
///
/// A handful of lines is a diagnostic; a hundred is a boot scrolling past
/// something nobody can read anyway. The full list is what `--check` is for, and
/// the last line says so.
fn complain(startup: &config::Startup) {
    const SHOWN: usize = 5;
    for problem in startup.problems.iter().take(SHOWN) {
        report(problem);
    }
    if let Some(more) = startup.problems.len().checked_sub(SHOWN).filter(|more| *more > 0) {
        report(&format!("and {more} more; run `raba --check`"));
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
