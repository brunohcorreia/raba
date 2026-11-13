# Contributing

## Themes are the easiest contribution

A theme is a recipe of about ten lines. You do not need to write Rust.

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
raba-gen themes/void.toml --preview
```

Then open a pull request with the `.toml`. Do not commit the generated `.dur` — it is a
build artifact.

### Rules for themes

- **ASCII, CP437 blocks and box-drawing characters only.** The Linux console holds
  256–512 glyphs; anything else renders as blanks. `raba-gen` will refuse to build a
  theme that breaks this, so you will find out immediately.
- **Keep the canvas tight.** It should be the bounding box of the art. Padding is stored
  in every frame and buys nothing, since RABA centres the canvas at display time.
- **Design for 16 colours.** Most consoles cannot do more.
- **Add an outro if you like.** For a seamless handoff its last frame must match the
  first frame of whatever the display manager shows.

Logo art comes from [fastfetch](https://github.com/fastfetch-cli/fastfetch) (MIT), which
ships 534 of them. `raba-gen --list-logos` shows what is available.

---

## Testing

**All boot testing happens in a virtual machine. Never test a splash change on a machine
you need to boot.** The failure mode of this project is an unbootable system.

### Iterating on art

No VM needed:

```sh
raba-gen themes/yours.toml --preview
raba --preview themes/yours.dur
```

Both render in the current terminal and touch no system state. Use these for anything
that is not boot integration.

### Two scripted runs before any of this

```sh
scripts/vm-test.sh           # kernel + our own initramfs: the VT layer, ~1s
scripts/image-test.sh        # an initramfs built by the real mkinitcpio, booted
packaging/tests/package.sh   # makepkg on the working tree, then what is inside
```

Neither touches the machine you are on. The second one needs mkinitcpio and skips
without it. Start there; the hand-driven VM below is for what they cannot reach.

### Virtual machine

```sh
qemu-system-x86_64 \
  -enable-kvm -m 2G \
  -bios /usr/share/edk2/x64/OVMF.4m.fd \
  -drive file=test.qcow2,if=virtio \
  -vga virtio \
  -serial stdio
```

Take a qcow2 snapshot of a clean install and restore it between runs.

Capture the framebuffer from the QEMU monitor for visual comparison:

```
(qemu) screendump shot.ppm
```

This makes automated visual regression testing possible — compare against a known-good
image.

### Matrix

A change to terminal handling should be tried across:

| axis | variants |
|---|---|
| firmware | UEFI (OVMF), BIOS (SeaBIOS) |
| video | `-vga virtio`, `-vga std`, `-vga none`, `nomodeset` |
| init | systemd, runit, OpenRC, dinit |
| disk | plain, LUKS |
| console | graphical, `console=ttyS0` |
| initramfs | mkinitcpio, dracut, initramfs-tools |
| resolution | 1024x768, 1920x1080, 3840x2160 |

### Failure cases matter more than success cases

Every one of these must leave a usable terminal and a booting system:

- corrupt, truncated, or non-gzip `.dur`
- `.dur` larger than the screen
- theme file missing from the initramfs
- `raba.enable=0`
- `SIGKILL` mid-animation
- display manager absent or failing to start
- no framebuffer console

The first and the last two are the ones that have historically broken other splash
programs.

---

## Code

### Non-negotiables

1. **Never prevent a boot.** RABA exits 0 on every error. A missing theme, an
   unreadable file, an absent console — all of these are "draw nothing and leave".
2. **Always restore the terminal.** On every exit path, including panic. A terminal left
   in `KD_GRAPHICS` can hang a later `VT_WAITACTIVE`, which stalls the boot. The panic
   handler restores before aborting.
3. **The initramfs hook never fails.** It warns; it does not return non-zero. A hook that
   errors can break image generation on a kernel update.
4. **Stay small.** No async runtime. `opt-level = "z"`, `lto`, `panic = "abort"`, static
   musl. Check binary size in CI; a regression is a bug.

### Layout

```
src/
  dur/        .dur parsing (v7 and v8)
  render/     terminal output, compositing
  vt/         virtual terminal acquisition and restoration
  ctl/        control socket
bin/
  raba-gen/   theme generator (build-time, never in the initramfs)
man/          scdoc sources
themes/       recipes
```

### Building the man pages

```sh
scdoc < man/raba.8.scd > raba.8
```

### Where to start

See `arquitetura.md` for the design and the implementation order. The first useful piece
is the `.dur` parser plus `--preview`, which is independent of all boot integration.
