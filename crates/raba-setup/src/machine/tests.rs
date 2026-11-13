//! Unit tests for [`super`].
//!
//! Its own file rather than a `mod tests` block inside the module it tests, so
//! that `scripts/coverage.sh` can leave it out: test code counted as covered
//! product code makes the only number anybody reads say the wrong thing. See
//! `docs/maintenance.md`.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

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
