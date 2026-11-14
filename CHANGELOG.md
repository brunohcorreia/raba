# Changelog

Dates are the day the tag was pushed. Versions follow [semantic
versioning](https://semver.org/), with the reservation that the `.raba` format and the
control protocol have version numbers of their own — see
[docs/format.md](docs/format.md) and [docs/protocol.md](docs/protocol.md).

## Unreleased

### Fixed

- **A console wider than 1024 columns now gets a splash.** The format's canvas
  ceiling was being applied to the screen as well, so `Player::new` returned `None`
  and `play` gave up without drawing — on a 4K panel with a small font, or any
  console past 1024 cells in an axis. A screen is now bounded by its total cells
  (`limits::MAX_SCREEN_CELLS`, the same 1024×1024 worth of memory as before) and may
  be any shape inside that; past it, the axes clamp and the art is drawn in the part
  of the console it can reach. `boot.rs` no longer has a "the console became NxM,
  which is not a size art can be laid out at" path: there is no longer such a size.

## 0.1.0 — 13 November 2026

The first release. A boot splash that plays an ASCII animation from inside the
initramfs, across `switch_root`, and gets out of the way when the display manager takes
the screen.

### What it does

- **`raba`** — the splash. Takes a free virtual terminal with `VT_OPENQRY`, plays a
  `.raba` animation on it, and restores what was there. Never hardcodes tty1.
- **`raba-ctl`** — `deactivate`, `quit`, `quit --wait`, `message`, `ping`, `status`.
  Exits 0 when no instance is running, so a display manager ordered after it cannot
  fail to start because the splash was absent.
- **`raba-setup`** — shows what it would change and changes nothing, until `--apply`.
  It will not edit configuration owned by another package and will not enable services
  on install.
- **`raba-gen`** — builds a `.raba` from a ten-line TOML recipe and a text file, and
  imports Durdraw `.dur` v7 and v8.
- **50 logo themes** plus the generic fallback, art and colours from fastfetch, covering the most-used distributions
  on desktop, server and gaming. Logos are the trademarks of their projects; see
  [themes/README.md](themes/README.md).

### Where it has been booted

Arch (mkinitcpio), Debian (initramfs-tools) and Fedora (dracut), installed from the
packages or from source with each distribution's own toolchain, booted in QEMU, and
checked from inside the guest: that the art was on the console, that nothing of ours
was left running, that the login prompt took the screen back. Plus, on each:

- **five power cuts** — `kill -9` to the virtual machine at 1.0, 1.6, 2.2, 3.0 and
  4.5 seconds, then the same disk booted again, asserting that the full check suite
  passes on the boot after the cut;
- **seven sabotage cases** — the binary deleted, every theme truncated, the
  configuration replaced with nonsense, a terminal that does not exist, a plain file at
  the control socket's path, a directory at the resume file's path — each asserting
  that the machine comes up anyway and that the reason is in the log, once, in words.

### Deliberately not in this release

- **No LUKS passphrase prompt.** On an encrypted root the splash gets out of the way
  and lets `systemd-cryptsetup` ask. Hiding and resuming around the prompt is the next
  substantial piece of work.
- **No init system but systemd is proven.** `integration/init/` ships service files for
  runit, OpenRC, s6 and dinit, written with care and never booted. Void is the first
  one that will be.
- **No Alpine.** The binary builds static against musl — `rustix` reaches the kernel
  through `linux-raw-sys`, with no C library — but there is no `mkinitfs` hook, so
  there is no early splash there.
- **No immutable distributions.** Nothing about `rpm-ostree`, ostree layering or a
  read-only `/usr` is implemented or designed.
- **The screen is handed to the display manager, not to the compositor.**
  `raba-quit.service` is ordered after `display-manager.service`, so the greeter takes
  the terminal and RABA leaves without restoring anything. Holding the animation past
  the greeter to a compositor's first frame is a different problem and is not solved.

### Size

| | |
|---|---|
| `raba`, static against musl | 521 KiB |
| `raba`, dynamic against glibc | 424 KiB |
| `raba-ctl` | 310 KiB |
| `raba-setup` | 437 KiB |
| all 50 logo themes together | 464 KiB, of which exactly one enters an initramfs |

Two direct dependencies outside the project's own crates: `rustix` for the VT ioctls
and `libc` for the signal mask, which between them pull in `bitflags` and
`linux-raw-sys` and nothing else. `rustix` reaches the kernel through `linux-raw-sys`
rather than a C library, which is why the static build needs no libc at all.
[DEPENDENCIES.md](DEPENDENCIES.md) justifies each one against the budget it is spent
from.

### If it goes wrong

`raba.enable=0` on the kernel command line, from your bootloader. It needs no working
RABA and no filesystem of ours. `man 8 raba` and
[docs/troubleshooting.md](docs/troubleshooting.md).
