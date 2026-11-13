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
//!
//! # What it will not do
//!
//! Edit `/etc/mkinitcpio.conf`, `/etc/default/grub`, or anything else belonging
//! to another package. `--set-theme` edits `/etc/raba/raba.conf`, which is ours
//! and is named in the package's `backup=`, and nothing else ever writes. It prints the line and the command; you run it. A
//! program that rewrites somebody else's configuration file bypasses the
//! `.pacnew` handling that exists to stop exactly that, and the failure mode of
//! getting it wrong is a machine that does not boot. See `docs/packaging.md`.

mod machine;
mod theme;

use machine::{Advice, Currency, Generator, Root, Tpm};
use raba::config;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

const USAGE: &str = "\
raba-setup - set up and verify a RABA installation

usage:
    raba-setup                 say what this machine needs; change nothing
    raba-setup --apply         rebuild the initramfs
    raba-setup --check         exit non-zero if the image is out of date
    raba-setup --set-theme <name>
                               put <name> in raba.conf and rebuild, which is
                               the only way a theme change reaches a boot

    raba-setup --freeze        print the configuration an initramfs should carry
    raba-setup --print-files   print the files it should carry beside it

options:
    --yes                  do not ask before rebuilding an initramfs on a
                           machine whose disk unlocks itself

paths:
    --root <dir>           look under <dir> instead of /; for testing
    --config <file>        instead of /etc/raba/raba.conf
    --defaults <file>      instead of /usr/share/raba/raba.defaults
    --themes <dir>         instead of /usr/share/raba/themes
    --cmdline <file>       instead of /proc/cmdline
    --os-release <file>    instead of /etc/os-release

    --version              print the version
    --help                 print this

raba-setup never edits a file belonging to another package. It prints the line
and the command; you run them.
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

/// What was asked for.
enum Action {
    /// The default: look, print, change nothing.
    DryRun,
    Apply,
    Check,
    Usage,
    Version,
    Freeze,
    PrintFiles,
    /// `--set-theme <name>`: edit our own configuration, then [`Action::Apply`].
    SetTheme(String),
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let mut action = Action::DryRun;
    let mut places = config::Places::default();
    let mut root = Root::default();
    let mut yes = false;
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
            "--apply" => action = Action::Apply,
            "--set-theme" => {
                let name = args.get(index).ok_or("--set-theme needs a theme name")?;
                index = index.saturating_add(1);
                action = Action::SetTheme(name.clone());
            }
            "--check" => action = Action::Check,
            "--yes" | "-y" => yes = true,
            "--root" => root = Root::new(value!("--root")),
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
        Action::DryRun => Ok(dry_run(&root, &places)),
        Action::Apply => apply(&root, &places, yes),
        Action::SetTheme(name) => set_theme(&root, &places, yes, &name),
        Action::Check => Ok(check(&root, &places)),
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

// --------------------------------------------------------------- dry run ----

/// Look at the machine and say what it needs. Changes nothing.
///
/// The default, because the dangerous half of this program is one flag away and
/// should stay that way: everything it would do is printed first, in the order
/// it would happen, with the one thing that can cost you a boot — a TPM-sealed
/// disk — printed before the step that breaks it.
fn dry_run(root: &Root, places: &config::Places) -> ExitCode {
    let survey = machine::survey(root);
    let startup = config::start(places);

    println!("configuration:");
    if startup.files.is_empty() {
        println!("  no files; every setting is a compiled-in default");
    }
    for file in &startup.files {
        println!("  {}", file.display());
    }
    match (startup.settings.theme.as_os_str().is_empty(), startup.tried.first()) {
        (false, _) => println!("  theme: {}", startup.settings.theme.display()),
        (true, Some(first)) => {
            println!("  no theme; nothing to draw. Looked for {}", first.display());
        }
        (true, None) => println!("  no theme; nothing to draw"),
    }
    for problem in &startup.problems {
        println!("  {problem}");
    }

    println!();
    println!("initramfs:");
    if survey.generators.is_empty() {
        println!("  none found. RABA can still be started by your init system;");
        println!("  the splash then appears after the root filesystem is mounted.");
    }
    for generator in &survey.generators {
        println!("  {}", generator.name());
    }
    if survey.generators.contains(&Generator::Mkinitcpio) {
        hooks_advice(&survey);
    }

    println!();
    println!("images:");
    if let Some(why) = survey.unreadable.as_deref() {
        println!("  cannot look: {why}");
        println!("  (/boot is root-only on most machines; try sudo)");
    } else if survey.images.is_empty() {
        println!("  none found under /boot");
    }
    let frozen = startup.freeze().unwrap_or_default();
    let sources = source_times(&startup);
    for image in &survey.images {
        let currency = machine::currency(image, &frozen, &sources);
        println!("  {}  {}", image.path.display(), describe(&currency));
    }

    println!();
    tpm_advice(&survey.tpm);

    println!();
    println!("then:");
    for generator in &survey.generators {
        println!("  sudo {}", generator.rebuild().join(" "));
    }
    println!("  sudo systemctl enable raba-quit.service raba-quit-wait.service");
    println!("     — or rely on Boot.Timeout, which needs nothing enabled");
    println!();
    println!("kernel command line: `quiet loglevel=3`, in that order, is recommended.");
    println!("Nothing above has been changed. `raba-setup --apply` rebuilds the image.");
    ExitCode::SUCCESS
}

fn hooks_advice(survey: &machine::Survey) {
    let Some(hooks) = survey.hooks.as_ref() else {
        println!("  /etc/mkinitcpio.conf has no HOOKS= line this could read");
        return;
    };
    if machine::shares_with_plymouth(hooks) {
        println!("  HOOKS also has `plymouth`. Both end up in the image; RABA sees");
        println!("  plymouthd running, declines, and logs why — so the splash you get");
        println!("  is Plymouth's. Remove one of them. See docs/coexistence.md.");
    }
    match machine::advise(hooks) {
        Advice::Good => println!("  HOOKS has `raba`, after `encrypt`"),
        Advice::NotInHooks => {
            println!("  HOOKS has no `raba`; the image will have no splash. Edit");
            println!("  /etc/mkinitcpio.conf and make the line read:");
            println!("      {}", machine::suggested(hooks));
        }
        Advice::BeforeEncrypt => {
            println!("  HOOKS has `raba` *before* `encrypt`. RABA does not supply the");
            println!("  passphrase prompt, so the splash would be drawn over it. Move it:");
            println!("      {}", machine::suggested(hooks));
        }
    }
}

/// The TPM warning, which is the whole reason this command exists before
/// `--apply` rather than inside it.
fn tpm_advice(tpm: &Tpm) {
    println!("disk encryption:");
    match tpm {
        Tpm::Clear => println!("  no TPM-sealed volume found; rebuilding is uneventful"),
        Tpm::Unknown(why) => {
            println!("  not checked: {why}.");
            println!("  If your disk unlocks itself without a passphrase, read");
            println!("  docs/secure-boot.md before rebuilding the initramfs.");
        }
        Tpm::Sealed(devices) => {
            for device in devices {
                println!("  {device} releases its key against a TPM measurement.");
            }
            println!();
            println!("  Putting RABA in the initramfs changes that measurement (PCR 9, and");
            println!("  PCR 11 for a UKI). The next boot will ask for your recovery");
            println!("  passphrase instead of unlocking itself. This is not specific to");
            println!("  RABA — any change to initramfs contents does it.");
            println!();
            println!("  Before: make sure you have a recovery passphrase that works.");
            println!("  After:  sudo systemd-cryptenroll --wipe-slot=tpm2 <device>");
            println!("          sudo systemd-cryptenroll --tpm2-device=auto \\");
            println!("               --tpm2-pcrs=7+11 <device>");
            println!("  See docs/secure-boot.md, including the signed-policy way out.");
        }
    }
}

const fn describe(currency: &Currency) -> &'static str {
    match currency {
        Currency::Current => "carries the current configuration",
        Currency::Stale(why) => why,
        Currency::ProbablyStale => "older than your configuration; probably stale",
        Currency::ProbablyCurrent => "newer than your configuration; probably current",
        Currency::Unknown => "nothing here could tell whether it is current",
    }
}

/// When the things that go into an image last changed.
fn source_times(startup: &config::Startup) -> Vec<Option<u64>> {
    startup
        .files
        .iter()
        .chain(startup.settings.outro.iter())
        .chain(std::iter::once(&startup.settings.theme).filter(|path| !path.as_os_str().is_empty()))
        .map(|path| machine::seconds(path))
        .collect()
}

// ----------------------------------------------------------------- check ----

/// Is the image current? The question a package's post-install asks.
fn check(root: &Root, places: &config::Places) -> ExitCode {
    let survey = machine::survey(root);
    let startup = config::start(places);
    let Some(frozen) = startup.freeze() else {
        println!("raba-setup: no theme resolved; there is nothing to put in an image");
        return ExitCode::FAILURE;
    };
    if survey.images.is_empty() {
        println!("raba-setup: no initramfs found under /boot");
        return ExitCode::FAILURE;
    }
    let sources = source_times(&startup);
    let mut ok = true;
    for image in &survey.images {
        let currency = machine::currency(image, &frozen, &sources);
        println!("{}: {}", image.path.display(), describe(&currency));
        if matches!(currency, Currency::Stale(_) | Currency::ProbablyStale | Currency::Unknown) {
            ok = false;
        }
    }
    if ok {
        return ExitCode::SUCCESS;
    }
    for generator in &survey.generators {
        println!("rebuild with: sudo {}", generator.rebuild().join(" "));
    }
    ExitCode::FAILURE
}

// ----------------------------------------------------------------- apply ----

/// Rebuild the initramfs, and nothing else.
///
/// The one state-changing thing this program does, and it is somebody else's
/// program doing it. Everything that would mean editing a file we do not own is
/// printed by the dry run instead.
/// The answer to "may I rebuild this machine's initramfs?"
enum Permission {
    /// Yes, and this is the machine to do it to.
    Granted(Box<machine::Survey>),
    /// No. The reason has already been printed in full, including what to do
    /// about it.
    Refused,
}

/// Everything that has to be true before anything changes.
///
/// Separated from the rebuild for one reason: `--set-theme` edits a file first,
/// and every question here has to be answered before that edit, not after it. A
/// review found all three ways the old arrangement got this wrong — a declined
/// TPM prompt that printed "Nothing was changed" over a file that had just
/// changed, a `raba` missing from `HOOKS`, and `--root` pointing at somebody
/// else's filesystem while the edit landed on the running one.
///
/// `needs_theme` is false for `--set-theme`, which is about to supply one: a
/// machine whose configuration currently resolves to nothing is exactly the
/// machine that command is for.
fn permission(
    root: &Root,
    places: &config::Places,
    yes: bool,
    needs_theme: bool,
) -> Result<Permission, String> {
    // First, because it is the only one that is about the caller rather than
    // the machine, and because getting it wrong means writing to the wrong
    // computer.
    if !root.is_the_running_system() {
        return Err("--root is for inspecting, not for rebuilding somebody else's boot".to_owned());
    }
    let survey = machine::survey(root);
    if survey.generators.is_empty() {
        return Err("no initramfs generator found; there is nothing to rebuild".to_owned());
    }
    // Asked before the rebuild rather than discovered during it: the hooks all
    // decline quietly when there is no theme, so without this the command would
    // run for thirty seconds and produce an image with no splash in it.
    if needs_theme && config::start(places).freeze().is_none() {
        return Err("no theme resolved; the image would have no splash in it (try: raba --check)"
            .to_owned());
    }
    if let Some(hooks) = survey.hooks.as_ref() {
        if machine::advise(hooks) == Advice::NotInHooks {
            println!("mkinitcpio has no `raba` in HOOKS, so rebuilding would change nothing.");
            println!("Make the line read:");
            println!("    {}", machine::suggested(hooks));
            return Ok(Permission::Refused);
        }
    }
    if let Tpm::Sealed(devices) = &survey.tpm {
        tpm_advice(&survey.tpm);
        if !yes && !agreed(devices) {
            println!("Nothing was changed.");
            return Ok(Permission::Refused);
        }
    }
    Ok(Permission::Granted(Box::new(survey)))
}

/// Run each generator this machine has. The only state-changing thing here, and
/// it is somebody else's program doing it.
fn rebuild(survey: &machine::Survey) -> Result<ExitCode, String> {
    for generator in &survey.generators {
        let Some((program, arguments)) = generator.rebuild().split_first() else {
            continue;
        };
        println!("raba-setup: {} {}", program, arguments.join(" "));
        let status = Command::new(program)
            .args(arguments)
            .status()
            .map_err(|why| format!("running {program}: {why}"))?;
        if !status.success() {
            return Err(format!("{program} exited {status}; the image was not rebuilt"));
        }
    }
    println!("raba-setup: done. `raba-setup --check` confirms what is in the image.");
    Ok(ExitCode::SUCCESS)
}

fn apply(root: &Root, places: &config::Places, yes: bool) -> Result<ExitCode, String> {
    match permission(root, places, yes, true)? {
        Permission::Refused => Ok(ExitCode::FAILURE),
        Permission::Granted(survey) => rebuild(&survey),
    }
}

/// `--set-theme <name>`: change the line, then rebuild the image.
///
/// The two halves are one command because doing only the first is the trap
/// `raba.conf`(5) warns about on every reading: RABA runs before the root
/// filesystem is mounted, so it reads a copy of the configuration baked into
/// the initramfs, and an edit to `/etc/raba/raba.conf` changes nothing at all
/// until the image is regenerated. A person who edits the file and reboots sees
/// the old logo and concludes the program is broken.
///
/// The theme is checked before the file is touched. A name that does not resolve
/// would otherwise produce a valid-looking configuration, a thirty-second
/// rebuild, and an image with no splash in it.
fn set_theme(
    root: &Root,
    places: &config::Places,
    yes: bool,
    wanted: &str,
) -> Result<ExitCode, String> {
    let parsed = config::Theme::parse(wanted).map_err(|why| format!("{wanted}: {why}"))?;
    match &parsed {
        config::Theme::Auto => {}
        config::Theme::Named(name) => {
            let file = places.themes.join(format!("{name}.raba"));
            if !file.exists() {
                return Err(format!("{}: no such theme. {}", file.display(), available(places)));
            }
        }
        config::Theme::Path(path) => {
            if !path.exists() {
                return Err(format!("{}: no such file", path.display()));
            }
        }
    }

    // Everything that could refuse, before anything is written. The order is
    // the finding: a declined TPM prompt used to print "Nothing was changed"
    // over a file that had already changed, and `--root` used to edit the
    // running system's configuration and *then* say it would not touch somebody
    // else's boot.
    let survey = match permission(root, places, yes, false)? {
        Permission::Refused => return Ok(ExitCode::FAILURE),
        Permission::Granted(survey) => survey,
    };

    let before = match std::fs::read_to_string(&places.config) {
        Ok(text) => text,
        // No file is not a failure: the setting has a compiled-in default, and
        // somebody who removed the file still gets to change the theme.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("{}: {error}", places.config.display())),
    };
    let after = theme::rewrite(&before, wanted);
    if after == before {
        println!("raba-setup: {} already says Theme={wanted}", places.config.display());
    } else {
        write_config(&places.config, &after)?;
        println!("raba-setup: {}: Theme={wanted}", places.config.display());
    }

    // And now the half that makes it true at boot.
    rebuild(&survey)
}

/// The names a person could have meant, so the error is useful rather than only
/// correct. Cut short: fifty logos printed at somebody who made a typo is not
/// help.
fn available(places: &config::Places) -> String {
    let Ok(entries) = std::fs::read_dir(&places.themes) else {
        return format!("Looked in {}.", places.themes.display());
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".raba").map(str::to_owned)
        })
        .collect();
    names.sort();
    let total = names.len();
    if total == 0 {
        return format!("There are no themes in {}.", places.themes.display());
    }
    names.truncate(8);
    let listed = names.join(", ");
    if total > 8 {
        format!("{total} in {}, among them: {listed}.", places.themes.display())
    } else {
        format!("In {}: {listed}.", places.themes.display())
    }
}

/// Replace the file without ever leaving a half-written one in its place: a
/// truncated `raba.conf` is a machine whose next initramfs build has no theme.
///
/// `O_NOFOLLOW` and `O_CREAT|O_EXCL` on the staging file, which is the rule
/// `docs/security.md` finding 4 sets for this whole project and which
/// `raba::state::save` already follows. Without them this is a root process
/// writing the contents of a configuration file through whatever symlink
/// somebody left at `raba.conf.new` — cheap to arrange whenever the directory
/// is writable by anyone else, which `--config` makes possible by design.
fn write_config(path: &std::path::Path, text: &str) -> Result<(), String> {
    use std::io::Write as _;
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};

    // Spelled out rather than taken from `libc`, which this crate does not
    // depend on and should not grow a dependency for one integer. The same
    // constant, with the same reasoning, is in `raba::state`.
    const O_NOFOLLOW: i32 = 0o400_000;

    let staging = path.with_extension("conf.new");
    let blame = |why: std::io::Error| format!("{}: {why}", staging.display());

    let open = || {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(O_NOFOLLOW)
            .open(&staging)
    };
    let mut file = match open() {
        Ok(file) => file,
        // Ours, from a run that died between creating and renaming. Removing it
        // is safe for the same reason creating it was: nothing followed a link
        // to get here.
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            std::fs::remove_file(&staging).map_err(blame)?;
            open().map_err(blame)?
        }
        Err(error) => return Err(blame(error)),
    };

    // From here on, every failure takes the staging file with it. A half-written
    // raba.conf.new left in /etc is litter at best, and at worst the file the
    // *next* run finds and deletes without knowing what is in it.
    let written = file.write_all(text.as_bytes()).and_then(|()| file.flush()).and_then(|()| {
        // The mode the real file had, so a rename does not silently widen
        // it. A file that was never there gets whatever the umask says.
        if let Ok(meta) = std::fs::metadata(path) {
            file.set_permissions(std::fs::Permissions::from_mode(meta.permissions().mode()))?;
        }
        Ok(())
    });
    drop(file);
    if let Err(error) = written {
        let _ = std::fs::remove_file(&staging);
        return Err(blame(error));
    }
    if let Err(error) = std::fs::rename(&staging, path) {
        let _ = std::fs::remove_file(&staging);
        return Err(format!("{}: {error}", path.display()));
    }
    Ok(())
}

/// Ask, once, in words that say what is about to happen.
fn agreed(devices: &[String]) -> bool {
    println!();
    print!("Rebuild anyway, knowing {} will ask for a passphrase? [y/N] ", devices.join(" and "));
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}
