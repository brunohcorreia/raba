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

use std::fmt;
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

/// A write that did not happen, and **which** of the two paths refused.
///
/// The distinction is the whole reason this is not a bare [`io::Error`]. Most
/// of what goes wrong here goes wrong on the staging path — a directory planted
/// at `raba.conf.new`, a file that cannot be removed — and a caller that
/// prepends the destination's name sends somebody to look at a file that is
/// perfectly fine.
#[derive(Debug)]
pub struct Failed {
    /// The path the error is actually about.
    pub path: PathBuf,
    pub error: io::Error,
}

impl fmt::Display for Failed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.error)
    }
}

impl std::error::Error for Failed {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// Put `bytes` at `path`, atomically, without following a symlink to do it.
///
/// Ordered, not durable: the bytes are in the page cache and the rename cannot
/// expose a half-written file, but a power cut immediately afterwards may leave
/// the old contents. That is the right trade for `/run`, which is a tmpfs a
/// power cut destroys anyway, and it is what the resume state uses — it is
/// written on every frame, and an `fsync` eight times a second to a filesystem
/// that cannot outlive the boot is a syscall spent on nothing.
///
/// Use [`replace_durably`] for anything on real storage.
///
/// # Errors
///
/// [`Failed`], naming whichever of the two paths refused.
pub fn replace(path: &Path, bytes: &[u8]) -> Result<(), Failed> {
    write(path, bytes, false)
}

/// The same, and on real storage.
///
/// `fsync` before the rename, so that a power cut leaves either the old file or
/// the new one. Without it the usual outcome on a journalling filesystem is a
/// zero-length file, which for `raba.conf` is a machine whose next initramfs
/// build has no theme — the exact failure `--set-theme` exists to avoid.
///
/// # Errors
///
/// [`Failed`], naming whichever of the two paths refused.
pub fn replace_durably(path: &Path, bytes: &[u8]) -> Result<(), Failed> {
    write(path, bytes, true)
}

fn write(path: &Path, bytes: &[u8], durable: bool) -> Result<(), Failed> {
    let staging = staging(path);
    let blame = |at: &Path, error: io::Error| Failed { path: at.to_path_buf(), error };

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
            std::fs::remove_file(&staging).map_err(|why| blame(&staging, why))?;
            open().map_err(|why| blame(&staging, why))?
        }
        Err(error) => return Err(blame(&staging, error)),
    };

    let written = file.write_all(bytes).and_then(|()| {
        // `symlink_metadata`, not `metadata`, inside the one function whose job
        // is not to be fooled by a symlink at the destination: `metadata`
        // follows it, and the mode on the far end — `st_mode`, setuid bits and
        // all — would then be stamped onto a root-owned file about to be
        // renamed into place. A symlink has no mode worth preserving, so a
        // destination that is one gets the umask's answer like any new file.
        if let Ok(meta) = std::fs::symlink_metadata(path) {
            if meta.file_type().is_file() {
                file.set_permissions(meta.permissions())?;
            }
        }
        if durable {
            file.sync_all()?;
        }
        Ok(())
    });
    drop(file);
    if let Err(error) = written {
        let _ = std::fs::remove_file(&staging);
        return Err(blame(&staging, error));
    }

    // The rename can fail on its own — something that is not our file sitting at
    // the destination, a directory at the path — and when it does, the staging
    // file is ours and has to go. Otherwise a path that cannot be written leaves
    // `raba.state.new` in /run for the rest of the boot, which is a splash that
    // failed quietly *and* left litter behind it.
    if let Err(error) = std::fs::rename(&staging, path) {
        let _ = std::fs::remove_file(&staging);
        return Err(blame(path, error));
    }
    Ok(())
}
