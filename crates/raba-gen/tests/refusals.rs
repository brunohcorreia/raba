//! What `raba-gen` does when the filesystem, or the person typing, says no.
//!
//! `tests/cli.rs` drives the paths that work and the arguments that are wrong.
//! What it leaves alone is the other half of every line that ends in `?`: the
//! recipe that is not there, the output path that cannot be written, the art
//! file a recipe points at that was renamed. Coverage showed those as the
//! largest remaining hole in `main.rs`, and they are not an edge case — a
//! mistyped path is the most ordinary thing that happens to anybody using this
//! tool for the first time.
//!
//! The rule under test throughout: **a refusal names the file it is about.**
//! `raba-gen` is run from a Makefile over a directory of recipes, and "No such
//! file or directory (os error 2)" with no path in front of it is a message
//! that sends somebody to read the Makefile.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let mut path = std::env::temp_dir();
        path.push(format!("raba-gen-refusals-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch");
        Scratch(path)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn write(&self, name: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.path(name);
        std::fs::write(&path, bytes).expect("writing");
        path
    }

    /// A directory, which is the portable way to have a path that exists and
    /// cannot be written to as a file. Running the suite as root — which
    /// happens, this project has a VM tier — makes a 0444 file writable again
    /// and would turn these into tests that pass for the wrong reason.
    fn directory(&self, name: &str) -> PathBuf {
        let path = self.path(name);
        std::fs::create_dir_all(&path).expect("a directory");
        path
    }

    /// A recipe and the art it points at.
    fn recipe(&self, name: &str, art: &str) -> PathBuf {
        self.write(&format!("{name}.txt"), art);
        self.write(&format!("{name}.toml"), format!("[logo]\nsource = \"file:{name}.txt\"\n"))
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

fn utf8(path: &Path) -> &str {
    path.to_str().expect("a utf-8 path")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Assert that the command failed and said each of `must_say`.
fn refused(output: &Output, must_say: &[&str]) {
    assert!(!output.status.success(), "exited 0; stderr was {:?}", stderr(output));
    let said = stderr(output);
    for fragment in must_say {
        assert!(said.contains(fragment), "{said:?} does not mention {fragment:?}");
    }
}

// --- a verb with nothing after it --------------------------------------------

#[test]
fn build_with_no_recipe_says_what_it_wanted() {
    refused(&gen(&["build"]), &["recipe"]);
}

#[test]
fn convert_with_no_file_says_what_it_wanted() {
    refused(&gen(&["convert"]), &["file"]);
}

// --- a path that is not there -------------------------------------------------

#[test]
fn a_recipe_that_does_not_exist_is_named_in_the_refusal() {
    let scratch = Scratch::new("no-recipe");
    let missing = scratch.path("nothing-here.toml");
    refused(&gen(&["build", utf8(&missing)]), &["nothing-here.toml", "No such file"]);
}

#[test]
fn a_file_to_convert_that_does_not_exist_is_named_in_the_refusal() {
    let scratch = Scratch::new("no-input");
    let missing = scratch.path("nothing-here.ans");
    refused(&gen(&["convert", utf8(&missing)]), &["nothing-here.ans", "No such file"]);
}

#[test]
fn art_a_recipe_points_at_that_is_not_there_names_both_files() {
    // The recipe parsed; what is missing is the thing it refers to. Naming
    // only one of the two sends the reader to the wrong file.
    let scratch = Scratch::new("no-art");
    let recipe = scratch.write("thing.toml", "[logo]\nsource = \"file:gone.txt\"\n");
    let output = gen(&["build", utf8(&recipe)]);
    refused(&output, &["thing.toml", "gone.txt"]);
}

// --- a path that cannot be written -------------------------------------------

#[test]
fn an_output_path_that_cannot_be_written_is_a_failure_and_not_a_silent_success() {
    let scratch = Scratch::new("build-unwritable");
    let recipe = scratch.recipe("art", "##\n##\n");
    let blocked = scratch.directory("taken.raba");
    refused(&gen(&["build", utf8(&recipe), "-o", utf8(&blocked)]), &["taken.raba"]);
}

#[test]
fn a_conversion_that_cannot_be_written_is_a_failure_and_not_a_silent_success() {
    let scratch = Scratch::new("convert-unwritable");
    let art = scratch.write("art.txt", "hello\n");
    let blocked = scratch.directory("taken.raba");
    refused(&gen(&["convert", utf8(&art), "-o", utf8(&blocked)]), &["taken.raba"]);
}

#[test]
fn a_second_dur_file_that_cannot_be_written_fails_the_whole_command() {
    // The .raba is written first. If the --also-dur beside it cannot be, that
    // is still a failed command: a build system that treats it as success
    // ships half of what was asked for.
    let scratch = Scratch::new("also-dur-unwritable");
    let recipe = scratch.recipe("art", "##\n##\n");
    let blocked = scratch.directory("taken.dur");
    refused(&gen(&["build", utf8(&recipe), "--also-dur", utf8(&blocked)]), &["taken.dur"]);
}

#[test]
fn a_generated_animation_that_cannot_be_written_is_a_failure() {
    let scratch = Scratch::new("generate-unwritable");
    let blocked = scratch.directory("taken.raba");
    refused(
        &gen(&["generate", "life", "-o", utf8(&blocked), "--columns", "20", "--lines", "8"]),
        &["taken.raba"],
    );
}

// --- what a file is, when the name does not say ------------------------------

#[test]
fn a_durdraw_file_under_any_name_is_read_as_durdraw_and_the_guess_is_reported() {
    // gzip's two magic bytes. The extension says nothing, so the contents
    // decide — and because they did, the guess is printed, since the one way
    // out of a wrong guess is knowing one was made.
    let scratch = Scratch::new("detect-dur");
    let disguised = scratch.write("mystery.bin", [0x1f, 0x8b, 0x00, 0x00]);
    let output = gen(&["convert", utf8(&disguised)]);
    let said = stderr(&output);
    assert!(said.contains("read as dur"), "{said:?}");
    assert!(said.contains("--format overrides"), "{said:?}: say how to correct the guess");
    // And it then fails, because four bytes is not a gzip member. The point of
    // the test is which importer ran.
    assert!(!output.status.success());
}

#[test]
fn a_file_with_an_escape_sequence_in_it_is_read_as_ansi_whatever_it_is_called() {
    let scratch = Scratch::new("detect-ansi");
    let disguised = scratch.write("mystery.nfo", "\x1b[31mred\x1b[0m\n");
    let output = gen(&["convert", utf8(&disguised), "-o", utf8(&scratch.path("out.raba"))]);
    assert!(output.status.success(), "{}", stderr(&output));
    let said = stderr(&output);
    assert!(said.contains("read as ansi"), "{said:?}");
}

#[test]
fn a_file_named_ans_is_taken_at_its_word_without_a_guess_being_reported() {
    // The extension settles it, so there is no guess to warn about. A line
    // saying "read as ansi" on every .ans file is noise that trains people to
    // stop reading stderr.
    let scratch = Scratch::new("detect-named");
    let named = scratch.write("art.ans", "\x1b[31mred\x1b[0m\n");
    let output = gen(&["convert", utf8(&named), "-o", utf8(&scratch.path("out.raba"))]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!stderr(&output).contains("read as"), "{:?}", stderr(&output));
}

#[test]
fn format_overrides_what_the_contents_look_like() {
    // The same escape sequences, read as text: the file is then the escape
    // characters themselves rather than one line of red. Which is why it needs
    // --allow-unsafe-glyphs — ESC is in no console font, and the glyph check
    // catching that is the check doing its job, not a reason to skip it.
    let scratch = Scratch::new("format-override");
    let art = scratch.write("art.ans", "\x1b[31mred\x1b[0m\n");
    let out = scratch.path("out.raba");
    let as_text = gen(&[
        "convert",
        utf8(&art),
        "--format",
        "text",
        "--allow-unsafe-glyphs",
        "-o",
        utf8(&out),
    ]);
    assert!(as_text.status.success(), "{}", stderr(&as_text));
    let text_size = std::fs::metadata(&out).expect("the file").len();

    // And without the override the same file is ANSI art: red text, no ESC in
    // it anywhere, and so no complaint about glyphs.
    let as_ansi = gen(&["convert", utf8(&art), "-o", utf8(&scratch.path("ansi.raba"))]);
    assert!(as_ansi.status.success(), "{}", stderr(&as_ansi));
    assert!(!stderr(&as_ansi).contains("cannot draw"), "{:?}", stderr(&as_ansi));
    assert_ne!(
        text_size,
        std::fs::metadata(scratch.path("ansi.raba")).expect("the file").len(),
        "--format changed nothing"
    );
}

#[test]
fn an_unknown_format_name_is_refused_rather_than_guessed_at() {
    let scratch = Scratch::new("format-unknown");
    let art = scratch.write("art.txt", "hello\n");
    refused(&gen(&["convert", utf8(&art), "--format", "rle"]), &["rle"]);
}

// --- what the importer has to say --------------------------------------------

#[test]
fn an_ansi_file_that_is_too_wide_for_the_canvas_is_refused_by_name() {
    let scratch = Scratch::new("ansi-wide");
    let wide = "x".repeat(2000);
    let art = scratch.write("wide.ans", format!("{wide}\n"));
    refused(&gen(&["convert", utf8(&art), "--columns", "2000"]), &["wide.ans"]);
}

#[test]
fn an_empty_file_is_refused_by_name_rather_than_written_as_an_empty_theme() {
    let scratch = Scratch::new("empty");
    let art = scratch.write("empty.txt", "");
    refused(&gen(&["convert", utf8(&art)]), &["empty.txt"]);
}

// --- numbers that do not fit --------------------------------------------------

#[test]
fn every_numeric_option_refuses_a_number_too_large_for_it_instead_of_wrapping() {
    // `u16::try_from(70000)` and `u8::try_from(300)` are the whole subject.
    // Before these refused, --columns 70000 silently became the default and
    // the file that came out was the wrong size with no warning anywhere.
    let scratch = Scratch::new("too-big");
    let art = scratch.write("art.txt", "hi\n");
    for (flag, value) in [("--columns", "70000"), ("--fg", "300"), ("--bg", "300")] {
        let output = gen(&["convert", utf8(&art), flag, value]);
        refused(&output, &[flag]);
    }
}

#[test]
fn a_numeric_option_given_something_that_is_not_a_number_says_which_option() {
    let scratch = Scratch::new("not-a-number");
    let art = scratch.write("art.txt", "hi\n");
    refused(&gen(&["convert", utf8(&art), "--columns", "wide"]), &["--columns"]);
}

#[test]
fn a_numeric_option_given_nothing_at_all_says_which_option() {
    let scratch = Scratch::new("no-number");
    let art = scratch.write("art.txt", "hi\n");
    refused(&gen(&["convert", utf8(&art), "--columns"]), &["--columns"]);
}

#[test]
fn an_output_flag_with_no_path_after_it_says_so() {
    let scratch = Scratch::new("no-output");
    let art = scratch.write("art.txt", "hi\n");
    refused(&gen(&["convert", utf8(&art), "-o"]), &["-o", "path"]);
    let recipe = scratch.recipe("thing", "##\n");
    refused(&gen(&["build", utf8(&recipe), "-o"]), &["-o", "path"]);
}

#[test]
fn a_dur_version_this_writer_does_not_produce_is_refused_for_every_verb() {
    let scratch = Scratch::new("dur-version");
    let recipe = scratch.recipe("thing", "##\n");
    let art = scratch.write("art.txt", "hi\n");
    for args in [
        vec!["build", utf8(&recipe), "--also-dur", "--dur-version", "9"],
        vec!["convert", utf8(&art), "--also-dur", "--dur-version", "9"],
        vec!["generate", "life", "--also-dur", "--dur-version", "9"],
    ] {
        refused(&gen(&args), &["7 or 8"]);
    }
}

// --- what the importer has to say that is not a refusal ----------------------

#[test]
fn a_file_that_is_not_utf_8_is_read_as_cp437_and_said_so() {
    // The note is the whole value here: CP437 is a *guess*, and the art coming
    // out with box-drawing characters where the author meant accented letters
    // is something the reader can only correct if they know a guess was made.
    let scratch = Scratch::new("cp437");
    // 0xc9 0xcd 0xbb is the top of a double-line box in CP437 and is not
    // valid UTF-8 in any arrangement.
    let art = scratch.write("box.txt", [0xc9, 0xcd, 0xbb, b'\n']);
    let output = gen(&["convert", utf8(&art), "-o", utf8(&scratch.path("out.raba"))]);
    assert!(output.status.success(), "{}", stderr(&output));
    let said = stderr(&output);
    assert!(said.contains("CP437"), "{said:?}");
    assert!(said.contains("box.txt"), "{said:?}: the note names the file it is about");
}

#[test]
fn ansi_art_that_asks_for_something_a_console_cannot_do_says_which_thing() {
    let scratch = Scratch::new("ansi-notes");
    let art = scratch.write("blink.ans", "\x1b[5mblinking\x1b[0m and \x1b[105mbright\x1b[0m\n");
    let output = gen(&["convert", utf8(&art), "-o", utf8(&scratch.path("out.raba"))]);
    assert!(output.status.success(), "{}", stderr(&output));
    let said = stderr(&output);
    assert!(said.contains("blinking"), "{said:?}");
    assert!(said.contains("bright background"), "{said:?}");
}

#[test]
fn a_durdraw_file_with_a_title_and_no_artist_is_credited_to_unknown() {
    // Round trip: `build --also-dur` writes a .dur carrying the recipe's name
    // and an empty artist, which is exactly the case the import has to have an
    // answer for. An empty pair of quotes where a name goes reads like a bug.
    let scratch = Scratch::new("dur-roundtrip");
    let recipe = scratch.recipe("heraldry", "##\n##\n");
    let built = gen(&["build", utf8(&recipe), "--also-dur"]);
    assert!(built.status.success(), "{}", stderr(&built));
    let dur = scratch.path("heraldry.dur");
    assert!(dur.exists(), "--also-dur wrote nothing");

    let back = gen(&["convert", utf8(&dur), "-o", utf8(&scratch.path("back.raba"))]);
    assert!(back.status.success(), "{}", stderr(&back));
    let said = stderr(&back);
    assert!(said.contains("heraldry"), "{said:?}: the title survives the round trip");
    assert!(said.contains("unknown"), "{said:?}: no artist is `unknown`, not an empty string");
}

// --- where the output goes when nobody says ----------------------------------

#[test]
fn a_conversion_with_no_output_path_lands_beside_its_input() {
    // `raba-gen convert arch.ans` writing `arch.raba` in the same directory is
    // the whole command line most people ever type.
    let scratch = Scratch::new("convert-default-out");
    let art = scratch.write("arch.txt", "hello\n");
    let output = gen(&["convert", utf8(&art)]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(scratch.path("arch.raba").exists(), "nothing was written beside the input");
}

#[test]
fn a_generated_animation_with_no_output_path_is_named_after_what_it_generates() {
    // There is no input file to sit beside, so the name comes from the
    // generator — its canonical name and not the alias that was typed, so
    // `generate life` and `generate gameoflife` write the same file rather
    // than two copies of it.
    let scratch = Scratch::new("generate-default-out");
    let ran = Command::new(env!("CARGO_BIN_EXE_raba-gen"))
        .args(["generate", "life", "--columns", "20", "--lines", "8", "--frames", "4"])
        .current_dir(&scratch.0)
        .output()
        .expect("running raba-gen");
    assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
    assert!(scratch.path("gameoflife.raba").exists(), "nothing was written for `life`");
    assert!(!scratch.path("life.raba").exists(), "the alias was used as the file name");
}

#[test]
fn a_second_dur_file_beside_a_conversion_or_a_generation_fails_the_same_way() {
    // The .raba is written first in all three verbs; `build` is covered above
    // and these are the other two.
    let scratch = Scratch::new("also-dur-others");
    let art = scratch.write("art.txt", "hi\n");
    let blocked = scratch.directory("taken.dur");
    refused(&gen(&["convert", utf8(&art), "--also-dur", utf8(&blocked)]), &["taken.dur"]);
    refused(
        &gen(&[
            "generate",
            "life",
            "--columns",
            "20",
            "--lines",
            "8",
            "--also-dur",
            utf8(&blocked),
        ]),
        &["taken.dur"],
    );
}

// --- the checker ---------------------------------------------------------------

#[test]
fn checking_a_file_that_is_not_there_names_it() {
    let scratch = Scratch::new("check-missing");
    refused(&gen(&["--check", utf8(&scratch.path("gone.raba"))]), &["gone.raba", "No such file"]);
}

#[test]
fn checking_something_that_is_not_a_raba_file_says_so_rather_than_crashing() {
    let scratch = Scratch::new("check-junk");
    let junk = scratch.write("junk.raba", "this is not a theme at all");
    refused(&gen(&["--check", utf8(&junk)]), &["junk.raba"]);
}

#[test]
fn an_unknown_option_to_check_is_refused_rather_than_read_as_a_filename() {
    // Otherwise `--check --verbose thing.raba` reads `--verbose` as the path,
    // then reads `thing.raba` over it, and the typo goes unmentioned.
    refused(&gen(&["--check", "--verbose", "thing.raba"]), &["--verbose"]);
}

// --- numbers that are not numbers ----------------------------------------------

#[test]
fn every_numeric_option_in_every_verb_says_which_one_was_not_a_number() {
    let scratch = Scratch::new("nan");
    let art = scratch.write("art.txt", "hi\n");
    let recipe = scratch.recipe("thing", "##\n");
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (vec!["convert", utf8(&art), "--fg", "scarlet"], "--fg"),
        (vec!["convert", utf8(&art), "--bg", "scarlet"], "--bg"),
        (vec!["convert", utf8(&art), "--also-dur", "--dur-version", "seven"], "--dur-version"),
        (vec!["build", utf8(&recipe), "--also-dur", "--dur-version", "seven"], "--dur-version"),
        (vec!["generate", "life", "--also-dur", "--dur-version", "seven"], "--dur-version"),
        (vec!["generate", "life", "--lines", "tall"], "--lines"),
        (vec!["generate", "life", "--frames", "many"], "--frames"),
        (vec!["generate", "life", "--delay", "soon"], "--delay"),
        (vec!["generate", "life", "--seed", "random"], "--seed"),
    ];
    for (args, flag) in cases {
        refused(&gen(&args), &[flag]);
    }
}

#[test]
fn an_unknown_option_to_generate_is_refused() {
    refused(&gen(&["generate", "life", "--sparkles"]), &["--sparkles"]);
}

// --- a recipe the builder cannot build -----------------------------------------

#[test]
fn a_recipe_whose_art_will_not_fit_a_canvas_is_refused_with_the_recipe_named() {
    // The art parses and the recipe parses; it is putting them together that
    // fails. The message has to name the recipe, because that is the file the
    // author will open.
    let scratch = Scratch::new("too-big");
    let wide = "#".repeat(1500);
    scratch.write("huge.txt", format!("{wide}\n"));
    let recipe = scratch.write("huge.toml", "[logo]\nsource = \"file:huge.txt\"\n");
    refused(&gen(&["build", utf8(&recipe)]), &["huge.toml"]);
}

#[test]
fn a_spinner_whose_period_does_not_divide_the_frame_count_is_refused() {
    let scratch = Scratch::new("period");
    scratch.write("art.txt", "##\n");
    let recipe = scratch.write(
        "art.toml",
        "[logo]\nsource = \"file:art.txt\"\n[spinner]\nstyle = \"dots\"\nperiod = 7\n\
         [canvas]\nframes = 20\n",
    );
    let output = gen(&["build", utf8(&recipe)]);
    assert!(!output.status.success(), "a period that does not divide was accepted");
    assert!(stderr(&output).contains("art.toml"), "{}", stderr(&output));
}
