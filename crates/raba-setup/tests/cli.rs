//! The command line of the program that edits `/etc` and rebuilds initramfs.
//!
//! Coverage said 13% of `main.rs`, and the uncovered part was the argument
//! parser and every refusal in front of the one write this program does. That
//! is the wrong thing to have untested: a mistake here does not produce a wrong
//! answer, it produces a wrong *action* on somebody's machine.
//!
//! Everything below drives the real binary with every path pointed inside a
//! scratch directory, so no test can read — or write — the machine it runs on.
//! Nothing here reaches `rebuild()`: that runs mkinitcpio, and a test suite has
//! no business doing that. What is proved instead is the half that decides
//! whether it ever gets there, and the rule that makes that half worth having:
//! **when raba-setup refuses, the configuration file is exactly as it was.**

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let mut path = std::env::temp_dir();
        path.push(format!("raba-setup-cli-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("themes")).expect("scratch");
        Scratch(path)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// A theme file, by name. Empty on purpose: every refusal under test here
    /// is decided by `config::candidates` and `Path::exists`, which is the same
    /// resolution the splash does and does not open anything. A test that built
    /// a real `.raba` would be testing the encoder twice and this not at all.
    fn theme(&self, name: &str) {
        std::fs::write(self.path(&format!("themes/{name}.raba")), b"").expect("a theme");
    }

    fn os_release(&self, text: &str) {
        std::fs::write(self.path("os-release"), text).expect("os-release");
    }

    fn config(&self, text: &str) {
        std::fs::write(self.path("raba.conf"), text).expect("a configuration");
    }

    fn config_now(&self) -> Option<String> {
        std::fs::read_to_string(self.path("raba.conf")).ok()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn setup(scratch: &Scratch, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_raba-setup"))
        .arg("--config")
        .arg(scratch.path("raba.conf"))
        .arg("--defaults")
        .arg(scratch.path("raba.defaults"))
        .arg("--themes")
        .arg(scratch.path("themes"))
        .arg("--cmdline")
        .arg(scratch.path("cmdline"))
        .arg("--os-release")
        .arg(scratch.path("os-release"))
        .args(args)
        .output()
        .expect("running raba-setup")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn failed(output: &Output) -> bool {
    !output.status.success()
}

// --- the parser --------------------------------------------------------------

#[test]
fn usage_lists_the_actions_the_man_page_documents() {
    let scratch = Scratch::new("usage");
    let output = setup(&scratch, &["--help"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    for flag in ["--apply", "--check", "--set-theme", "--freeze", "--print-files", "--root"] {
        assert!(text.contains(flag), "--help does not mention {flag}:\n{text}");
    }
}

#[test]
fn an_unknown_option_is_refused_and_the_usage_comes_with_it() {
    let scratch = Scratch::new("unknown");
    let output = setup(&scratch, &["--rebuild-everything"]);
    assert!(failed(&output));
    let text = stderr(&output);
    assert!(text.contains("unknown option --rebuild-everything"), "{text}");
    assert!(text.contains("--apply"), "the refusal should show what is allowed:\n{text}");
}

#[test]
fn an_option_that_needs_a_value_and_has_none_says_so() {
    let scratch = Scratch::new("no-value");
    let output = setup(&scratch, &["--set-theme"]);
    assert!(failed(&output));
    assert!(stderr(&output).contains("--set-theme needs a theme name"), "{}", stderr(&output));
}

#[test]
fn the_version_is_the_one_cargo_knows() {
    let scratch = Scratch::new("version");
    let output = setup(&scratch, &["--version"]);
    assert!(output.status.success());
    assert_eq!(stdout(&output).trim(), format!("raba-setup {}", env!("CARGO_PKG_VERSION")));
}

// --- what the hooks call -----------------------------------------------------

#[test]
fn freeze_refuses_when_nothing_resolves_and_names_what_it_looked_for() {
    let scratch = Scratch::new("freeze-empty");
    scratch.os_release("ID=nothinglikethis\n");
    let output = setup(&scratch, &["--freeze"]);
    // The documented contract: "Exits non-zero when no theme resolves, which is
    // how a hook knows to put nothing in the image rather than something broken."
    assert!(failed(&output));
    let text = stderr(&output);
    assert!(text.contains("no theme resolved"), "{text}");
    assert!(text.contains("generic.raba"), "it should say where it looked:\n{text}");
}

#[test]
fn print_files_refuses_when_nothing_resolves() {
    let scratch = Scratch::new("files-empty");
    scratch.os_release("ID=nothinglikethis\n");
    let output = setup(&scratch, &["--print-files"]);
    assert!(failed(&output));
    assert!(stderr(&output).contains("nothing to copy"), "{}", stderr(&output));
    assert_eq!(stdout(&output), "", "nothing should be printed for a hook to copy");
}

#[test]
fn print_files_lists_the_theme_it_resolved() {
    let scratch = Scratch::new("files");
    scratch.os_release("ID=arch\n");
    scratch.theme("arch");
    let output = setup(&scratch, &["--print-files"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("arch.raba"), "{}", stdout(&output));
}

// --- the refusals in front of the one write ----------------------------------
//
// Each of these asserts twice: the right reason, and an unchanged file. The
// second assertion is the one that matters. A review in November 2026 found
// `--set-theme` writing the configuration *before* checking `--root`, so the
// program declined to touch somebody else's machine having already touched this
// one, and printed "Nothing was changed" over a file that had just changed.

#[test]
fn a_theme_that_is_not_installed_is_refused_and_the_file_is_untouched() {
    let scratch = Scratch::new("no-such-theme");
    scratch.config("[Boot]\nTheme=generic\n");
    scratch.theme("arch");
    let output = setup(&scratch, &["--set-theme", "gentoo"]);
    assert!(failed(&output));
    let text = stderr(&output);
    assert!(text.contains("gentoo.raba"), "{text}");
    assert!(text.contains("no such theme"), "{text}");
    assert!(text.contains("arch"), "it should list what is there:\n{text}");
    assert_eq!(scratch.config_now().as_deref(), Some("[Boot]\nTheme=generic\n"));
}

#[test]
fn a_theme_given_as_a_path_that_is_not_there_is_refused() {
    let scratch = Scratch::new("no-such-path");
    let output = setup(&scratch, &["--set-theme", "/nonexistent/somewhere.raba"]);
    assert!(failed(&output));
    assert!(stderr(&output).contains("no such file"), "{}", stderr(&output));
    assert_eq!(scratch.config_now(), None, "nothing should have been created");
}

/// The finding this test exists for. `auto` is the one theme name that resolves
/// to a *list* rather than to a file, and the editor used to accept it without
/// checking any of them — so on a machine whose `ID` has no art and which has
/// no `generic.raba`, `--set-theme auto` wrote the file, spent thirty seconds
/// rebuilding, and exited 0 with an image that has no splash in it.
/// `raba-setup`(8) documents the opposite.
#[test]
fn auto_that_resolves_to_nothing_is_refused_and_the_file_is_untouched() {
    let scratch = Scratch::new("auto-nothing");
    scratch.config("[Boot]\nTheme=arch\n");
    scratch.os_release("ID=nothinglikethis\n");
    scratch.theme("arch"); // present, but not what `auto` would pick
    let output = setup(&scratch, &["--set-theme", "auto"]);
    assert!(failed(&output), "auto resolved to nothing and was accepted anyway");
    let text = stderr(&output);
    assert!(text.contains("auto resolves to nothing"), "{text}");
    assert!(text.contains("generic.raba"), "it should say what it tried:\n{text}");
    assert_eq!(scratch.config_now().as_deref(), Some("[Boot]\nTheme=arch\n"));
}

/// And the other half: `auto` resolves the way the splash resolves it, through
/// `ID` and `ID_LIKE`, so a derivative with no art of its own gets its parent's.
/// Proved by the error that comes *after* it — reaching the `--root` refusal
/// means the theme check passed.
#[test]
fn auto_resolves_through_os_release_like_the_splash_does() {
    let scratch = Scratch::new("auto-id-like");
    scratch.os_release("ID=cachyos\nID_LIKE=\"arch\"\n");
    scratch.theme("arch");
    let elsewhere = scratch.path("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("a root to point at");
    let output =
        setup(&scratch, &["--root", elsewhere.to_str().expect("utf-8"), "--set-theme", "auto"]);
    assert!(failed(&output));
    let text = stderr(&output);
    assert!(
        text.contains("--root is for inspecting"),
        "auto should have resolved to arch.raba through ID_LIKE and reached the next check:\n{text}"
    );
}

/// The ordering finding, directly: `--root` names somebody else's filesystem,
/// so nothing may be written to this one — and the check has to happen before
/// the edit, not after it.
#[test]
fn root_elsewhere_is_refused_before_anything_is_written() {
    let scratch = Scratch::new("root-elsewhere");
    scratch.config("[Boot]\nTheme=generic\n");
    scratch.theme("arch");
    let elsewhere = scratch.path("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("a root to point at");
    let output =
        setup(&scratch, &["--root", elsewhere.to_str().expect("utf-8"), "--set-theme", "arch"]);
    assert!(failed(&output));
    assert!(stderr(&output).contains("--root is for inspecting"), "{}", stderr(&output));
    assert_eq!(
        scratch.config_now().as_deref(),
        Some("[Boot]\nTheme=generic\n"),
        "the theme line was written before the refusal that says it would not be"
    );
}

#[test]
fn inspecting_somebody_elses_root_is_allowed() {
    let scratch = Scratch::new("root-check");
    let elsewhere = scratch.path("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("a root to point at");
    // --check reads and reports; it is the one thing --root exists for. What is
    // asserted is that it is not refused the way a write is, whatever it finds.
    let output = setup(&scratch, &["--root", elsewhere.to_str().expect("utf-8"), "--check"]);
    assert!(
        !stderr(&output).contains("--root is for inspecting"),
        "inspecting was refused as if it were a write:\n{}",
        stderr(&output)
    );
}

// --- the dry run, against a machine made of directories ------------------------
//
// Added when coverage said `dry_run`, `check`, `hooks_advice` and `describe`
// were at 0%: the default action of this program, the one a person runs first
// and the only one the man page tells them to start with, had never been run by
// anything. `--root` is what makes it safe to: every path below is inside a
// scratch directory, and `Root::is_the_running_system` keeps `--apply` from
// doing anything to it.

/// A machine's worth of directories, under `<scratch>/root`.
struct Machine(PathBuf);

impl Machine {
    fn new(scratch: &Scratch) -> Machine {
        let root = scratch.path("root");
        std::fs::create_dir_all(root.join("etc")).expect("etc");
        std::fs::create_dir_all(root.join("boot")).expect("boot");
        Machine(root)
    }

    /// `/etc/mkinitcpio.conf`, which is also how `survey` decides mkinitcpio is
    /// the generator here.
    fn mkinitcpio(self, hooks: &str) -> Machine {
        std::fs::write(self.0.join("etc/mkinitcpio.conf"), format!("{hooks}\n"))
            .expect("mkinitcpio.conf");
        self
    }

    fn dracut(self) -> Machine {
        std::fs::create_dir_all(self.0.join("etc/dracut.conf.d")).expect("dracut.conf.d");
        self
    }

    fn initramfs_tools(self) -> Machine {
        std::fs::create_dir_all(self.0.join("etc/initramfs-tools")).expect("initramfs-tools");
        self
    }

    fn image(self, name: &str) -> Machine {
        std::fs::write(self.0.join("boot").join(name), b"not really an image").expect("an image");
        self
    }

    fn arg(&self) -> &str {
        self.0.to_str().expect("a utf-8 path")
    }
}

/// The dry run needs a theme to have anything to say about one.
fn with_a_theme(scratch: &Scratch) {
    scratch.theme("thing");
    scratch.config("[Boot]\nTheme=thing\n");
}

#[test]
fn the_dry_run_is_the_default_and_says_it_changed_nothing() {
    let scratch = Scratch::new("dry-run");
    with_a_theme(&scratch);
    let machine = Machine::new(&scratch).mkinitcpio("HOOKS=(base udev encrypt raba filesystems)");

    let output = setup(&scratch, &["--root", machine.arg()]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    for heading in ["configuration:", "initramfs:", "images:", "disk encryption:", "then:"] {
        assert!(text.contains(heading), "the dry run has no {heading} section:\n{text}");
    }
    assert!(text.contains("Nothing above has been changed."), "{text}");
    assert_eq!(
        scratch.config_now().as_deref(),
        Some("[Boot]\nTheme=thing\n"),
        "the default action edited the configuration"
    );
}

#[test]
fn the_dry_run_names_the_theme_it_resolved_and_the_files_it_read() {
    let scratch = Scratch::new("dry-run-theme");
    with_a_theme(&scratch);
    let machine = Machine::new(&scratch).mkinitcpio("HOOKS=(base raba filesystems)");

    let output = setup(&scratch, &["--root", machine.arg()]);
    let text = stdout(&output);
    assert!(text.contains("thing.raba"), "the resolved theme is not named:\n{text}");
    assert!(text.contains("raba.conf"), "the file it read is not named:\n{text}");
}

#[test]
fn a_machine_with_no_theme_is_told_where_it_looked() {
    let scratch = Scratch::new("dry-run-no-theme");
    scratch.config("[Boot]\nTheme=absent\n");
    let machine = Machine::new(&scratch).mkinitcpio("HOOKS=(base raba filesystems)");

    let output = setup(&scratch, &["--root", machine.arg()]);
    let text = stdout(&output);
    assert!(text.contains("no theme"), "{text}");
    assert!(text.contains("absent.raba"), "it did not say what it looked for:\n{text}");
}

#[test]
fn a_configuration_with_no_files_at_all_says_so_rather_than_printing_nothing() {
    let scratch = Scratch::new("dry-run-defaults");
    let machine = Machine::new(&scratch);

    let output = setup(&scratch, &["--root", machine.arg()]);
    let text = stdout(&output);
    assert!(text.contains("compiled-in default"), "{text}");
}

#[test]
fn every_initramfs_generator_on_the_machine_is_named() {
    let scratch = Scratch::new("generators");
    with_a_theme(&scratch);
    let machine = Machine::new(&scratch)
        .mkinitcpio("HOOKS=(base raba filesystems)")
        .dracut()
        .initramfs_tools();

    let output = setup(&scratch, &["--root", machine.arg()]);
    let text = stdout(&output);
    for generator in ["mkinitcpio", "dracut", "initramfs-tools"] {
        assert!(text.contains(generator), "{generator} was not reported:\n{text}");
    }
}

#[test]
fn a_machine_with_no_generator_is_told_the_splash_still_works() {
    let scratch = Scratch::new("no-generator");
    with_a_theme(&scratch);
    let machine = Machine::new(&scratch);

    let output = setup(&scratch, &["--root", machine.arg()]);
    let text = stdout(&output);
    assert!(text.contains("none found"), "{text}");
    assert!(
        text.contains("init system"),
        "a machine with no initramfs was left thinking RABA cannot work there:\n{text}"
    );
}

// --- what it says about HOOKS= ------------------------------------------------

#[test]
fn raba_after_encrypt_is_the_arrangement_it_approves_of() {
    let scratch = Scratch::new("hooks-good");
    with_a_theme(&scratch);
    let machine =
        Machine::new(&scratch).mkinitcpio("HOOKS=(base udev encrypt raba filesystems fsck)");

    let output = setup(&scratch, &["--root", machine.arg()]);
    assert!(stdout(&output).contains("after `encrypt`"), "{}", stdout(&output));
}

#[test]
fn raba_missing_from_hooks_comes_with_the_line_as_it_should_read() {
    let scratch = Scratch::new("hooks-missing");
    with_a_theme(&scratch);
    let machine = Machine::new(&scratch).mkinitcpio("HOOKS=(base udev encrypt filesystems fsck)");

    let output = setup(&scratch, &["--root", machine.arg()]);
    let text = stdout(&output);
    assert!(text.contains("no `raba`"), "{text}");
    // Not just "add raba": the whole line, in the right order, to paste.
    assert!(text.contains("encrypt raba"), "it did not show where to put it:\n{text}");
}

#[test]
fn raba_before_encrypt_is_refused_with_the_reason_and_the_fix() {
    let scratch = Scratch::new("hooks-order");
    with_a_theme(&scratch);
    let machine = Machine::new(&scratch).mkinitcpio("HOOKS=(base udev raba encrypt filesystems)");

    let output = setup(&scratch, &["--root", machine.arg()]);
    let text = stdout(&output);
    assert!(text.contains("before"), "{text}");
    // The reason matters more than the rule: RABA is not the passphrase prompt,
    // so a splash in front of it hides the prompt. See docs/luks.md.
    assert!(text.contains("passphrase"), "it gave the rule without the reason:\n{text}");
    assert!(text.contains("encrypt raba"), "it did not show the fixed line:\n{text}");
}

#[test]
fn plymouth_in_the_same_hooks_line_is_pointed_out_before_the_image_is_built() {
    let scratch = Scratch::new("hooks-plymouth");
    with_a_theme(&scratch);
    let machine =
        Machine::new(&scratch).mkinitcpio("HOOKS=(base udev plymouth encrypt raba filesystems)");

    let output = setup(&scratch, &["--root", machine.arg()]);
    let text = stdout(&output);
    assert!(text.contains("plymouth"), "{text}");
    assert!(text.contains("Remove one of them"), "it noticed and said nothing useful:\n{text}");
}

#[test]
fn a_mkinitcpio_conf_with_no_hooks_line_is_said_out_loud_rather_than_guessed_at() {
    let scratch = Scratch::new("hooks-none");
    with_a_theme(&scratch);
    let machine = Machine::new(&scratch).mkinitcpio("# somebody commented the whole thing out");

    let output = setup(&scratch, &["--root", machine.arg()]);
    assert!(stdout(&output).contains("no HOOKS="), "{}", stdout(&output));
}

// --- what it says about the images --------------------------------------------

#[test]
fn an_image_is_listed_with_a_verdict_on_whether_it_is_current() {
    let scratch = Scratch::new("images");
    with_a_theme(&scratch);
    let machine = Machine::new(&scratch)
        .mkinitcpio("HOOKS=(base raba filesystems)")
        .image("initramfs-linux.img");

    let output = setup(&scratch, &["--root", machine.arg()]);
    let text = stdout(&output);
    assert!(text.contains("initramfs-linux.img"), "{text}");
    // Whatever the verdict is, there has to be one: a path with nothing after
    // it is the report failing to answer the question it exists for.
    let line = text
        .lines()
        .find(|line| line.contains("initramfs-linux.img"))
        .unwrap_or_else(|| panic!("no line for the image:\n{text}"));
    assert!(
        line.contains("current") || line.contains("stale") || line.contains("nothing here"),
        "the image was listed without a verdict: {line:?}"
    );
}

#[test]
fn fallback_and_superseded_images_are_left_out_of_the_report() {
    let scratch = Scratch::new("images-filtered");
    with_a_theme(&scratch);
    let machine = Machine::new(&scratch)
        .mkinitcpio("HOOKS=(base raba filesystems)")
        .image("initramfs-linux.img")
        .image("initramfs-linux-fallback.img")
        .image("initramfs-linux.img.old");

    let output = setup(&scratch, &["--root", machine.arg()]);
    let text = stdout(&output);
    assert!(text.contains("initramfs-linux.img"), "{text}");
    // A fallback is built without autodetect and is not the one being booted;
    // reporting on it would double every line for no information.
    assert!(!text.contains("fallback"), "the fallback image was reported:\n{text}");
    assert!(!text.contains(".old"), "a superseded image was reported:\n{text}");
}

#[test]
fn a_boot_directory_that_cannot_be_read_says_so_and_suggests_sudo() {
    let scratch = Scratch::new("boot-unreadable");
    with_a_theme(&scratch);
    let machine = Machine::new(&scratch).mkinitcpio("HOOKS=(base raba filesystems)");
    // The ordinary case on a real machine: /boot is an ESP, mode 0700, root's.
    let boot = machine.0.join("boot");
    let mut mode = std::fs::metadata(&boot).expect("boot").permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut mode, 0o000);
    std::fs::set_permissions(&boot, mode).expect("locking /boot");

    let output = setup(&scratch, &["--root", machine.arg()]);
    let text = stdout(&output);

    // Put it back before any assertion, so a failure here does not leave a
    // directory the scratch cleanup cannot remove.
    let mut mode = std::fs::metadata(&boot).expect("boot").permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut mode, 0o755);
    std::fs::set_permissions(&boot, mode).expect("unlocking /boot");

    assert!(text.contains("cannot look"), "{text}");
    assert!(
        text.contains("sudo"),
        "'I could not look' must not read as 'there is nothing':\n{text}"
    );
}

#[test]
fn a_machine_that_is_not_the_running_one_is_not_guessed_about_for_disk_encryption() {
    let scratch = Scratch::new("tpm-unknown");
    with_a_theme(&scratch);
    let machine = Machine::new(&scratch).mkinitcpio("HOOKS=(base raba filesystems)");

    let output = setup(&scratch, &["--root", machine.arg()]);
    let text = stdout(&output);
    assert!(text.contains("not checked"), "{text}");
    assert!(
        text.contains("docs/secure-boot.md"),
        "a machine it could not check was left with no way to find out:\n{text}"
    );
}

// --- --check -------------------------------------------------------------------

#[test]
fn check_with_no_theme_fails_because_there_is_nothing_to_put_in_an_image() {
    let scratch = Scratch::new("check-no-theme");
    let machine = Machine::new(&scratch).mkinitcpio("HOOKS=(base raba filesystems)");

    let output = setup(&scratch, &["--root", machine.arg(), "--check"]);
    assert!(failed(&output), "it passed with nothing to check");
    assert!(stdout(&output).contains("no theme resolved"), "{}", stdout(&output));
}

#[test]
fn check_with_no_image_fails_rather_than_passing_vacuously() {
    let scratch = Scratch::new("check-no-image");
    with_a_theme(&scratch);
    let machine = Machine::new(&scratch).mkinitcpio("HOOKS=(base raba filesystems)");

    let output = setup(&scratch, &["--root", machine.arg(), "--check"]);
    assert!(failed(&output), "no images at all was reported as up to date");
    assert!(stdout(&output).contains("no initramfs found"), "{}", stdout(&output));
}

#[test]
fn check_reports_every_image_and_says_how_to_rebuild_a_stale_one() {
    let scratch = Scratch::new("check-stale");
    with_a_theme(&scratch);
    let machine = Machine::new(&scratch)
        .mkinitcpio("HOOKS=(base raba filesystems)")
        .image("initramfs-linux.img");

    let output = setup(&scratch, &["--root", machine.arg(), "--check"]);
    let text = stdout(&output);
    assert!(text.contains("initramfs-linux.img"), "{text}");
    if failed(&output) {
        assert!(
            text.contains("rebuild with: sudo mkinitcpio"),
            "it said the image was stale and not how to fix it:\n{text}"
        );
    }
}

// --- --apply, which must not touch a machine made of directories ---------------

#[test]
fn apply_against_a_root_elsewhere_is_refused_before_anything_runs() {
    let scratch = Scratch::new("apply-elsewhere");
    with_a_theme(&scratch);
    let machine = Machine::new(&scratch).mkinitcpio("HOOKS=(base raba filesystems)");

    let output = setup(&scratch, &["--root", machine.arg(), "--apply", "--yes"]);
    assert!(failed(&output), "--apply ran against a directory pretending to be a machine");
    let said = stderr(&output);
    assert!(
        said.contains("--root") || said.contains("running system"),
        "it refused without saying why: {said}"
    );
}

// --- the names it offers when a theme is misspelt -------------------------------

#[test]
fn a_misspelt_theme_with_nothing_installed_says_the_directory_is_empty() {
    let scratch = Scratch::new("available-none");
    let output = setup(&scratch, &["--set-theme", "arch"]);
    assert!(failed(&output));
    assert!(stderr(&output).contains("no themes in"), "{}", stderr(&output));
}

#[test]
fn a_misspelt_theme_lists_the_ones_that_are_there() {
    let scratch = Scratch::new("available-some");
    for name in ["arch", "debian", "fedora"] {
        scratch.theme(name);
    }
    let output = setup(&scratch, &["--set-theme", "gentooo"]);
    assert!(failed(&output));
    let said = stderr(&output);
    assert!(said.contains("arch") && said.contains("fedora"), "{said}");
}

#[test]
fn a_long_list_of_themes_is_cut_short_and_says_how_many_there_were() {
    let scratch = Scratch::new("available-many");
    for n in 0..12 {
        scratch.theme(&format!("theme{n:02}"));
    }
    let output = setup(&scratch, &["--set-theme", "nope"]);
    assert!(failed(&output));
    let said = stderr(&output);
    assert!(said.contains("12 in"), "it did not say how many there are: {said}");
    assert!(said.contains("among them"), "{said}");
    assert!(!said.contains("theme11"), "twelve names were printed at somebody who made a typo");
}

#[test]
fn a_themes_directory_that_is_not_there_says_where_it_looked() {
    let scratch = Scratch::new("available-missing");
    let output = Command::new(env!("CARGO_BIN_EXE_raba-setup"))
        .arg("--config")
        .arg(scratch.path("raba.conf"))
        .arg("--defaults")
        .arg(scratch.path("raba.defaults"))
        .arg("--themes")
        .arg(scratch.path("no-such-directory"))
        .arg("--cmdline")
        .arg(scratch.path("cmdline"))
        .arg("--os-release")
        .arg(scratch.path("os-release"))
        .args(["--set-theme", "arch"])
        .output()
        .expect("running raba-setup");
    assert!(failed(&output));
    assert!(stderr(&output).contains("Looked in"), "{}", stderr(&output));
}

// --- the parser, for the flags the actions above need ---------------------------

#[test]
fn the_flag_that_skips_the_prompt_is_accepted_wherever_a_prompt_could_appear() {
    let scratch = Scratch::new("yes");
    with_a_theme(&scratch);
    let machine = Machine::new(&scratch).mkinitcpio("HOOKS=(base raba filesystems)");
    for flag in ["--yes", "-y"] {
        let output = setup(&scratch, &["--root", machine.arg(), flag]);
        assert!(output.status.success(), "{flag}: {}", stderr(&output));
    }
}

#[test]
fn a_configuration_problem_goes_to_stderr_and_does_not_stop_the_report() {
    let scratch = Scratch::new("complain");
    scratch.theme("thing");
    scratch.config("[Boot]\nTheme=thing\nNotAKey=7\n");
    let machine = Machine::new(&scratch).mkinitcpio("HOOKS=(base raba filesystems)");

    let output = setup(&scratch, &["--root", machine.arg()]);
    // The hook runs this during somebody's `mkinitcpio -P`: a configuration
    // that is merely wrong in places must produce a warning and an image, not a
    // failed kernel update.
    assert!(output.status.success(), "a stray key failed the whole report");
    assert!(stdout(&output).contains("initramfs:"), "{}", stdout(&output));
}
