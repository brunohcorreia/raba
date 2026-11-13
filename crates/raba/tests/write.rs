//! The one function in the program that puts bytes in a file.
//!
//! `docs/security.md` finding 4 is the reason it exists: a root process that
//! writes through a symlink somebody else planted is a root process writing
//! wherever that person chose. Those two cases are proved in `tests/state.rs`,
//! through the writer's busiest caller. What is here is the rest of the
//! contract, which coverage found nothing was exercising at all — including
//! `replace_durably`, the variant that writes `/etc/raba/raba.conf`.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let mut path = std::env::temp_dir();
        path.push(format!("raba-write-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch");
        Scratch(path)
    }

    fn file(&self) -> PathBuf {
        self.0.join("thing.conf")
    }

    fn staging(&self) -> PathBuf {
        self.0.join("thing.conf.new")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).expect("reading")
}

#[test]
fn the_staging_name_sits_beside_the_file_and_not_in_a_shared_directory() {
    // Beside it, because a rename across filesystems is not atomic and /tmp is
    // frequently a different filesystem from the thing being replaced.
    assert_eq!(
        raba::write::staging(Path::new("/etc/raba/raba.conf")),
        PathBuf::from("/etc/raba/raba.conf.new")
    );
}

#[test]
fn replace_writes_the_bytes_and_leaves_no_staging_file() {
    let scratch = Scratch::new("plain");
    raba::write::replace(&scratch.file(), b"one\n").expect("writing");
    assert_eq!(read(&scratch.file()), "one\n");
    assert!(!scratch.staging().exists(), "the staging file outlived the rename");
}

#[test]
fn replacing_an_existing_file_replaces_all_of_it() {
    let scratch = Scratch::new("shorter");
    std::fs::write(scratch.file(), "a much longer first version\n").expect("the first");
    raba::write::replace(&scratch.file(), b"short\n").expect("writing");
    // Through a rename rather than a truncate, so there is no window in which
    // the file is the first few bytes of the new contents.
    assert_eq!(read(&scratch.file()), "short\n");
}

/// `replace_durably` is what writes `/etc/raba/raba.conf`, and until this test
/// existed nothing had ever run it. The `fsync` itself cannot be observed from
/// here — proving it would mean cutting power to the machine, which is what
/// `scripts/resilience.sh` is for — so what is asserted is that the durable
/// path produces exactly the same file as the ordered one.
#[test]
fn the_durable_path_writes_the_same_file_as_the_ordered_one() {
    let scratch = Scratch::new("durable");
    raba::write::replace_durably(&scratch.file(), b"[Boot]\nTheme=arch\n").expect("writing");
    assert_eq!(read(&scratch.file()), "[Boot]\nTheme=arch\n");
    assert!(!scratch.staging().exists());
}

#[test]
fn the_durable_path_replaces_an_existing_file_too() {
    let scratch = Scratch::new("durable-over");
    std::fs::write(scratch.file(), "[Boot]\nTheme=generic\n").expect("the first");
    raba::write::replace_durably(&scratch.file(), b"[Boot]\nTheme=arch\n").expect("writing");
    assert_eq!(read(&scratch.file()), "[Boot]\nTheme=arch\n");
}

/// The error has to name the path that actually refused. Most of what goes
/// wrong here goes wrong on the staging path, and a caller that prepends the
/// destination sends somebody to look at a file that is perfectly fine.
#[test]
fn a_failure_on_the_staging_path_is_reported_against_the_staging_path() {
    let scratch = Scratch::new("staging-blocked");
    // A directory where the staging file goes. `O_CREAT|O_EXCL` says it is in
    // the way, and removing it fails because it is not a file.
    std::fs::create_dir(scratch.staging()).expect("a directory in the way");
    let failed = raba::write::replace(&scratch.file(), b"x").expect_err("should refuse");
    assert_eq!(failed.path, scratch.staging(), "blamed the wrong path: {failed}");
    assert!(failed.to_string().contains("thing.conf.new"), "{failed}");
    assert!(!scratch.file().exists(), "the destination was created anyway");
}

#[test]
fn a_failure_at_the_destination_is_reported_against_the_destination() {
    let scratch = Scratch::new("destination-blocked");
    // A directory at the destination: the staging file is written fine and the
    // rename is what refuses.
    std::fs::create_dir(scratch.file()).expect("a directory in the way");
    let failed = raba::write::replace(&scratch.file(), b"x").expect_err("should refuse");
    assert_eq!(failed.path, scratch.file(), "blamed the wrong path: {failed}");
    assert!(!scratch.staging().exists(), "a failed rename left the staging file behind");
}

/// Not every refusal is `AlreadyExists`. A missing parent directory fails at
/// the open with `ENOENT`, which is the arm that had never run.
#[test]
fn a_path_whose_directory_does_not_exist_is_an_error_and_not_a_panic() {
    let scratch = Scratch::new("no-directory");
    let path = scratch.0.join("nowhere").join("thing.conf");
    let failed = raba::write::replace(&path, b"x").expect_err("should refuse");
    assert_eq!(failed.path, path.with_file_name("thing.conf.new"));
    assert_eq!(failed.error.kind(), std::io::ErrorKind::NotFound, "{failed}");
}

/// A staging file left by a run that died between creating and renaming is
/// ours, and taking it away is safe for the same reason creating it was.
#[test]
fn a_stale_staging_file_is_removed_rather_than_refused() {
    let scratch = Scratch::new("stale-staging");
    std::fs::write(scratch.staging(), "from a run that died").expect("the leftover");
    raba::write::replace(&scratch.file(), b"new\n").expect("writing");
    assert_eq!(read(&scratch.file()), "new\n");
    assert!(!scratch.staging().exists());
}

#[test]
fn the_destinations_permissions_are_kept_so_a_replacement_cannot_widen_it() {
    use std::os::unix::fs::PermissionsExt as _;
    let scratch = Scratch::new("mode");
    std::fs::write(scratch.file(), "first").expect("the first");
    std::fs::set_permissions(scratch.file(), std::fs::Permissions::from_mode(0o600))
        .expect("chmod");
    raba::write::replace(&scratch.file(), b"second").expect("writing");
    let mode = std::fs::symlink_metadata(scratch.file()).expect("stat").permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "came out as {mode:o}");
}

#[test]
fn the_error_keeps_the_cause_so_a_caller_can_match_on_it() {
    // The whole point of implementing `std::error::Error`: a caller that turns
    // this into a `Box<dyn Error>` must still be able to reach the io::Error
    // and decide whether it was ENOENT or EACCES.
    use std::error::Error as _;
    let scratch = Scratch::new("source");
    let path = scratch.0.join("nowhere").join("thing.conf");
    let failed = raba::write::replace(&path, b"x").expect_err("should refuse");
    let source = failed.source().expect("an error with no cause is not worth boxing");
    assert!(
        source.to_string().contains("No such file") || source.to_string().contains("os error 2"),
        "{source}"
    );
}
