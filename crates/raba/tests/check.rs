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

use raba_core::{Cell, Grid, Movie};
use std::path::PathBuf;
use std::process::{Command, Output};

/// A real `.raba`, written into the sandbox.
///
/// Generated rather than taken from `themes/`: those are built by
/// `scripts/build-themes.sh` and `*.raba` is in `.gitignore`, so a fresh
/// checkout — CI's, and a new contributor's — has none. A test that passes only
/// on a machine where somebody has already run the build script is a test that
/// fails for everybody else, which is how this one was found.
fn theme(scratch: &Scratch, name: &str) {
    let mut grid = Grid::new(8, 2, raba_core::Palette::Ansi16).expect("a grid");
    for column in 0..8 {
        grid.set(column, 0, Cell { ch: '#', fg: 7, bg: 0 });
    }
    let ops: Vec<raba_core::Op> = grid
        .cells()
        .iter()
        .enumerate()
        .map(|(index, cell)| raba_core::Op {
            cell: u32::try_from(index).expect("a small index"),
            ch: cell.ch,
            fg: cell.fg,
            bg: cell.bg,
        })
        .collect();
    let movie = Movie {
        columns: grid.columns(),
        lines: grid.lines(),
        palette: grid.palette(),
        scaling: raba_core::Scaling::Crop,
        looping: true,
        frames: vec![raba_core::Frame {
            delay_ms: 10,
            op_start: 0,
            op_count: u16::try_from(ops.len()).expect("a small frame"),
        }],
        ops,
    };
    std::fs::create_dir_all(scratch.path("themes")).expect("a themes directory");
    std::fs::write(scratch.path(&format!("themes/{name}.raba")), raba_core::encode(&movie))
        .expect("writing a theme");
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
        .arg(scratch.path("themes"))
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

/// A missing theme is the most common mistake there is — a typo in a name, a
/// themes directory that was never installed, an initramfs built before the art
/// package was — and until this test existed the boot path reported it as
/// `not drawing: : No such file or directory`: the empty path that the failed
/// resolution left behind, and nothing a person could act on.
#[test]
fn a_theme_that_does_not_exist_says_what_it_looked_for() {
    let scratch = Scratch::new("nowhere");
    scratch.write("raba.conf", "[Boot]\nTheme=nosuchtheme\n");
    // No `--check`: this is the boot path, the one that runs on the machine of
    // somebody who is not watching.
    let output = raba(&scratch, &[]);
    let said = String::from_utf8_lossy(&output.stderr).into_owned();

    // Never fatal. The alternative to a splash is no splash.
    assert!(output.status.success(), "{said}");
    assert!(said.contains("no theme found"), "{said}");
    assert!(said.contains("nosuchtheme.raba"), "{said}");
    // One line for one problem. A boot log is not the place to say it twice.
    assert_eq!(said.lines().filter(|line| line.contains("no theme")).count(), 1, "{said}");
}

#[test]
fn a_configuration_that_names_a_real_theme_checks_out() {
    let scratch = Scratch::new("good");
    theme(&scratch, "arch");
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
    theme(&scratch, "arch");
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
    theme(&scratch, "arch");
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

// --- the command line, which has the last word over every file ---------------
//
// Coverage found none of these flags had ever been parsed by a test. They are
// the ones `man 8 raba` documents for exactly the situation where nothing else
// works: a machine that will not boot, a bootloader prompt, and one chance to
// get the spelling right.

#[test]
fn the_command_line_overrides_the_file_and_check_says_so() {
    let scratch = Scratch::new("override");
    theme(&scratch, "generic");
    scratch.write("raba.conf", "[Boot]\nTty=3\nShowDelay=1\nTimeout=20\n");
    let output =
        raba(&scratch, &["--check", "--tty", "7", "--show-delay", "0.25", "--timeout", "9"]);
    let text = stdout(&output);
    // Reported as what *would happen*, which is the man page's own words. This
    // used to print the file's values while the daemon used the flag's.
    assert!(text.contains("Tty           /dev/tty7"), "{text}");
    assert!(text.contains("ShowDelay     0.25s"), "{text}");
    assert!(text.contains("Timeout       9s"), "{text}");
}

#[test]
fn a_timeout_of_zero_is_reported_as_disabled_rather_than_as_zero_seconds() {
    let scratch = Scratch::new("no-timeout");
    theme(&scratch, "generic");
    let output = raba(&scratch, &["--check", "--timeout", "0"]);
    assert!(stdout(&output).contains("disabled, not recommended"), "{}", stdout(&output));
}

#[test]
fn a_terminal_number_the_kernel_does_not_have_is_refused_at_the_flag() {
    let scratch = Scratch::new("bad-tty");
    theme(&scratch, "generic");
    let output = raba(&scratch, &["--check", "--tty", "64"]);
    assert!(!output.status.success());
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(text.contains("--tty"), "{text}");
    assert!(text.contains("64"), "the number it refused should be in the message:\n{text}");
}

#[test]
fn a_duration_that_is_not_a_number_is_refused_and_names_the_flag() {
    let scratch = Scratch::new("bad-delay");
    theme(&scratch, "generic");
    let output = raba(&scratch, &["--check", "--show-delay", "soon"]);
    assert!(!output.status.success());
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(text.contains("--show-delay"), "{text}");
}

#[test]
fn an_outro_given_on_the_command_line_is_the_one_reported() {
    let scratch = Scratch::new("outro");
    theme(&scratch, "generic");
    theme(&scratch, "generic-outro");
    let outro = scratch.path("themes/generic-outro.raba");
    let output = raba(&scratch, &["--check", "--outro", outro.to_str().expect("utf-8")]);
    let text = stdout(&output);
    assert!(text.contains("generic-outro.raba"), "{text}");
}

#[test]
fn a_flag_that_needs_a_value_and_has_none_says_which_flag() {
    let scratch = Scratch::new("no-value");
    let output = raba(&scratch, &["--check", "--tty"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--tty"));
}

// --- the options the parser has and nothing ran --------------------------------

/// A theme with a character no console font has, for the two commands that are
/// supposed to notice.
fn theme_with_an_undrawable_glyph(scratch: &Scratch, name: &str) -> PathBuf {
    theme_of(scratch, name, '\u{1f600}')
}

/// A one-line theme made entirely of `ch`.
fn theme_of(scratch: &Scratch, name: &str, ch: char) -> PathBuf {
    let mut grid = Grid::new(2, 1, raba_core::Palette::Ansi16).expect("a grid");
    grid.set(0, 0, Cell { ch, fg: 7, bg: 0 });
    grid.set(1, 0, Cell { ch, fg: 7, bg: 0 });
    let ops: Vec<raba_core::Op> = grid
        .cells()
        .iter()
        .enumerate()
        .map(|(index, cell)| raba_core::Op {
            cell: u32::try_from(index).expect("a small index"),
            ch: cell.ch,
            fg: cell.fg,
            bg: cell.bg,
        })
        .collect();
    let movie = Movie {
        columns: 2,
        lines: 1,
        palette: raba_core::Palette::Ansi16,
        scaling: raba_core::Scaling::Crop,
        looping: true,
        frames: vec![raba_core::Frame {
            delay_ms: 10,
            op_start: 0,
            op_count: u16::try_from(ops.len()).expect("a small frame"),
        }],
        ops,
    };
    std::fs::create_dir_all(scratch.path("themes")).expect("a themes directory");
    let path = scratch.path(&format!("themes/{name}.raba"));
    std::fs::write(&path, raba_core::encode(&movie)).expect("writing a theme");
    path
}

#[test]
fn preview_and_check_together_are_refused_as_two_different_questions() {
    let scratch = Scratch::new("preview-and-check");
    theme(&scratch, "thing");
    let output = raba(&scratch, &["--check", "--preview", "themes/thing.raba"]);
    assert!(!output.status.success(), "both at once exited 0");
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("different questions"), "{said}");
}

#[test]
fn preview_plays_a_theme_and_says_nothing_when_it_is_drawable() {
    let scratch = Scratch::new("preview");
    theme(&scratch, "thing");
    let path = scratch.path("themes/thing.raba");
    let output = raba(&scratch, &["--preview", path.to_str().expect("utf-8"), "--loops", "1"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(!output.stdout.is_empty(), "--preview drew nothing");
}

#[test]
fn preview_warns_about_a_glyph_a_console_cannot_draw_and_plays_it_anyway() {
    // A preview runs on a capable terminal with somebody watching: seeing the
    // problem is the point, so this warns rather than refuses. `--check` is
    // where the same finding is an exit code.
    let scratch = Scratch::new("preview-glyph");
    let path = theme_with_an_undrawable_glyph(&scratch, "emoji");
    let output = raba(&scratch, &["--preview", path.to_str().expect("utf-8"), "--loops", "1"]);
    assert!(output.status.success(), "a preview refused to play");
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("will not draw on a console"), "{said}");
    assert!(said.contains("docs/glyphs.md"), "it warned with nowhere to read about it: {said}");
}

#[test]
fn check_fails_on_a_theme_with_a_glyph_no_console_can_draw() {
    let scratch = Scratch::new("check-glyph");
    theme_with_an_undrawable_glyph(&scratch, "emoji");
    scratch.write("raba.conf", "[Boot]\nTheme=emoji\n");
    let output = raba(&scratch, &["--check"]);
    assert!(!output.status.success(), "--check passed a theme the console cannot draw");
    let text = stdout(&output);
    assert!(text.contains("will not draw on a console"), "{text}");
}

#[test]
fn check_says_how_big_the_theme_is_and_how_many_frames_it_has() {
    let scratch = Scratch::new("check-theme");
    theme(&scratch, "thing");
    scratch.write("raba.conf", "[Boot]\nTheme=thing\n");
    let output = raba(&scratch, &["--check"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let text = stdout(&output);
    assert!(text.contains("1 frames"), "{text}");
    assert!(text.contains("8x2"), "{text}");
}

#[test]
fn check_says_why_when_the_theme_cannot_be_read_instead_of_only_that_it_failed() {
    let scratch = Scratch::new("check-unreadable");
    std::fs::create_dir_all(scratch.path("themes")).expect("themes");
    scratch.write("themes/broken.raba", "not a raba file");
    scratch.write("raba.conf", "[Boot]\nTheme=broken\n");
    let output = raba(&scratch, &["--check"]);
    assert!(!output.status.success());
    assert!(stdout(&output).contains("theme:"), "{}", stdout(&output));
}

#[test]
fn a_file_too_large_to_be_a_theme_is_refused_before_it_is_read_into_memory() {
    let scratch = Scratch::new("too-large");
    std::fs::create_dir_all(scratch.path("themes")).expect("themes");
    let path = scratch.path("themes/huge.raba");
    let file = std::fs::File::create(&path).expect("creating");
    // Sparse: the point is the length in the metadata, which is what the guard
    // reads, and allocating 17 MB to prove it would be rude to a test suite.
    file.set_len(17 * 1024 * 1024).expect("a large sparse file");
    drop(file);

    let output = raba(&scratch, &["--preview", path.to_str().expect("utf-8")]);
    assert!(!output.status.success(), "a 17 MB theme was read");
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("past the"), "{said}");
}

#[test]
fn the_outro_ceiling_can_be_set_on_the_command_line_and_check_reports_it() {
    let scratch = Scratch::new("max-duration");
    theme(&scratch, "thing");
    scratch.write("raba.conf", "[Boot]\nTheme=thing\n");
    let output = raba(&scratch, &["--check", "--max-duration", "2.5"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(stdout(&output).contains("2.5"), "{}", stdout(&output));
}

#[test]
fn an_option_that_wants_a_value_and_does_not_get_one_names_what_it_wanted() {
    let scratch = Scratch::new("missing-values");
    for option in ["--theme", "--outro", "--tty", "--show-delay", "--timeout", "--max-duration"] {
        let output = raba(&scratch, &[option]);
        assert!(!output.status.success(), "{option} with no value exited 0");
        let said = String::from_utf8_lossy(&output.stderr);
        assert!(said.contains(option), "{option} refused without naming itself: {said}");
    }
}

#[test]
fn more_configuration_problems_than_fit_on_a_console_are_cut_short_with_a_count() {
    // A handful of lines is a diagnostic; a hundred scrolling past during a boot
    // is noise. The last line has to say where the rest are.
    let scratch = Scratch::new("many-problems");
    theme(&scratch, "thing");
    let mut config = String::from("[Boot]\nTheme=thing\n");
    for n in 0..9 {
        config.push_str(&format!("NotAKey{n}=7\n"));
    }
    scratch.write("raba.conf", &config);

    let output = raba(&scratch, &[]);
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("more; run `raba --check`"), "{said}");
    assert!(said.matches("NotAKey").count() <= 5, "every problem was printed:\n{said}");
}

#[test]
fn a_machine_nobody_has_set_up_is_told_every_name_auto_tried() {
    let scratch = Scratch::new("no-theme-at-all");
    // No config, no themes directory, and an os-release nothing matches: the
    // state a machine is in before anybody has set it up. `auto` still has a
    // list — the ID, then `generic` — and the list is the answer to "why is
    // there no splash".
    scratch.write("os-release", "ID=nosuchdistro\n");
    let output = raba(&scratch, &[]);
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("nosuchdistro.raba"), "{said}");
    assert!(said.contains("generic.raba"), "auto did not fall through to generic: {said}");
    assert!(output.status.success(), "the absence of a splash is not a boot failure");
}

// --- the kernel command line, and the settings nothing had set ----------------

#[test]
fn the_kernel_command_line_can_name_the_terminal_and_auto_means_auto() {
    let scratch = Scratch::new("cmdline-tty");
    theme(&scratch, "thing");
    scratch.write("raba.conf", "[Boot]\nTheme=thing\nTty=5\n");

    scratch.write("cmdline", "quiet raba.tty=9\n");
    let fixed = raba(&scratch, &["--check"]);
    assert!(stdout(&fixed).contains("tty9"), "{}", stdout(&fixed));

    // `auto` on the command line is not "no opinion": it overrides a file that
    // named a terminal, and puts the choice back to VT_OPENQRY.
    scratch.write("cmdline", "quiet raba.tty=auto\n");
    let auto = raba(&scratch, &["--check"]);
    assert!(stdout(&auto).contains("auto"), "{}", stdout(&auto));
}

#[test]
fn a_kernel_parameter_with_a_value_that_is_not_one_is_a_problem_naming_the_parameter() {
    let scratch = Scratch::new("cmdline-bad");
    theme(&scratch, "thing");
    scratch.write("raba.conf", "[Boot]\nTheme=thing\n");
    scratch.write("cmdline", "quiet raba.enable=maybe raba.debug=perhaps raba.tty=left\n");

    let output = raba(&scratch, &["--check"]);
    let text = stdout(&output);
    for parameter in ["raba.enable", "raba.debug", "raba.tty"] {
        assert!(text.contains(parameter), "{parameter} was not reported:\n{text}");
    }
    // The file it came from, not just the text: a problem with no provenance
    // sends somebody to edit the wrong thing.
    assert!(text.contains("cmdline"), "{text}");
}

#[test]
fn a_kernel_parameter_raba_does_not_know_is_reported_rather_than_ignored() {
    let scratch = Scratch::new("cmdline-unknown");
    theme(&scratch, "thing");
    scratch.write("raba.conf", "[Boot]\nTheme=thing\n");
    scratch.write("cmdline", "quiet raba.sparkles=1\n");
    let output = raba(&scratch, &["--check"]);
    assert!(stdout(&output).contains("sparkles"), "{}", stdout(&output));
}

#[test]
fn a_setting_whose_value_is_not_one_of_its_words_says_what_it_wanted() {
    let scratch = Scratch::new("bad-enum");
    theme(&scratch, "thing");
    scratch.write("raba.conf", "[Boot]\nTheme=thing\n\n[Log]\nLevel=verbose\n");
    let output = raba(&scratch, &["--check"]);
    let text = stdout(&output);
    assert!(text.contains("verbose"), "{text}");
    assert!(text.contains("is not"), "it refused the value without saying what it wanted:\n{text}");
}

#[test]
fn a_log_file_that_is_not_an_absolute_path_is_refused() {
    // Relative to what? The daemon's working directory in an initramfs is not
    // a thing anybody can predict.
    let scratch = Scratch::new("relative-log");
    theme(&scratch, "thing");
    scratch.write("raba.conf", "[Boot]\nTheme=thing\n\n[Log]\nFile=raba.log\n");
    let output = raba(&scratch, &["--check"]);
    assert!(stdout(&output).contains("absolute path"), "{}", stdout(&output));
}

#[test]
fn an_outro_that_is_turned_off_reports_as_off_and_resolves_nothing() {
    let scratch = Scratch::new("outro-off");
    theme(&scratch, "thing");
    scratch.write("raba.conf", "[Boot]\nTheme=thing\n\n[Outro]\nEnable=false\n");
    let output = raba(&scratch, &["--check"]);
    let text = stdout(&output);
    assert!(text.contains("Enable        false"), "{text}");
    assert!(!text.contains("MaxDuration"), "an outro that is off reported its ceiling:\n{text}");
}

#[test]
fn an_outro_named_by_hand_is_looked_for_under_the_themes_directory() {
    let scratch = Scratch::new("outro-named");
    theme(&scratch, "thing");
    theme(&scratch, "thing-goodbye");
    scratch
        .write("raba.conf", "[Boot]\nTheme=thing\n\n[Outro]\nEnable=true\nTheme=thing-goodbye\n");
    let output = raba(&scratch, &["--check"]);
    assert!(stdout(&output).contains("thing-goodbye.raba"), "{}", stdout(&output));
}

#[test]
fn an_auto_outro_with_no_boot_theme_to_sit_beside_resolves_to_nothing() {
    // `auto` means "the boot theme's name with -outro": with no boot theme
    // there is no name to append to, and the answer is nothing rather than a
    // guess.
    let scratch = Scratch::new("outro-auto-nothing");
    scratch.write("raba.conf", "[Boot]\nTheme=absent\n\n[Outro]\nEnable=true\n");
    let output = raba(&scratch, &["--check"]);
    assert!(stdout(&output).contains("nothing"), "{}", stdout(&output));
}

#[test]
fn a_theme_given_as_a_path_is_reported_as_that_path() {
    let scratch = Scratch::new("theme-path");
    theme(&scratch, "thing");
    let path = scratch.path("themes/thing.raba");
    scratch.write("raba.conf", &format!("[Boot]\nTheme={}\n", path.display()));
    let output = raba(&scratch, &["--check"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(stdout(&output).contains(&path.display().to_string()), "{}", stdout(&output));
}

#[test]
fn a_configuration_file_too_large_to_be_one_is_refused_by_size() {
    let scratch = Scratch::new("huge-config");
    theme(&scratch, "thing");
    let huge = format!("[Boot]\nTheme=thing\n{}", "# padding\n".repeat(8000));
    scratch.write("raba.conf", &huge);
    let output = raba(&scratch, &["--check"]);
    let text = stdout(&output);
    assert!(text.contains("past the"), "a 80 KB configuration was read:\n{text}");
}

#[test]
fn preview_tells_a_glyph_that_is_merely_unreliable_apart_from_one_that_cannot_draw() {
    // `é` is in the kernel's built-in font and missing from some fonts that
    // replace it, so art using it works until somebody runs `setfont`. That is
    // a different warning from an emoji, which never draws anywhere, and a
    // contributor deciding whether to redraw their art needs to know which one
    // they have.
    let scratch = Scratch::new("preview-unreliable");
    let path = theme_of(&scratch, "accented", '\u{e9}');
    let output = raba(&scratch, &["--preview", path.to_str().expect("utf-8"), "--loops", "1"]);
    assert!(output.status.success(), "a preview refused to play");
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("may not draw after setfont"), "{said}");
    assert!(!said.contains("will not draw on a console"), "{said}: the wrong warning");
}

#[test]
fn check_passes_a_theme_whose_glyphs_are_only_unreliable() {
    // Only `Unsafe` is an exit code. Refusing every CP437 accented letter
    // would refuse most of the ANSI art this project exists to play.
    let scratch = Scratch::new("check-unreliable");
    theme_of(&scratch, "accented", '\u{e9}');
    scratch.write("raba.conf", "[Boot]\nTheme=accented\n");
    let output = raba(&scratch, &["--check"]);
    assert!(output.status.success(), "{}", stdout(&output));
}

#[test]
fn tty_auto_on_the_command_line_means_the_same_as_not_saying_which() {
    // `--tty auto` is how a command line overrides a `Tty=7` in the file back
    // to "whichever is free". Silently keeping the 7 would make the flag a
    // no-op in the one case it is for.
    let scratch = Scratch::new("tty-auto");
    theme(&scratch, "thing");
    scratch.write("raba.conf", "[Boot]\nTheme=thing\nTty=7\n");

    let fixed = raba(&scratch, &["--check"]);
    assert!(stdout(&fixed).contains("tty7"), "{}", stdout(&fixed));

    let auto = raba(&scratch, &["--check", "--tty", "auto"]);
    assert!(auto.status.success(), "{}", stdout(&auto));
    assert!(!stdout(&auto).contains("tty7"), "--tty auto left Tty=7 in place:\n{}", stdout(&auto));
    assert!(stdout(&auto).contains("auto"), "{}", stdout(&auto));
}

#[test]
fn check_says_the_kill_switch_is_on_rather_than_reporting_a_splash_that_will_not_appear() {
    // Somebody running `raba --check` because no splash appeared has already
    // forgotten about the `raba.enable=0` they added to the bootloader three
    // months ago. A report that lists a resolved theme and a terminal and says
    // nothing about the kernel command line sends them to look at the theme.
    let scratch = Scratch::new("check-disabled");
    theme(&scratch, "arch");
    scratch.write("raba.conf", "[Boot]\nTheme=arch\n");
    scratch.write("cmdline", "root=/dev/sda2 quiet raba.enable=0\n");
    let output = raba(&scratch, &["--check"]);

    let text = stdout(&output);
    assert!(text.contains("raba.enable=0"), "{text}");
    assert!(text.contains("nothing will be drawn"), "{text}");
}
