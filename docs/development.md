# Development plan

## Can we start?

Yes, and we should. The design docs have outrun what has been validated, which is its own
risk — every decision recorded so far rests on an assumption nobody has tested:

> **Does ASCII art at console cell size actually look good enough to justify this?**

A console font is typically 8x16 pixels. A fastfetch logo is roughly 40x20 cells. Nobody
in this project has yet looked at one on a screen. If the answer is "it looks bad", no
amount of correct `switch_root` handling fixes that.

That question should be answered this week, not in month six. It shapes the first
milestone.

Nothing else is blocking. The byte layout of `.raba` is a sketch, but freezing it *is*
the first task — a format gets specified by being implemented, not by being described.

---

## Milestone 0 — see a logo animate

**Goal: a distro logo with a spinner, animating in my terminal.** Nothing else.

```
fastfetch logo (plain text)  →  compose  →  .raba  →  preview in terminal
```

No `.dur`, no VT, no initramfs, no root, no VM. Plain text in, pixels on a screen out.

Roughly 500 lines. It freezes the format, produces the first real artifact, builds the
`Surface` abstraction that everything later plugs into, and — the point — answers whether
the premise holds.

If the art looks bad at this size, we find out for the price of a weekend, and the
remaining design docs become a record of a decision not to proceed. That is a good
outcome for a weekend.

---

## Why the parser first

Five reasons, in order of weight.

**1. The format is what everything else indexes into.** Changing it later touches the
renderer, the generator, every fixture, every golden test and the on-disk artifacts users
already have. It is the one decision with no cheap reversal, so it should be made first,
by implementation.

**2. It is pure.** No I/O, no root, no terminal, no VM. It can be tested completely on the
host in milliseconds, which means it can be tested *properly* rather than aspirationally.

**3. It is where the security risk concentrates.** We parse a file as root, early in boot,
unsupervised. The parser is the highest-value fuzz target in the project. Writing it first
means fuzzing it from the beginning rather than retrofitting.

**4. It unblocks everything cheap.** `--preview` depends on it; art iteration depends on
`--preview`; theme contributions depend on art iteration. The parser is the bottom of that
stack.

**5. It depends on no decision we have not made.** The VT layer still has open questions
— which terminal, how to detect conflicts, exactly how `VT_PROCESS` behaves under each
init. The parser has none.

### What the parser is not first for

It is not the risky part. The genuinely dangerous code is the VT layer, where a mistake
hangs a boot. That comes second, and it comes with a VM harness built before a line of it
is written.

---

## Implementation order, by risk

Ordered so that the cheapest-to-test and most existential questions come first, and the
dangerous code arrives only once there is a harness to catch it.

| | milestone | validates | needs a VM |
|---|---|---|---|
| 0 | text → `.raba` → preview | **the premise** | no |
| 1 | `raba-gen build`, spinner, glyph validation | theme authoring works | no |
| 2 | `raba-gen convert` from `.dur` | import path, v7/v8 handling | no |
| 3 | VT acquire / draw / restore | **the dangerous part** | yes |
| 4 | control socket, `raba-ctl`, state and resume | handoff, kill-survival | yes |
| 5 | mkinitcpio hook, dracut module | early boot | yes |
| 6 | init integrations, LUKS hide-and-resume | breadth | yes |

Milestones 0–2 need no privileges and no virtual machine. That is deliberate: most of the
program should be buildable and testable on a laptop with `cargo test`.

---

## Test environment

Three tiers, in order of how often they run.

### Tier 1 — host, pure

`cargo test`. Milliseconds. This should be where 90% of the suite lives, which is the
whole reason for pushing logic into pure functions ([maintenance.md](maintenance.md)).

### Tier 2 — host, with a pseudo-terminal

The escape-sequence encoder can be tested against a real pty without touching a virtual
terminal: write to the pty, read back what a terminal emulator made of it, assert on the
resulting cell grid.

This covers the part people assume needs a VM but does not — cursor positioning, colour
sequences, redraw minimisation, and the control-character sanitisation from
[security.md](security.md).

What it cannot cover: `VT_OPENQRY`, `KD_SETMODE`, VT switching. Those are real-kernel
only.

`crates/raba/tests/pty.rs` drives the **real binary** with a pty of a chosen size as its
stdout, then replays what came back through the same strict terminal model the renderer's
own tests use. So it covers `TIOCGWINSZ` against a real terminal, the whole play loop, and
the program's entire output alphabet rather than the parts a test thought to look at.

It earned its place immediately by finding two things the model could not:

- **`ONLCR`.** A pty is not a pipe: the line discipline turns an outbound `\n` into
  `\r\n`. The one newline the program writes comes back with a carriage return attached.
  That is now a test, because it is the mechanism behind the rule that the renderer emits
  no line feeds at all — one mid-frame would silently move the cursor to column 0.
- **A cursor park one line past the bottom, on every frame.** `best_fit` always returns a
  grid exactly the size of the screen, so "the line after the art" stopped existing and
  nothing noticed, because terminals clamp an out-of-range `CUP`. A splash resting on a
  clamp on consoles it has never run on is not a thing to leave alone.

### Tier 3a — QEMU, kernel plus initramfs, no disk

The important trick, and the one that makes VT testing practical:

```sh
qemu-system-x86_64 \
  -kernel /boot/vmlinuz-linux \
  -initrd test-initramfs.img \
  -append "console=ttyS0 panic=-1" \
  -display none -serial stdio -no-reboot \
  -device isa-debug-exit,iobase=0xf4,iosize=0x04
```

The test binary is `/init`. It boots in about a second, runs, writes results to the serial
line, and exits QEMU with a status code through `isa-debug-exit`.

**This is nearly as fast as a unit test and gives a real kernel, a real VT and real
ioctls.** It is how the VT layer gets tested on every commit rather than occasionally.
Most of what we feared needed a slow full-distro VM does not.

```sh
scripts/vm-test.sh                 # run the checks
scripts/vm-test.sh --self-test     # assert that a failing run comes back failed
scripts/vm-test.sh --console       # watch the guest's VGA console too
```

The guest is `crates/raba-vmtest`, and the whole image is two static binaries and a
generated `.raba` — about 470 KB. The kernel is the one already installed: the copy
under `/usr/lib/modules/$(uname -r)/vmlinuz` is tried before `/boot`, because `/boot`
is often an ESP only root can read while that copy is the same image and world
readable. `RABA_VM_KERNEL` overrides it.

A run takes **0.7 seconds** on a machine with KVM.

### Three things that make it work, each found by it not working

**`console=tty0 console=ttyS0`, in that order.** `console=tty0` is what makes the kernel
register the VGA text console at all — with only `console=ttyS0`, `/proc/consoles` lists
the serial line and nothing else, and the guest has terminals that nothing draws on.
`console=ttyS0` comes *last* because the last console named becomes `/dev/console`, which
is where PID 1's output goes: results leave by the serial line while `tty0` stays free to
be drawn on. Without that split the tests would quietly be exercising a serial console
and proving nothing about a VT.

**`-cpu max`, or `-cpu host` with KVM.** Not a performance choice. QEMU's default
`qemu64` is roughly a 2003 processor, and a distribution that builds for x86-64-v3 or v4
— CachyOS, and more over time — produces a kernel that will not run on it. The failure
has no error message whatsoever: the kernel faults before it can print, `panic=-1`
reboots, `-no-reboot` makes QEMU exit **0**, and the whole run looks like a silent
success. The script says so explicitly when it sees no output at all, because the first
time this happens it is not obvious.

**Checking which driver is bound to the terminal.** `CONFIG_VT` gives a kernel 64
terminals whether or not anything can display them. When no real console driver binds,
`dummycon` takes over: every VT ioctl succeeds, every test passes, and the splash is
invisible on real hardware. Booting with `-kernel` is exactly the case where this
happens, because QEMU's loader may leave the boot parameters `vgacon` looks for unfilled.
So the guest reads `/sys/class/vtconsole/*/name` and fails if only a dummy is bound. The
run above reports `VGA+`, which is real 80x25 VGA text — the environment RABA targets.

Nothing in the guest asks for `unsafe`. Exiting through `isa-debug-exit` normally needs
inline assembly to reach an I/O port; writing to `/dev/port` at offset `0xf4` reaches the
same port through a file, and `o` to `/proc/sysrq-trigger` is the fallback. So the
harness stays inside the workspace's `unsafe_code = "forbid"` even though the thing it
exists to test will not.

#### The harness proves itself first

`--self-test` puts `raba.selftest=fail` on the kernel command line, the guest fails one
check on purpose, and the script asserts the run came back **failed**. CI runs it before
it runs anything else.

This is not ceremony. A test rig that cannot fail reports success for reasons unrelated
to the code, and the first anybody hears of it is when something that should have been
caught was not. For the VT layer — where the failure mode is somebody's machine not
booting — that is not an acceptable way to find out.

### Tier 3b — QEMU, full distribution images

Only for what genuinely needs a distribution: real mkinitcpio and dracut runs, real init
systems, real display manager handoff, Secure Boot and TPM behaviour.

Slow, so it runs nightly rather than per-commit. `qcow2` snapshots of clean installs,
restored between runs. Images for Arch, Void (runit + dracut), Artix (OpenRC), Chimera
(dinit), Debian (initramfs-tools), Fedora (dracut + systemd).

Visual results come from the QEMU monitor:

```
(qemu) screendump shot.ppm
```

which makes automated visual regression possible — compare against a known-good frame.

---

## Test suite

### Categories

**Round-trip.** Encode a movie, decode it, assert equality. The cheapest possible
guarantee that the format implementation is self-consistent.

**Golden renders.** A fixture renders to a grid; compare against a committed snapshot.
Catches rendering regressions in milliseconds. The critical pair is the same artwork as
`.dur` v7 and v8 — both must produce **byte-identical** `.raba`, which is the regression
test for the colour-map transposition.

**The failure table.** Every malformed input asserts exit 0 and no panic:

| input | expect |
|---|---|
| truncated header | rejected, exit 0 |
| `total_bytes` disagrees with file size | rejected before allocating |
| canvas 10⁹ × 10⁹ | rejected at header validation |
| frame count beyond ceiling | rejected |
| op index out of range | rejected at load, not at render |
| zero frames | rejected |
| CRC mismatch | rejected |
| theme file absent | exit 0, nothing drawn |
| no framebuffer console | exit 0 |
| `SIGKILL` mid-frame | terminal usable afterwards |

This table is the project's central promise expressed as code. It should be the first test
file written and the one reviewed most carefully.

**Fuzzing.** `cargo-fuzz` over the parser, seeded with the fixture corpus. Short run on
every pull request, long run nightly, with the corpus committed so coverage accumulates
rather than restarting.

**Property tests.** `proptest`, where the invariants are naturally universal:
- any valid movie renders entirely within its canvas bounds
- delta reconstruction equals the full-frame reconstruction, for any frame
- the glyph validator accepts exactly the documented safe set
- `to_ansi(grid, prev)` followed by applying the output to `prev` yields `grid`

That last one is a strong property for the redraw-minimisation logic, which is otherwise
easy to get subtly wrong.

**Sanitisation.** Control bytes, escape sequences and over-long strings fed into
`raba-ctl message`; assert nothing reaches the terminal but printable characters.

**VT lifecycle.** Tier 3a. Acquire, draw, restore — then assert the terminal is in
`KD_TEXT` and the original VT is active. Including after `SIGKILL`, which is the case that
hangs boots.

### Dev-dependencies are free

`proptest`, `cargo-fuzz`, pty helpers and test harnesses do not ship and do not enter an
initramfs. The dependency budget in [maintenance.md](maintenance.md) applies to `raba`'s
runtime dependencies only. There is no reason to be frugal with test tooling.

### Fixtures and the support table

```
tests/fixtures/
  valid/      one per format feature claimed in format.md
  corrupt/    one per row of the failure table
  import/     .dur v7 and v8 of identical artwork
```

**The support table in the docs and the fixture directory are the same list.** A row with
no fixture is an untested claim, and should be treated as a documentation bug.

---

## CI

| on every push | nightly |
|---|---|
| `cargo test` | full-distribution VM matrix |
| `cargo clippy` with the deny lints | long fuzz run |
| `cargo fmt --check` | upstream watch: latest Durdraw, kernel versions |
| `cargo deny` — licences, advisories | visual regression via `screendump` |
| tier 3a VM tests | |
| binary size check | |

**Binary size is a gate, not a metric.** A threshold with a small allowance; crossing it
fails the build. It is the only thing that keeps "stay small" from decaying into a slogan,
and it is the number that will be quoted in every comparison with Plymouth.

MSRV is pinned and tested alongside stable. AUR users are always on the newest `rustc`;
anyone packaging this for a distribution later will not be.
