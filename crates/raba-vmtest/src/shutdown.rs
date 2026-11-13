//! Switching the machine off, and telling the host how it went.
//!
//! Two routes, tried in order, and neither of them needs `unsafe`.
//!
//! **`isa-debug-exit`** is a QEMU device that exits the emulator when written to,
//! with the exit code `(value << 1) | 1`. The usual way to reach an I/O port is
//! inline assembly; `/dev/port` is a file that writes to one, which keeps this
//! whole crate inside the workspace's `unsafe_code = "forbid"`.
//!
//! **SysRq** is the fallback: `o` to `/proc/sysrq-trigger` powers the machine off.
//! It cannot carry a status, so the host falls back to reading the TAP output —
//! which it parses anyway, because a per-check reason is worth more in a log than
//! a single number.

use std::io::{Seek as _, SeekFrom, Write as _};

/// The `iobase` given to `-device isa-debug-exit` in `scripts/vm-test.sh`.
const DEBUG_EXIT_PORT: u64 = 0xf4;

/// Values chosen so neither collides with a status QEMU produces by itself.
/// QEMU exits with `(value << 1) | 1`, so these arrive as 33 and 35.
const PASS: u8 = 0x10;
const FAIL: u8 = 0x11;

/// Report the result and stop the machine. Does not return, if all goes well.
pub fn power_off(failed: bool) {
    let code = if failed { FAIL } else { PASS };
    if let Err(error) = write_debug_exit(code) {
        println!("# isa-debug-exit unavailable ({error}); falling back to sysrq");
        let _ = std::io::stdout().flush();
    }
    let _ = std::fs::write("/proc/sysrq-trigger", b"o");

    // Neither worked. Say so rather than spinning silently: the host's timeout
    // will kill the machine, and this line is what tells somebody why.
    println!("# could not power off; waiting for the host to time out");
    let _ = std::io::stdout().flush();
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

fn write_debug_exit(code: u8) -> std::io::Result<()> {
    let mut port = std::fs::OpenOptions::new().write(true).open("/dev/port")?;
    port.seek(SeekFrom::Start(DEBUG_EXIT_PORT))?;
    port.write_all(&[code])?;
    Ok(())
}
