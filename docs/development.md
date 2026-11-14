# Development plan

## Where this is

0.1.0 is released. [milestones.md](milestones.md) is the log of what was built and what
each step cost; this document is the part a contributor needs: how the code is laid out,
what the test tiers are, and what CI will refuse.

The question this file used to open with was whether ASCII art at console cell size
looks good enough to justify the project at all — nobody had looked at one on a screen,
and every design decision rested on the answer. It was answered in M0, in a weekend, and
the answer was yes. It is recorded here because the habit that produced it is the useful
part: the most existential question goes first, and it goes first even when it is the
least interesting one to build.

> The three sections that follow are the original plan, kept because the reasoning in
> them — parser first, by risk rather than by dependency — is what the code still looks
> like. [milestones.md](milestones.md) is what actually happened.

## Milestone 0 — see a logo animate

**Goal: a distro logo with a spinner, animating in my terminal.** Nothing else.

```
fastfetch logo (plain text)  →  compose  →  .raba  →  preview in terminal
```

No `.dur`, no VT, no initramfs, no root, no VM. Plain text in, pixels on a screen out.

Roughly 500 lines. It freezes the format, produces the first real artifact, builds the
`Surface` abstraction that everything later plugs into, and — the point — answers whether
the premise holds.

If the art looks bad at this size, that is known for the price of a weekend, and the
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

**5. It depends on no decision still open.** The VT layer has unanswered questions
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
  clamp on consoles it has never run on is not something to leave in place.

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
Most of what looked as though it needed a slow full-distro VM does not.

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

`--self-test` puts `rabatest.selftest=fail` on the kernel command line, the guest fails one
check on purpose, and the script asserts the run came back **failed**. CI runs it before
it runs anything else.

This is not ceremony. A test rig that cannot fail reports success for reasons unrelated
to the code, and the first anybody hears of it is when something that should have been
caught was not. For the VT layer — where the failure mode is somebody's machine not
booting — that is not an acceptable way to find out.

### Tier 3a½ — the real generator's image, booted

Tier 3a boots an image this project assembled itself, which proves the program and
proves nothing about the hook that has to put it there. `scripts/image-test.sh` boots
an image built by `/usr/bin/mkinitcpio`, unmodified, with the shipped install hook in
a `--hookdir`:

```sh
scripts/image-test.sh            # build, boot, report
scripts/image-test.sh --keep     # leave the image and the sandbox behind
scripts/image-test.sh --console  # watch the guest's VGA console too
```

The checks ride along as a second hook, `rabacheck`, placed after `raba` in `HOOKS`,
so they run inside the image as an ordinary runtime hook with the splash already up.
Nine TAP lines come back over the serial line. It needs mkinitcpio, so it skips on
machines that have none — `integration/tests/*.sh` cover those from stubs.

Two things about mkinitcpio shape the whole script, and both are worth knowing before
writing anything that drives it:

- it does `export PATH="/usr/bin:/bin"` at startup, so a directory prepended to `PATH`
  on the command line is gone before any hook runs;
- `add_binary raba` installs the binary into the image **at the path it resolved to**,
  so a `raba` found anywhere but `/usr/bin` lands in the image at that same odd path.

Hence the build runs inside a throwaway user namespace whose `/usr/bin` is an overlay
with the freshly built binaries on top, under `fakeroot` — being root in a namespace is
not being root, host-owned files appear there as 65534, and `cp --preserve=all` cannot
chown to an unmapped id, which would make every `add_binary` return non-zero. Nothing
outside `target/` is touched; a machine without user namespaces, overlayfs or fakeroot
gets a skip.

### Tier 3b — QEMU, full distribution images

Only for what genuinely needs a distribution: real mkinitcpio and dracut runs, real init
systems, real display manager handoff, Secure Boot and TPM behaviour.

```sh
scripts/distro-test.sh                     # Arch: packages, mkinitcpio
scripts/distro-test.sh --distro debian     # source build, initramfs-tools
scripts/distro-test.sh --distro fedora     # source build, dracut
scripts/distro-test.sh --all               # each in turn
scripts/distro-test.sh --console           # watch it happen
scripts/distro-test.sh --keep              # leave the disks and frames behind
```

| | installed how | initramfs | what it also answers |
|---|---|---|---|
| Arch | the packages the PKGBUILD builds, `pacman -U` | mkinitcpio | does the package work |
| Debian | `make all && make install`, with apt's cargo | initramfs-tools | can a Debian packager build it today |
| Fedora | `make all && make install`, with dnf's cargo | dracut | same question, dnf's answer |

The two source builds are not a formality. They use the toolchain in each
distribution's own repositories, so a `cargo` older than the MSRV shows up as a loud
failure — which is exactly the finding worth having before promising anybody a package.

The distribution's own cloud image, cached once in `target/distro/<distro>/base.qcow2`
and never written to — every run gets a copy-on-write overlay. **Nothing here needs
root on your machine**: the cloud-init seed is a FAT filesystem built with `mkfs.vfat`
and `mcopy`, the packages ride in on a second FAT disk, and QEMU runs as you.

Two boots. The first one is a person following `raba.install`: install the packages with
`pacman -U`, add `raba` to `HOOKS` in the real `/etc/mkinitcpio.conf`, run `mkinitcpio
-P`, enable the units, reboot. The second one is the test: a `raba-check.service`
ordered after `multi-user.target` emits TAP on the serial line and powers the machine
off. Meanwhile the host is taking `screendump`s through QEMU's monitor, so there is
evidence about what was actually on the screen rather than only what the guest believes.

The screen check classifies rather than thresholds, and the difference matters:
`scripts/screen-colours.py` calls a many-coloured bitmap firmware, console text against
the left margin a boot log or a prompt, and a *centred* block of console text a splash.
The first version only counted how much of the frame was not black, and passed happily
on the TianoCore logo that OVMF draws. It is still not a pixel comparison against a
reference frame — that would pin a font, a resolution and a timing at once, three ways
to go red for no reason — but "something centred was drawn in text" is a claim about the
right thing.

One frame is kept as `target/distro/<distro>/screen.png` and the rest are thrown away; a
run captures a hundred three-megabyte screenshots otherwise.

Visual results come from the QEMU monitor:

```
(qemu) screendump shot.ppm
```

which makes automated visual regression possible — compare against a known-good frame.

### Tier 3c — power cuts and sabotage

Tier 3b asks whether a real distribution installs this and draws. This asks the first
promise, the one in [CONTRIBUTING.md](../CONTRIBUTING.md) that predates the code:

> no pior caso, se quebrar, nao podemos impedir o computador de dar boot

```sh
scripts/resilience.sh --distro fedora              # everything
scripts/resilience.sh --distro fedora --cuts       # power cuts only
scripts/resilience.sh --distro fedora --sabotage   # sabotage only
scripts/resilience.sh --distro arch --case no-binary
```

Nothing here is a unit test. Every case is a whole machine, booted, and the assertion is
always the same: it came up anyway.

**Power cuts.** `kill -9` to QEMU at 1.0, 1.6, 2.2, 3.0 and 4.5 seconds — no SIGTERM, no
ACPI, no flush, the machine stops existing between two instructions — and then the *same
disk* is booted again. RABA writes nothing to persistent storage at boot, only to `/run`,
which is a tmpfs, so the claim under test is that a cut cannot leave anything of ours
behind that the next boot trips over. Both halves are asserted: the machine boots, and
the full tier 3b check suite passes on the boot after the cut.

**Sabotage.** Seven cases, chosen because a person could do any of them to their own
machine — an upgrade that removed a file, a half-finished download, a configuration
edited in anger at two in the morning:

| case | what is broken | what is expected |
|---|---|---|
| `none` | nothing; the control | the strict tier 3b checks pass |
| `no-binary` | `/usr/bin/raba` deleted | systemd reports the failed unit |
| `truncated-art` | every theme cut to 100 bytes | a sentence naming the file and the sizes |
| `garbage-config` | `raba.conf` replaced with nonsense | a sentence naming the file |
| `impossible-tty` | `Boot.Tty=64` | a sentence naming the range |
| `socket-in-the-way` | a plain file at `/run/raba.sock` | **silence** — `bind()` replaces it |
| `state-in-the-way` | `/run/raba.state` is a directory | a sentence, said once, not per frame |

The host chooses the case by attaching a small vfat disk labelled `RABACASE`, the same
trick as the cloud-init seed, so the guest's filesystem is never edited from outside.
`integration/tests/distro/case.sh` applies it before the splash starts, and
`survived.sh` runs instead of `check.sh` afterwards and asks five questions: is the
machine up, is there a login prompt, is the root filesystem writable, is nothing of ours
still holding the screen, and is the reason somewhere a person would look.

That last expectation is per-case and it is the subtle one. `socket-in-the-way` is a
thing RABA is supposed to *cope* with, so a complaint about it would be the defect and
the assertion is the absence of one. Demanding a sentence from every case is how this
check first went red against a splash that was working correctly.

Both groups boot an already-installed image rather than installing each time:
`scripts/distro-test.sh --keep-installed` leaves `installed.qcow2` behind, and
`resilience.sh` builds one if it is missing. The install is about ten minutes; each case
after that is about thirty seconds.

What this does **not** cover yet: the sabotage lands in the real root, after the
initramfs instance is already drawing. The two configuration cases edit
`/etc/raba/raba.conf`, which the boot-time instance never reads — `man 5 raba.conf` on
why — and `no-binary` cannot reach the copy baked into the image. Breaking the early
instance means regenerating the initramfs inside the guest per case, which is minutes
rather than seconds, and it is not done.

### Recording a boot

The animation at the top of the README is made by `scripts/record.sh`, which boots the
same installed image tier 3c boots and photographs the screen through QEMU's monitor:

```sh
scripts/record.sh                       # arch, into docs/images/boot.gif
scripts/record.sh --distro fedora --seconds 20 --out /tmp/boot.gif
scripts/record.sh --crop 1280x800       # the whole screen rather than a window
```

Two decisions in it are worth knowing before changing them.

**It crops; it does not scale.** The guest's console is 1280x800 of 8x16 glyphs.
Resampling that to a README-friendly width turns every glyph into a smear and the logo
into horizontal stripes — the first attempt looked like a damaged CRT. Cutting a window
out of the middle at one-to-one keeps every pixel the kernel drew, and costs nothing in
file size, because what is cut away is black. `paletteuse=dither=none` for the same
reason: console text is flat colour, and dithering invents noise in the one place the
image has none.

**It finds the splash by asking the classifier.** A whole boot is mostly not what anybody
came to see — four seconds of the firmware's logo, a kernel log, about a second of RABA,
then a login prompt. `scripts/screen-colours.py` already tells a bitmap from a boot log
from a centred block of console text, so the cut is the longest *run* of centred frames
plus a short pad either side. The run, not the span: a login prompt with one line on it
classifies as centred too, and taking first-to-last would quietly include the rest of
the boot.

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

Most of what would be a fixture directory in another project is built in code here, and
on purpose: a malformed file is easier to read as the two lines that construct it than as
a hex dump somebody has to open in an editor.

| | where | what it holds |
|---|---|---|
| the failure table | `crates/raba-core/tests/failure_table.rs` | 36 shapes of malformed input, each with the error it must produce |
| import | `crates/raba-gen/tests/dur.rs` | 15 cases, including the same art as `.dur` v7 and v8 |
| golden renders | `themes/golden/` | one rendered grid per shipped theme |
| a real file from a past version | `tests/fixtures/valid/v1-generic.raba` | written by an older generator; `crates/raba-core/tests/compatibility.rs` still reads it |

That last one is the exception that has to be a file. A compatibility test whose input is
generated by the code under test proves nothing; the bytes have to come from before the
change.

**The support table in the docs and these tests are the same list.** A row with no test is
an untested claim, and should be treated as a documentation bug.

---

## CI

| on every push | nightly |
|---|---|
| `cargo test` | full-distribution VM matrix |
| `cargo clippy` with the deny lints | long fuzz run |
| `cargo fmt --check` | upstream watch: latest Durdraw, kernel versions |
| `cargo deny` — licences, advisories | visual regression via `screendump` |
| tier 3a VM tests | tier 3c: power cuts and the sabotage matrix |
| binary size check | |

**Binary size is a gate, not a metric.** A threshold with a small allowance; crossing it
fails the build. It is the only thing that keeps "stay small" from decaying into a slogan,
and it is the number that will be quoted in every comparison with Plymouth.

MSRV is pinned and tested alongside stable. AUR users are always on the newest `rustc`;
anyone packaging this for a distribution later will not be.
