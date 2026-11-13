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
