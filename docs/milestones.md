# Milestones

Ordered by risk, not by dependency: the cheapest-to-test and most existential questions
first, and the dangerous code only once there is a harness to catch it. See
[development.md](development.md) for the reasoning.

---

## M0 — See a logo animate

**Answers: does ASCII art at console cell size look good enough to justify this project?**

Everything else rests on that, and nobody has tested it. No `.dur`, no VT, no initramfs,
no root, no VM — plain text in, animation on a screen out.

| # | issue | |
|---|---|---|
| 1 | `raba-core`: `.raba` types — `Movie`, `Frame`, `Op`, `Palette` | done |
| 2 | `raba-core`: encoder, `Movie` → bytes | done |
| 3 | `raba-core`: decoder with bounded validation, bytes → `Movie` | done |
| 4 | **The failure table** — every malformed input, exit 0, no panic | done |
| 5 | `raba-core`: `Surface` trait and `BufferSurface` for tests | |
| 6 | `raba-core`: grid → ANSI encoder with redraw minimisation |
| 7 | `raba-core`: glyph validator against the console-safe set |
| 8 | `raba-gen`: load fastfetch-style text art with `$1`..`$9` colour markers |
| 9 | `raba-gen`: compose logo + spinner, least-common-multiple frame count |
| 10 | `raba`: `--preview`, rendering to stdout |
| 11 | First theme, and the verdict on the premise |

**Done when** `raba --preview themes/cachyos.raba` animates a logo in a terminal, and we
have looked at it and decided it is worth continuing.

### Progress

The format is implemented and frozen at version 1. `docs/format.md` describes what was
built rather than what was sketched.

- 41 tests pass; `clippy` is clean under `-D warnings` with the workspace deny lints
- the failure table covers 24 shapes of malformed input, each asserting a specific error
- property tests cover round-trip, determinism, truncation and "arbitrary bytes never
  panic"
- a `cargo-fuzz` target asserts internal consistency and round-trip, not merely absence
  of panics
- `raba` release binary: 281 KB against a 512 KB gate

---

## M1 — Theme authoring

Makes themes contributable by someone who does not write Rust.

- `raba-gen build` from a TOML recipe, full `raba-theme(5)` schema
- 256 → 16 colour downsampling; clamp backgrounds to 0–7
- Spinner styles: `ascii`, `dots`, `bar`, `custom`
- `--list-logos`, `--check`, `--preview`
- Vendor fastfetch logo art at a pinned commit
- Golden render tests
- Themes for the first ten distributions

---

## M2 — Import

- `.dur` v7 and v8 → `.raba`, handling the colour-map transposition
- **The critical test:** identical artwork as v7 and v8 must produce byte-identical `.raba`
- ANSI art and plain text importers
- `--also-dur` for display managers that play their own animation
- Fuzz target on the `.dur` parser

---

## M3 — The dangerous part

The VT layer, where a mistake hangs a boot. **The VM harness is built before a line of
this is written.**

- Tier 3a harness: QEMU kernel + initramfs, ~1 second per run, `isa-debug-exit`
- `VT_OPENQRY` to acquire a free terminal — never hardcode tty1
- Draw, and restore on **every** exit path including panic
- `TIOCL_SETKMSGREDIRECT` to move kernel messages aside; restore exactly
- Detect a terminal already in `KD_GRAPHICS` and yield
- `SIGKILL` mid-frame leaves a usable terminal — the test that matters most
- Pty tests for the escape encoder

---

## M4 — Lifecycle

- Control socket: `0600`, root-owned, `SO_PEERCRED` check rejecting non-root
- `raba-ctl`: `quit`, `deactivate`, `reactivate`, `message`, `ping`, `status`
- Message sanitisation — no caller bytes reach a terminal
- `/run/raba.state`, resumable: being killed stops being an event
- Phases: boot loop → outro → handoff, with `MaxDuration`
- Timeout **reveals the console** rather than exiting
- Conflict detection in the binary, not in service files

---

## M5 — Early boot

- mkinitcpio hook, busybox and systemd initramfs, one hook name
- dracut module — **v1, not later**: Void, Gentoo and Chimera are non-systemd and use dracut
- systemd units: start, quit, quit-wait
- `raba-setup` with `--dry-run` default, `--apply`, `--check`
- **TPM2 token detection with a warning before touching the initramfs**
- Hook never fails an image build
- Config frozen into the initramfs; `raba-set-theme` rebuilds

---

## M6 — Breadth

- runit, OpenRC, s6, dinit service scripts
- initramfs-tools
- LUKS hide-and-resume: inotify on `/run/systemd/ask-password/`, `cryptsetup` detection
- Full-distribution VM matrix, nightly
- Man pages built with scdoc; packaging
- Progress overlay, `pulse` by default

---

## Not scheduled

Deliberately unplanned until something justifies them: DRM backend, Esc-to-show-details,
bootloader first-frame image, `.raba` v2 with wider colour, Level 2 LUKS (collecting the
passphrase — see [luks.md](luks.md) for why this is probably never).
