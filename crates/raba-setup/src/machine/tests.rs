//! Unit tests for [`super`].
//!
//! Its own file rather than a `mod tests` block inside the module it tests, so
//! that `scripts/coverage.sh` can leave it out: test code counted as covered
//! product code makes the only number anybody reads say the wrong thing. See
//! `docs/maintenance.md`.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
#![allow(clippy::arithmetic_side_effects, clippy::cast_lossless)]

use super::*;

fn hooks(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

/// The real one from a CachyOS machine, with the comments mkinitcpio ships.
const CONF: &str = "\
# vim:set ft=sh
MODULES=()
BINARIES=()
FILES=()
# HOOKS=(base udev autodetect modconf block filesystems fsck)
HOOKS=(base systemd autodetect microcode kms modconf block keyboard sd-vconsole lvm2 filesystems)
COMPRESSION=\"zstd\"
";

#[test]
fn the_hooks_line_is_the_last_one_that_is_not_a_comment() {
    let parsed = parse_hooks(CONF).expect("a HOOKS line");
    assert_eq!(parsed.first().map(String::as_str), Some("base"));
    assert_eq!(parsed.last().map(String::as_str), Some("filesystems"));
    // The commented-out example above it is not it.
    assert!(!parsed.contains(&"fsck".to_owned()), "{parsed:?}");
}

#[test]
fn a_hooks_line_may_be_spread_over_several_lines() {
    // mkinitcpio.conf is shell, and people wrap long arrays.
    let parsed = parse_hooks(
        "HOOKS=(base udev autodetect\n        modconf block encrypt\n        filesystems)\n",
    )
    .expect("a HOOKS line");
    assert_eq!(
        parsed,
        hooks(&["base", "udev", "autodetect", "modconf", "block", "encrypt", "filesystems"])
    );
}

#[test]
fn a_trailing_comment_is_not_a_hook() {
    let parsed = parse_hooks("HOOKS=(base udev) # the defaults\n").expect("a HOOKS line");
    assert_eq!(parsed, hooks(&["base", "udev"]));
}

#[test]
fn quoted_hooks_are_the_same_hooks() {
    let parsed = parse_hooks("HOOKS=('base' \"udev\")\n").expect("a HOOKS line");
    assert_eq!(parsed, hooks(&["base", "udev"]));
}

#[test]
fn a_file_with_no_hooks_line_yields_nothing_rather_than_a_guess() {
    assert_eq!(parse_hooks("MODULES=()\nCOMPRESSION=zstd\n"), None);
}

#[test]
fn the_one_ordering_rule_is_after_encrypt() {
    // RABA does not supply the passphrase prompt, so it must not be up
    // when cryptsetup writes one. See docs/luks.md.
    assert_eq!(advise(&hooks(&["base", "encrypt", "raba", "filesystems"])), Advice::Good);
    assert_eq!(advise(&hooks(&["base", "raba", "encrypt", "filesystems"])), Advice::BeforeEncrypt);
    assert_eq!(
        advise(&hooks(&["base", "raba", "sd-encrypt", "filesystems"])),
        Advice::BeforeEncrypt
    );
    // No encryption at all: anywhere is fine.
    assert_eq!(advise(&hooks(&["base", "raba", "filesystems"])), Advice::Good);
    assert_eq!(advise(&hooks(&["base", "filesystems"])), Advice::NotInHooks);
}

#[test]
fn the_suggested_line_puts_raba_where_it_belongs() {
    assert_eq!(
        suggested(&hooks(&["base", "encrypt", "filesystems", "fsck"])),
        "HOOKS=(base encrypt raba filesystems fsck)"
    );
    // No encrypt: before the hook that mounts the root, which is the last
    // useful moment.
    assert_eq!(
        suggested(&hooks(&["base", "udev", "filesystems"])),
        "HOOKS=(base udev raba filesystems)"
    );
    // Neither: the end is as good as anywhere.
    assert_eq!(suggested(&hooks(&["base", "udev"])), "HOOKS=(base udev raba)");
}

#[test]
fn suggesting_a_line_that_already_has_raba_moves_it_rather_than_doubling_it() {
    let line = suggested(&hooks(&["base", "raba", "encrypt", "filesystems"]));
    assert_eq!(line, "HOOKS=(base encrypt raba filesystems)");
    assert_eq!(line.matches("raba").count(), 1, "{line}");
}

#[test]
fn plymouth_in_the_same_hooks_is_worth_saying_out_loud() {
    assert!(shares_with_plymouth(&hooks(&["base", "plymouth", "raba"])));
    assert!(shares_with_plymouth(&hooks(&["base", "sd-plymouth"])));
    assert!(!shares_with_plymouth(&hooks(&["base", "raba"])));
}

#[test]
fn encrypted_volumes_come_out_of_lsblk() {
    let listing = "\
/dev/sda \n/dev/sda1 ext4\n/dev/nvme0n1 \n/dev/nvme0n1p1 vfat\n/dev/nvme0n1p2 crypto_LUKS\n";
    assert_eq!(parse_luks(listing), ["/dev/nvme0n1p2"]);
}

#[test]
fn a_tpm2_token_is_found_in_a_luks_header_dump() {
    let sealed = "\
LUKS header information
Version:        2
Keyslots:
  0: luks2
Tokens:
  0: systemd-tpm2
    Keyslot:    1
Digests:
  0: pbkdf2
";
    let plain = "\
LUKS header information
Version:        2
Keyslots:
  0: luks2
Tokens:
Digests:
  0: pbkdf2
";
    assert!(dump_has_tpm2(sealed));
    assert!(!dump_has_tpm2(plain));
}

fn image(carries: Option<&str>, modified: Option<u64>) -> Image {
    Image {
        path: PathBuf::from("/boot/initramfs-linux.img"),
        modified,
        carries: carries.map(ToOwned::to_owned),
    }
}

#[test]
fn an_image_carrying_the_current_configuration_is_current() {
    let frozen = "[Boot]\nTheme=/usr/share/raba/themes/arch.raba\n";
    assert_eq!(currency(&image(Some(frozen), Some(100)), frozen, &[Some(500)]), Currency::Current);
}

/// The content answer beats the timestamp answer, in both directions.
///
/// An image rebuilt for an unrelated reason is newer than everything and
/// still carries the old copy; an image older than a theme file somebody
/// touched carries the right one. Timestamps are the fallback, and they are
/// reported as a guess.
#[test]
fn what_is_inside_the_image_beats_how_old_it_is() {
    let now = "[Boot]\nTheme=/usr/share/raba/themes/void.raba\n";
    let old = "[Boot]\nTheme=/usr/share/raba/themes/arch.raba\n";
    assert!(matches!(
        currency(&image(Some(old), Some(9_000)), now, &[Some(1)]),
        Currency::Stale(_)
    ));
    assert_eq!(currency(&image(Some(now), Some(1)), now, &[Some(9_000)]), Currency::Current);
}

#[test]
fn with_nothing_readable_inside_the_answer_is_a_guess_and_says_so() {
    let frozen = "[Boot]\n";
    assert_eq!(
        currency(&image(None, Some(100)), frozen, &[Some(500), None]),
        Currency::ProbablyStale
    );
    assert_eq!(
        currency(&image(None, Some(900)), frozen, &[Some(500), None]),
        Currency::ProbablyCurrent
    );
    assert_eq!(currency(&image(None, Some(900)), frozen, &[None]), Currency::Unknown);
    assert_eq!(currency(&image(None, None), frozen, &[Some(5)]), Currency::Unknown);
}

// --- what the machine says it is ---------------------------------------------

/// A stand-in for `lsblk` and `cryptsetup`, so that the answers under test can
/// be arranged. See [`super::really_run`] for why this seam exists.
fn answering(replies: &[(&'static str, Ran)]) -> impl FnMut(&str, &[&str]) -> Ran {
    let replies: Vec<(String, Ran)> =
        replies.iter().map(|(program, ran)| ((*program).to_owned(), ran.clone())).collect();
    move |program, _arguments| {
        replies
            .iter()
            .find(|(name, _)| name == program)
            .map_or(Ran::Missing, |(_, ran)| ran.clone())
    }
}

/// One LUKS volume and one that is not, as `lsblk --raw` prints them.
const LSBLK: &str = "/dev/nvme0n1p1 vfat\n/dev/nvme0n1p2 crypto_LUKS\n";

/// The three lines of `cryptsetup luksDump` that matter.
const SEALED: &str = "\
Keyslots:
  0: luks2
Tokens:
  0: systemd-tpm2
";

const NOT_SEALED: &str = "\
Keyslots:
  0: luks2
Tokens:
";

fn here() -> Root {
    Root::default()
}

#[test]
fn a_root_that_is_not_this_machine_is_never_reported_as_having_no_tpm() {
    // "I am looking at a directory" and "there is no encryption here" are
    // opposite statements and the dry run prints them differently.
    let answer = tpm_with(&Root::new(PathBuf::from("/mnt")), &mut answering(&[]));
    assert!(
        matches!(answer, Tpm::Unknown(ref why) if why.contains("running system")),
        "{answer:?}"
    );
}

#[test]
fn a_machine_with_no_encrypted_volume_is_clear() {
    let mut run = answering(&[("lsblk", Ran::Said("/dev/sda1 ext4\n/dev/sda2 swap\n".to_owned()))]);
    assert_eq!(tpm_with(&here(), &mut run), Tpm::Clear);
}

#[test]
fn an_encrypted_volume_with_a_tpm_token_is_the_warning_this_whole_check_exists_for() {
    let mut run = answering(&[
        ("lsblk", Ran::Said(LSBLK.to_owned())),
        ("cryptsetup", Ran::Said(SEALED.to_owned())),
    ]);
    assert_eq!(tpm_with(&here(), &mut run), Tpm::Sealed(vec!["/dev/nvme0n1p2".to_owned()]));
}

#[test]
fn an_encrypted_volume_with_a_passphrase_and_no_token_is_clear() {
    // Rebuilding an initramfs on this machine is uneventful: the key is not
    // sealed against anything a changed image would alter.
    let mut run = answering(&[
        ("lsblk", Ran::Said(LSBLK.to_owned())),
        ("cryptsetup", Ran::Said(NOT_SEALED.to_owned())),
    ]);
    assert_eq!(tpm_with(&here(), &mut run), Tpm::Clear);
}

#[test]
fn a_header_that_could_not_be_read_says_so_and_names_the_device() {
    // The usual reason is not being root. Reporting it as `Clear` would be
    // this program telling somebody it is safe to rebuild when it has no idea.
    let mut run =
        answering(&[("lsblk", Ran::Said(LSBLK.to_owned())), ("cryptsetup", Ran::Refused)]);
    let answer = tpm_with(&here(), &mut run);
    assert!(
        matches!(answer, Tpm::Unknown(ref why) if why.contains("/dev/nvme0n1p2") && why.contains("root")),
        "{answer:?}"
    );
}

#[test]
fn a_missing_cryptsetup_is_unknown_rather_than_clear() {
    let mut run = answering(&[("lsblk", Ran::Said(LSBLK.to_owned()))]);
    let answer = tpm_with(&here(), &mut run);
    assert!(matches!(answer, Tpm::Unknown(ref why) if why.contains("cryptsetup")), "{answer:?}");
}

#[test]
fn a_missing_lsblk_is_unknown_rather_than_clear() {
    let answer = tpm_with(&here(), &mut answering(&[]));
    assert!(matches!(answer, Tpm::Unknown(ref why) if why.contains("lsblk")), "{answer:?}");
}

#[test]
fn an_lsblk_that_runs_and_fails_is_as_uninformative_as_one_that_is_missing() {
    let answer = tpm_with(&here(), &mut answering(&[("lsblk", Ran::Refused)]));
    assert!(matches!(answer, Tpm::Unknown(_)), "{answer:?}");
}

#[test]
fn every_sealed_volume_is_reported_and_not_only_the_first() {
    let two = "/dev/sda2 crypto_LUKS\n/dev/sdb1 crypto_LUKS\n";
    let mut run = answering(&[
        ("lsblk", Ran::Said(two.to_owned())),
        ("cryptsetup", Ran::Said(SEALED.to_owned())),
    ]);
    assert_eq!(
        tpm_with(&here(), &mut run),
        Tpm::Sealed(vec!["/dev/sda2".to_owned(), "/dev/sdb1".to_owned()])
    );
}

// --- reading inside an image --------------------------------------------------

const FROZEN: &str = "# Generated for the initramfs.\n\n[Boot]\nTheme=/usr/share/raba/arch.raba\n";

#[test]
fn the_configuration_inside_an_image_is_read_by_whichever_tool_is_installed() {
    for installed in ["lsinitrd", "bsdtar"] {
        let mut run = answering(&[(
            if installed == "lsinitrd" { "lsinitrd" } else { "bsdtar" },
            Ran::Said(FROZEN.to_owned()),
        )]);
        let read = read_from_image_with(Path::new("/boot/initramfs-linux.img"), &mut run);
        assert_eq!(read.as_deref(), Some(FROZEN), "with only {installed}");
    }
}

#[test]
fn a_machine_with_neither_tool_reads_nothing_rather_than_guessing() {
    // Which is not a failure: the caller falls back to comparing timestamps
    // and says in so many words that that is what it did.
    let read = read_from_image_with(Path::new("/boot/initramfs-linux.img"), &mut answering(&[]));
    assert_eq!(read, None);
}

#[test]
fn an_image_with_no_raba_in_it_reads_nothing_even_though_the_tool_succeeded() {
    // `bsdtar` extracting a path that is not in the archive prints nothing and
    // exits 0 on some versions. An empty string is not a configuration.
    let mut run = answering(&[("bsdtar", Ran::Said(String::new()))]);
    assert_eq!(read_from_image_with(Path::new("/boot/x.img"), &mut run), None);
}

#[test]
fn a_tool_that_fails_is_passed_over_so_the_other_one_still_gets_a_turn() {
    // lsinitrd is tried first and cannot open an mkinitcpio image on a machine
    // without dracut's helpers; bsdtar then reads it.
    let mut run =
        answering(&[("lsinitrd", Ran::Refused), ("bsdtar", Ran::Said(FROZEN.to_owned()))]);
    assert_eq!(
        read_from_image_with(Path::new("/boot/initramfs-linux.img"), &mut run).as_deref(),
        Some(FROZEN)
    );
}

// --- which program builds the image -------------------------------------------

#[test]
fn every_generator_names_itself_and_the_command_that_rebuilds_everything() {
    for (generator, name, command) in [
        (Generator::Mkinitcpio, "mkinitcpio", "mkinitcpio -P"),
        (Generator::Dracut, "dracut", "dracut --force"),
        (Generator::InitramfsTools, "initramfs-tools", "update-initramfs -u"),
    ] {
        assert_eq!(generator.name(), name);
        assert_eq!(generator.rebuild().join(" "), command);
    }
}

// --- which files in /boot are initramfs images --------------------------------

/// A `/boot` with the given files in it, newest last.
fn boot(name: &str, files: &[&str]) -> (PathBuf, Root) {
    let mut path = std::env::temp_dir();
    path.push(format!("raba-setup-boot-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(path.join("boot")).expect("a /boot");
    for (order, file) in files.iter().enumerate() {
        let at = path.join("boot").join(file);
        std::fs::write(&at, b"not really an image").expect("an image");
        // Distinct mtimes, increasing, so the ordering assertion is about the
        // sort and not about what the filesystem's clock granularity was.
        let when = std::time::SystemTime::UNIX_EPOCH
            + std::time::Duration::from_secs(1_700_000_000 + order as u64 * 60);
        let _ = std::fs::File::open(&at).map(|file| file.set_modified(when));
    }
    (path.clone(), Root::new(path))
}

fn names(found: &[Image]) -> Vec<String> {
    found
        .iter()
        .map(|image| image.path.file_name().unwrap_or_default().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn both_spellings_of_an_initramfs_are_found_because_distributions_disagree() {
    // Arch writes initramfs-linux.img, Debian writes initrd.img-6.1.0.
    let (dir, root) = boot("spellings", &["initramfs-linux.img", "initrd.img-6.1.0", "vmlinuz"]);
    let found = images(&root).expect("a readable /boot");
    let mut listed = names(&found);
    listed.sort();
    assert_eq!(listed, vec!["initramfs-linux.img", "initrd.img-6.1.0"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_fallback_image_and_a_saved_old_one_are_not_reported_on() {
    // The fallback is built without autodetect and is not the one being
    // booted; .old is the previous build. Reporting on either doubles every
    // line of the dry run with an answer about an image nobody boots.
    let (dir, root) = boot(
        "fallback",
        &["initramfs-linux.img", "initramfs-linux-fallback.img", "initramfs-linux.img.old"],
    );
    let found = images(&root).expect("a readable /boot");
    assert_eq!(names(&found), vec!["initramfs-linux.img"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn images_come_back_newest_first_so_the_dry_run_leads_with_the_one_being_booted() {
    let (dir, root) =
        boot("order", &["initramfs-oldest.img", "initramfs-middle.img", "initramfs-newest.img"]);
    let found = images(&root).expect("a readable /boot");
    assert_eq!(
        names(&found),
        vec!["initramfs-newest.img", "initramfs-middle.img", "initramfs-oldest.img"]
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_boot_directory_that_cannot_be_read_is_an_error_that_names_it() {
    // `/boot` is drwx------ on a great many machines. "None found" to somebody
    // with six images is worse than useless, so this has to be told apart from
    // an empty directory.
    let mut path = std::env::temp_dir();
    path.push(format!("raba-setup-boot-{}-missing", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    let why = images(&Root::new(path)).expect_err("a missing /boot is an error");
    assert!(why.contains("boot"), "{why}");
}
