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

// --- the options, one at a time ----------------------------------------------
//
// Everything below was added when coverage said `main.rs` was at 45%: the verbs
// were exercised and almost none of their options were. An option nobody has
// ever run is an option that does not work, and the person who finds out is a
// theme contributor reading a message that does not match what they typed.

/// A recipe, its art, and the path to build it with.
fn recipe_with(scratch: &Scratch, name: &str, art: &str, body: &str) -> PathBuf {
    scratch.write(&format!("{name}.txt"), art);
    scratch.write(&format!("{name}.toml"), &format!("[logo]\nsource = \"file:{name}.txt\"\n{body}"))
}

fn utf8(path: &std::path::Path) -> &str {
    path.to_str().expect("a utf-8 path")
}

#[test]
fn an_option_that_wants_a_value_and_does_not_get_one_says_so() {
    for option in ["-o", "--columns", "--lines", "--frames", "--delay", "--seed"] {
        let output = gen(&["generate", "matrix", option]);
        assert!(!output.status.success(), "{option} with no value exited 0");
        let said = stderr(&output);
        assert!(
            said.contains(option) || said.contains("needs"),
            "{option} refused without naming itself: {said}"
        );
    }
}

#[test]
fn a_number_that_is_not_one_is_refused_with_the_option_that_wanted_it() {
    let output = gen(&["generate", "matrix", "--columns", "wide"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("--columns"), "{}", stderr(&output));
}

/// Both verbs, because they disagreed: `convert --columns 70000` refused and
/// `generate --columns 70000` quietly drew an 80-column canvas.
#[test]
fn a_number_too_large_for_the_field_is_refused_rather_than_defaulted() {
    let scratch = Scratch::new("too-large");
    let art = scratch.write("hi.txt", "hi\n");
    let cases: [(Vec<&str>, &str); 5] = [
        (vec!["convert", utf8(&art), "--columns", "70000"], "--columns"),
        (vec!["generate", "matrix", "--columns", "70000"], "--columns"),
        (vec!["generate", "matrix", "--lines", "70000"], "--lines"),
        (vec!["generate", "matrix", "--frames", "70000"], "--frames"),
        (vec!["generate", "matrix", "--delay", "70000"], "--delay"),
    ];
    for (args, option) in cases {
        let output = gen(&args);
        assert!(!output.status.success(), "{args:?} accepted a number that does not fit");
        assert!(stderr(&output).contains(option), "{args:?}: {}", stderr(&output));
    }
}

#[test]
fn an_unknown_option_is_refused_by_every_verb_and_the_usage_comes_with_it() {
    let scratch = Scratch::new("unknown-option");
    let recipe = recipe_with(&scratch, "art", "##\n", "");
    let art = scratch.write("plain.txt", "hi\n");
    let cases: [Vec<&str>; 3] = [
        vec!["build", utf8(&recipe), "--jazz"],
        vec!["convert", utf8(&art), "--jazz"],
        vec!["generate", "matrix", "--jazz"],
    ];
    for args in cases {
        let output = gen(&args);
        assert!(!output.status.success(), "{args:?} accepted --jazz");
        let said = stderr(&output);
        assert!(said.contains("--jazz"), "{args:?} did not name the option: {said}");
        assert!(said.contains("usage:"), "{args:?} refused without the usage: {said}");
    }
}

// --- --list-logos -------------------------------------------------------------

#[test]
fn list_logos_names_the_logos_and_credits_where_they_came_from() {
    let output = gen(&["--list-logos", "arch"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("arch"), "no arch logo in the list:\n{text}");
    // The art is fastfetch's, under MIT, and the distribution marks belong to
    // their projects. A list that does not say so is a licence problem, not a
    // formatting one. See themes/README.md.
    assert!(text.to_lowercase().contains("fastfetch"), "the list does not credit fastfetch");
}

#[test]
fn list_logos_marks_which_ones_a_console_can_actually_draw() {
    let output = gen(&["--list-logos"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains("console"),
        "nothing is marked as console-safe:\n{}",
        &text[..200.min(text.len())]
    );
    assert!(text.contains("terminal"), "nothing is marked as terminal-only");
}

#[test]
fn a_pattern_that_matches_no_logo_is_an_error_that_says_how_many_there_are() {
    let output = gen(&["--list-logos", "zzzznotadistribution"]);
    assert!(!output.status.success(), "a pattern matching nothing exited 0");
    assert!(stderr(&output).contains("zzzznotadistribution"), "{}", stderr(&output));
}

// --- build --------------------------------------------------------------------

#[test]
fn a_fastfetch_logo_can_be_named_instead_of_supplying_art() {
    let scratch = Scratch::new("fastfetch-logo");
    let recipe = scratch.write("arch.toml", "[logo]\nsource = \"fastfetch:arch\"\n");
    let output = gen(&["build", utf8(&recipe), "--allow-unsafe-glyphs"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(scratch.path("arch.raba").exists());
    // The logo brings its own colours when the recipe gives none, and says so:
    // an author who did not choose them should know they were chosen.
    assert!(stderr(&output).contains("logo.colors"), "{}", stderr(&output));
}

#[test]
fn a_logo_nobody_has_is_refused_with_a_suggestion_rather_than_a_list_of_534() {
    let scratch = Scratch::new("misspelt-logo");
    let recipe = scratch.write("x.toml", "[logo]\nsource = \"fastfetch:archh\"\n");
    let output = gen(&["build", utf8(&recipe)]);
    assert!(!output.status.success());
    let said = stderr(&output);
    assert!(said.contains("archh"), "{said}");
    assert!(
        said.contains("did you mean") || said.contains("--list-logos"),
        "refused with no way forward: {said}"
    );
}

#[test]
fn a_source_that_is_neither_a_file_a_logo_nor_inline_is_refused() {
    let scratch = Scratch::new("bad-source");
    let recipe = scratch.write("x.toml", "[logo]\nsource = \"carrier-pigeon:arch\"\n");
    let output = gen(&["build", utf8(&recipe)]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("carrier-pigeon"), "{}", stderr(&output));
}

#[test]
fn a_recipe_key_that_is_accepted_and_not_acted_on_says_so_out_loud() {
    // The alternative is an author setting canvas.align, seeing nothing move,
    // and concluding the tool is broken. See `Theme::unhonoured`.
    let scratch = Scratch::new("unhonoured");
    let recipe = recipe_with(&scratch, "art", "##\n##\n", "\n[canvas]\nx_offset = 4\n");
    let output = gen(&["build", utf8(&recipe)]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stderr(&output).contains("x_offset"), "{}", stderr(&output));
}

#[test]
fn art_using_a_colour_slot_the_recipe_never_filled_is_warned_about() {
    let scratch = Scratch::new("missing-slot");
    let recipe = recipe_with(&scratch, "art", "$3##\n", "");
    let output = gen(&["build", utf8(&recipe)]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("$3"),
        "the unfilled slot was not mentioned: {}",
        stderr(&output)
    );
}

#[test]
fn a_theme_with_a_glyph_no_console_can_draw_is_refused_unless_it_is_asked_for() {
    let scratch = Scratch::new("unsafe-glyph");
    // An emoji: there is no console font with this, and the runtime would draw a
    // space. At build time there is somebody who can fix it.
    let recipe = recipe_with(&scratch, "art", "\u{1f600}\u{1f600}\n", "");

    let refused = gen(&["build", utf8(&recipe)]);
    assert!(!refused.status.success(), "an undrawable glyph was accepted silently");
    assert!(stderr(&refused).contains("--allow-unsafe-glyphs"), "{}", stderr(&refused));
    assert!(!scratch.path("art.raba").exists(), "it refused and wrote the file anyway");

    let allowed = gen(&["build", utf8(&recipe), "--allow-unsafe-glyphs"]);
    assert!(allowed.status.success(), "{}", stderr(&allowed));
    assert!(scratch.path("art.raba").exists());
    // Allowed is not silent: the warning still names the character and the count.
    assert!(stderr(&allowed).contains("cell(s)"), "{}", stderr(&allowed));
}

#[test]
fn preview_plays_the_theme_and_writes_no_file() {
    let scratch = Scratch::new("preview-build");
    let recipe = recipe_with(&scratch, "art", "##\n##\n", "");
    let output = gen(&["build", utf8(&recipe), "--preview"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        !scratch.path("art.raba").exists(),
        "--preview is for looking at a theme without leaving an artifact behind"
    );
    assert!(!output.stdout.is_empty(), "--preview drew nothing");
}

// --- --also-dur ---------------------------------------------------------------

#[test]
fn also_dur_writes_a_second_file_beside_the_first() {
    let scratch = Scratch::new("also-dur");
    let recipe = recipe_with(&scratch, "art", "##\n##\n", "");
    let output = gen(&["build", utf8(&recipe), "--also-dur"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(scratch.path("art.raba").exists(), "the .raba is still the main output");
    assert!(scratch.path("art.dur").exists(), "no .dur beside it");
    assert!(stdout(&output).contains("Durdraw"), "{}", stdout(&output));
}

#[test]
fn also_dur_takes_a_path_of_its_own_when_one_is_given() {
    let scratch = Scratch::new("also-dur-path");
    let recipe = recipe_with(&scratch, "art", "##\n##\n", "");
    let elsewhere = scratch.path("for-ly.dur");
    let output = gen(&["build", utf8(&recipe), "--also-dur", utf8(&elsewhere)]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(elsewhere.exists(), "the named .dur was not written");
    assert!(!scratch.path("art.dur").exists(), "it wrote the default one as well");
}

#[test]
fn the_durdraw_version_can_be_chosen_and_an_unknown_one_is_refused() {
    let scratch = Scratch::new("dur-version");
    let recipe = recipe_with(&scratch, "art", "##\n##\n", "");

    for version in ["7", "8"] {
        let out = scratch.path(&format!("v{version}.dur"));
        let output =
            gen(&["build", utf8(&recipe), "--also-dur", utf8(&out), "--dur-version", version]);
        assert!(output.status.success(), "v{version}: {}", stderr(&output));
        assert!(stdout(&output).contains(&format!("Durdraw v{version}")), "{}", stdout(&output));
    }

    let refused = gen(&["build", utf8(&recipe), "--also-dur", "--dur-version", "9"]);
    assert!(!refused.status.success(), "Durdraw v9 was accepted");
    assert!(stderr(&refused).contains("7 or 8"), "{}", stderr(&refused));
}

// --- convert ------------------------------------------------------------------

#[test]
fn the_format_can_be_overridden_when_the_file_does_not_look_like_what_it_is() {
    let scratch = Scratch::new("format-override");
    // Bytes that sniff as plain text, declared as text explicitly: the point is
    // that `--format` is read and acted on, not that it changes the answer here.
    let art = scratch.write("thing.nfo", "plain\nart\n");
    let out = scratch.path("thing.raba");
    let output = gen(&["convert", utf8(&art), "--format", "text", "-o", utf8(&out)]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(out.exists());
}

#[test]
fn a_format_nobody_implements_is_refused_with_the_ones_that_exist() {
    let scratch = Scratch::new("bad-format");
    let art = scratch.write("thing.txt", "hi\n");
    let output = gen(&["convert", utf8(&art), "--format", "petscii"]);
    assert!(!output.status.success());
    let said = stderr(&output);
    assert!(said.contains("petscii"), "{said}");
    assert!(
        said.contains("dur") && said.contains("ansi"),
        "it did not list the known ones: {said}"
    );
}

#[test]
fn format_without_a_value_is_refused() {
    let scratch = Scratch::new("format-empty");
    let art = scratch.write("thing.txt", "hi\n");
    let output = gen(&["convert", utf8(&art), "--format"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("dur"), "{}", stderr(&output));
}

#[test]
fn plain_text_takes_the_colours_it_is_given() {
    let scratch = Scratch::new("fg-bg");
    let art = scratch.write("hi.txt", "hi\n");
    let out = scratch.path("hi.raba");
    let output = gen(&["convert", utf8(&art), "--fg", "3", "--bg", "4", "-o", utf8(&out)]);
    assert!(output.status.success(), "{}", stderr(&output));

    let checked = gen(&["--check", utf8(&out)]);
    assert!(checked.status.success(), "{}", stderr(&checked));
}

#[test]
fn a_colour_outside_a_byte_is_refused_rather_than_truncated() {
    let scratch = Scratch::new("bad-colour");
    let art = scratch.write("hi.txt", "hi\n");
    let output = gen(&["convert", utf8(&art), "--fg", "300"]);
    assert!(!output.status.success(), "fg 300 was accepted");
    assert!(stderr(&output).contains("--fg"), "{}", stderr(&output));
}

/// A `.dur` out and the same `.dur` back in, which is the round trip the Ly
/// display manager case depends on — and the only path that carries a name and
/// an author at all.
#[test]
fn a_dur_round_trips_and_convert_says_what_it_claimed_to_be_and_who_made_it() {
    let scratch = Scratch::new("metadata");
    let raba = scratch.path("m.raba");
    let made = gen(&[
        "generate",
        "matrix",
        "--columns",
        "12",
        "--lines",
        "4",
        "--frames",
        "2",
        "-o",
        utf8(&raba),
        "--also-dur",
    ]);
    assert!(made.status.success(), "{}", stderr(&made));
    let dur = scratch.path("m.dur");
    assert!(dur.exists(), "no .dur to read back");

    let back = scratch.path("back.raba");
    let output = gen(&["convert", utf8(&dur), "-o", utf8(&back)]);
    assert!(output.status.success(), "{}", stderr(&output));
    let said = stderr(&output);
    assert!(said.contains("matrix"), "the name did not survive the round trip: {said}");
    assert!(said.contains("raba-gen"), "the author did not survive the round trip: {said}");

    let checked = gen(&["--check", utf8(&back)]);
    assert!(checked.status.success(), "{}", stderr(&checked));
    assert!(stdout(&checked).contains("12x4"), "{}", stdout(&checked));
}

#[test]
fn art_larger_than_the_console_raba_assumes_is_warned_about_with_what_will_happen() {
    let scratch = Scratch::new("oversize");
    let wide: String = std::iter::repeat_n("#".repeat(120), 30).collect::<Vec<_>>().join("\n");
    let art = scratch.write("wide.txt", &format!("{wide}\n"));
    let out = scratch.path("wide.raba");
    let output = gen(&["convert", utf8(&art), "-o", utf8(&out)]);
    assert!(output.status.success(), "{}", stderr(&output));
    let said = stderr(&output);
    assert!(said.contains("80x25"), "the fallback console size was not named: {said}");
    assert!(
        said.contains("cropped") || said.contains("scaled"),
        "it did not say what happens: {said}"
    );
}

#[test]
fn converted_art_with_an_undrawable_glyph_is_refused_the_same_way_a_theme_is() {
    let scratch = Scratch::new("convert-unsafe");
    let art = scratch.write("emoji.txt", "\u{1f600}\n");
    let out = scratch.path("emoji.raba");

    let refused = gen(&["convert", utf8(&art), "-o", utf8(&out)]);
    assert!(!refused.status.success());
    assert!(stderr(&refused).contains("--allow-unsafe-glyphs"), "{}", stderr(&refused));

    let allowed = gen(&["convert", utf8(&art), "-o", utf8(&out), "--allow-unsafe-glyphs"]);
    assert!(allowed.status.success(), "{}", stderr(&allowed));
    assert!(out.exists());
}

#[test]
fn convert_can_preview_instead_of_writing_too() {
    let scratch = Scratch::new("preview-convert");
    let art = scratch.write("hi.txt", "hi\n");
    let output = gen(&["convert", utf8(&art), "--preview"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!scratch.path("hi.raba").exists(), "--preview wrote a file");
    assert!(!output.stdout.is_empty(), "--preview drew nothing");
}

// --- generate -------------------------------------------------------------------

#[test]
fn the_palette_can_be_chosen_and_anything_else_is_refused() {
    let scratch = Scratch::new("palette");
    for (depth, expected) in [("16", "16"), ("256", "256")] {
        let out = scratch.path(&format!("p{depth}.raba"));
        let output = gen(&[
            "generate",
            "matrix",
            "--columns",
            "12",
            "--lines",
            "4",
            "--frames",
            "2",
            "--palette",
            depth,
            "-o",
            utf8(&out),
        ]);
        assert!(output.status.success(), "{}", stderr(&output));
        let checked = gen(&["--check", utf8(&out)]);
        assert!(stdout(&checked).contains(expected), "{}", stdout(&checked));
    }

    let refused = gen(&["generate", "matrix", "--palette", "truecolor"]);
    assert!(!refused.status.success(), "--palette truecolor was accepted");
    assert!(stderr(&refused).contains("16 or 256"), "{}", stderr(&refused));
}

#[test]
fn generate_with_no_kind_lists_the_ones_there_are() {
    let output = gen(&["generate"]);
    assert!(!output.status.success());
    let said = stderr(&output);
    assert!(said.contains("matrix") && said.contains("life"), "it did not list the kinds: {said}");
}

#[test]
fn generate_can_preview_and_can_write_a_dur_as_well() {
    let scratch = Scratch::new("generate-extras");
    let preview = gen(&[
        "generate",
        "matrix",
        "--columns",
        "12",
        "--lines",
        "4",
        "--frames",
        "2",
        "--preview",
    ]);
    assert!(preview.status.success(), "{}", stderr(&preview));
    assert!(!preview.stdout.is_empty(), "--preview drew nothing");

    let out = scratch.path("m.raba");
    let also = gen(&[
        "generate",
        "matrix",
        "--columns",
        "12",
        "--lines",
        "4",
        "--frames",
        "2",
        "-o",
        utf8(&out),
        "--also-dur",
    ]);
    assert!(also.status.success(), "{}", stderr(&also));
    assert!(scratch.path("m.dur").exists(), "no .dur beside the .raba");
}

// --- --check ---------------------------------------------------------------------

#[test]
fn check_reports_the_size_the_frames_and_the_palette() {
    let scratch = Scratch::new("check-report");
    let out = scratch.path("c.raba");
    let built = gen(&[
        "generate",
        "fire",
        "--columns",
        "14",
        "--lines",
        "6",
        "--frames",
        "5",
        "-o",
        utf8(&out),
    ]);
    assert!(built.status.success(), "{}", stderr(&built));

    let output = gen(&["--check", utf8(&out)]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("14x6"), "{text}");
    assert!(text.contains("5 frames"), "{text}");
    assert!(text.contains("colours"), "{text}");
}

#[test]
fn check_refuses_a_theme_with_glyphs_no_console_can_draw_unless_told_not_to() {
    let scratch = Scratch::new("check-unsafe");
    let art = scratch.write("emoji.txt", "\u{1f600}\n");
    let out = scratch.path("emoji.raba");
    let built = gen(&["convert", utf8(&art), "-o", utf8(&out), "--allow-unsafe-glyphs"]);
    assert!(built.status.success(), "{}", stderr(&built));

    let refused = gen(&["--check", utf8(&out)]);
    assert!(!refused.status.success(), "--check passed a theme the console cannot draw");
    assert!(stderr(&refused).contains("--allow-unsafe-glyphs"), "{}", stderr(&refused));

    let allowed = gen(&["--check", utf8(&out), "--allow-unsafe-glyphs"]);
    assert!(allowed.status.success(), "{}", stderr(&allowed));
}

#[test]
fn check_without_a_file_is_a_usage_error() {
    let output = gen(&["--check"]);
    assert!(!output.status.success(), "--check with no file exited 0");
}
