//! The only way this program puts bytes in a file.
//!
//! One function, used by every writer in the project, because the rule it keeps
//! is easier to audit as a property of the program than as a habit of each
//! caller. `docs/security.md` finding 4:
//!
//! > If `/run/raba.log` is pre-created as a symlink, RABA writes through it as
//! > root. **Mitigation:** `O_NOFOLLOW` everywhere, `O_CREAT|O_EXCL` where the
//! > file should be new, and never follow a path we did not create.
//!
//! That mitigation was written down, applied to `/run/raba.state`, and never
//! applied to the file the finding actually names: both log writes in `main.rs`
//! were a plain `std::fs::write`, which follows a symlink and truncates
//! whatever is on the other end. `raba` runs as root in an initramfs, so the
//! other end is anything on the filesystem.
//!
//! Every write goes through a staging file, so a crash halfway through cannot
//! leave half a configuration or half a resume at a path something else is
//! about to read.

use std::io::{self, Write as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

/// `O_NOFOLLOW` on Linux. Spelled out rather than taken from `libc`, which this
/// crate depends on only for the signal mask and should not be reached for to
/// obtain one integer.
const O_NOFOLLOW: i32 = 0o400_000;

/// `<name>.new`, beside the file it will become.
///
/// Beside, and not in a temporary directory, because a rename is only atomic
/// within one filesystem and `/run` and `/tmp` are not required to be the same
/// one.
#[must_use]
pub fn staging(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".new");
    path.with_file_name(name)
}

/// Put `bytes` at `path`, atomically, without following a symlink to do it.
///
/// The destination's permissions are preserved when it already exists, so that
/// replacing a file cannot silently widen it. A file that was not there gets
/// whatever the umask says.
///
/// # Errors
///
/// The directory is not writable, the staging path exists as something that
/// cannot be removed, or the rename fails — a directory at the destination is
/// the interesting case, and it is the one the sabotage matrix exercises.
/// Nothing of ours is left behind on any of those paths.
pub fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let staging = staging(path);

    // `O_EXCL` so that a pre-planted file is never written through, and
    // `O_NOFOLLOW` so that a pre-planted *symlink* is never written through
    // either — the second is the one that matters, because it is what turns
    // this into a root process writing to a path somebody else chose.
    let open = || {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(O_NOFOLLOW)
            .open(&staging)
    };
    let mut file = match open() {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            // Ours, from a run that died between creating and renaming.
            // Removing it is safe for the same reason creating it was.
            std::fs::remove_file(&staging)?;
            open()?
        }
        Err(error) => return Err(error),
    };

    let written = file.write_all(bytes).and_then(|()| file.flush()).and_then(|()| {
        if let Ok(meta) = std::fs::metadata(path) {
            file.set_permissions(meta.permissions())?;
        }
        Ok(())
    });
    drop(file);
    if let Err(error) = written {
        let _ = std::fs::remove_file(&staging);
        return Err(error);
    }

    // The rename can fail on its own — something that is not our file sitting at
    // the destination, a directory at the path — and when it does, the staging
    // file is ours and has to go. Otherwise a path that cannot be written leaves
    // `raba.state.new` in /run for the rest of the boot, which is a splash that
    // failed quietly *and* left litter behind it.
    if let Err(error) = std::fs::rename(&staging, path) {
        let _ = std::fs::remove_file(&staging);
        return Err(error);
    }
    Ok(())
}
