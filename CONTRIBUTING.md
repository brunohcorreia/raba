# Contributing

The most useful thing to know first: **the failure mode of this project is a computer
that will not start.** Everything below follows from that. All boot testing happens in a
virtual machine, errors are never fatal, and a change that cannot be tested without
rebooting your own machine is a change that needs a VM.

If something here is wrong or missing, that is a bug worth reporting on its own.

---

## Themes are the easiest contribution

A theme is a recipe of about ten lines and no Rust at all.

```toml
# themes/void.toml
[logo]
source = "fastfetch:void"
colors = [2, 10]

[spinner]
style = "ascii"
position = "below"

[canvas]
framerate = 10
```

```sh
cargo run --release -p raba-gen -- build themes/void.toml --preview
```

`--preview` plays it in the terminal you are sitting at. Nothing is installed, no
virtual terminal is taken, and no root is needed.

Open a pull request with the `.toml` and, if the art is yours, the `.txt` beside it. Do
not commit the generated `.raba`: it is a build artifact and `.gitignore` says so.

### Rules for themes

- **ASCII, CP437 blocks and box-drawing characters only.** A Linux console holds 256 to
  512 glyphs, and anything outside the loaded font renders as a blank or a question
  mark. `raba-gen` refuses to build art that breaks this and names the characters, so
  you find out in a second rather than during a boot. See [docs/glyphs.md](docs/glyphs.md).
- **Keep the canvas tight.** It should be the bounding box of the art: padding is stored
  in every frame and buys nothing, because RABA centres the canvas when it draws.
- **Design for 16 colours.** Most consoles cannot do more, and the ones that can are not
  the ones that matter at boot.
- **An outro is optional.** For a seamless handoff its last frame should match the first
  frame of whatever takes the screen next.

Logo art comes from [fastfetch](https://github.com/fastfetch-cli/fastfetch) (MIT),
vendored at a pinned commit in `vendor/fastfetch-logos`. 545 logos are available and
`raba-gen --list-logos` says which of them a console can actually draw — about thirty
are braille or sextants and need a terminal font. [themes/README.md](themes/README.md)
explains how the fifty shipped ones were chosen.

Distribution logos are trademarks of their projects and are included on the same basis
as neofetch and fastfetch. If you hold a mark and want it gone, open an issue; it will be
removed.

---

## Code

### Four rules that are not up for discussion

1. **Never prevent a boot.** The project's founding constraint, written in the author's
   first notes before a line of code existed:

   > no pior caso, se quebrar, nao podemos impedir o computador de dar boot
   >
   > *in the worst case, if it breaks, we cannot stop the computer from booting*

   `raba` exits 0 on every error. A missing theme, an
   unreadable file, an absent console: all of them are "draw nothing and leave". The
   only thing that may be fatal is a flag a human typed wrong at a prompt.
2. **Always give the terminal back.** On every exit path, including a panic — the
   release profile is `panic = "abort"` precisely so that the panic case is the same
   case as `SIGKILL`, which is tested. A terminal left in `KD_GRAPHICS` can hang a later
   `VT_WAITACTIVE` and stall the boot.
3. **An initramfs hook never fails.** It warns and returns 0. A hook that errors can
   break image generation on a kernel update, which breaks a machine that was working.
4. **Stay small.** No async runtime, no graphics stack. `opt-level = "z"`, `lto`,
   `panic = "abort"`, and a static musl build measured by CI against
   [`.binary-size-limit`](.binary-size-limit). Every direct dependency needs a row in
   [DEPENDENCIES.md](DEPENDENCIES.md) before it is added; the boot binary's budget is
   two and both are spent.

### Where things are

```
crates/
  raba-core/    the .raba format, layout, rendering. Pure: no I/O, no syscalls
  raba-term/    a terminal surface and frame timing on top of raba-core
  raba/         the splash and raba-ctl: virtual terminals, the control socket
  raba-setup/   installation, initramfs regeneration, raba-setup --set-theme
  raba-gen/     the authoring tool: recipes, .dur and ANSI import, generators
  raba-vmtest/  the in-guest half of the VM tests
integration/    initramfs hooks and init-system units, per generator and per init
man/            scdoc sources; the pages are built, not committed
themes/         recipes, art, and one golden render per theme
scripts/        the test harnesses and the logo extractor
```

`raba-core` is pure on purpose: it is the part that parses a file nobody supervised, and
it can be fuzzed and property-tested without a kernel anywhere near it.

### Before you open a pull request

```sh
cargo fmt --all
cargo clippy --workspace --all-targets      # the deny lints are not advisory
cargo test --workspace
```

CI runs those plus `cargo deny`, a musl build with a size gate, and the tier 3a VM
tests. The full distribution matrix runs nightly.

---

## Testing

**Never test a splash change on a machine you need to boot.** Not once, not "just to
see". Everything below runs in a VM or in your own terminal and touches no system state.

| | what it is | how long |
|---|---|---|
| `cargo test --workspace` | the pure tiers: format, layout, rendering, config, the failure table | seconds |
| `scripts/vm-test.sh` | a kernel and an initramfs we build: the virtual-terminal layer on a real console | ~1 min |
| `scripts/image-test.sh` | an initramfs built by the real mkinitcpio, booted | ~2 min |
| `packaging/tests/package.sh` | `makepkg` on the working tree, and what ends up inside the packages | ~3 min |
| `scripts/distro-test.sh` | Arch, Debian or Fedora installed from scratch and rebooted | ~12 min |
| `scripts/resilience.sh` | five power cuts and seven sabotage cases on an installed machine | ~20 min |
| `scripts/record.sh` | the animation at the top of the README | ~2 min |

None of them needs root on your machine: the cloud-init seed is a FAT filesystem built
with `mkfs.vfat` and `mcopy`, and QEMU runs as you.
[docs/development.md](docs/development.md) describes each tier and why it exists.

Five of those seven are shell, and CI runs `shellcheck --severity=warning` over all of
it — the harness as well as the hooks. That is not fussiness about style. A harness that
fails quietly reports success for a run that never happened, and every shell mistake
this project has had was of exactly that kind: a glob that matched nothing and killed
the script with no message, a `die` inside a command substitution that ended the
subshell and let an unsabotaged machine pass a sabotage case. If you touch a `.sh`, run
it:

```sh
shellcheck --severity=warning scripts/*.sh packaging/tests/*.sh integration/tests/distro/*.sh
```

### Iterating on art needs no VM at all

```sh
cargo run --release -p raba-gen -- build themes/yours.toml --preview
cargo run --release -p raba -- --preview themes/yours.raba
```

### Failure cases matter more than success cases

Every one of these must leave a usable terminal and a booting system, and all of them
are in `scripts/resilience.sh` or the failure table:

- a corrupt, truncated or non-gzip input file
- art larger than the screen
- the theme missing from the initramfs
- `raba.enable=0` on the kernel command line
- `SIGKILL` mid-animation, and losing power mid-animation
- a display manager that is absent, or that fails to start
- no framebuffer console at all

The first and the last two are the ones that have historically broken other splash
programs.

### What is not covered yet

Said plainly, because a contributor deserves to know where the edge is: only systemd has
actually been booted. The service files for runit, OpenRC, s6 and dinit ship and have
never run. There is no Alpine `mkinitfs` hook, nothing for immutable distributions, and
no LUKS passphrase prompt. Any of those is a real contribution.

---

## Reporting a bug

A boot failure is the most serious class of bug this project can have, and it is also the
hardest to report from a machine that will not start. If you are in that position:

1. Add `raba.enable=0` to the kernel command line from your bootloader. RABA then does
   nothing at all, and the machine boots as it did before.
2. Then read [docs/troubleshooting.md](docs/troubleshooting.md), which says what to
   collect and where it lives.

Open an issue either way. "It did not draw" with the output of `raba-setup` and
`journalctl -b | grep raba` is a better report than it looks.

---

## Licence

GPL-3.0-or-later. By contributing you agree your work is licensed under it.
