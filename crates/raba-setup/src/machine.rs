//! What this machine looks like, and what it would take to put a splash on it.
//!
//! Everything that decides anything here is a pure function over text that some
//! other program printed — `mkinitcpio.conf`, `lsblk`, `cryptsetup luksDump`,
//! the contents of an initramfs. The I/O is a thin layer at the bottom that
//! runs those programs and reads those files.
//!
//! That split is not decoration. The interesting cases are a `HOOKS=` line
//! written by hand over several years, a LUKS header with a TPM2 token, an
//! initramfs older than the configuration that was supposed to go into it —
//! none of which anybody can arrange on a test machine, and all of which are
//! two lines of text in a test.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Which program builds the initramfs here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Generator {
    Mkinitcpio,
    Dracut,
    InitramfsTools,
}

impl Generator {
    /// The command that rebuilds every image.
    #[must_use]
    pub const fn rebuild(self) -> &'static [&'static str] {
        match self {
            Self::Mkinitcpio => &["mkinitcpio", "-P"],
            Self::Dracut => &["dracut", "--force"],
            Self::InitramfsTools => &["update-initramfs", "-u"],
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Mkinitcpio => "mkinitcpio",
            Self::Dracut => "dracut",
            Self::InitramfsTools => "initramfs-tools",
        }
    }
}

// ----------------------------------------------------------- mkinitcpio -----

/// The `HOOKS=(...)` array, which may be spread over several lines.
///
/// The last uncommented one wins, the way the shell that sources this file
/// would see it. Anything that is not an array assignment is skipped rather
/// than guessed at: this file belongs to another package and we only read it.
#[must_use]
pub fn parse_hooks(text: &str) -> Option<Vec<String>> {
    let mut found = None;
    let mut collecting: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(open) = collecting.as_mut() {
            open.push(' ');
            open.push_str(line.split('#').next().unwrap_or_default());
        } else if line.starts_with('#') {
            continue;
        } else if let Some(rest) = line.strip_prefix("HOOKS=") {
            collecting = Some(rest.split('#').next().unwrap_or_default().to_owned());
        } else {
            continue;
        }
        let Some(open) = collecting.clone() else {
            continue;
        };
        if let Some(end) = open.find(')') {
            let inside = open.get(..end).unwrap_or_default();
            found = Some(
                inside
                    .trim_start_matches('(')
                    .split_whitespace()
                    .map(|word| word.trim_matches(['"', '\'']).to_owned())
                    .filter(|word| !word.is_empty())
                    .collect(),
            );
            collecting = None;
        }
    }
    found
}

/// What is wrong with a `HOOKS=` line, from RABA's point of view only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Advice {
    /// `raba` is not in `HOOKS` at all. The splash will not be in the image.
    NotInHooks,
    /// `raba` comes before `encrypt`, so the splash is already drawing when
    /// `cryptsetup` prompts for a passphrase on the same console.
    BeforeEncrypt,
    /// Both are there, in the right order.
    Good,
}

/// Is Plymouth going into the same image?
///
/// Not a conflict at build time — both can be in an image, and RABA checks at
/// runtime and declines. It is still the explanation for "I installed RABA and
/// nothing changed", and this is the only place anybody will read it before
/// rebooting. See `docs/coexistence.md`.
#[must_use]
pub fn shares_with_plymouth(hooks: &[String]) -> bool {
    hooks.iter().any(|hook| hook == "plymouth" || hook == "sd-plymouth")
}

/// Where `raba` sits in a `HOOKS=` line, and whether that is where it should.
///
/// The one ordering rule RABA has: **after `encrypt`**. RABA does not supply
/// the passphrase prompt — `cryptsetup` does, on the console — so a splash that
/// is already up when the prompt appears draws over it. Plymouth needs the
/// opposite order because Plymouth *is* the prompt. See `docs/luks.md`.
#[must_use]
pub fn advise(hooks: &[String]) -> Advice {
    let position = |name: &str| hooks.iter().position(|hook| hook == name);
    let Some(raba) = position("raba") else {
        return Advice::NotInHooks;
    };
    match position("encrypt").or_else(|| position("sd-encrypt")) {
        Some(encrypt) if raba < encrypt => Advice::BeforeEncrypt,
        _ => Advice::Good,
    }
}

/// The `HOOKS=` line as it should read, with `raba` put where it belongs.
///
/// Printed, never written: `/etc/mkinitcpio.conf` belongs to another package,
/// and a program that rewrites somebody else's configuration file bypasses the
/// `.pacnew` handling that exists to stop exactly that. See `docs/packaging.md`.
#[must_use]
pub fn suggested(hooks: &[String]) -> String {
    let mut line: Vec<&str> = hooks.iter().map(String::as_str).filter(|h| *h != "raba").collect();
    // After `encrypt` if there is one, otherwise before `filesystems`, which is
    // the hook that mounts the root and therefore the last useful moment.
    let after_encrypt = line.iter().position(|h| *h == "encrypt" || *h == "sd-encrypt");
    let at = after_encrypt
        .map(|index| index.saturating_add(1))
        .or_else(|| line.iter().position(|h| *h == "filesystems"))
        .unwrap_or(line.len());
    line.insert(at.min(line.len()), "raba");
    format!("HOOKS=({})", line.join(" "))
}

// ------------------------------------------------------------------ LUKS ----

/// Encrypted volumes, from `lsblk --raw --noheadings --output PATH,FSTYPE`.
#[must_use]
pub fn parse_luks(listing: &str) -> Vec<String> {
    listing
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let path = fields.next()?;
            let fstype = fields.next()?;
            (fstype == "crypto_LUKS").then(|| path.to_owned())
        })
        .collect()
}

/// Does this `cryptsetup luksDump` show a TPM2 token?
///
/// A token is what binds a key slot to something other than a passphrase, and
/// `systemd-tpm2` is the one that breaks when the initramfs changes. The field
/// is printed as part of the `Tokens:` section:
///
/// ```text
/// Tokens:
///   0: systemd-tpm2
/// ```
#[must_use]
pub fn dump_has_tpm2(dump: &str) -> bool {
    dump.lines().any(|line| {
        let line = line.trim().to_ascii_lowercase();
        line.contains("tpm2")
    })
}

/// What was learned about TPM-sealed keys, including "nothing, and why".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tpm {
    /// Not root, or `cryptsetup` is not installed. **Not** the same as "no".
    Unknown(String),
    /// Looked, found nothing bound to a TPM.
    Clear,
    /// These volumes will stop unlocking themselves when the initramfs changes.
    Sealed(Vec<String>),
}

// ----------------------------------------------------------------- images ---

/// An initramfs on disk, and what it has to say about itself.
#[derive(Debug, Clone)]
pub struct Image {
    pub path: PathBuf,
    /// Seconds since the epoch, or `None` if it could not be read.
    pub modified: Option<u64>,
    /// The frozen configuration found inside it, when something on this machine
    /// could read one out.
    pub carries: Option<String>,
}

/// Whether an image still matches the configuration on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Currency {
    /// The configuration inside the image is the one that would be generated
    /// now. The only answer that is actually an answer.
    Current,
    /// It is not, and here is where they differ.
    Stale(&'static str),
    /// Nothing here could read inside the image, so the only evidence is a
    /// timestamp: the image is older than the configuration or the theme.
    ProbablyStale,
    /// Same, the other way: older files, newer image. Weak evidence of nothing
    /// being wrong.
    ProbablyCurrent,
    /// There is no image, or no theme, or nothing to compare.
    Unknown,
}

/// Compare what an image carries with what it would be given now.
///
/// The content comparison is the real one. The timestamp comparison is what is
/// left when nothing on the machine can read a file out of a compressed cpio,
/// and it is reported as the guess it is — an image rebuilt for an unrelated
/// reason is newer than the configuration and still carries the old copy.
#[must_use]
pub fn currency(image: &Image, frozen_now: &str, sources: &[Option<u64>]) -> Currency {
    if let Some(carried) = image.carries.as_deref() {
        return if carried.trim() == frozen_now.trim() {
            Currency::Current
        } else {
            Currency::Stale("the configuration inside the image is not the current one")
        };
    }
    let Some(built) = image.modified else {
        return Currency::Unknown;
    };
    let newest = sources.iter().flatten().copied().max();
    match newest {
        None => Currency::Unknown,
        Some(changed) if changed > built => Currency::ProbablyStale,
        Some(_) => Currency::ProbablyCurrent,
    }
}

// -------------------------------------------------------------------- I/O ---

/// Everything the dry run found out, in one pass.
#[derive(Debug)]
pub struct Survey {
    pub generators: Vec<Generator>,
    pub hooks: Option<Vec<String>>,
    pub images: Vec<Image>,
    /// Why there are no images, when the reason is not "there are none".
    /// `/boot` is `drwx------` on a great many machines, and a dry run that
    /// says "none found" to a user who has six of them is worse than useless.
    pub unreadable: Option<String>,
    pub tpm: Tpm,
}

/// Where to look. A prefix, so that a test can be a directory.
#[derive(Debug, Clone)]
pub struct Root(PathBuf);

impl Default for Root {
    fn default() -> Self {
        Root(PathBuf::from("/"))
    }
}

impl Root {
    #[must_use]
    pub fn new(path: PathBuf) -> Root {
        Root(path)
    }

    /// `/etc/mkinitcpio.conf` under this root.
    #[must_use]
    pub fn path(&self, absolute: &str) -> PathBuf {
        self.0.join(absolute.trim_start_matches('/'))
    }

    /// Is this a real machine, or a directory a test made?
    #[must_use]
    pub fn is_the_running_system(&self) -> bool {
        self.0 == Path::new("/")
    }
}

/// Look at the machine: which generator, which hooks, which images, which TPM.
#[must_use]
pub fn survey(root: &Root) -> Survey {
    let mut generators = Vec::new();
    // The config file rather than the binary: a machine can have dracut
    // installed as a dependency of something and still be built by mkinitcpio.
    if root.path("/etc/mkinitcpio.conf").is_file() {
        generators.push(Generator::Mkinitcpio);
    }
    if root.path("/etc/dracut.conf").is_file() || root.path("/etc/dracut.conf.d").is_dir() {
        generators.push(Generator::Dracut);
    }
    if root.path("/etc/initramfs-tools").is_dir() {
        generators.push(Generator::InitramfsTools);
    }

    let hooks = std::fs::read_to_string(root.path("/etc/mkinitcpio.conf"))
        .ok()
        .and_then(|text| parse_hooks(&text));

    let (images, unreadable) = match images(root) {
        Ok(found) => (found, None),
        Err(why) => (Vec::new(), Some(why)),
    };
    Survey { generators, hooks, images, unreadable, tpm: tpm(root) }
}

/// Initramfs images in `/boot`, newest first.
fn images(root: &Root) -> Result<Vec<Image>, String> {
    let boot = root.path("/boot");
    let entries = std::fs::read_dir(&boot).map_err(|why| format!("{}: {why}", boot.display()))?;
    let mut found: Vec<Image> = entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            (name.starts_with("initramfs") || name.starts_with("initrd"))
                && !name.ends_with(".old")
                // A fallback image is built without autodetect and is not the
                // one being booted; reporting on it would double every line.
                && !name.contains("fallback")
        })
        .map(|entry| {
            let path = entry.path();
            let modified = seconds(&path);
            let carries = read_from_image(&path);
            Image { path, modified, carries }
        })
        .collect();
    found.sort_by_key(|image| std::cmp::Reverse(image.modified));
    Ok(found)
}

/// Seconds since the epoch, for comparing two files.
#[must_use]
pub fn seconds(path: &Path) -> Option<u64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    modified.duration_since(std::time::UNIX_EPOCH).ok().map(|since| since.as_secs())
}

/// Read the frozen configuration out of an image, if anything here can.
///
/// `lsinitrd` is dracut's and handles a concatenated microcode image; `bsdtar`
/// is everywhere pacman is. Neither is a dependency: when both are missing the
/// caller falls back to comparing timestamps and says that it did.
fn read_from_image(image: &Path) -> Option<String> {
    let attempts: [(&str, Vec<String>); 2] = [
        ("lsinitrd", vec!["-f".into(), "etc/raba/raba.conf".into(), display(image)]),
        ("bsdtar", vec!["-xOf".into(), display(image), "etc/raba/raba.conf".into()]),
    ];
    for (program, arguments) in attempts {
        let Ok(output) = Command::new(program).args(arguments).output() else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        if text.contains("[Boot]") {
            return Some(text);
        }
    }
    None
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

/// Is any encrypted volume on this machine bound to the TPM?
///
/// The check that has to happen **before** the initramfs is touched. Adding
/// RABA to an image changes PCR 9, and PCR 11 for a UKI, and a key sealed
/// against either stops being released — the machine boots and asks for a
/// recovery passphrase instead. That is a bad surprise and one `lsblk` away
/// from being a warning. See `docs/secure-boot.md`.
fn tpm(root: &Root) -> Tpm {
    if !root.is_the_running_system() {
        return Tpm::Unknown("not looking at the running system".to_owned());
    }
    let Ok(listing) =
        Command::new("lsblk").args(["--raw", "--noheadings", "--output", "PATH,FSTYPE"]).output()
    else {
        return Tpm::Unknown("lsblk is not installed".to_owned());
    };
    let devices = parse_luks(&String::from_utf8_lossy(&listing.stdout));
    if devices.is_empty() {
        return Tpm::Clear;
    }
    let mut sealed = Vec::new();
    for device in devices {
        let Ok(dump) = Command::new("cryptsetup").arg("luksDump").arg(&device).output() else {
            return Tpm::Unknown("cryptsetup is not installed".to_owned());
        };
        if !dump.status.success() {
            // The usual reason is not being root, and "I could not look" must
            // never be printed as "there is nothing there".
            return Tpm::Unknown(format!("cannot read the LUKS header on {device}; run as root"));
        }
        if dump_has_tpm2(&String::from_utf8_lossy(&dump.stdout)) {
            sealed.push(device);
        }
    }
    if sealed.is_empty() {
        Tpm::Clear
    } else {
        Tpm::Sealed(sealed)
    }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(
            advise(&hooks(&["base", "raba", "encrypt", "filesystems"])),
            Advice::BeforeEncrypt
        );
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
        assert_eq!(
            currency(&image(Some(frozen), Some(100)), frozen, &[Some(500)]),
            Currency::Current
        );
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
}
