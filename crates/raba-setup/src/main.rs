//! The install-time half of RABA: everything that happens on a running system
//! rather than during a boot.
//!
//! # Why this is a separate binary
//!
//! `raba`(8) goes into an initramfs and has a size gate. Freezing a
//! configuration, inspecting an image and warning about a TPM are all things
//! that happen on a machine with a shell, a package manager and a `/etc` — so
//! none of them belong in the binary that has to be small, and all of them are
//! allowed to be as large as they need to be here.
//!
//! It depends on `raba` as a library for exactly one thing: the configuration
//! parser. The file this writes into an image is read back by `raba` at boot,
//! and a second implementation of the format would be a second set of bugs.
//!
//! # What the hook uses
//!
//! ```text
//! raba-setup --freeze        the effective configuration, one self-contained file
//! raba-setup --print-files   the art that has to be copied in beside it
//! ```
//!
//! Both are plumbing for `integration/mkinitcpio/install/raba` and its dracut
//! and initramfs-tools counterparts, and both exit non-zero when there is
//! nothing to say — which is how a hook knows to put nothing in the image
//! rather than something broken.

use raba::config;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
raba-setup - set up and verify a RABA installation

usage:
    raba-setup --freeze        print the configuration an initramfs should carry
    raba-setup --print-files   print the files it should carry beside it

paths:
    --config <file>        instead of /etc/raba/raba.conf
    --defaults <file>      instead of /usr/share/raba/raba.defaults
    --themes <dir>         instead of /usr/share/raba/themes
    --cmdline <file>       instead of /proc/cmdline
    --os-release <file>    instead of /etc/os-release

    --version              print the version
    --help                 print this

the dry run, --apply and --check are not implemented yet; until they are, the
manual steps are in docs/integration.md.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("raba-setup: {message}");
            ExitCode::FAILURE
        }
    }
}

/// What was asked for. Two of these so far, and a place to put the rest.
enum Action {
    Usage,
    Version,
    Freeze,
    PrintFiles,
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let mut action = Action::Usage;
    let mut places = config::Places::default();
    let mut index = 0;

    macro_rules! value {
        ($flag:literal) => {{
            let value = args.get(index).ok_or(concat!($flag, " needs a path"))?;
            index = index.saturating_add(1);
            PathBuf::from(value)
        }};
    }

    while let Some(arg) = args.get(index) {
        index = index.saturating_add(1);
        match arg.as_str() {
            "--help" | "-h" => action = Action::Usage,
            "--version" => action = Action::Version,
            "--freeze" => action = Action::Freeze,
            "--print-files" => action = Action::PrintFiles,
            "--config" => places.config = value!("--config"),
            "--defaults" => places.defaults = value!("--defaults"),
            "--themes" => places.themes = value!("--themes"),
            "--cmdline" => places.cmdline = value!("--cmdline"),
            "--os-release" => places.os_release = value!("--os-release"),
            other => return Err(format!("unknown option {other}\n\n{USAGE}")),
        }
    }

    match action {
        Action::Usage => {
            print!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        Action::Version => {
            println!("raba-setup {}", env!("CARGO_PKG_VERSION"));
            Ok(ExitCode::SUCCESS)
        }
        Action::Freeze => freeze(&places),
        Action::PrintFiles => print_files(&places),
    }
}

/// Print the configuration an image should carry, or say why there is none.
///
/// Every complaint the parser made goes to stderr and none of them stops this:
/// the hook is running during somebody's `mkinitcpio -P`, and a configuration
/// that is merely wrong in places should produce an image with a splash and a
/// warning, not a failed kernel update. The one thing that *does* stop it is
/// having no theme, because then there is nothing to freeze.
fn freeze(places: &config::Places) -> Result<ExitCode, String> {
    let startup = config::start(places);
    complain(&startup);
    let Some(text) = startup.freeze() else {
        return Err(format!(
            "no theme resolved; looked for {}",
            startup
                .tried
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    };
    print!("{text}");
    Ok(ExitCode::SUCCESS)
}

/// Print the files to copy in, one per line, or exit 1 having printed nothing.
fn print_files(places: &config::Places) -> Result<ExitCode, String> {
    let startup = config::start(places);
    complain(&startup);
    let files = startup.files();
    if files.is_empty() {
        return Err("no theme resolved; there is nothing to copy".to_owned());
    }
    for file in files {
        println!("{}", file.display());
    }
    Ok(ExitCode::SUCCESS)
}

/// Configuration problems, on stderr, where a hook's output goes.
fn complain(startup: &config::Startup) {
    for problem in &startup.problems {
        eprintln!("raba-setup: {problem}");
    }
}
