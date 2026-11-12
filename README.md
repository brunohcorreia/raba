<h1 align="center">RABA</h1>
<p align="center"><b>R</b>ust <b>A</b>SCII <b>B</b>oot <b>A</b>rt</p>
<p align="center">A tiny boot splash that plays ASCII animations, then gets out of the way.</p>

---

> [!WARNING]
> **Status: pre-alpha. There is no boot splash yet.**
> The format, the renderer and the theme builder work; nothing touches a virtual
> terminal, an initramfs or your bootloader. This README otherwise describes the
> intended design rather than shipped behaviour.
> Do not install this on a machine you need to boot.

---

## Try it

No root, no initramfs, no virtual machine. Everything below runs in an ordinary
terminal.

```sh
./scripts/build-themes.sh
cargo run --release -p raba -- --preview themes/arch.raba
```

Thirty-three themes are included, covering the most-used distributions across desktop,
server and gaming — Ubuntu, Debian, Mint, Fedora, Arch, CachyOS, SteamOS, Pop!_OS,
Manjaro, Kali, RHEL and its rebuilds, Raspberry Pi OS and the rest. Art and colours are
taken from fastfetch. [themes/README.md](themes/README.md) says how they were chosen
and how to add another, which needs no Rust.

That is [M0](docs/milestones.md#m0--see-a-logo-animate): a logo and a spinner,
animating, so the project's one untested assumption can be looked at rather than
argued about. If ASCII art at console cell size does not look good enough, that is
worth finding out for the price of a weekend.

Build a theme of your own. A recipe is about ten lines of TOML next to the art:

```toml
# themes/mine.toml
[logo]
source = "file:mine.txt"
colors = [2, 10, 6]     # what the art's $1, $2 and $3 mean

[spinner]
style = "ascii"
position = "below"

[canvas]
framerate = 8
```

```sh
raba-gen build themes/mine.toml
raba --preview themes/mine.raba --loops 0
```

Art is plain text with `$1` to `$9` marking colour changes, the format fastfetch
stores its logos in. `scripts/fastfetch-logo.py arch > themes/arch.txt` pulls one out
of an installed fastfetch. `man 5 raba-theme` has the full schema; `raba-gen --check`
tells you whether a console can actually draw what you made.

---

## What it is

RABA draws an ASCII art animation on the console while your system boots, then hands
off to your display manager. That's all it does.

It is built around three constraints:

- **It must never prevent your machine from booting.** Every failure mode exits
  cleanly and gives the terminal back.
- **It must be small.** No GPU, no DRM drivers in your initramfs, no graphics stack.
  It writes escape codes to a virtual terminal, which is what ASCII art wants anyway.
  It lowers the *console* log level while it draws and restores it on exit — `dmesg` and
  the journal are untouched.
- **It must not care what you run.** systemd or runit, GRUB or Limine, Arch or Void,
  any display manager or none. At run time it depends on no other program.

## Why not Plymouth

Plymouth is a capable graphical splash and RABA is not trying to replace it for
people who want graphics. The honest tradeoff:

| | Plymouth | RABA |
|---|---|---|
| Rendering | DRM/KMS, real pixels | text cells on a VT |
| initramfs cost | DRM modules roughly double the image | one static binary + a theme file |
| Needs GPU driver early | yes — a slow `amdgpu` can mean no splash at all | no |
| Graphics capability | full | ASCII art |

A note on honesty: Plymouth's reputation for slowing boot is partly a measurement
artifact. `plymouth-quit-wait.service` shows up large in `systemd-analyze blame`, but
it keeps running *after* your login screen is already up. Fedora maintainers measured
~3.5s real cost against the 8.3s the tool reported. RABA's advantage is initramfs size
and not depending on a GPU driver — not "Plymouth is slow".

## How it works

```
    boot ──► RABA plays the loop animation ──► system ready
                                                    │
                                        raba-ctl quit
                                                    │
                                                    ▼
                    RABA plays the outro once ──► whatever comes next
```

"Whatever comes next" is any display manager, or a plain getty, or nothing. RABA knows
nothing about display managers; it exits when told and restores the terminal. A single
`raba-ctl quit` is the entire interface, and a timeout handles the case where nobody
calls it.

Two animations: one that loops until the system is ready, and one that plays a single
time on the way out. If the last frame of your outro matches the first frame of the
`.dur` your display manager shows, the transition is invisible.

The `.dur` file is the background. Anything that reacts to runtime state — a passphrase
prompt, a status message, a progress indicator — is drawn over it by RABA. Themes stay
pure data, which is what lets them be validated at build time and shared with Ly.

## Animations

RABA plays `.raba` files — a small binary format of its own. The whole file's memory
footprint is stated in a fixed header and validated before anything is allocated, so a
corrupt or hostile file is rejected rather than becoming an out-of-memory failure in
early boot. Frames after the first store only what changed.

You do not have to author in it. `raba-gen` converts from formats that already have
editors:

```sh
raba-gen convert art.dur -o art.raba      # Durdraw
raba-gen convert art.ans -o art.raba      # ANSI art
raba-gen build theme.toml -o theme.raba   # logo + spinner recipe
```

The rule the design follows:

> **Authoring time may depend on anything. Run time depends on nothing.**

`raba-gen` is a build tool and never enters the initramfs. If an upstream format changes,
a converter breaks and is fixed at leisure. Nobody's machine stops booting.

See [docs/format.md](docs/format.md).

## Install

> Not packaged yet.

```sh
# planned
paru -S raba raba-themes
```

> [!CAUTION]
> **If your disk unlocks automatically via TPM, read
> [docs/secure-boot.md](docs/secure-boot.md) first.** Adding anything to the initramfs
> changes the TPM measurements, and a key sealed to PCR 9 or 11 will stop releasing —
> you will be asked for your recovery passphrase instead. `raba-setup` checks for this
> and warns, but keep a recovery key enrolled regardless.

Then, deliberately — RABA will not edit your bootloader or initramfs config for you:

```sh
raba-setup            # shows what it would change, changes nothing
raba-setup --apply    # does it
```

See [docs/integration.md](docs/integration.md) for doing it by hand, and for
distributions and init systems beyond Arch.

## Configure

`/etc/raba/raba.conf`:

```ini
[Boot]
Theme=auto          # match the distro in /etc/os-release
Timeout=30

[Outro]
Enable=true
Theme=auto-outro
MaxDuration=1.5
```

> [!IMPORTANT]
> Editing this file does nothing until the initramfs is rebuilt — RABA runs before
> your root filesystem is mounted, so it reads a copy baked into the initramfs image.
> Use `raba-set-theme`, which rebuilds for you, or run `raba-setup --check` to find
> out whether your initramfs is stale.

Full reference: `man 5 raba.conf`.

## Themes

A theme is a `.dur` file. You can draw one in Durdraw, or generate one from a recipe:

```toml
# themes/arch.toml
[logo]
source = "fastfetch:arch"
colors = [6, 4]

[spinner]
style = "ascii"      # | / - \
position = "below"

[canvas]
framerate = 10
```

```sh
raba-gen themes/arch.toml -o arch.dur
raba --preview arch.dur          # render in the current terminal, no VT involved
```

`raba-gen` validates every glyph against what the Linux console can actually display.
The console is capped at 256–512 glyphs, so Braille, arrows and emoji render as blanks
or question marks. The generator fails the build and tells you which characters are a
problem, rather than letting you find out during a boot.

Logo art is sourced from [fastfetch](https://github.com/fastfetch-cli/fastfetch) (MIT),
which ships 534 distro logos.

## Documentation

| | |
|---|---|
| `man 8 raba` | the boot program |
| `man 1 raba-ctl` | controlling a running instance |
| `man 5 raba.conf` | configuration reference |
| `man 1 raba-gen` | theme generator |
| `man 5 raba-theme` | theme recipe format |
| [docs/integration.md](docs/integration.md) | per-distro and per-init setup |
| [docs/troubleshooting.md](docs/troubleshooting.md) | **start here if your boot broke** |
| [docs/format.md](docs/format.md) | the `.raba` format and the converters |
| [docs/early-boot.md](docs/early-boot.md) | starting early, and surviving `switch_root` |
| [docs/dur-support.md](docs/dur-support.md) | importing `.dur`, and its version traps |
| [docs/progress.md](docs/progress.md) | progress indication, and why it is hard |
| [docs/luks.md](docs/luks.md) | disk encryption, and why RABA stays out of it |
| [docs/risks.md](docs/risks.md) | pre-mortem: what can break, including upstream |
| [docs/maintenance.md](docs/maintenance.md) | code structure, dependency budget, testing |
| [docs/security.md](docs/security.md) | threat model and hardening |
| [docs/boot-order.md](docs/boot-order.md) | kernel parameters and hook placement |
| [docs/coexistence.md](docs/coexistence.md) | Plymouth, getty, and when not to draw |
| [docs/secure-boot.md](docs/secure-boot.md) | Secure Boot, TPM measurements, lockdown |
| [docs/packaging.md](docs/packaging.md) | packaging conventions, and what we refuse to do |
| [CONTRIBUTING.md](CONTRIBUTING.md) | adding themes, testing in VMs |
| [docs/development.md](docs/development.md) | implementation order, test environment and suite |
| [docs/milestones.md](docs/milestones.md) | what is being built, in what order |
| [docs/glyphs.md](docs/glyphs.md) | which characters a console can draw, and why |
| [docs/3d-logos.md](docs/3d-logos.md) | 3D rotating logos, and where they would live |

## If your machine will not boot

Add this to your kernel command line from the bootloader:

```
raba.enable=0
```

RABA does nothing at all with that set. Then read
[docs/troubleshooting.md](docs/troubleshooting.md) and please file a bug — a boot
failure is the most serious class of bug this project can have.

## Contributing

Themes are the easiest and most welcome contribution: a recipe is about ten lines.
See [CONTRIBUTING.md](CONTRIBUTING.md).

All boot testing happens in virtual machines. Never test a splash change on a machine
you need.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).

Distribution logos are trademarks of their respective projects and are included on the
same basis as neofetch, fastfetch and similar tools. If you hold a mark and would like
it removed, open an issue and it will be removed.

## Thanks

- [Durdraw](https://github.com/cmang/durdraw) — a good ASCII animation editor, and the
  easiest way to author a theme
- [fastfetch](https://github.com/fastfetch-cli/fastfetch) for the logo collection
- [psplash](https://git.yoctoproject.org/psplash/) for showing how to handle a VT safely
- [Plymouth](https://www.freedesktop.org/wiki/Software/Plymouth/) for the integration
  model this project copies almost exactly
