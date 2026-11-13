//! Bringing the guest's coverage profile back out, over the serial line.
//!
//! # Why this exists
//!
//! `crates/raba/src/vt` is the part of RABA that can only run on a real machine:
//! `VT_OPENQRY`, `VT_ACTIVATE`, `KDGETMODE`, a console that is not a pipe. It is
//! also the part where a mistake does not look like a bug report — it looks like
//! a computer that will not finish booting. The virtual-machine tier has always
//! exercised it; what it could not do was *say which lines*, so the report from
//! `scripts/coverage.sh` read 0% for the whole module and the honest answer to
//! "is this tested?" was "yes, somewhere in there".
//!
//! # How
//!
//! With `--coverage`, `scripts/vm-test.sh` builds the `raba` binary with
//! `-C instrument-coverage`. Every instance the checks start writes its counters
//! on the way out, and because the default profile name uses `%m` — the binary's
//! own signature, not its pid — LLVM pools all of them into one file under a
//! lock rather than racing. An instance that is `SIGKILL`ed writes nothing,
//! which is correct: nothing ran that the survivors did not also run.
//!
//! Getting the file to the host is the awkward part. This guest has no disk, no
//! network and no shared filesystem — that is what makes it boot in a second —
//! so the only channel wide enough is the serial console the results already
//! leave by. The file goes out base64-encoded between two markers, and the host
//! cuts it back out of the log.
//!
//! A megabyte of base64 through an emulated 16550 is a few seconds. That is why
//! this is behind a flag and not part of every run.

use std::io::Write as _;

/// Where LLVM leaves profiles written by processes whose working directory is
/// `/`, which is every process in this guest. `%m` in the default name means
/// one pooled file per binary rather than one per process.
const WHERE: &str = "/";

/// The host looks for exactly these. Changing one means changing
/// `scripts/vm-test.sh` with it.
const BEGIN: &str = "# raba-profile-begin";
const END: &str = "# raba-profile-end";

/// Write every profile this boot produced to stdout, base64-encoded.
///
/// Silent when there are none, which is the ordinary run: a guest built without
/// instrumentation leaves no `.profraw` behind and this costs one failed
/// `read_dir` filter.
pub fn emit() {
    let Ok(entries) = std::fs::read_dir(WHERE) else {
        return;
    };
    let mut found: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "profraw"))
        .collect();
    // Sorted so that a diff between two runs of the log is about the coverage
    // and not about the order `read_dir` happened to return.
    found.sort();

    for path in found {
        let Ok(bytes) = std::fs::read(&path) else {
            println!("# could not read {}", path.display());
            continue;
        };
        let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        println!("{BEGIN} {name} {}", bytes.len());
        // In lines, because a single multi-megabyte line is a line that
        // something between here and the host will eventually decide to wrap.
        for chunk in bytes.chunks(48) {
            println!("{}", base64(chunk));
        }
        println!("{END}");
        let _ = std::io::stdout().flush();
    }
}

/// Standard base64, padded. Here rather than from a crate because this guest's
/// dependency list is three entries long and reviewing twenty lines is cheaper
/// than reviewing a supply chain.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3).saturating_mul(4));
    for group in bytes.chunks(3) {
        let (a, b, c) = (
            u32::from(*group.first().unwrap_or(&0)),
            u32::from(*group.get(1).unwrap_or(&0)),
            u32::from(*group.get(2).unwrap_or(&0)),
        );
        let packed = (a << 16) | (b << 8) | c;
        // `& 0x3f` cannot exceed 63 and the alphabet has 64 entries, so the
        // fallback is unreachable; `get` rather than `[]` because an index that
        // is obviously in range today is a panic nobody re-checks tomorrow.
        let symbol = |shift: u32| {
            ALPHABET.get(((packed >> shift) & 0x3f) as usize).map_or('=', |b| char::from(*b))
        };
        out.push(symbol(18));
        out.push(symbol(12));
        out.push(if group.len() > 1 { symbol(6) } else { '=' });
        out.push(if group.len() > 2 { symbol(0) } else { '=' });
    }
    out
}
