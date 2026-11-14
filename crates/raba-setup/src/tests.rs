//! The half of `raba-setup` that changes a machine, driven against a machine
//! that is not real.
//!
//! # Why these are in the crate and not in `tests/`
//!
//! `tests/cli.rs` runs the real binary, and that is the right way to test an
//! argument parser and every refusal printed in front of the one write this
//! program does. It stops at exactly one line: `rebuild()`, which runs
//! `mkinitcpio -P`. A test suite has no business doing that, so for a long time
//! nothing ran the code past it — `permission` on a machine with a TPM-sealed
//! disk, the prompt, the error when the generator exits non-zero, the order in
//! which `--set-theme` writes and rebuilds. That order is the subject of a
//! review finding recorded in `permission`'s own doc comment: a declined prompt
//! used to print "Nothing was changed" over a file that had just changed.
//!
//! So these drive [`run`] — the same entry point `main` calls, including the
//! argument parsing — with a [`Host`] that is a fake machine. The fake records
//! what it was asked to do and answers however the test needs. What is under
//! test is the decision and the order, which is the part that can cost somebody
//! a boot; what the fake stands in for is `lsblk`, `mkinitcpio` and a terminal.
//!
//! One thing these cannot see: anything only `println!` says. The test harness
//! captures it and there is no stable way to read it back, so the wording of
//! every message stays the business of `tests/cli.rs`, which reads real stdout
//! from a real process. These assert on what *happened*.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]

use super::{machine, permission, run, Host, Permission, Root};
use raba::config;
use std::path::{Path, PathBuf};
use std::process::{ExitCode, ExitStatus};

// ------------------------------------------------------------- the fake ----

/// One thing the program asked the machine to do.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Did {
    Surveyed,
    Asked(Vec<String>),
    Ran(String, Vec<String>),
    Wrote(PathBuf, String),
}

/// A machine that is whatever the test says it is.
struct Fake {
    /// What [`Host::survey`] answers. Built by [`Machine`].
    survey: machine::Survey,
    /// What the person at the keyboard says to the TPM prompt.
    agrees: bool,
    /// What the initramfs generator exits with.
    exit: i32,
    /// Whether the generator can be started at all.
    runnable: bool,
    /// Whether the file can be written.
    writable: bool,
    did: Vec<Did>,
}

impl Host for Fake {
    fn survey(&mut self, _root: &Root) -> machine::Survey {
        self.did.push(Did::Surveyed);
        machine::Survey {
            generators: self.survey.generators.clone(),
            hooks: self.survey.hooks.clone(),
            images: self.survey.images.clone(),
            unreadable: self.survey.unreadable.clone(),
            tpm: self.survey.tpm.clone(),
        }
    }

    fn confirm(&mut self, devices: &[String]) -> bool {
        self.did.push(Did::Asked(devices.to_vec()));
        self.agrees
    }

    fn run(&mut self, program: &str, arguments: &[&str]) -> Result<ExitStatus, String> {
        self.did.push(Did::Ran(
            program.to_owned(),
            arguments.iter().map(|a| (*a).to_owned()).collect(),
        ));
        if !self.runnable {
            return Err(format!("running {program}: No such file or directory (os error 2)"));
        }
        use std::os::unix::process::ExitStatusExt as _;
        // `from_raw` takes a wait(2) status, where the exit code is the high
        // byte. 0 is success and anything else is not, which is all this needs.
        Ok(ExitStatus::from_raw(self.exit << 8))
    }

    fn write(&mut self, path: &Path, text: &str) -> Result<(), String> {
        self.did.push(Did::Wrote(path.to_owned(), text.to_owned()));
        if self.writable {
            std::fs::write(path, text).map_err(|why| why.to_string())
        } else {
            Err(format!("{}: Permission denied (os error 13)", path.display()))
        }
    }
}

impl Fake {
    fn ran(&self) -> Vec<&Did> {
        self.did.iter().filter(|did| matches!(**did, Did::Ran(..))).collect()
    }

    fn wrote(&self) -> Option<&String> {
        self.did.iter().find_map(|did| match did {
            Did::Wrote(_, text) => Some(text),
            _ => None,
        })
    }

    fn asked(&self) -> bool {
        self.did.iter().any(|did| matches!(*did, Did::Asked(_)))
    }
}

/// A survey, built a clause at a time so each test says only what it is about.
struct Machine(machine::Survey);

impl Machine {
    fn new() -> Machine {
        Machine(machine::Survey {
            generators: vec![machine::Generator::Mkinitcpio],
            hooks: Some(["base", "encrypt", "raba", "filesystems"].map(str::to_owned).to_vec()),
            images: Vec::new(),
            unreadable: None,
            tpm: machine::Tpm::Clear,
        })
    }

    fn no_generator(mut self) -> Machine {
        self.0.generators.clear();
        self
    }

    fn generator(mut self, which: machine::Generator) -> Machine {
        self.0.generators = vec![which];
        self
    }

    fn hooks(mut self, line: &[&str]) -> Machine {
        self.0.hooks = Some(line.iter().map(|hook| (*hook).to_owned()).collect());
        self
    }

    fn sealed(mut self, devices: &[&str]) -> Machine {
        self.0.tpm = machine::Tpm::Sealed(devices.iter().map(|d| (*d).to_owned()).collect());
        self
    }

    fn tpm(mut self, tpm: machine::Tpm) -> Machine {
        self.0.tpm = tpm;
        self
    }

    fn image(mut self, name: &str, modified: Option<u64>, carries: Option<&str>) -> Machine {
        self.0.images.push(machine::Image {
            path: PathBuf::from(name),
            modified,
            carries: carries.map(str::to_owned),
        });
        self
    }

    fn unreadable(mut self, why: &str) -> Machine {
        self.0.unreadable = Some(why.to_owned());
        self
    }

    fn into_host(self) -> Fake {
        Fake {
            survey: self.0,
            agrees: false,
            exit: 0,
            runnable: true,
            writable: true,
            did: Vec::new(),
        }
    }
}

// ------------------------------------------------------------- scratch -----

/// A directory to point every path at, so no test can read the machine it runs
/// on, let alone write to it.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let mut path = std::env::temp_dir();
        path.push(format!("raba-setup-unit-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("themes")).expect("scratch");
        Scratch(path)
    }

    fn at(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// A theme file, by name. Empty: what is being resolved is a path, and the
    /// resolution never opens it. See the same note in `tests/cli.rs`.
    fn theme(self, name: &str) -> Scratch {
        std::fs::write(self.at(&format!("themes/{name}.raba")), b"").expect("a theme");
        self
    }

    fn config(self, text: &str) -> Scratch {
        std::fs::write(self.at("raba.conf"), text).expect("a configuration");
        self
    }

    fn config_now(&self) -> Option<String> {
        std::fs::read_to_string(self.at("raba.conf")).ok()
    }

    fn places(&self) -> config::Places {
        config::Places {
            defaults: self.at("raba.defaults"),
            config: self.at("raba.conf"),
            themes: self.at("themes"),
            cmdline: self.at("cmdline"),
            os_release: self.at("os-release"),
        }
    }

    /// The flags that put [`Self::places`] on a command line.
    fn args(&self) -> Vec<String> {
        let mut args = Vec::new();
        for (flag, path) in [
            ("--config", self.at("raba.conf")),
            ("--defaults", self.at("raba.defaults")),
            ("--themes", self.at("themes")),
            ("--cmdline", self.at("cmdline")),
            ("--os-release", self.at("os-release")),
        ] {
            args.push(flag.to_owned());
            args.push(path.display().to_string());
        }
        args
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `ExitCode` is opaque and does not compare, so this does what the shell would.
fn succeeded(code: &ExitCode) -> bool {
    format!("{code:?}") == format!("{:?}", ExitCode::SUCCESS)
}

/// Run the program the way `main` does, with `rest` after the scratch flags.
fn setup(scratch: &Scratch, host: &mut Fake, rest: &[&str]) -> Result<ExitCode, String> {
    let mut args = scratch.args();
    args.extend(rest.iter().map(|arg| (*arg).to_owned()));
    run(host, &args)
}

/// The running system, which is the only root `--apply` accepts. Nothing under
/// it is ever read: every path the test touches comes from `Scratch`, and the
/// machine itself comes from `Fake`.
fn here() -> Root {
    Root::default()
}

// ---------------------------------------------------- the gate in front ----

#[test]
fn another_filesystems_boot_is_never_rebuilt_however_it_is_asked_for() {
    let scratch = Scratch::new("elsewhere").theme("arch");
    let mut host = Machine::new().into_host();
    let refusal =
        permission(&mut host, &Root::new(PathBuf::from("/mnt")), &scratch.places(), true, true)
            .expect_err("--root /mnt must be refused");
    assert!(refusal.contains("somebody else's boot"), "{refusal}");
    // Refused before anything was looked at, let alone run: a survey of the
    // wrong machine is already the wrong answer.
    assert!(host.did.is_empty(), "{:?}", host.did);
}

#[test]
fn the_running_system_is_the_one_root_that_gets_past_the_gate() {
    let scratch = Scratch::new("here").theme("arch").config("[Boot]\nTheme=arch\n");
    let mut host = Machine::new().into_host();
    let granted = permission(&mut host, &here(), &scratch.places(), true, true).expect("granted");
    assert!(matches!(granted, Permission::Granted(_)));
}

// ------------------------------------------------- what stops a rebuild ----

#[test]
fn a_machine_with_no_initramfs_generator_has_nothing_to_rebuild() {
    let scratch = Scratch::new("no-gen").theme("arch").config("[Boot]\nTheme=arch\n");
    let mut host = Machine::new().no_generator().into_host();
    let answer = permission(&mut host, &here(), &scratch.places(), true, true).expect("an answer");
    assert!(matches!(answer, Permission::NothingToRebuild));
}

#[test]
fn a_rebuild_that_would_produce_an_image_with_no_splash_is_refused_before_it_starts() {
    // No theme file anywhere, so `freeze()` is None: thirty seconds of
    // mkinitcpio for an image with nothing in it.
    let scratch = Scratch::new("no-theme");
    let mut host = Machine::new().into_host();
    let refusal =
        permission(&mut host, &here(), &scratch.places(), true, true).expect_err("refused");
    assert!(refusal.contains("no splash"), "{refusal}");
    assert!(refusal.contains("raba --check"), "{refusal}: say how to find out why");
}

#[test]
fn set_theme_is_allowed_on_the_machine_that_currently_resolves_to_no_theme_at_all() {
    // The same machine as above, and the opposite answer, because `--set-theme`
    // is the command that is about to supply the thing that is missing.
    let scratch = Scratch::new("no-theme-yet");
    let mut host = Machine::new().into_host();
    let answer = permission(&mut host, &here(), &scratch.places(), true, false).expect("an answer");
    assert!(matches!(answer, Permission::Granted(_)));
}

#[test]
fn a_hooks_line_without_raba_in_it_is_refused_rather_than_rebuilt_into_nothing() {
    let scratch = Scratch::new("no-hook").theme("arch").config("[Boot]\nTheme=arch\n");
    let mut host = Machine::new().hooks(&["base", "udev", "autodetect", "filesystems"]).into_host();
    let answer = permission(&mut host, &here(), &scratch.places(), true, true).expect("an answer");
    assert!(matches!(answer, Permission::Refused));
}

#[test]
fn a_machine_whose_mkinitcpio_conf_could_not_be_read_is_not_second_guessed() {
    // No HOOKS line at all is not the same as a HOOKS line without `raba`:
    // the first is "this program could not tell", and guessing it means
    // refusing a rebuild that would have worked.
    let scratch = Scratch::new("no-hooks-line").theme("arch").config("[Boot]\nTheme=arch\n");
    let mut machine = Machine::new();
    machine.0.hooks = None;
    let mut host = machine.into_host();
    let answer = permission(&mut host, &here(), &scratch.places(), true, true).expect("an answer");
    assert!(matches!(answer, Permission::Granted(_)));
}

// ------------------------------------------------------------- the TPM -----

#[test]
fn a_tpm_sealed_disk_is_asked_about_before_anything_is_rebuilt() {
    let scratch = Scratch::new("sealed").theme("arch").config("[Boot]\nTheme=arch\n");
    let mut host = Machine::new().sealed(&["/dev/nvme0n1p2"]).into_host();
    let answer = permission(&mut host, &here(), &scratch.places(), false, true).expect("an answer");
    assert!(matches!(answer, Permission::Refused), "a `no` must stop it");
    assert_eq!(host.did.last(), Some(&Did::Asked(vec!["/dev/nvme0n1p2".to_owned()])));
}

#[test]
fn saying_yes_to_the_tpm_question_lets_the_rebuild_through() {
    let scratch = Scratch::new("sealed-yes").theme("arch").config("[Boot]\nTheme=arch\n");
    let mut host = Machine::new().sealed(&["/dev/sda2"]).into_host();
    host.agrees = true;
    let answer = permission(&mut host, &here(), &scratch.places(), false, true).expect("an answer");
    assert!(matches!(answer, Permission::Granted(_)));
    assert!(host.asked());
}

#[test]
fn yes_on_the_command_line_means_the_question_is_not_asked_at_all() {
    let scratch = Scratch::new("sealed-flag").theme("arch").config("[Boot]\nTheme=arch\n");
    let mut host = Machine::new().sealed(&["/dev/sda2"]).into_host();
    let answer = permission(&mut host, &here(), &scratch.places(), true, true).expect("an answer");
    assert!(matches!(answer, Permission::Granted(_)));
    assert!(!host.asked(), "--yes is an answer, not a reason to ask again");
}

#[test]
fn every_sealed_device_is_named_in_the_question_and_not_just_the_first() {
    let scratch = Scratch::new("sealed-two").theme("arch").config("[Boot]\nTheme=arch\n");
    let mut host = Machine::new().sealed(&["/dev/sda2", "/dev/sdb1"]).into_host();
    let _ = permission(&mut host, &here(), &scratch.places(), false, true);
    assert_eq!(
        host.did.last(),
        Some(&Did::Asked(vec!["/dev/sda2".to_owned(), "/dev/sdb1".to_owned()]))
    );
}

#[test]
fn a_tpm_that_could_not_be_checked_does_not_stop_a_rebuild_and_does_not_ask() {
    // `Unknown` is "not root" or "cryptsetup is missing". Treating it as
    // `Sealed` would put a prompt in front of every rebuild on every machine
    // without cryptsetup; treating it as `Clear` is what the dry run's warning
    // is for.
    let scratch = Scratch::new("tpm-unknown").theme("arch").config("[Boot]\nTheme=arch\n");
    let mut host = Machine::new()
        .tpm(machine::Tpm::Unknown("cryptsetup is not installed".to_owned()))
        .into_host();
    let answer = permission(&mut host, &here(), &scratch.places(), false, true).expect("an answer");
    assert!(matches!(answer, Permission::Granted(_)));
    assert!(!host.asked());
}

// ------------------------------------------------------------- --apply -----

#[test]
fn apply_runs_the_generator_this_machine_actually_has() {
    for (generator, expected) in [
        (machine::Generator::Mkinitcpio, ("mkinitcpio", vec!["-P"])),
        (machine::Generator::Dracut, ("dracut", vec!["--force"])),
        (machine::Generator::InitramfsTools, ("update-initramfs", vec!["-u"])),
    ] {
        let scratch = Scratch::new("apply").theme("arch").config("[Boot]\nTheme=arch\n");
        let mut host = Machine::new().generator(generator).into_host();
        let code = setup(&scratch, &mut host, &["--apply", "--yes"]).expect("apply");
        assert!(succeeded(&code));
        assert_eq!(
            host.ran(),
            vec![&Did::Ran(
                expected.0.to_owned(),
                expected.1.iter().map(|a| (*a).to_owned()).collect()
            )]
        );
    }
}

#[test]
fn a_generator_that_fails_is_reported_as_a_failure_and_not_as_a_rebuilt_image() {
    let scratch = Scratch::new("apply-fails").theme("arch").config("[Boot]\nTheme=arch\n");
    let mut host = Machine::new().into_host();
    host.exit = 1;
    let why = setup(&scratch, &mut host, &["--apply", "--yes"]).expect_err("a failure");
    assert!(why.contains("mkinitcpio"), "{why}: name the program that failed");
    assert!(why.contains("not rebuilt"), "{why}: say what did not happen");
}

#[test]
fn a_generator_that_is_not_installed_says_so_rather_than_claiming_success() {
    let scratch = Scratch::new("apply-missing").theme("arch").config("[Boot]\nTheme=arch\n");
    let mut host = Machine::new().into_host();
    host.runnable = false;
    let why = setup(&scratch, &mut host, &["--apply", "--yes"]).expect_err("a failure");
    assert!(why.contains("mkinitcpio"), "{why}");
}

#[test]
fn apply_on_a_machine_with_no_generator_is_an_error_and_not_a_quiet_success() {
    // The one place `NothingToRebuild` is fatal: `--apply` is *about* the
    // rebuild, so there being nothing to rebuild is a failed command.
    let scratch = Scratch::new("apply-no-gen").theme("arch").config("[Boot]\nTheme=arch\n");
    let mut host = Machine::new().no_generator().into_host();
    let why = setup(&scratch, &mut host, &["--apply", "--yes"]).expect_err("a failure");
    assert!(why.contains("nothing to rebuild"), "{why}");
    assert!(host.ran().is_empty());
}

#[test]
fn a_refused_apply_exits_non_zero_and_runs_nothing() {
    let scratch = Scratch::new("apply-refused").theme("arch").config("[Boot]\nTheme=arch\n");
    let mut host = Machine::new().sealed(&["/dev/sda2"]).into_host();
    let code = setup(&scratch, &mut host, &["--apply"]).expect("a decision, not an error");
    assert!(!succeeded(&code));
    assert!(host.ran().is_empty());
}

// --------------------------------------------------------- --set-theme -----

#[test]
fn set_theme_writes_the_line_and_then_rebuilds_in_that_order() {
    let scratch = Scratch::new("set").theme("arch").theme("debian").config("[Boot]\nTheme=arch\n");
    let mut host = Machine::new().into_host();
    let code = setup(&scratch, &mut host, &["--set-theme", "debian", "--yes"]).expect("set-theme");
    assert!(succeeded(&code));
    assert!(scratch.config_now().expect("the file").contains("Theme=debian"));
    let order: Vec<&Did> =
        host.did.iter().filter(|did| matches!(**did, Did::Wrote(..) | Did::Ran(..))).collect();
    assert!(matches!(order.first(), Some(Did::Wrote(..))), "{order:?}");
    assert!(matches!(order.get(1), Some(Did::Ran(..))), "{order:?}");
}

#[test]
fn a_refused_set_theme_leaves_the_configuration_byte_for_byte_as_it_was() {
    // The review finding this order exists for: the prompt used to come after
    // the write, so declining it printed "Nothing was changed" over a file
    // that had just changed.
    let before = "[Boot]\nTheme=arch\nTty=7\n";
    let scratch = Scratch::new("set-refused").theme("arch").theme("debian").config(before);
    let mut host = Machine::new().sealed(&["/dev/sda2"]).into_host();
    let code = setup(&scratch, &mut host, &["--set-theme", "debian"]).expect("a decision");
    assert!(!succeeded(&code));
    assert_eq!(scratch.config_now().as_deref(), Some(before));
    assert!(host.wrote().is_none(), "nothing may be written after a refusal");
    assert!(host.ran().is_empty());
}

#[test]
fn set_theme_on_a_machine_with_no_initramfs_still_writes_the_line_and_succeeds() {
    // A machine started by its init system reads /etc directly, so the edit is
    // the whole operation and there is nothing to bake it into.
    let scratch = Scratch::new("set-no-gen").theme("arch").config("[Boot]\nTheme=foo\n");
    let mut host = Machine::new().no_generator().into_host();
    let code = setup(&scratch, &mut host, &["--set-theme", "arch", "--yes"]).expect("set-theme");
    assert!(succeeded(&code));
    assert!(scratch.config_now().expect("the file").contains("Theme=arch"));
    assert!(host.ran().is_empty());
}

#[test]
fn setting_the_theme_that_is_already_set_writes_nothing_and_still_rebuilds() {
    // The rebuild is the half that makes the line true at boot, and an image
    // can be out of date whatever the file says.
    let scratch = Scratch::new("set-same").theme("arch").config("[Boot]\nTheme=arch\n");
    let mut host = Machine::new().into_host();
    let code = setup(&scratch, &mut host, &["--set-theme", "arch", "--yes"]).expect("set-theme");
    assert!(succeeded(&code));
    assert!(host.wrote().is_none(), "an unchanged file must not be rewritten");
    assert_eq!(host.ran().len(), 1);
}

#[test]
fn set_theme_on_a_machine_with_no_raba_conf_at_all_creates_one() {
    let scratch = Scratch::new("set-nofile").theme("arch");
    let mut host = Machine::new().into_host();
    let code = setup(&scratch, &mut host, &["--set-theme", "arch", "--yes"]).expect("set-theme");
    assert!(succeeded(&code));
    assert!(scratch.config_now().expect("a new file").contains("Theme=arch"));
}

#[test]
fn a_file_that_cannot_be_written_stops_the_command_before_the_rebuild() {
    // Rebuilding after a failed write would bake the *old* theme into the
    // image and report success for a theme change that did not happen.
    let scratch = Scratch::new("set-readonly").theme("arch").theme("debian").config("[Boot]\n");
    let mut host = Machine::new().into_host();
    host.writable = false;
    let why =
        setup(&scratch, &mut host, &["--set-theme", "debian", "--yes"]).expect_err("a failure");
    assert!(why.contains("Permission denied"), "{why}");
    assert!(host.ran().is_empty(), "nothing may be rebuilt after a failed write");
}

#[test]
fn a_theme_that_does_not_exist_is_refused_without_the_machine_being_touched() {
    let scratch = Scratch::new("set-unknown").theme("arch").config("[Boot]\nTheme=arch\n");
    let mut host = Machine::new().into_host();
    let why =
        setup(&scratch, &mut host, &["--set-theme", "nosuch", "--yes"]).expect_err("a failure");
    assert!(why.contains("no such theme"), "{why}");
    assert!(host.did.is_empty(), "{:?}: refused before the machine is read", host.did);
}

// ----------------------------------------------------------------- what -----
// ------------------------------------------------- an image is, or is not ---

#[test]
fn describe_says_which_of_the_five_answers_this_is() {
    use machine::Currency;
    for (currency, must_say) in [
        (Currency::Current, "current"),
        (
            Currency::Stale("the configuration inside the image is not the current one"),
            "not the current",
        ),
        (Currency::ProbablyStale, "probably stale"),
        (Currency::ProbablyCurrent, "probably current"),
        (Currency::Unknown, "nothing here could tell"),
    ] {
        let said = super::describe(&currency);
        assert!(said.contains(must_say), "{currency:?} reads {said:?}");
    }
}

#[test]
fn check_passes_only_when_every_image_carries_the_configuration_that_would_be_generated_now() {
    let scratch = Scratch::new("check-current").theme("arch").config("[Boot]\nTheme=arch\n");
    let frozen = config::start(&scratch.places()).freeze().expect("a frozen configuration");
    let mut host =
        Machine::new().image("/boot/initramfs-linux.img", Some(1), Some(&frozen)).into_host();
    let code = setup(&scratch, &mut host, &["--check"]);
    assert!(succeeded(&code.expect("check")));
}

#[test]
fn an_image_carrying_a_different_configuration_fails_the_check() {
    let scratch = Scratch::new("check-stale").theme("arch").config("[Boot]\nTheme=arch\n");
    let mut host = Machine::new()
        .image(
            "/boot/initramfs-linux.img",
            Some(1),
            Some("[Boot]\nTheme=/usr/share/raba/old.raba\n"),
        )
        .into_host();
    let code = setup(&scratch, &mut host, &["--check"]).expect("check");
    assert!(!succeeded(&code));
}

#[test]
fn an_image_nothing_could_look_inside_fails_the_check_rather_than_passing_it() {
    // Unknown is not a pass. A post-install that treats "could not tell" as
    // "fine" is a post-install that never reports anything.
    let scratch = Scratch::new("check-unknown").theme("arch").config("[Boot]\nTheme=arch\n");
    let mut host = Machine::new().image("/boot/initramfs-linux.img", None, None).into_host();
    let code = setup(&scratch, &mut host, &["--check"]).expect("check");
    assert!(!succeeded(&code));
}

#[test]
fn an_image_older_than_the_theme_it_should_carry_fails_the_check() {
    let scratch = Scratch::new("check-old").theme("arch").config("[Boot]\nTheme=arch\n");
    // The theme file was written a moment ago; the image claims 1970.
    let mut host = Machine::new().image("/boot/initramfs-linux.img", Some(1), None).into_host();
    let code = setup(&scratch, &mut host, &["--check"]).expect("check");
    assert!(!succeeded(&code));
}

#[test]
fn an_image_newer_than_everything_that_goes_into_it_passes_on_the_weaker_evidence() {
    let scratch = Scratch::new("check-new").theme("arch").config("[Boot]\nTheme=arch\n");
    let far_future = 4_102_444_800; // 2100-01-01
    let mut host =
        Machine::new().image("/boot/initramfs-linux.img", Some(far_future), None).into_host();
    let code = setup(&scratch, &mut host, &["--check"]).expect("check");
    assert!(succeeded(&code));
}

#[test]
fn a_machine_with_no_image_fails_the_check_and_says_so() {
    let scratch = Scratch::new("check-none").theme("arch").config("[Boot]\nTheme=arch\n");
    let mut host = Machine::new().into_host();
    let code = setup(&scratch, &mut host, &["--check"]).expect("check");
    assert!(!succeeded(&code));
}

#[test]
fn a_machine_with_no_theme_fails_the_check_before_it_looks_at_any_image() {
    let scratch = Scratch::new("check-no-theme");
    let mut host = Machine::new().image("/boot/initramfs-linux.img", Some(1), None).into_host();
    let code = setup(&scratch, &mut host, &["--check"]).expect("check");
    assert!(!succeeded(&code));
}

// ------------------------------------------------------------ dry run ------

#[test]
fn the_dry_run_changes_nothing_whatever_the_machine_looks_like() {
    let sealed = Machine::new()
        .sealed(&["/dev/nvme0n1p2"])
        .image("/boot/initramfs-linux.img", Some(1), None)
        .hooks(&["base", "raba", "encrypt", "filesystems"]);
    let plymouth = Machine::new().hooks(&["base", "plymouth", "raba", "filesystems"]);
    let unknown = Machine::new()
        .tpm(machine::Tpm::Unknown("not root".to_owned()))
        .unreadable("/boot: Permission denied (os error 13)");
    let bare = Machine::new().no_generator().tpm(machine::Tpm::Clear);
    let dracut = Machine::new().generator(machine::Generator::Dracut);
    for machine in [sealed, plymouth, unknown, bare, dracut] {
        let scratch = Scratch::new("dry").theme("arch").config("[Boot]\nTheme=arch\n");
        let mut host = machine.into_host();
        let code = setup(&scratch, &mut host, &[]).expect("a dry run");
        assert!(succeeded(&code), "the dry run always succeeds; it is a report");
        assert!(host.wrote().is_none());
        assert!(host.ran().is_empty());
    }
}

#[test]
fn a_dry_run_on_a_machine_with_no_configuration_at_all_still_reports() {
    let scratch = Scratch::new("dry-bare");
    let mut host = Machine::new().into_host();
    let code = setup(&scratch, &mut host, &[]).expect("a dry run");
    assert!(succeeded(&code));
}

// ------------------------------------------------------- the one question ---

/// What was typed, as if at a terminal.
fn typed(answer: &str) -> bool {
    super::answered_yes(&mut std::io::BufReader::new(answer.as_bytes()))
}

#[test]
fn only_a_yes_is_a_yes_and_everything_else_is_a_no() {
    // What this decides is whether an initramfs is rebuilt on a machine whose
    // disk will stop unlocking itself afterwards. The only safe way to be
    // wrong is to read a yes as a no.
    for yes in ["y\n", "Y\n", "yes\n", "YES\n", "  yes  \n", "Yes"] {
        assert!(typed(yes), "{yes:?} was not taken as a yes");
    }
    for no in ["n\n", "no\n", "", "\n", "  \n", "yeah\n", "yes please\n", "ok\n", "sim\n", "1\n"] {
        assert!(!typed(no), "{no:?} was taken as a yes");
    }
}

#[test]
fn end_of_file_is_a_no_so_a_command_run_from_a_script_does_not_rebuild_by_accident() {
    // `raba-setup --apply < /dev/null` from a post-install hook reads nothing.
    // Nobody said yes.
    assert!(!typed(""));
}

// ---------------------------------------------------------- the real one ----

/// The shipped [`Host`] is four lines of delegation and the fake cannot prove
/// any of them. If `run` reported failure as success, every test above would
/// still pass and `raba-setup --apply` would say "done" over an mkinitcpio
/// that exited 1.
#[test]
fn the_real_host_reports_what_actually_happened() {
    let mut host = super::ThisMachine;

    let ran = host.run("true", &[]).expect("`true` is installed");
    assert!(ran.success());
    let failed = host.run("false", &[]).expect("`false` is installed");
    assert!(!failed.success(), "a program that exited 1 was reported as success");
    let missing = host.run("raba-no-such-program", &[]);
    assert!(missing.is_err(), "a program that cannot be started was reported as a run");

    let scratch = Scratch::new("real-host");
    let at = scratch.at("written.conf");
    host.write(&at, "[Boot]\nTheme=arch\n").expect("writing");
    assert_eq!(std::fs::read_to_string(&at).expect("reading back"), "[Boot]\nTheme=arch\n");

    // And a path that cannot be written is an error rather than a silent
    // nothing: `raba-setup --set-theme` reports success on what `write`
    // returns.
    let blocked = scratch.at("themes");
    assert!(host.write(&blocked, "x").is_err(), "writing over a directory was reported as done");

    // A survey of a directory that is not a machine finds no generator, which
    // is the answer `permission` turns into `NothingToRebuild`.
    let looked = host.survey(&Root::new(scratch.at("nowhere")));
    assert!(looked.generators.is_empty());
    assert!(matches!(looked.tpm, machine::Tpm::Unknown(_)), "{:?}", looked.tpm);
}

#[test]
fn a_configuration_file_that_cannot_be_read_stops_set_theme_before_it_writes() {
    // Not "there is no file" — that is allowed, and `set_theme` creates one.
    // This is a file that exists and will not open, which on a running system
    // means permissions and here means a directory sitting where `raba.conf`
    // should be. Either way, writing over it blind is how a half-readable
    // /etc becomes an unreadable one.
    let scratch = Scratch::new("unreadable-config").theme("arch");
    std::fs::create_dir_all(scratch.at("raba.conf")).expect("something in the way");
    let mut host = Machine::new().into_host();
    let why = setup(&scratch, &mut host, &["--set-theme", "arch", "--yes"])
        .expect_err("an unreadable configuration is a failure");
    assert!(why.contains("raba.conf"), "{why}");
    assert!(host.wrote().is_none(), "it wrote over a file it could not read");
    assert!(host.ran().is_empty(), "it rebuilt an image for a theme it never set");
}

#[test]
fn an_answer_that_is_not_text_is_not_a_yes() {
    // The prompt is read from whatever stdin is, and in a hook that can be a
    // pipe carrying anything. `read_line` fails on bytes that are not UTF-8,
    // and a failed read must come out as "no": the question being answered is
    // whether to rebuild an initramfs that will then ask for a passphrase at
    // every boot, and the one answer that must never be inferred is yes.
    assert!(!super::answered_yes(&mut &b"\xff\xfe not text\n"[..]));
    // End of file — `raba-setup </dev/null` — is the same answer.
    assert!(!super::answered_yes(&mut &b""[..]));
    // And the ones that are a yes still are.
    for yes in ["y\n", "Y\n", "yes\n", "YES\n", " yes \n"] {
        assert!(super::answered_yes(&mut yes.as_bytes()), "{yes:?}");
    }
    for no in ["n\n", "\n", "no\n", "yeah\n", "sure\n"] {
        assert!(!super::answered_yes(&mut no.as_bytes()), "{no:?}");
    }
}
