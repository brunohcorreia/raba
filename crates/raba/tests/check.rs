//! `raba --check`, and the two kernel parameters that make the binary do
//! nothing.
//!
//! These drive the real binary, because what is being tested is not the parser —
//! `tests/config.rs` does that — but the wiring: that a configuration problem
//! reaches an exit code, that `raba.enable=0` really does stop before anything
//! is touched, and that `raba.debug=1` writes the report somewhere a user can
//! be asked to paste from.
//!
//! Only the paths that draw nothing are exercised here. A test that started the
//! daemon would take a virtual terminal away from whoever is running
//! `cargo test`, which is what `crates/raba-vmtest` is for.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The repository's own themes, which are real `.raba` files.
fn themes() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes")
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let mut path = std::env::temp_dir();
        path.push(format!("raba-check-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch");
        Scratch(path)
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, text).expect("writing");
        path
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run the binary with every path pointed inside the scratch directory, so that
/// a test can never read — or write — the machine it is running on.
fn raba(scratch: &Scratch, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_raba"))
        .arg("--config")
        .arg(scratch.path("raba.conf"))
        .arg("--defaults")
        .arg(scratch.path("raba.defaults"))
        .arg("--themes")
        .arg(themes())
        .arg("--cmdline")
        .arg(scratch.path("cmdline"))
        .arg("--os-release")
        .arg(scratch.path("os-release"))
        .arg("--socket")
        .arg(scratch.path("raba.sock"))
        .arg("--state")
        .arg(scratch.path("raba.state"))
        .args(args)
        .output()
        .expect("running raba")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn a_configuration_that_names_a_real_theme_checks_out() {
    let scratch = Scratch::new("good");
    scratch.write("raba.conf", "[Boot]\nTheme=arch\nTimeout=20\n");
    let output = raba(&scratch, &["--check"]);
    let report = stdout(&output);

    assert!(output.status.success(), "{report}{}", String::from_utf8_lossy(&output.stderr));
    assert!(report.contains("arch.raba"), "{report}");
    assert!(report.contains("problems: none"), "{report}");
    // The theme is decoded, not just found: `--check` is the answer to "will
    // this play", and a file that exists and does not decode is the case that
    // question is really about.
    assert!(report.contains("frames"), "{report}");
}

#[test]
fn check_is_strict_where_the_boot_path_is_lenient() {
    let scratch = Scratch::new("typo");
    scratch.write("raba.conf", "[Boot]\nThem=arch\n");
    let output = raba(&scratch, &["--check"]);
    let report = stdout(&output);

    assert!(!output.status.success(), "a typo has to be an exit code somewhere");
    assert!(report.contains("Them is not a key"), "{report}");
}

#[test]
fn a_theme_that_resolves_to_nothing_is_a_failed_check() {
    let scratch = Scratch::new("nothing");
    scratch.write("raba.conf", "[Boot]\nTheme=no-such-distribution\n");
    let output = raba(&scratch, &["--check"]);
    let report = stdout(&output);

    assert!(!output.status.success(), "{report}");
    assert!(report.contains("no-such-distribution.raba"), "{report}");
}

#[test]
fn a_theme_that_is_not_a_theme_is_a_failed_check() {
    let scratch = Scratch::new("garbage");
    let path = scratch.write("fake.raba", "this is not a .raba file");
    let output = raba(&scratch, &["--check", "--theme", &path.display().to_string()]);
    let report = stdout(&output);

    assert!(!output.status.success(), "{report}");
    assert!(report.contains("theme:"), "{report}");
}

#[test]
fn the_kill_switch_stops_before_anything_is_touched() {
    let scratch = Scratch::new("kill");
    scratch.write("raba.conf", "[Boot]\nTheme=arch\n");
    scratch.write("cmdline", "root=/dev/sda2 quiet raba.enable=0\n");
    let output = raba(&scratch, &[]);

    assert!(output.status.success());
    assert!(!scratch.path("raba.sock").exists(), "a disabled raba bound a socket");
    assert!(!scratch.path("raba.state").exists(), "a disabled raba wrote state");
}

#[test]
fn debug_writes_the_report_to_the_log_and_draws_nothing() {
    let scratch = Scratch::new("debug");
    let log = scratch.path("raba.log");
    scratch.write("raba.conf", &format!("[Boot]\nTheme=arch\n[Log]\nFile={}\n", log.display()));
    scratch.write("cmdline", "quiet raba.debug=1\n");
    let output = raba(&scratch, &[]);

    assert!(output.status.success());
    let written = std::fs::read_to_string(&log).expect("a log");
    assert!(written.contains("arch.raba"), "{written}");
    assert!(!scratch.path("raba.sock").exists(), "a debug run bound a socket");
}

#[test]
fn an_unknown_option_is_a_usage_error_and_not_a_boot() {
    let scratch = Scratch::new("usage");
    let output = raba(&scratch, &["--splash"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown option"));
}

#[test]
fn help_and_version_are_not_errors() {
    let scratch = Scratch::new("help");
    for flag in ["--help", "--version"] {
        let output = raba(&scratch, &[flag]);
        assert!(output.status.success(), "{flag}");
        assert!(!stdout(&output).is_empty(), "{flag}");
    }
}
