//! The command line of the authoring tool.
//!
//! Coverage said 0% of `main.rs` — 709 regions, the largest unexercised file in
//! the workspace outside the guest program that runs in a VM. The library
//! underneath is tested thoroughly by the twenty other files in this directory;
//! what nothing touched was the part a contributor actually types.
//!
//! This is build-time code and `docs/maintenance.md` is explicit that a
//! dependency breaking `raba-gen` is "a build failure someone fixes at
//! leisure". That is a reason to spend fewer tests here than on the boot path,
//! not a reason to spend none: a theme contributor meeting a wrong error
//! message is the first impression this project makes.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let mut path = std::env::temp_dir();
        path.push(format!("raba-gen-cli-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch");
        Scratch(path)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.path(name);
        std::fs::write(&path, text).expect("writing");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn gen(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_raba-gen")).args(args).output().expect("running raba-gen")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

// --- the parser --------------------------------------------------------------

#[test]
fn usage_lists_every_verb() {
    let output = gen(&["--help"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    for verb in ["build", "convert", "generate", "--check", "--list-logos"] {
        assert!(text.contains(verb), "--help does not mention {verb}:\n{text}");
    }
}

#[test]
fn no_arguments_at_all_is_a_usage_error_and_not_a_silent_success() {
    let output = gen(&[]);
    assert!(!output.status.success(), "an empty command line exited 0");
}

#[test]
fn an_unknown_verb_is_refused() {
    let output = gen(&["transmogrify", "thing.toml"]);
    assert!(!output.status.success());
    assert!(!stderr(&output).is_empty(), "it refused without saying why");
}

#[test]
fn the_version_names_the_format_version_too() {
    // Two different numbers, and a bug report that gives only the first is a
    // bug report that cannot say whether the file or the reader is at fault.
    let output = gen(&["--version"]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains(env!("CARGO_PKG_VERSION")), "{text}");
    assert!(text.to_lowercase().contains("format"), "{text}");
}

// --- generate, which needs no input file -------------------------------------

#[test]
fn generate_writes_a_file_the_checker_accepts() {
    let scratch = Scratch::new("generate");
    let out = scratch.path("matrix.raba");
    let output = gen(&[
        "generate",
        "matrix",
        "--columns",
        "20",
        "--lines",
        "5",
        "--frames",
        "3",
        "--delay",
        "40",
        "-o",
        out.to_str().expect("utf-8"),
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(out.exists(), "nothing was written");

    let checked = gen(&["--check", out.to_str().expect("utf-8")]);
    assert!(checked.status.success(), "{}", stderr(&checked));
    assert!(stdout(&checked).contains("3 frames"), "{}", stdout(&checked));
}

#[test]
fn the_same_seed_gives_the_same_file() {
    // Documented in the usage, and the reason a generated fixture can be a
    // golden test at all.
    let scratch = Scratch::new("seed");
    let once = |name: &str| {
        let out = scratch.path(name);
        let output = gen(&[
            "generate",
            "life",
            "--columns",
            "16",
            "--lines",
            "8",
            "--frames",
            "4",
            "--seed",
            "12345",
            "-o",
            out.to_str().expect("utf-8"),
        ]);
        assert!(output.status.success(), "{}", stderr(&output));
        std::fs::read(&out).expect("reading")
    };
    assert_eq!(once("one.raba"), once("two.raba"), "the same seed produced two different files");
}

#[test]
fn a_generator_nobody_wrote_is_refused_by_name() {
    let output = gen(&["generate", "nonsense"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("nonsense"), "{}", stderr(&output));
}

// --- convert -----------------------------------------------------------------

#[test]
fn plain_text_converts_and_the_size_follows_the_art() {
    let scratch = Scratch::new("text");
    let art = scratch.write("hello.txt", "hello\nthere\n");
    let out = scratch.path("hello.raba");
    let output =
        gen(&["convert", art.to_str().expect("utf-8"), "-o", out.to_str().expect("utf-8")]);
    assert!(output.status.success(), "{}", stderr(&output));

    let checked = gen(&["--check", out.to_str().expect("utf-8")]);
    assert!(checked.status.success(), "{}", stderr(&checked));
    assert!(stdout(&checked).contains("5x2"), "{}", stdout(&checked));
}

#[test]
fn a_file_that_is_not_there_is_refused_by_name() {
    let output = gen(&["convert", "/nonexistent/art.txt"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("art.txt"), "{}", stderr(&output));
}

// --- build -------------------------------------------------------------------

#[test]
fn a_recipe_builds_and_the_default_output_sits_beside_it() {
    let scratch = Scratch::new("build");
    scratch.write("logo.txt", "##\n##\n");
    let recipe = scratch.write("thing.toml", "[logo]\nsource = \"file:logo.txt\"\n");
    let output = gen(&["build", recipe.to_str().expect("utf-8")]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(scratch.path("thing.raba").exists(), "no thing.raba beside the recipe");
}

#[test]
fn a_recipe_that_is_not_toml_is_refused_with_the_line() {
    let scratch = Scratch::new("bad-toml");
    let recipe = scratch.write("broken.toml", "name = \nthis is not toml\n");
    let output = gen(&["build", recipe.to_str().expect("utf-8")]);
    assert!(!output.status.success());
    assert!(!stderr(&output).is_empty(), "it refused without saying why");
}

#[test]
fn checking_something_that_is_not_a_theme_fails_rather_than_panicking() {
    let scratch = Scratch::new("not-a-theme");
    let path = scratch.write("notatheme.raba", "this is not a raba file at all\n");
    let output = gen(&["--check", path.to_str().expect("utf-8")]);
    assert!(!output.status.success());
    assert!(!stderr(&output).is_empty() || !stdout(&output).is_empty());
}
