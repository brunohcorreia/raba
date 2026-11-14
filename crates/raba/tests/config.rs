//! `raba.conf`(5): what it accepts, what it refuses, and what it does with a
//! file it does not understand.
//!
//! The interesting half is the second one. A configuration parser that reads a
//! good file correctly is table stakes; this one is read as root, before the
//! root filesystem is mounted, by a program that is not allowed to fail — so
//! every test below about a *broken* file is really a test that the boot
//! survives it, and the ones about an unknown key are tests that a file written
//! for a later version still boots an earlier one.
//!
//! The defaults are asserted against `man 5 raba.conf` by hand, because the man
//! page is the contract — see `docs/maintenance.md` §5 — and a default that
//! drifts from its documentation is a lie nobody notices.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use raba::config::{self, Boot, Config, Level, Mode, Outro, Position, Source, Style, Theme, Tty};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The file name a [`Problem`](config::Problem) is tagged with. Never opened.
fn named() -> &'static Path {
    Path::new("/etc/raba/raba.conf")
}

fn parse(text: &str) -> (Config, Vec<String>) {
    let mut config = Config::default();
    let problems = config::apply(text, named(), &mut config);
    (config, problems.iter().map(ToString::to_string).collect())
}

/// Parse something that is supposed to be clean, and fail loudly if it is not.
fn clean(text: &str) -> Config {
    let (config, problems) = parse(text);
    assert!(problems.is_empty(), "expected no complaints, got {problems:?}");
    config
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let mut path = std::env::temp_dir();
        path.push(format!("raba-config-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch");
        Scratch(path)
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("directory");
        }
        std::fs::write(&path, text).expect("writing");
        path
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// Places pointing entirely inside the scratch directory, so that a test
    /// can never read the machine it is running on.
    fn places(&self) -> config::Places {
        config::Places {
            defaults: self.path("raba.defaults"),
            config: self.path("raba.conf"),
            themes: self.path("themes"),
            cmdline: self.path("cmdline"),
            os_release: self.path("os-release"),
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ------------------------------------------------------------- defaults ----

#[test]
fn the_defaults_are_the_ones_the_man_page_documents() {
    let config = Config::default();
    assert_eq!(config.boot.theme, Theme::Auto);
    assert_eq!(config.boot.tty, Tty::Auto);
    assert_eq!(config.boot.show_delay, Duration::ZERO);
    assert_eq!(config.boot.timeout, Some(Duration::from_secs(30)));
    assert_eq!(config.boot.frame_rate, None);

    assert!(config.outro.enable);
    assert_eq!(config.outro.theme, Theme::Auto);
    assert_eq!(config.outro.max_duration, Duration::from_millis(1500));
    assert!(config.outro.finish_frame);

    assert!(!config.progress.enable);
    assert_eq!(config.progress.style, Style::Pulse);
    assert_eq!(config.progress.source, Source::Ctl);
    assert_eq!(config.progress.position, Position::Below);
    assert_eq!(config.progress.width, 40);
    assert_eq!(config.progress.color, 7);
    assert_eq!(config.progress.background, 8);

    assert_eq!(config.handoff.mode, Mode::Retain);
    assert!(config.handoff.restore_tty);

    assert_eq!(config.log.level, Level::Warn);
    assert_eq!(config.log.file, PathBuf::from("/run/raba.log"));
}

/// The example in `man 5 raba.conf`, verbatim. If this ever complains, either
/// the parser or the manual is wrong, and the test does not care which.
#[test]
fn the_example_from_the_man_page_parses() {
    let config = clean(
        "\
# /etc/raba/raba.conf

[Boot]
Theme=cachyos
Tty=1
ShowDelay=0.3
Timeout=20

[Outro]
Enable=true
Theme=cachyos-outro
MaxDuration=1.0

[Handoff]
Mode=retain
",
    );
    assert_eq!(config.boot.theme, Theme::Named("cachyos".to_owned()));
    assert_eq!(config.boot.tty, Tty::Fixed(1));
    assert_eq!(config.boot.show_delay, Duration::from_millis(300));
    assert_eq!(config.boot.timeout, Some(Duration::from_secs(20)));
    assert_eq!(config.outro.theme, Theme::Named("cachyos-outro".to_owned()));
    assert_eq!(config.outro.max_duration, Duration::from_secs(1));
    assert_eq!(config.handoff.mode, Mode::Retain);
}

#[test]
fn every_key_the_man_page_documents_is_accepted() {
    let config = clean(
        "\
[Boot]
Theme=/opt/art.raba
Tty=auto
ShowDelay=1
Timeout=45
FrameRateOverride=30
[Outro]
Enable=no
Theme=auto
MaxDuration=2
FinishFrame=false
[Progress]
Enable=true
Style=bar
Source=time
Position=above
Width=0
Color=15
BackgroundColor=0
[Handoff]
Mode=clear
RestoreTty=false
[Log]
Level=debug
File=/var/log/raba.log
",
    );
    assert_eq!(config.boot.theme, Theme::Path(PathBuf::from("/opt/art.raba")));
    assert_eq!(config.boot.frame_rate, Some(30.0));
    assert!(!config.outro.enable);
    assert!(!config.outro.finish_frame);
    assert_eq!(config.progress.style, Style::Bar);
    assert_eq!(config.progress.source, Source::Time);
    assert_eq!(config.progress.position, Position::Above);
    assert_eq!(config.progress.width, 0);
    assert_eq!(config.handoff.mode, Mode::Clear);
    assert!(!config.handoff.restore_tty);
    assert_eq!(config.log.level, Level::Debug);
    assert_eq!(config.log.file, PathBuf::from("/var/log/raba.log"));
}

// --------------------------------------------------------------- syntax ----

#[test]
fn whitespace_around_a_key_and_its_value_is_not_part_of_either() {
    let config = clean("[Boot]\n   Theme   =   arch   \n");
    assert_eq!(config.boot.theme, Theme::Named("arch".to_owned()));
}

#[test]
fn a_hash_starts_a_comment_only_at_the_start_of_a_line() {
    // A path may contain one, and a parser that ate the rest of the line would
    // turn this into a file that does not exist.
    let config = clean("# a comment\n[Boot]\nTheme=/opt/art#1.raba\n");
    assert_eq!(config.boot.theme, Theme::Path(PathBuf::from("/opt/art#1.raba")));
}

#[test]
fn the_last_spelling_of_a_key_wins() {
    let config = clean("[Boot]\nTimeout=10\nTimeout=20\n");
    assert_eq!(config.boot.timeout, Some(Duration::from_secs(20)));
}

#[test]
fn a_key_before_any_section_is_refused_rather_than_guessed_at() {
    let (config, problems) = parse("Theme=arch\n[Boot]\nTheme=void\n");
    assert_eq!(problems.len(), 1);
    assert!(problems.iter().any(|p| p.contains("before any [Section]")), "{problems:?}");
    assert_eq!(config.boot.theme, Theme::Named("void".to_owned()));
}

#[test]
fn an_unfinished_section_header_is_a_complaint_and_not_a_key() {
    let (_, problems) = parse("[Boot\nTheme=arch\n");
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("does not finish it")), "{problems:?}");
}

#[test]
fn a_line_that_is_not_a_pair_is_a_complaint_with_a_line_number() {
    let (_, problems) = parse("[Boot]\nTheme arch\n");
    assert_eq!(problems.len(), 1);
    assert!(problems.iter().any(|p| p.contains(":2:")), "{problems:?}");
}

// ----------------------------------------------------- unknown and wrong ----

#[test]
fn an_unknown_key_is_stepped_over_and_the_rest_of_the_file_still_counts() {
    let (config, problems) = parse("[Boot]\nThem=arch\nTimeout=5\n");
    assert_eq!(problems.len(), 1);
    assert!(problems.iter().any(|p| p.contains("Them is not a key in [Boot]")), "{problems:?}");
    assert_eq!(config.boot.timeout, Some(Duration::from_secs(5)));
}

/// A file written for a later version. One line about the section, not one per
/// key — and everything this version does know still applies.
#[test]
fn a_section_from_the_future_costs_one_complaint_and_nothing_else() {
    let (config, problems) = parse(
        "\
[Boot]
Timeout=5
[Fireworks]
Enable=true
Colour=green
Sparkles=many
[Outro]
MaxDuration=2
",
    );
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("[Fireworks]")), "{problems:?}");
    assert_eq!(config.boot.timeout, Some(Duration::from_secs(5)));
    assert_eq!(config.outro.max_duration, Duration::from_secs(2));
}

#[test]
fn a_value_that_does_not_parse_leaves_the_setting_where_it_was() {
    let (config, problems) = parse("[Boot]\nTimeout=soon\n");
    assert_eq!(problems.len(), 1);
    assert_eq!(config.boot.timeout, Config::default().boot.timeout);
}

#[test]
fn nothing_in_a_broken_file_panics() {
    // Each of these has made some parser somewhere do something exciting.
    for text in [
        "[Boot]\nShowDelay=-1\n",
        "[Boot]\nShowDelay=NaN\n",
        "[Boot]\nShowDelay=inf\n",
        "[Boot]\nShowDelay=1e400\n",
        "[Boot]\nTimeout=99999999999999999999\n",
        "[Boot]\nTty=-1\n",
        "[Boot]\nTty=99999\n",
        "[Boot]\nFrameRateOverride=NaN\n",
        "[Progress]\nColor=300\n",
        "[Progress]\nWidth=-5\n",
        "=\n",
        "[]\n",
        "[Boot]\n=arch\n",
        "[Boot]\nTheme=\n",
        "\u{0}\u{0}\u{0}",
    ] {
        let (_, problems) = parse(text);
        assert!(!problems.is_empty(), "{text:?} was accepted");
    }
}

#[test]
fn a_timeout_of_zero_is_the_documented_way_to_disable_it() {
    assert_eq!(clean("[Boot]\nTimeout=0\n").boot.timeout, None);
}

#[test]
fn a_frame_rate_of_zero_means_the_theme_decides() {
    assert_eq!(clean("[Boot]\nFrameRateOverride=0\n").boot.frame_rate, None);
}

#[test]
fn a_boolean_may_be_written_the_way_a_person_would_write_it() {
    for yes in ["true", "TRUE", "yes", "on", "1"] {
        assert!(clean(&format!("[Outro]\nEnable={yes}\n")).outro.enable, "{yes}");
    }
    for no in ["false", "False", "no", "off", "0"] {
        assert!(!clean(&format!("[Outro]\nEnable={no}\n")).outro.enable, "{no}");
    }
    let (_, problems) = parse("[Outro]\nEnable=maybe\n");
    assert_eq!(problems.len(), 1);
}

// ---------------------------------------------------------------- theme ----

#[test]
fn a_theme_name_may_not_climb_out_of_the_theme_directory() {
    // The file is root-owned, and the value is still pasted onto a directory
    // and opened as root. A path is spelled with a leading slash, and anything
    // else is a name.
    for refused in ["../../etc/shadow", "a/b", "", ".hidden", "arch\u{0}", "a b"] {
        assert!(Theme::parse(refused).is_err(), "{refused:?} was accepted");
    }
    assert_eq!(Theme::parse("auto"), Ok(Theme::Auto));
    assert_eq!(Theme::parse("arch-v8_1.2"), Ok(Theme::Named("arch-v8_1.2".to_owned())));
    assert_eq!(Theme::parse("/opt/x.raba"), Ok(Theme::Path(PathBuf::from("/opt/x.raba"))));
}

#[test]
fn auto_tries_the_distribution_then_its_parents_then_generic() {
    let names = config::identity("NAME=\"CachyOS\"\nID=cachyos\nID_LIKE=\"arch archlinux\"\n");
    assert_eq!(names, ["cachyos", "arch", "archlinux"]);

    let tried = config::candidates(&Theme::Auto, Path::new("/t"), &names);
    assert_eq!(
        tried,
        [
            PathBuf::from("/t/cachyos.raba"),
            PathBuf::from("/t/arch.raba"),
            PathBuf::from("/t/archlinux.raba"),
            PathBuf::from("/t/generic.raba"),
        ]
    );
}

#[test]
fn an_os_release_that_names_something_unusable_is_ignored_rather_than_followed() {
    // `ID` is not validated by anything upstream of us, and it ends up as a
    // path component.
    let names = config::identity("ID=../../etc\nID_LIKE=\"ok ../bad\"\n");
    assert_eq!(names, ["ok"]);
}

#[test]
fn a_named_theme_is_looked_for_in_one_place_only() {
    let tried = config::candidates(&Theme::Named("void".to_owned()), Path::new("/t"), &[]);
    assert_eq!(tried, [PathBuf::from("/t/void.raba")]);
}

#[test]
fn an_auto_outro_is_looked_for_beside_the_boot_theme() {
    let boot = PathBuf::from("/opt/art/cachyos.raba");
    let tried = config::outro_candidates(&Theme::Auto, Path::new("/t"), Some(&boot));
    assert_eq!(tried, [PathBuf::from("/opt/art/cachyos-outro.raba")]);
}

#[test]
fn an_auto_outro_with_no_boot_theme_has_nothing_to_look_for() {
    assert!(config::outro_candidates(&Theme::Auto, Path::new("/t"), None).is_empty());
}

// ------------------------------------------------------------------ tty ----

#[test]
fn a_terminal_number_has_to_be_one_the_kernel_could_have() {
    assert_eq!(config::parse_tty("auto"), Ok(Tty::Auto));
    assert_eq!(config::parse_tty("1"), Ok(Tty::Fixed(1)));
    assert_eq!(config::parse_tty("63"), Ok(Tty::Fixed(63)));
    for refused in ["0", "64", "-1", "tty1", ""] {
        assert!(config::parse_tty(refused).is_err(), "{refused:?} was accepted");
    }
}

// -------------------------------------------------------- not honoured -----

#[test]
fn only_keys_a_file_actually_set_are_reported_as_not_honoured() {
    let config = clean("[Boot]\nTimeout=5\n[Progress]\nEnable=true\nStyle=bar\n");
    assert_eq!(config.unhonoured(), ["Progress.Enable", "Progress.Style"]);
    assert!(config.was_set("Boot.Timeout"));
    assert!(!config.was_set("Boot.Theme"));
}

#[test]
fn a_file_that_sets_nothing_unimplemented_says_nothing() {
    assert!(clean("[Boot]\nTheme=arch\nTimeout=5\n").unhonoured().is_empty());
}

// ------------------------------------------------------------- cmdline -----

#[test]
fn the_kill_switch_is_read_off_the_kernel_command_line() {
    let kernel = config::cmdline("root=/dev/sda2 quiet raba.enable=0 loglevel=3");
    assert_eq!(kernel.enable, Some(false));
    assert!(kernel.problems.is_empty());
}

#[test]
fn a_kernel_line_with_nothing_of_ours_on_it_says_nothing() {
    let kernel = config::cmdline("root=/dev/sda2 rw quiet splash rabarbaro=1 notraba.enable=0");
    assert_eq!(kernel.enable, None);
    assert_eq!(kernel.debug, None);
    assert_eq!(kernel.tty, None);
    assert!(kernel.problems.is_empty(), "{:?}", kernel.problems);
}

#[test]
fn a_misspelled_parameter_is_reported_rather_than_silently_doing_nothing() {
    // Typed at a bootloader prompt by somebody whose machine will not boot.
    // "It did nothing" is the one thing they must not have to guess at.
    let kernel = config::cmdline("raba.enabled=0 raba.debug raba.tty=99");
    assert_eq!(kernel.enable, None);
    assert_eq!(kernel.problems.len(), 3, "{:?}", kernel.problems);
    assert!(kernel.problems.iter().any(|p| p.contains("raba.enabled")), "{:?}", kernel.problems);
}

#[test]
fn a_quoted_kernel_value_is_the_value_without_the_quotes() {
    assert_eq!(config::cmdline("raba.tty=\"3\"").tty, Some(3));
    assert_eq!(config::cmdline("raba.debug=1").debug, Some(true));
}

// --------------------------------------------------------------- layers ----

#[test]
fn the_users_file_overrides_the_distributions_key_by_key() {
    let scratch = Scratch::new("layers");
    scratch.write("raba.defaults", "[Boot]\nTheme=generic\nTimeout=45\nShowDelay=2\n");
    scratch.write("raba.conf", "[Boot]\nTimeout=5\n");
    let startup = config::start(&scratch.places());

    assert_eq!(startup.config.boot.timeout, Some(Duration::from_secs(5)), "overridden");
    assert_eq!(startup.config.boot.show_delay, Duration::from_secs(2), "from the defaults");
    assert_eq!(startup.config.boot.theme, Theme::Named("generic".to_owned()), "from the defaults");
    assert_eq!(startup.files.len(), 2);
    assert!(startup.problems.is_empty(), "{:?}", startup.problems);
}

#[test]
fn no_configuration_at_all_is_not_a_problem() {
    let scratch = Scratch::new("absent");
    let startup = config::start(&scratch.places());
    assert!(startup.problems.is_empty(), "{:?}", startup.problems);
    assert!(startup.files.is_empty());
    assert!(startup.enabled);
    assert!(!startup.debug);
    assert_eq!(startup.settings.timeout, Some(Duration::from_secs(30)));
    // Nothing resolved, and the report can say what it looked for.
    assert_eq!(startup.tried, [scratch.path("themes/generic.raba")]);
    assert!(!startup.ok(), "a splash with no theme is what --check exists to report");
}

#[test]
fn the_kernel_command_line_wins_over_the_file() {
    let scratch = Scratch::new("kernel");
    scratch.write("raba.conf", "[Boot]\nTty=2\n");
    scratch.write("cmdline", "quiet raba.tty=5 raba.enable=0 raba.debug=1\n");
    let startup = config::start(&scratch.places());

    assert_eq!(startup.config.boot.tty, Tty::Fixed(5));
    assert_eq!(startup.settings.tty, Some(5));
    assert!(!startup.enabled);
    assert!(startup.debug);
}

#[test]
fn a_theme_that_exists_is_resolved_and_its_outro_found_beside_it() {
    let scratch = Scratch::new("resolve");
    scratch.write("themes/cachyos.raba", "not really a theme");
    scratch.write("themes/cachyos-outro.raba", "nor is this");
    scratch.write("os-release", "ID=cachyos\n");
    let startup = config::start(&scratch.places());

    assert_eq!(startup.settings.theme, scratch.path("themes/cachyos.raba"));
    assert_eq!(startup.settings.outro, Some(scratch.path("themes/cachyos-outro.raba")));
}

#[test]
fn an_outro_that_is_turned_off_is_not_looked_for() {
    let scratch = Scratch::new("no-outro");
    scratch.write("themes/generic.raba", "x");
    scratch.write("themes/generic-outro.raba", "x");
    scratch.write("raba.conf", "[Outro]\nEnable=false\n");
    let startup = config::start(&scratch.places());

    assert_eq!(startup.settings.theme, scratch.path("themes/generic.raba"));
    assert_eq!(startup.settings.outro, None);
    assert!(startup.outro_tried.is_empty());
}

#[test]
fn a_configuration_file_too_large_to_be_one_is_refused_before_it_is_read() {
    let scratch = Scratch::new("huge");
    scratch.write("raba.conf", &"# comment\n".repeat(20_000));
    let startup = config::start(&scratch.places());

    assert_eq!(startup.problems.len(), 1, "{:?}", startup.problems);
    assert!(startup.problems.iter().any(|p| p.to_string().contains("past the")));
    assert_eq!(startup.config.boot.timeout, Some(Duration::from_secs(30)));
}

#[test]
fn a_configuration_file_that_is_not_text_is_a_complaint_and_not_a_crash() {
    let scratch = Scratch::new("binary");
    std::fs::write(scratch.path("raba.conf"), [0xff_u8, 0xfe, 0x00, 0x01]).expect("writing");
    let startup = config::start(&scratch.places());

    assert_eq!(startup.problems.len(), 1, "{:?}", startup.problems);
    assert!(startup.problems.iter().any(|p| p.to_string().contains("UTF-8")));
}

#[test]
fn the_report_says_what_was_read_what_it_resolved_to_and_what_was_wrong() {
    let scratch = Scratch::new("report");
    scratch.write("themes/arch.raba", "x");
    scratch.write("raba.conf", "[Boot]\nTheme=arch\nThem=arch\n[Progress]\nEnable=true\n");
    let report = config::start(&scratch.places()).report();

    assert!(report.contains("raba.conf"), "{report}");
    assert!(report.contains("themes/arch.raba"), "{report}");
    assert!(report.contains("Them is not a key"), "{report}");
    assert!(report.contains("Progress.Enable"), "{report}");
}

#[test]
fn the_report_of_a_theme_that_resolved_to_nothing_lists_what_was_looked_for() {
    let scratch = Scratch::new("missing");
    scratch.write("os-release", "ID=void\n");
    let report = config::start(&scratch.places()).report();

    assert!(report.contains("void.raba"), "{report}");
    assert!(report.contains("generic.raba"), "{report}");
}

// ---------------------------------------------------------------- freeze ----

/// The property the whole freeze rests on: what comes out is something this
/// same parser reads back as the same thing.
///
/// An image carries a file written by one version of this code and read by
/// another — the one on the installed system writes it, the one inside the
/// initramfs reads it — so a value that is spelled in a way the parser does not
/// accept is a splash that works until it is put where it is meant to run. The
/// first draft of `length` printed `30s`, which the parser refuses.
#[test]
fn a_frozen_configuration_parses_back_to_the_same_configuration() {
    let scratch = Scratch::new("freeze-roundtrip");
    scratch.write("themes/void.raba", "x");
    scratch.write("themes/void-outro.raba", "x");
    scratch.write(
        "raba.conf",
        "\
[Boot]
Theme=void
Tty=4
ShowDelay=0.3
Timeout=45
FrameRateOverride=24
[Outro]
MaxDuration=0.75
FinishFrame=false
[Progress]
Enable=true
Style=percent
Source=none
Position=above
Width=0
Color=15
BackgroundColor=1
[Handoff]
Mode=clear
RestoreTty=false
[Log]
Level=debug
File=/var/log/raba.log
",
    );
    let before = config::start(&scratch.places());
    let frozen = before.freeze().expect("something to freeze");

    let (after, problems) = parse(&frozen);
    assert!(problems.is_empty(), "a frozen file complains when read back: {problems:?}");

    // Theme is the one key that deliberately changes: a name becomes the path
    // it resolved to, because an initramfs has no os-release to resolve with.
    assert_eq!(after.boot.theme, Theme::Path(scratch.path("themes/void.raba")));
    assert_eq!(after.outro.theme, Theme::Path(scratch.path("themes/void-outro.raba")));
    assert_eq!(
        Boot { theme: before.config.boot.theme.clone(), ..after.boot.clone() },
        before.config.boot
    );
    assert_eq!(
        Outro { theme: before.config.outro.theme.clone(), ..after.outro.clone() },
        before.config.outro
    );
    assert_eq!(after.progress, before.config.progress);
    assert_eq!(after.handoff, before.config.handoff);
    assert_eq!(after.log, before.config.log);
}

#[test]
fn freezing_resolves_auto_so_that_an_initramfs_never_has_to() {
    let scratch = Scratch::new("freeze-auto");
    scratch.write("themes/cachyos.raba", "x");
    scratch.write("os-release", "ID=cachyos\nID_LIKE=arch\n");
    let frozen = config::start(&scratch.places()).freeze().expect("something to freeze");

    let wanted = format!("Theme={}", scratch.path("themes/cachyos.raba").display());
    assert!(frozen.contains(&wanted), "{frozen}");
    assert!(!frozen.contains("Theme=auto\n"), "an unresolved auto reached the image:\n{frozen}");
}

#[test]
fn a_disabled_timeout_survives_being_frozen() {
    let scratch = Scratch::new("freeze-zero");
    scratch.write("themes/generic.raba", "x");
    scratch.write("raba.conf", "[Boot]\nTimeout=0\nShowDelay=1.25\n");
    let frozen = config::start(&scratch.places()).freeze().expect("something to freeze");

    assert!(frozen.contains("Timeout=0\n"), "{frozen}");
    assert!(frozen.contains("ShowDelay=1.25\n"), "{frozen}");
    assert_eq!(clean(&frozen).boot.timeout, None);
}

#[test]
fn an_outro_that_did_not_resolve_is_frozen_as_disabled() {
    // Writing Enable=true next to a theme that is not there would make every
    // boot look for a file the build already knew was missing.
    let scratch = Scratch::new("freeze-outro");
    scratch.write("themes/generic.raba", "x");
    let frozen = config::start(&scratch.places()).freeze().expect("something to freeze");

    assert!(frozen.contains("Enable=false"), "{frozen}");
    assert!(!clean(&frozen).outro.enable);
}

#[test]
fn there_is_nothing_to_freeze_without_a_theme() {
    let scratch = Scratch::new("freeze-nothing");
    let startup = config::start(&scratch.places());
    assert!(startup.freeze().is_none());
    assert!(startup.files().is_empty());
}

#[test]
fn the_files_an_image_needs_are_the_art_and_only_the_art() {
    let scratch = Scratch::new("freeze-files");
    scratch.write("themes/generic.raba", "x");
    scratch.write("themes/generic-outro.raba", "x");
    scratch.write("raba.conf", "[Boot]\nTheme=generic\n");
    let files = config::start(&scratch.places()).files();

    assert_eq!(
        files,
        [scratch.path("themes/generic.raba"), scratch.path("themes/generic-outro.raba")]
    );
}

#[test]
fn an_os_release_line_that_is_not_a_key_and_a_value_is_stepped_over() {
    // Real `/etc/os-release` files have blank lines and comments in them, and
    // one Fedora release shipped a stray continuation. None of that is a
    // reason to stop reading the file.
    let names = config::identity(
        "# a comment\n\nNAME=\"Thing\"\nnonsense\nID=thing\n   \nID_LIKE=\"arch\"\n",
    );
    assert_eq!(names, ["thing", "arch"]);
}

#[test]
fn an_os_release_that_says_nothing_useful_resolves_auto_to_generic_alone() {
    assert!(config::identity("").is_empty());
    assert_eq!(
        config::candidates(&Theme::Auto, Path::new("/t"), &[]),
        [PathBuf::from("/t/generic.raba")]
    );
}

#[test]
fn an_auto_outro_is_named_after_the_theme_it_follows_and_is_nothing_without_one() {
    // `Outro.Theme = auto` means "the outro that goes with this logo", which
    // only has an answer once a logo has been resolved. On a machine where
    // none was, there is no outro rather than a wrong one.
    let beside = config::outro_candidates(
        &Theme::Auto,
        Path::new("/t"),
        Some(Path::new("/usr/share/raba/themes/arch.raba")),
    );
    assert_eq!(beside, [PathBuf::from("/usr/share/raba/themes/arch-outro.raba")]);

    assert!(config::outro_candidates(&Theme::Auto, Path::new("/t"), None).is_empty());
    assert!(
        config::outro_candidates(&Theme::Auto, Path::new("/t"), Some(Path::new("/"))).is_empty(),
        "a path with no file name is not a theme to hang an outro off"
    );
}

#[test]
fn a_named_outro_is_looked_for_where_every_other_theme_is() {
    assert_eq!(
        config::outro_candidates(&Theme::Named("fade".to_owned()), Path::new("/t"), None),
        [PathBuf::from("/t/fade.raba")]
    );
}

/// Every key in `raba.conf`(5), given a value it cannot possibly mean.
///
/// This is the whole contract of the lenient parser in one test. A file read
/// as root, before the root filesystem is mounted, by a program that may not
/// fail — so for each key the answer has to be the same three things: the
/// setting keeps its default, the complaint names the section *and* the key,
/// and every other key in the file is still applied.
///
/// One case per key rather than a clever loop over a table of them, because
/// the thing most likely to go wrong is a key missing from the match in
/// `set()`, and a loop over a list written next to it would share the mistake.
#[test]
fn a_value_no_key_can_mean_is_complained_about_by_name_and_changes_nothing() {
    // Compared as `Debug`, because `Config` does not derive `PartialEq` and
    // adding one to shipped code for a test's benefit is the wrong trade.
    let defaults = format!("{:?}", Config::default());
    let cases: &[(&str, &str, &str)] = &[
        ("Boot", "Theme", "../../etc/shadow"),
        ("Boot", "Tty", "sixty-four"),
        ("Boot", "ShowDelay", "soon"),
        ("Boot", "Timeout", "a while"),
        ("Boot", "FrameRateOverride", "fast"),
        ("Outro", "Enable", "perhaps"),
        ("Outro", "Theme", "two words"),
        ("Outro", "MaxDuration", "briefly"),
        ("Outro", "FinishFrame", "sometimes"),
        ("Progress", "Enable", "maybe"),
        ("Progress", "Style", "barber-pole"),
        ("Progress", "Source", "vibes"),
        ("Progress", "Position", "diagonally"),
        ("Progress", "Width", "wide"),
        ("Progress", "Color", "puce"),
        ("Progress", "BackgroundColor", "taupe"),
        ("Handoff", "Mode", "interpretive-dance"),
        ("Handoff", "RestoreTty", "if you like"),
        ("Log", "Level", "shouty"),
        ("Log", "File", "relative/path.log"),
    ];

    for (section, key, nonsense) in cases {
        // A second key after the bad one, so "the parser stopped at the first
        // problem" shows up as a failure rather than as a pass.
        let text = format!("[{section}]\n{key} = {nonsense}\n[Boot]\nTty = 9\n");
        let (config, problems) = parse(&text);

        assert_eq!(problems.len(), 1, "{section}.{key} = {nonsense:?} gave {problems:?}");
        let said = problems.first().expect("one problem");
        assert!(said.contains(section), "{said:?} does not name the section");
        assert!(said.contains(key), "{said:?} does not name the key");
        assert!(said.contains(nonsense), "{said:?} does not quote what was written");
        assert_eq!(
            config.boot.tty,
            Tty::Fixed(9),
            "{section}.{key} being wrong stopped the rest of the file being read"
        );
    }

    // And none of the above moved a setting off its default, which is the
    // property that makes a half-broken file safe to boot from.
    for (section, key, nonsense) in cases {
        let (config, _) = parse(&format!("[{section}]\n{key} = {nonsense}\n"));
        assert_eq!(
            format!("{config:?}"),
            defaults,
            "{section}.{key} = {nonsense:?} changed something"
        );
    }
}

#[test]
fn a_key_that_belongs_to_another_section_is_refused_where_it_does_not_belong() {
    // `Theme` is a key in both [Boot] and [Outro], and in neither [Progress]
    // nor [Log]. Accepting it anywhere would make a misplaced line silently
    // do nothing.
    let (_, problems) = parse("[Progress]\nTheme = arch\n");
    assert_eq!(problems.len(), 1, "{problems:?}");
    let said = problems.first().expect("one problem");
    assert!(said.contains("Theme"), "{said:?}");
    assert!(said.contains("Progress"), "{said:?}");
}

#[test]
fn every_setting_with_a_fixed_set_of_values_prints_back_the_spelling_it_accepts() {
    // `Display` here is what `raba --check` prints and what `--freeze` writes
    // into the initramfs, which `raba` then reads back. So a `Display` that
    // disagreed with `parse` would be a configuration this program wrote and
    // cannot read — and the generated copy is the one that runs at boot.
    let cases: Vec<(String, &str)> = vec![
        (Style::Pulse.to_string(), "Progress.Style"),
        (Style::Bar.to_string(), "Progress.Style"),
        (Style::Percent.to_string(), "Progress.Style"),
        (Source::Ctl.to_string(), "Progress.Source"),
        (Source::Time.to_string(), "Progress.Source"),
        (Source::None.to_string(), "Progress.Source"),
        (Position::Above.to_string(), "Progress.Position"),
        (Position::Below.to_string(), "Progress.Position"),
        (Mode::Retain.to_string(), "Handoff.Mode"),
        (Mode::Clear.to_string(), "Handoff.Mode"),
        (Level::Error.to_string(), "Log.Level"),
        (Level::Warn.to_string(), "Log.Level"),
        (Level::Info.to_string(), "Log.Level"),
        (Level::Debug.to_string(), "Log.Level"),
    ];
    for (spelled, key) in cases {
        assert!(!spelled.is_empty(), "{key} printed nothing");
        let (section, name) = key.split_once('.').expect("a dotted key");
        let text = format!("[{section}]\n{name} = {spelled}\n");
        let (_, problems) = parse(&text);
        assert!(problems.is_empty(), "{key} = {spelled} was written and not read: {problems:?}");
    }
}

#[test]
fn a_configuration_path_that_cannot_be_opened_is_reported_rather_than_skipped() {
    // Absent is normal and silent. Present-but-unreadable is neither: the file
    // says something, we cannot see what, and booting on the defaults while
    // saying nothing means a person who edited `raba.conf` watches their
    // change have no effect with no clue why. A regular file where a directory
    // should be is the reproducible version of it — `ENOTDIR` rather than a
    // permission bit, so the test says the same thing when the suite runs as
    // root, which it does inside the VM harness.
    let scratch = Scratch::new("unreadable");
    scratch.write("in-the-way", "not a directory\n");
    let mut places = scratch.places();
    places.config = scratch.path("in-the-way").join("raba.conf");

    let startup = config::start(&places);
    let problems = &startup.problems;
    let [problem] = problems.as_slice() else { panic!("exactly one problem: {problems:?}") };
    assert_eq!(problem.file, places.config);
    assert_eq!(problem.line, 0, "there is no line to blame: the file never opened");
    assert!(!problem.text.is_empty(), "the operating system's reason is the whole message");
    // And it still booted, on the defaults.
    assert_eq!(startup.config.boot.theme, Theme::Auto);
}

#[test]
fn junk_between_the_settings_does_not_move_where_a_setting_is() {
    // `placement` is what `raba-setup --set-theme` uses to rewrite the line
    // that sets a theme, in a file it did not write. Anything that is not a
    // header and not a pair — a stray word, an unterminated bracket, a
    // comment, a blank — has to be stepped over while still being counted, or
    // the rewrite lands on the wrong line and the edit eats somebody's
    // configuration.
    let text = concat!(
        "# a comment\n",          // 1
        "stray words\n",          // 2
        "[Unterminated\n",        // 3
        "Theme = not-this-one\n", // 4  (inside the unknown section)
        "[Boot]\n",               // 5
        "   \n",                  // 6
        "= no key\n",             // 7
        "Theme = first\n",        // 8
        "nonsense again\n",       // 9
        "Theme = second\n",       // 10
    );
    let found = config::placement(text, "Boot", "Theme");
    assert_eq!(found.header, Some(5));
    assert_eq!(found.lines, vec![8, 10], "both settings, in file order");
    assert_eq!(found.effective(), Some(10), "the last one is the one that counts");

    // A file that is nothing but junk has nowhere to put a setting, and says so
    // rather than offering line 1.
    let nothing = config::placement("stray\n\n# words\n", "Boot", "Theme");
    assert_eq!(nothing.header, None);
    assert!(nothing.lines.is_empty());
    assert_eq!(nothing.effective(), None);
}
