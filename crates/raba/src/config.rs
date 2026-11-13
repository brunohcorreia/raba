//! `raba.conf`(5): the file that says what the boot path should do.
//!
//! # Nothing in here is allowed to stop a boot
//!
//! A configuration file is written by hand, lives in `/etc`, and is read by a
//! program that runs before the root filesystem is mounted. Every way of getting
//! it wrong therefore has to end in a working boot: an unknown key, a value that
//! is not a number, a whole section from a newer version, a file that is not
//! UTF-8, a file that is four megabytes. All of them are **recorded and
//! stepped over**, and the setting keeps the value it already had.
//!
//! That is deliberately a lie-friendly rule — a typo in `Theme=` means the
//! splash silently does something other than what the file says — so it is paid
//! for in two places. `raba --check` reads the same files with the same parser
//! and is **strict**: it prints every complaint and exits non-zero, at a moment
//! when the user is at their keyboard. And `raba.debug=1` prints the same report
//! into the log. The parser is lenient; the tool that reports on the parser is
//! not. See `docs/maintenance.md` on exit code 0.
//!
//! # Layers
//!
//! ```text
//! compiled-in defaults      this file, matching raba.conf(5)
//!   /usr/share/raba/raba.defaults   the distribution's, replaced on upgrade
//!     /etc/raba/raba.conf           the user's, never touched by a package
//!       /proc/cmdline               raba.enable, raba.debug, raba.tty
//!         command-line flags        --theme and friends; testing and preview
//! ```
//!
//! Later layers override earlier ones key by key, which is why every layer is
//! applied onto one [`Config`] rather than merged at the end: a key absent from
//! your file keeps the distribution's value, and a key absent from both keeps
//! the one in this file.
//!
//! The kernel command line sits above both files on purpose. It is the layer you
//! can reach from a bootloader on a machine that will not boot, and a kill
//! switch that a file could override would not be one.
//!
//! # What is parsed and what is honoured
//!
//! Everything `raba.conf`(5) documents is parsed and validated. Not all of it is
//! *honoured* — there is no progress indicator yet, and no `Handoff.Mode=clear`.
//! Those keys are in [`NOT_HONOURED`], and a key from that list that appears in
//! your file is reported by `--check`. Accepting a key and quietly ignoring it is
//! how a configuration file stops meaning anything.

use crate::boot::Settings;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The distribution's defaults. Belongs to the package; replaced on upgrade.
pub const DEFAULTS: &str = "/usr/share/raba/raba.defaults";
/// The user's overrides. Never touched by a package manager.
pub const CONFIG: &str = "/etc/raba/raba.conf";
/// Where a bare `Theme=name` is looked up.
pub const THEMES: &str = "/usr/share/raba/themes";
/// Read only to resolve `Theme=auto`. See [`identity`].
pub const OS_RELEASE: &str = "/etc/os-release";
/// The kernel command line.
pub const CMDLINE: &str = "/proc/cmdline";

/// Past this, it is not a configuration file. Checked before reading, so a
/// planted four-gigabyte `raba.conf` costs a `stat` rather than four gigabytes.
pub const LIMIT: u64 = 64 * 1024;

/// The sections `raba.conf`(5) defines. A header that is not one of these is
/// skipped whole, so a file written for a later version loses the section it
/// added and keeps everything else.
const SECTIONS: [&str; 5] = ["Boot", "Outro", "Progress", "Handoff", "Log"];

/// Keys this version parses, validates, and then does nothing with, because the
/// feature behind them does not exist yet.
///
/// Listed here rather than left implicit so that `--check` can tell you, and so
/// that whoever implements one of these features has a list to delete from.
pub const NOT_HONOURED: [&str; 12] = [
    "Boot.FrameRateOverride",
    "Outro.FinishFrame",
    "Progress.Enable",
    "Progress.Style",
    "Progress.Source",
    "Progress.Position",
    "Progress.Width",
    "Progress.Color",
    "Progress.BackgroundColor",
    "Handoff.Mode",
    "Handoff.RestoreTty",
    "Log.Level",
];

// ---------------------------------------------------------------- values ----

/// `Theme=`, in both sections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Theme {
    /// Resolved from `os-release`(5) — normally at initramfs build time, and
    /// here only when the root filesystem happens to be available.
    Auto,
    /// A bare name, looked up under [`THEMES`].
    Named(String),
    /// An absolute path, used as given.
    Path(PathBuf),
}

impl Theme {
    /// # Errors
    ///
    /// A name that could escape the theme directory, or an empty value. A name
    /// is pasted onto a directory and opened as root, so it gets the same
    /// treatment as any other untrusted path component even though the file it
    /// comes from is root-owned: `Theme=../../etc/shadow` should be a complaint
    /// and not a read.
    pub fn parse(value: &str) -> Result<Theme, String> {
        if value == "auto" {
            return Ok(Theme::Auto);
        }
        if value.starts_with('/') {
            return Ok(Theme::Path(PathBuf::from(value)));
        }
        let shaped = !value.is_empty()
            && value.len() <= 64
            && !value.starts_with('.')
            && value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'));
        if shaped {
            Ok(Theme::Named(value.to_owned()))
        } else {
            Err(format!("{value:?} is not a theme name, a path, or auto"))
        }
    }
}

impl std::fmt::Display for Theme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => write!(f, "auto"),
            Self::Named(name) => write!(f, "{name}"),
            Self::Path(path) => write!(f, "{}", path.display()),
        }
    }
}

/// `Boot.Tty`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tty {
    /// Ask the kernel for a free terminal — `VT_OPENQRY`. The default, and the
    /// only setting that cannot land on top of something already in use.
    Auto,
    /// `/dev/tty<n>`, whatever is on it.
    Fixed(u16),
}

/// A word with a fixed set of spellings: `Progress.Style` and its neighbours.
macro_rules! word {
    ($name:ident, $want:literal, $($variant:ident => $spelling:literal),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name { $($variant),+ }

        impl $name {
            fn parse(value: &str) -> Result<$name, String> {
                match value {
                    $($spelling => Ok($name::$variant),)+
                    other => Err(format!("{other:?} is not {}", $want)),
                }
            }

            /// As it is spelled in the file.
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self { $($name::$variant => $spelling),+ }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.name())
            }
        }
    };
}

word!(Style, "pulse, bar or percent", Pulse => "pulse", Bar => "bar", Percent => "percent");
word!(Source, "ctl, time or none", Ctl => "ctl", Time => "time", None => "none");
word!(Position, "above or below", Above => "above", Below => "below");
word!(Mode, "retain or clear", Retain => "retain", Clear => "clear");
word!(
    Level,
    "error, warn, info or debug",
    Error => "error",
    Warn => "warn",
    Info => "info",
    Debug => "debug",
);

/// `true`, and every other way people write it.
///
/// Generous on input because a configuration file is written by a person and
/// `Enable=yes` meaning nothing would be a worse outcome than accepting it.
fn boolean(value: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Ok(true),
        "false" | "no" | "off" | "0" => Ok(false),
        other => Err(format!("{other:?} is not true or false")),
    }
}

/// Seconds, as `raba.conf`(5) spells them: a number, possibly fractional.
///
/// `Duration::from_secs_f64` panics on a negative or a NaN, and this is a number
/// out of a file. The fallible spelling, and the complaint goes to the user.
///
/// # Errors
///
/// Anything that is not a non-negative, finite count of seconds.
pub fn seconds(value: &str) -> Result<Duration, String> {
    let parsed: f64 = value.parse().map_err(|_| format!("{value:?} is not a number"))?;
    Duration::try_from_secs_f64(parsed).map_err(|_| format!("{value:?} is not a length of time"))
}

fn whole<T: std::str::FromStr>(value: &str, want: &str) -> Result<T, String> {
    value.parse().map_err(|_| format!("{value:?} is not {want}"))
}

// --------------------------------------------------------------- sections ---

/// `[Boot]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Boot {
    pub theme: Theme,
    pub tty: Tty,
    pub show_delay: Duration,
    /// `None` is `Timeout=0`: disabled, and not recommended.
    pub timeout: Option<Duration>,
    /// `None` is `FrameRateOverride=0`: use the theme's own rate.
    pub frame_rate: Option<f64>,
}

/// `[Outro]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Outro {
    pub enable: bool,
    pub theme: Theme,
    pub max_duration: Duration,
    pub finish_frame: bool,
}

/// `[Progress]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub enable: bool,
    pub style: Style,
    pub source: Source,
    pub position: Position,
    /// `0` matches the animation.
    pub width: u16,
    pub color: u8,
    pub background: u8,
}

/// `[Handoff]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Handoff {
    pub mode: Mode,
    pub restore_tty: bool,
}

/// `[Log]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Log {
    pub level: Level,
    pub file: PathBuf,
}

/// Everything `raba.conf`(5) defines, with the defaults it documents.
#[derive(Debug, Clone)]
pub struct Config {
    pub boot: Boot,
    pub outro: Outro,
    pub progress: Progress,
    pub handoff: Handoff,
    pub log: Log,
    /// Canonical `Section.Key` names that a file actually set, in the order they
    /// were first seen. What makes it possible to say "you set this and it does
    /// nothing" rather than "this does nothing", which nobody needs to be told.
    seen: Vec<&'static str>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            boot: Boot {
                theme: Theme::Auto,
                tty: Tty::Auto,
                show_delay: Duration::ZERO,
                timeout: Some(Duration::from_secs(30)),
                frame_rate: None,
            },
            outro: Outro {
                enable: true,
                theme: Theme::Auto,
                max_duration: Duration::from_millis(1500),
                finish_frame: true,
            },
            progress: Progress {
                enable: false,
                style: Style::Pulse,
                source: Source::Ctl,
                position: Position::Below,
                width: 40,
                color: 7,
                background: 8,
            },
            handoff: Handoff { mode: Mode::Retain, restore_tty: true },
            log: Log { level: Level::Warn, file: PathBuf::from("/run/raba.log") },
            seen: Vec::new(),
        }
    }
}

impl Config {
    /// Keys this file set that this version does not act on.
    #[must_use]
    pub fn unhonoured(&self) -> Vec<&'static str> {
        self.seen.iter().copied().filter(|key| NOT_HONOURED.contains(key)).collect()
    }

    /// Was this key set by a file, as opposed to defaulted?
    #[must_use]
    pub fn was_set(&self, key: &str) -> bool {
        self.seen.contains(&key)
    }
}

// ---------------------------------------------------------------- parsing ---

/// Something wrong with a line, and where.
///
/// A complaint rather than an error: the parser carries on, and these are what
/// `--check` prints. Line `0` means the file as a whole.
#[derive(Debug, Clone)]
pub struct Problem {
    pub file: PathBuf,
    pub line: usize,
    pub text: String,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let file = self.file.display();
        if self.line == 0 {
            write!(f, "{file}: {}", self.text)
        } else {
            write!(f, "{file}:{}: {}", self.line, self.text)
        }
    }
}

/// Which section the parser is in.
enum Within {
    /// Before any header. A key here has nowhere to belong.
    Nowhere,
    /// A header this version does not know. Its keys are skipped without
    /// further complaint — one line about the section is the useful message,
    /// and twenty about its keys is noise.
    Unknown,
    In(&'static str),
}

/// What one line of a configuration file turned out to be.
///
/// The parser's view, and the only one: anything that wants to know what a file
/// says — reading it, or editing it — goes through [`scan`] rather than
/// splitting lines again. A second reader that disagrees with this one about
/// what counts as the `[Boot]` section is a tool that edits a line nobody
/// obeys, which is worse than a tool that refuses.
enum Step<'a> {
    /// `[Name]`, and whether the bracket was closed.
    Header { name: &'a str, closed: bool },
    /// `Key=Value`, both already trimmed.
    Pair { key: &'a str, value: &'a str },
    /// Something that is none of the above.
    Junk,
}

/// Every meaningful line, with its 1-based number and its trimmed text.
///
/// Blank lines and whole-line comments are not yielded at all — they are the
/// one thing every caller agrees to ignore. A `#` *inside* a value is part of
/// the value: paths and messages contain them, and a parser that ate the rest
/// of the line would turn `Theme=/opt/#1.raba` into a file that does not exist.
fn scan(text: &str) -> impl Iterator<Item = (usize, &str, Step<'_>)> {
    text.lines().enumerate().filter_map(|(index, raw)| {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let number = index.saturating_add(1);
        if let Some(rest) = line.strip_prefix('[') {
            return Some(match rest.strip_suffix(']') {
                Some(name) => (number, line, Step::Header { name: name.trim(), closed: true }),
                None => (number, line, Step::Header { name: "", closed: false }),
            });
        }
        Some(match line.split_once('=') {
            Some((key, value)) => {
                (number, line, Step::Pair { key: key.trim(), value: value.trim() })
            }
            None => (number, line, Step::Junk),
        })
    })
}

/// Apply one file's text onto a configuration, returning what was wrong with it.
///
/// Pure: no file is read here and no path is touched. `file` is carried only so
/// that a [`Problem`] can say where it came from.
#[must_use]
pub fn apply(text: &str, file: &Path, config: &mut Config) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut within = Within::Nowhere;
    let mut complain = |line: usize, text: String| {
        problems.push(Problem { file: file.to_path_buf(), line, text });
    };

    for (number, line, step) in scan(text) {
        match step {
            Step::Header { closed: false, .. } => {
                complain(number, format!("{line:?} starts a section and does not finish it"));
                within = Within::Unknown;
            }
            Step::Header { name, closed: true } => {
                within = match SECTIONS.iter().find(|known| **known == name) {
                    Some(known) => Within::In(known),
                    None => {
                        complain(number, format!("[{name}] is not a section this version knows"));
                        Within::Unknown
                    }
                };
            }
            Step::Junk => {
                complain(number, format!("{line:?} is not a comment, a section, or Key=Value"));
            }
            Step::Pair { key, value } => {
                let section = match within {
                    Within::In(section) => section,
                    Within::Unknown => continue,
                    Within::Nowhere => {
                        complain(number, format!("{key} is before any [Section]"));
                        continue;
                    }
                };
                match set(config, section, key, value) {
                    Ok(name) => {
                        if !config.seen.contains(&name) {
                            config.seen.push(name);
                        }
                    }
                    Err(why) => complain(number, why),
                }
            }
        }
    }
    problems
}

/// Where a file sets one key, as [`apply`] sees it.
///
/// Exists so that an editor — `raba-setup --set-theme` — can change the line
/// that is actually obeyed instead of the first one that looks right. The three
/// ways those differ are all reachable by hand:
///
/// * **Two lines set the key.** `apply` runs through the whole file and the last
///   assignment wins. An editor that stops at the first one reports success and
///   changes nothing.
/// * **The section is spelled `[boot]`.** `SECTIONS` is matched exactly, so that
///   is an unknown section and every key in it is skipped. Editing a line there
///   edits something no boot will read.
/// * **The header is unterminated.** `[Boot` leaves the parser in
///   [`Within::Unknown`], and so everything under it is skipped too.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Placement {
    /// 1-based line numbers of every line that sets it, in file order. The last
    /// is the one that decides the value.
    pub lines: Vec<usize>,
    /// 1-based line number of the `[Section]` header, if the file has one that
    /// the parser accepts.
    pub header: Option<usize>,
}

impl Placement {
    /// The line that decides the value, if any does.
    #[must_use]
    pub fn effective(&self) -> Option<usize> {
        self.lines.last().copied()
    }
}

/// Find where `section.key` is set. Pure; the same rules as [`apply`].
#[must_use]
pub fn placement(text: &str, section: &str, key: &str) -> Placement {
    let mut found = Placement::default();
    let mut within = Within::Nowhere;
    for (number, _, step) in scan(text) {
        match step {
            Step::Header { closed: false, .. } => within = Within::Unknown,
            Step::Header { name, closed: true } => {
                within = match SECTIONS.iter().find(|known| **known == name) {
                    Some(known) => {
                        if *known == section && found.header.is_none() {
                            found.header = Some(number);
                        }
                        Within::In(known)
                    }
                    None => Within::Unknown,
                };
            }
            Step::Junk => {}
            Step::Pair { key: found_key, .. } => {
                if matches!(within, Within::In(name) if name == section) && found_key == key {
                    found.lines.push(number);
                }
            }
        }
    }
    found
}

/// One key. Returns its canonical name, so that the caller can record that a
/// file set it without building the name back out of two strings.
#[allow(clippy::too_many_lines)]
fn set(config: &mut Config, section: &str, key: &str, value: &str) -> Result<&'static str, String> {
    let fault = |why: String| format!("{section}.{key}: {why}");
    Ok(match (section, key) {
        ("Boot", "Theme") => {
            config.boot.theme = Theme::parse(value).map_err(fault)?;
            "Boot.Theme"
        }
        ("Boot", "Tty") => {
            config.boot.tty = parse_tty(value).map_err(fault)?;
            "Boot.Tty"
        }
        ("Boot", "ShowDelay") => {
            config.boot.show_delay = seconds(value).map_err(fault)?;
            "Boot.ShowDelay"
        }
        ("Boot", "Timeout") => {
            let length = seconds(value).map_err(fault)?;
            // Zero disables it, which `raba.conf`(5) documents and does not
            // recommend: a boot that stalls then shows nothing for ever.
            config.boot.timeout = (!length.is_zero()).then_some(length);
            "Boot.Timeout"
        }
        ("Boot", "FrameRateOverride") => {
            let rate: f64 = whole(value, "a frame rate").map_err(fault)?;
            if !rate.is_finite() || rate < 0.0 || rate > 1000.0 {
                return Err(fault(format!("{value:?} is not a frame rate")));
            }
            config.boot.frame_rate = (rate > 0.0).then_some(rate);
            "Boot.FrameRateOverride"
        }
        ("Outro", "Enable") => {
            config.outro.enable = boolean(value).map_err(fault)?;
            "Outro.Enable"
        }
        ("Outro", "Theme") => {
            config.outro.theme = Theme::parse(value).map_err(fault)?;
            "Outro.Theme"
        }
        ("Outro", "MaxDuration") => {
            config.outro.max_duration = seconds(value).map_err(fault)?;
            "Outro.MaxDuration"
        }
        ("Outro", "FinishFrame") => {
            config.outro.finish_frame = boolean(value).map_err(fault)?;
            "Outro.FinishFrame"
        }
        ("Progress", "Enable") => {
            config.progress.enable = boolean(value).map_err(fault)?;
            "Progress.Enable"
        }
        ("Progress", "Style") => {
            config.progress.style = Style::parse(value).map_err(fault)?;
            "Progress.Style"
        }
        ("Progress", "Source") => {
            config.progress.source = Source::parse(value).map_err(fault)?;
            "Progress.Source"
        }
        ("Progress", "Position") => {
            config.progress.position = Position::parse(value).map_err(fault)?;
            "Progress.Position"
        }
        ("Progress", "Width") => {
            config.progress.width = whole(value, "a width in cells").map_err(fault)?;
            "Progress.Width"
        }
        ("Progress", "Color") => {
            config.progress.color = whole(value, "a palette index").map_err(fault)?;
            "Progress.Color"
        }
        ("Progress", "BackgroundColor") => {
            config.progress.background = whole(value, "a palette index").map_err(fault)?;
            "Progress.BackgroundColor"
        }
        ("Handoff", "Mode") => {
            config.handoff.mode = Mode::parse(value).map_err(fault)?;
            "Handoff.Mode"
        }
        ("Handoff", "RestoreTty") => {
            config.handoff.restore_tty = boolean(value).map_err(fault)?;
            "Handoff.RestoreTty"
        }
        ("Log", "Level") => {
            config.log.level = Level::parse(value).map_err(fault)?;
            "Log.Level"
        }
        ("Log", "File") => {
            if !value.starts_with('/') {
                return Err(fault(format!("{value:?} is not an absolute path")));
            }
            config.log.file = PathBuf::from(value);
            "Log.File"
        }
        _ => return Err(format!("{key} is not a key in [{section}]")),
    })
}

/// `Boot.Tty`, and `--tty`, and `raba.tty=`. One spelling for all three.
///
/// # Errors
///
/// A number the kernel cannot have a terminal for, or a word that is not `auto`.
pub fn parse_tty(value: &str) -> Result<Tty, String> {
    if value == "auto" {
        return Ok(Tty::Auto);
    }
    let number: u16 = whole(value, "a terminal number")?;
    if number == 0 || number > crate::state::MAX_TERMINAL {
        return Err(format!("tty{number} is outside 1..={}", crate::state::MAX_TERMINAL));
    }
    Ok(Tty::Fixed(number))
}

// --------------------------------------------------------- the kernel line --

/// What the bootloader had to say. Every field is `None` when it said nothing.
#[derive(Debug, Default, Clone)]
pub struct Cmdline {
    /// `raba.enable=0` — the kill switch, and the reason this layer sits above
    /// both files.
    pub enable: Option<bool>,
    /// `raba.debug=1` — report and draw nothing.
    pub debug: Option<bool>,
    /// `raba.tty=n`.
    pub tty: Option<u16>,
    pub problems: Vec<String>,
}

/// Read `raba.*` out of a kernel command line.
///
/// Everything that is not ours is ignored, and a `raba.` parameter that is *not*
/// one of ours is a complaint: somebody who typed `raba.enabled=0` at a
/// bootloader prompt on a machine that will not boot is owed the news that it
/// did nothing.
#[must_use]
pub fn cmdline(text: &str) -> Cmdline {
    let mut found = Cmdline::default();
    for word in text.split_ascii_whitespace() {
        let Some(parameter) = word.strip_prefix("raba.") else {
            continue;
        };
        let Some((key, value)) = parameter.split_once('=') else {
            found.problems.push(format!("raba.{parameter} needs a value"));
            continue;
        };
        // The kernel lets a value be quoted; what reaches us is the quotes.
        let value = value.trim_matches('"');
        match key {
            "enable" => match boolean(value) {
                Ok(yes) => found.enable = Some(yes),
                Err(why) => found.problems.push(format!("raba.enable: {why}")),
            },
            "debug" => match boolean(value) {
                Ok(yes) => found.debug = Some(yes),
                Err(why) => found.problems.push(format!("raba.debug: {why}")),
            },
            "tty" => match parse_tty(value) {
                Ok(Tty::Fixed(number)) => found.tty = Some(number),
                Ok(Tty::Auto) => found.tty = None,
                Err(why) => found.problems.push(format!("raba.tty: {why}")),
            },
            other => found.problems.push(format!("raba.{other} is not a parameter raba knows")),
        }
    }
    found
}

// -------------------------------------------------------------- resolving ---

/// The distribution's own names, most specific first, from `os-release`(5).
///
/// `ID` then every word of `ID_LIKE`. The point of `ID_LIKE` is exactly this
/// case: a derivative with no art of its own gets its parent's.
#[must_use]
pub fn identity(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut like = Vec::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim_matches('\'');
        match key.trim() {
            "ID" => names.push(value.to_owned()),
            "ID_LIKE" => {
                like.extend(value.split_ascii_whitespace().map(ToOwned::to_owned));
            }
            _ => {}
        }
    }
    names.extend(like);
    names.retain(|name| Theme::parse(name).is_ok() && name != "auto");
    names
}

/// Every file a theme setting could mean, in the order to try them.
///
/// Pure, and returning the whole list rather than the first hit, so that a
/// theme that resolves to nothing can say what it looked for. "No theme" is the
/// commonest way for a splash to do nothing, and the list is the answer to why.
#[must_use]
pub fn candidates(theme: &Theme, themes: &Path, identity: &[String]) -> Vec<PathBuf> {
    match theme {
        Theme::Path(path) => vec![path.clone()],
        Theme::Named(name) => vec![themes.join(format!("{name}.raba"))],
        Theme::Auto => identity
            .iter()
            .map(String::as_str)
            .chain(std::iter::once("generic"))
            .map(|name| themes.join(format!("{name}.raba")))
            .collect(),
    }
}

/// The same, for `Outro.Theme`.
///
/// `auto` means the boot theme's name with `-outro` appended — next to the boot
/// theme, wherever that turned out to be, so that a theme given as a path has
/// its outro looked for beside it rather than in the system directory.
#[must_use]
pub fn outro_candidates(theme: &Theme, themes: &Path, boot: Option<&Path>) -> Vec<PathBuf> {
    match theme {
        Theme::Auto => {
            let Some(boot) = boot else {
                return Vec::new();
            };
            let Some(stem) = boot.file_stem().and_then(|stem| stem.to_str()) else {
                return Vec::new();
            };
            let beside = boot.parent().unwrap_or(themes);
            vec![beside.join(format!("{stem}-outro.raba"))]
        }
        other => candidates(other, themes, &[]),
    }
}

/// The first candidate that is a file.
#[must_use]
pub fn first_existing(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|path| path.is_file()).cloned()
}

// ----------------------------------------------------------------- start ----

/// Where the configuration lives. Every one of these is a flag, because every
/// one of them has to be pointed somewhere else by a test.
#[derive(Debug, Clone)]
pub struct Places {
    pub defaults: PathBuf,
    pub config: PathBuf,
    pub themes: PathBuf,
    pub cmdline: PathBuf,
    pub os_release: PathBuf,
}

impl Default for Places {
    fn default() -> Self {
        Places {
            defaults: PathBuf::from(DEFAULTS),
            config: PathBuf::from(CONFIG),
            themes: PathBuf::from(THEMES),
            cmdline: PathBuf::from(CMDLINE),
            os_release: PathBuf::from(OS_RELEASE),
        }
    }
}

/// Everything reading the configuration produced: what it says, what it means
/// for the daemon, and everything that was wrong on the way.
#[derive(Debug)]
pub struct Startup {
    pub config: Config,
    pub settings: Settings,
    /// `raba.enable=0` says no.
    pub enabled: bool,
    /// `raba.debug=1`: report and draw nothing.
    pub debug: bool,
    /// Which files were actually read.
    pub files: Vec<PathBuf>,
    /// What was looked for, whether or not it was found. The answer to "why is
    /// there no splash".
    pub tried: Vec<PathBuf>,
    pub outro_tried: Vec<PathBuf>,
    pub problems: Vec<Problem>,
}

/// Read the lot: both files, the kernel command line, and the theme they name.
///
/// The only function here that touches a filesystem. Everything it decides with
/// is a pure function above, which is what makes a configuration parser testable
/// without a `/etc`.
#[must_use]
pub fn start(places: &Places) -> Startup {
    let mut config = Config::default();
    let mut problems = Vec::new();
    let mut files = Vec::new();

    for path in [&places.defaults, &places.config] {
        match read_bounded(path) {
            // Absent is the normal case for both of them: a source build has no
            // `raba.defaults`, and a user who has not overridden anything has no
            // `raba.conf`. Neither is worth a word.
            Ok(None) => {}
            Ok(Some(text)) => {
                files.push(path.clone());
                problems.extend(apply(&text, path, &mut config));
            }
            Err(why) => problems.push(Problem { file: path.clone(), line: 0, text: why }),
        }
    }

    let kernel = match read_bounded(&places.cmdline) {
        Ok(Some(text)) => cmdline(&text),
        _ => Cmdline::default(),
    };
    for text in &kernel.problems {
        problems.push(Problem { file: places.cmdline.clone(), line: 0, text: text.clone() });
    }
    if let Some(number) = kernel.tty {
        config.boot.tty = Tty::Fixed(number);
    }

    // `auto` is normally resolved when the initramfs is built, where `/etc` is
    // mounted and the answer can be frozen into the image. This is the other
    // case: a live system, `--check`, or an initramfs built without the hook.
    let names = match read_bounded(&places.os_release) {
        Ok(Some(text)) => identity(&text),
        _ => Vec::new(),
    };

    let tried = candidates(&config.boot.theme, &places.themes, &names);
    let theme = first_existing(&tried);
    let outro_tried = if config.outro.enable {
        outro_candidates(&config.outro.theme, &places.themes, theme.as_deref())
    } else {
        Vec::new()
    };

    let settings = Settings {
        theme: theme.clone().unwrap_or_default(),
        outro: first_existing(&outro_tried),
        show_delay: config.boot.show_delay,
        timeout: config.boot.timeout,
        max_duration: config.outro.max_duration,
        tty: match config.boot.tty {
            Tty::Auto => None,
            Tty::Fixed(number) => Some(number),
        },
        ..Settings::default()
    };

    Startup {
        config,
        settings,
        enabled: kernel.enable.unwrap_or(true),
        debug: kernel.debug.unwrap_or(false),
        files,
        tried,
        outro_tried,
        problems,
    }
}

/// Read a file that should be small, or report why not.
///
/// `Ok(None)` is "it is not there", which is not a problem anywhere this is
/// called from.
fn read_bounded(path: &Path) -> Result<Option<String>, String> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(why) => return Err(format!("{why}")),
    };
    if metadata.len() > LIMIT {
        return Err(format!(
            "{} bytes, past the {LIMIT} a configuration file may be",
            metadata.len()
        ));
    }
    let bytes = std::fs::read(path).map_err(|why| format!("{why}"))?;
    String::from_utf8(bytes).map(Some).map_err(|_| "not UTF-8".to_owned())
}

// ---------------------------------------------------------------- report ----

impl Startup {
    /// Is there anything to tell the user about?
    ///
    /// The question `--check` exits on. A theme that resolved to nothing counts:
    /// it is the one failure that leaves a working boot and no splash at all,
    /// which is precisely what somebody running `--check` is asking about.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.problems.is_empty() && !self.settings.theme.as_os_str().is_empty()
    }

    /// What was read, what it resolved to, and what was wrong with it.
    ///
    /// One function behind both `--check` and `raba.debug=1`, so that the thing
    /// printed on a screen and the thing written into a log at boot cannot drift
    /// apart.
    #[must_use]
    pub fn report(&self) -> String {
        let mut out = String::new();
        let push = |out: &mut String, line: &str| {
            out.push_str(line);
            out.push('\n');
        };

        push(&mut out, &format!("raba {} configuration", env!("CARGO_PKG_VERSION")));
        push(&mut out, "");

        push(&mut out, "files:");
        if self.files.is_empty() {
            push(&mut out, "  none; every setting is a compiled-in default");
        }
        for file in &self.files {
            push(&mut out, &format!("  {}", file.display()));
        }

        push(&mut out, "boot:");
        push(&mut out, &format!("  Theme         {}", self.config.boot.theme));
        match (self.settings.theme.as_os_str().is_empty(), self.tried.as_slice()) {
            (false, _) => {
                push(&mut out, &format!("  resolved      {}", self.settings.theme.display()))
            }
            (true, []) => push(&mut out, "  resolved      nothing, and nothing was looked for"),
            (true, tried) => {
                push(&mut out, "  resolved      nothing; looked for");
                for path in tried {
                    push(&mut out, &format!("                {}", path.display()));
                }
            }
        }
        push(&mut out, &format!("  Tty           {}", terminal(self.config.boot.tty)));
        push(&mut out, &format!("  ShowDelay     {}", length(self.config.boot.show_delay)));
        push(
            &mut out,
            &format!(
                "  Timeout       {}",
                self.config.boot.timeout.map_or("0 (disabled, not recommended)".to_owned(), length)
            ),
        );

        push(&mut out, "outro:");
        if !self.config.outro.enable {
            push(&mut out, "  Enable        false");
        } else {
            push(&mut out, &format!("  Theme         {}", self.config.outro.theme));
            match &self.settings.outro {
                Some(path) => push(&mut out, &format!("  resolved      {}", path.display())),
                None => push(&mut out, "  resolved      nothing; no outro will play"),
            }
            push(&mut out, &format!("  MaxDuration   {}", length(self.config.outro.max_duration)));
        }

        let unhonoured = self.config.unhonoured();
        if !unhonoured.is_empty() {
            push(&mut out, "set, and not honoured by this version:");
            for key in unhonoured {
                push(&mut out, &format!("  {key}"));
            }
        }

        if !self.enabled {
            push(&mut out, "raba.enable=0 on the kernel command line: nothing will be drawn");
        }

        if self.problems.is_empty() {
            push(&mut out, "problems: none");
        } else {
            push(&mut out, "problems:");
            for problem in &self.problems {
                push(&mut out, &format!("  {problem}"));
            }
        }
        out
    }
}

fn terminal(tty: Tty) -> String {
    match tty {
        Tty::Auto => "auto (a free one, from VT_OPENQRY)".to_owned(),
        Tty::Fixed(number) => format!("/dev/tty{number}"),
    }
}

/// A duration, spelled for a person, without pulling the float formatter into a
/// boot binary.
///
/// `{}` on an `f64` links Grisu *and* Dragon4 and their power-of-ten tables:
/// fifteen kilobytes, measured, which is more than this entire module and three
/// percent of the binary size gate. All it would buy is the way a number is
/// printed in a report nobody sees on most boots. Integers and a decimal point.
fn length(duration: Duration) -> String {
    format!("{}s", decimal(thousandths(duration)))
}

/// Milliseconds, saturating. A duration longer than half a billion years is not
/// a `Timeout`, and the alternative is a cast that lies.
fn thousandths(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Thousandths as a decimal, without a trailing `.000` and without a float.
///
/// `1500` is `1.5`, `30000` is `30`. Shared by the report, where it is read by a
/// person, and by [`Startup::freeze`], where it has to be read back by
/// [`seconds`] — which is why the only spelling it produces is one that parses.
fn decimal(thousandths: u64) -> String {
    let whole = thousandths / 1000;
    let fraction = thousandths % 1000;
    if fraction == 0 {
        return format!("{whole}");
    }
    let digits = format!("{fraction:03}");
    format!("{whole}.{}", digits.trim_end_matches('0'))
}

// ---------------------------------------------------------------- freeze ----

impl Startup {
    /// The whole effective configuration as one self-contained file.
    ///
    /// This is what goes into the initramfs. `raba`(8) cannot read
    /// `/etc/raba/raba.conf` at boot — the root filesystem is not mounted — so
    /// the hook that builds the image resolves everything here, on a running
    /// system where `/etc` and `os-release`(5) exist, and writes the answer
    /// inside. `Theme=auto` in particular **cannot** be resolved at boot: there
    /// is no `os-release` in an initramfs, which is why `raba.conf`(5) says
    /// resolution happens when the image is generated.
    ///
    /// Every key is written, not just the ones that differ from a default, and
    /// nothing is layered underneath it: `raba.defaults` is not in the image. A
    /// key that quietly failed to survive the freeze would be a setting that
    /// works on the installed system and not at boot, which is the one kind of
    /// difference nobody would think to look for.
    ///
    /// `None` when no theme resolved — there is nothing to freeze, and the hook
    /// has nothing to put in the image.
    #[must_use]
    pub fn freeze(&self) -> Option<String> {
        let theme = self.settings.theme.to_str()?;
        if theme.is_empty() {
            return None;
        }
        let config = &self.config;
        let outro = self.settings.outro.as_ref().and_then(|path| path.to_str());
        let mut out = String::new();

        out.push_str("# Generated for the initramfs. Do not edit.\n");
        out.push_str("#\n");
        out.push_str(
            "# Edits belong in /etc/raba/raba.conf on the root filesystem. This copy is\n",
        );
        out.push_str(
            "# rebuilt from it every time the image is, and every path in it is already\n",
        );
        out.push_str("# resolved: there is no os-release(5) in an initramfs to resolve `auto`\n");
        out.push_str("# against. See raba.conf(5).\n");

        out.push_str("\n[Boot]\n");
        row(&mut out, "Theme", theme);
        row(&mut out, "Tty", &tty_value(config.boot.tty));
        row(&mut out, "ShowDelay", &decimal(thousandths(config.boot.show_delay)));
        row(
            &mut out,
            "Timeout",
            &config.boot.timeout.map_or_else(|| "0".to_owned(), |t| decimal(thousandths(t))),
        );
        row(&mut out, "FrameRateOverride", &rate(config.boot.frame_rate));

        out.push_str("\n[Outro]\n");
        // An outro that did not resolve is an outro that is not there. Writing
        // `Enable=true` next to a theme that does not exist would make every
        // boot look for a file the build already knows is missing.
        row(&mut out, "Enable", boolean_value(config.outro.enable && outro.is_some()));
        // No `Theme=` at all when there is nothing to name. `auto` here would be
        // a word that cannot be resolved where this file is going, and an empty
        // value is one its own parser refuses.
        if let Some(outro) = outro {
            row(&mut out, "Theme", outro);
        }
        row(&mut out, "MaxDuration", &decimal(thousandths(config.outro.max_duration)));
        row(&mut out, "FinishFrame", boolean_value(config.outro.finish_frame));

        out.push_str("\n[Progress]\n");
        row(&mut out, "Enable", boolean_value(config.progress.enable));
        row(&mut out, "Style", config.progress.style.name());
        row(&mut out, "Source", config.progress.source.name());
        row(&mut out, "Position", config.progress.position.name());
        row(&mut out, "Width", &config.progress.width.to_string());
        row(&mut out, "Color", &config.progress.color.to_string());
        row(&mut out, "BackgroundColor", &config.progress.background.to_string());

        out.push_str("\n[Handoff]\n");
        row(&mut out, "Mode", config.handoff.mode.name());
        row(&mut out, "RestoreTty", boolean_value(config.handoff.restore_tty));

        out.push_str("\n[Log]\n");
        row(&mut out, "Level", config.log.level.name());
        row(&mut out, "File", &config.log.file.display().to_string());

        Some(out)
    }

    /// The files the image needs beside the frozen configuration, one per line.
    ///
    /// Only art: the binaries are the hook's business and the configuration is
    /// generated rather than copied. Every path here exists — the hook calls
    /// `add_file` on each, and in mkinitcpio an `add_*` that fails is an image
    /// build that fails. See `integration/mkinitcpio/install/raba`.
    #[must_use]
    pub fn files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        if !self.settings.theme.as_os_str().is_empty() {
            files.push(self.settings.theme.clone());
        }
        files.extend(self.settings.outro.clone());
        files.retain(|path| path.is_file());
        files
    }
}

fn row(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push('=');
    out.push_str(value);
    out.push('\n');
}

const fn boolean_value(yes: bool) -> &'static str {
    if yes {
        "true"
    } else {
        "false"
    }
}

fn tty_value(tty: Tty) -> String {
    match tty {
        Tty::Auto => "auto".to_owned(),
        Tty::Fixed(number) => number.to_string(),
    }
}

/// A frame rate, through the integer formatter.
///
/// The multiplication is the only arithmetic done on the value, and the
/// alternative — `{}` on an `f64` — costs fifteen kilobytes of Grisu in a
/// binary that has a size gate. `0` is the documented "use the theme's rate".
fn rate(rate: Option<f64>) -> String {
    let Some(rate) = rate else {
        return "0".to_owned();
    };
    let thousandths = (rate * 1000.0).round();
    if !thousandths.is_finite() || thousandths < 0.0 || thousandths > 1_000_000.0 {
        return "0".to_owned();
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    decimal(thousandths as u64)
}
