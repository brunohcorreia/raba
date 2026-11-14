# Pre-mortem

> Imagine it is a year from now and the project failed. What killed it?

Written before any code exists, so the answers can still change the design.

---

## 1. The architectural risk I under-weighted: `switch_root`

RABA's core use case requires it to be alive from early boot until the display manager
is ready. That spans `switch_root`, where the initramfs hands off to the real root.

systemd's own documentation is blunt about what happens there:

> The switch-root operation will result in a killing spree of all running processes.
> Some processes might need to be excluded from that.

Plymouth survives it, and the machinery is not small:

- `plymouth-switch-root.service` — a oneshot ordered `Before=initrd-switch-root.service`
  running `plymouth update-root-fs --new-root-dir=/sysroot`
- `KillMode=none`, `SendSIGKILL=no` on the daemon's unit
- keeping the daemon outside its cgroup so the killing spree misses it
- and a known upstream bug where `/dev` is not yet bind-mounted on `/sysroot/dev` when
  the re-root happens

Every one of those is systemd-specific. None of it exists for runit, OpenRC, s6 or
dinit, where an equivalent would have to be invented.

### Consequence: v1 should not run in the initramfs at all

Start RABA **after** `switch_root`, from the init system, as an ordinary early service.

| | in initramfs | after switch_root |
|---|---|---|
| survives the kill | needs per-init machinery | not applicable |
| LUKS collision | needs handling | cannot happen |
| initramfs hook | required | none |
| can break image generation | yes | no |
| splash covers | whole boot | post-root boot |
| complexity | high | low |

The cost is that the splash starts later. With `quiet loglevel=3` and
`systemd.show_status=false` the screen is blank or shows the firmware logo until then,
which is not obviously worse than a splash that appears and then has to dodge a
passphrase prompt.

The benefit is that an entire category of catastrophic failure disappears. No initramfs
hook means no possibility of breaking image generation on a kernel update, which is the
worst thing this project could do to someone.

**Revised staging:**

- **v1** — service after `switch_root`. No initramfs involvement whatsoever.
- **v2** — optional initramfs hook for systemd only, with the `switch_root` dance.
- **v3** — other init systems in the initramfs, if anyone actually asks.

This also means the LUKS question resolves itself in v1: RABA is not running when
`cryptsetup` prompts, because the root filesystem is already mounted by then.

---

## 2. Upstream changes that can break RABA

This is the risk class that kills long-lived small projects. Each entry is an upstream
outside the project's control, with evidence that it churns.

### `.dur` as a runtime format — **resolved by removing it**

Durdraw v8 changed the colour map orientation and renamed `sizeX`/`sizeY` — a silent
breaking change inside one format. v9 can do it again.

This was going to be a permanent tax: pinned between Durdraw's current version and
whatever a display manager happened to accept.

> **Resolved.** `.dur` is now an import format handled by `raba-gen` at build time; the
> runtime parses `.raba`, which is ours. See [format.md](format.md). A Durdraw release can
> now break a *converter*, which is a build error someone fixes at leisure. Golden
> fixtures per source version still apply, but the blast radius is no longer a boot.

### Display managers — **resolved by not depending on one**

Ly is pre-1.0, written in Zig (whose APIs break every release), already changed its config
format from `config.ini` to `config.lua`, and moved hosts. Any display manager can do the
equivalent.

> **Resolved.** The handoff is one command, `raba-ctl quit`, plus a timeout backstop.
> RABA knows nothing about display managers and reads none of their files. Matching art
> for a seamless transition is a property of the art, not an integration.

### The `plymouth-quit-wait.service` name — **subtle, silent failure**

Some display manager units carry `After=... plymouth-quit-wait.service ...`. It is
tempting to name RABA's unit similarly and get the ordering for free.

Do not. If that line disappears, handoff breaks with no error — just a flicker nobody can
reproduce.

> Mitigation: ship explicit ordering of RABA's own. Treat the coincidence as a convenience,
> never as the mechanism. `Boot.Timeout` is the backstop that makes a missed handoff
> survivable rather than fatal.

### The kernel VT subsystem — **low probability, fatal impact**

The whole text-mode approach rests on `CONFIG_VT` and `fbcon`. The kernel has seen
repeated proposals for a kernel-side bootsplash, Plymouth has added an internal terminal
emulator "for plugins that work with kernels that don't have fbcon", and some
distributions are experimenting with VT-less setups.

If fbcon goes away, the core design dies.

> Mitigation: keep the renderer behind an interface from day one so a DRM backend is an
> addition rather than a rewrite. Do not build it yet — just do not make it impossible.

### mkinitcpio / dracut hook APIs — **medium**

mkinitcpio changed hook behaviour before (the `kms` hook became default in v33). The
`encrypt` hook's hardcoded Plymouth probe could change.

> Mitigation: v1 has no hook. When v2 adds one, keep it small enough to read in one
> screen, and test against multiple mkinitcpio versions.

### Rust dependencies — **medium, and entirely self-inflicted**

Every crate is a maintenance liability in a program that must never break. API churn,
MSRV bumps, advisories, and transitive growth.

> Mitigation: see [maintenance.md](maintenance.md). The policy is a hard dependency
> budget, not good intentions.

### fastfetch logo art — **low**

Vendored art can change or disappear upstream.

> Mitigation: vendor at a pinned commit, record it, and treat updates as deliberate.

---

## 3. Failure modes not yet in the architecture notes

### Gzip bomb / malloc bomb in the parser

A `.dur` is gzipped JSON. A corrupt or hostile file can decompress to gigabytes. Early in
boot, with limited RAM and no swap, that is an OOM or a hang — which means no boot.

We parse this file as root, early, with no supervision.

> Mitigation: hard caps on decompressed size, canvas dimensions, and frame count, enforced
> *during* decompression rather than after. Fuzz the parser. This is the single most
> valuable test in the project.

### Two instances

The initramfs starts one, a service starts another, both draw. Garbage on screen.

> Mitigation: pidfile plus an abstract-namespace socket lock. Second instance detects the
> first and exits 0 silently.

### `getty` on the same VT

If RABA draws on tty1 and `getty@tty1` starts, they fight.

> Mitigation: acquire a free VT with `VT_OPENQRY` the way psplash does, rather than
> hardcoding tty1. Costs a VT switch, removes a whole class of conflict.

### 256-colour themes on a 16-colour console

Most Linux consoles do 16 colours. A theme authored in 256 will render wrong, not
gracefully degraded.

> Mitigation: downsample 256 → 16 by nearest match in the xterm palette at load time.
> Without this, "design for 16 colours" is advice users will ignore and then file bugs
> about.

### Bright background colours

The Linux console's background colour range is 8, not 16 — historically the high bit
drives blink rather than brightness. `setterm`'s own interface reflects this: `--background`
takes an 8-colour value while `--ulcolor` takes 16.

A theme using background 8–15 may blink instead of being bright.

> Mitigation: clamp backgrounds to 0–7 at load time and warn in `raba-gen`.

### Console not in UTF-8 mode

The VT has a UTF-8 mode flag. If it is off, multi-byte characters break.

> Mitigation: emit `ESC % G` on startup to force UTF-8 mode, and restore on exit.

### Console blanking mid-animation

The VT blanks after ten minutes by default. A boot that stalls leaves the user with a
black screen and no idea whether the machine is alive.

> Mitigation: disable blanking while drawing, restore on exit.

### Deferred firmware logo takeover

`CONFIG_FRAMEBUFFER_CONSOLE_DEFERRED_TAKEOVER` keeps the firmware logo on screen until
something writes to the console. The first thing RABA draws destroys it.

> Mitigation: worth knowing, probably worth leaving alone. Note it so a future "flickerless
> boot" issue is not investigated from scratch.

### The splash flashes for 200 ms

On a fast NVMe system the whole boot may be shorter than the animation. A splash that
appears and vanishes is worse than none.

> Mitigation: `ShowDelay` already covers it, but the default needs to be chosen by
> measuring real boots, not guessed.

### Escape to show boot messages

Users expect pressing Esc to reveal what is happening — Plymouth does this. Implementing
it means RABA holds the keyboard, and holding it wrongly can leak keystrokes to whatever
runs next.

> Mitigation: defer. If implemented, grab and restore keyboard mode with the same rigour
> as the VT, and test the restore path under `SIGKILL`.

### Logs vanish on reboot

`/run` is a tmpfs. After a failed boot, `/run/raba.log` is gone — exactly when it is
needed.

> Mitigation: a post-boot unit copies it to `/var/log/raba/last-boot.log`. Cheap, and it
> is the difference between a usable bug report and "it didn't work".

### Config forward compatibility

Config is baked into the initramfs (v2+). An upgrade that renames a key leaves a stale
copy that silently does something different.

> Mitigation: a `Version=` key, unknown keys warn rather than fail, and `raba-setup --check`
> compares against what is embedded.

---

## 4. Non-technical risks

**Scope creep.** Requests for images, video, transparency, per-monitor themes, Wayland.
Each is reasonable and each dilutes "one thing well". The README's constraints section is
the defence — point at it.

**The premise being wrong.** If ASCII art at 8x16 cells on a modern display just looks
bad, no amount of engineering fixes it. `--preview` as item one of the implementation
order exists partly to find this out in week one rather than month six.

**Bus factor.** A boot-critical package with one maintainer is a liability for anyone who
installs it. Clear code, written-down contracts and a real test suite are what make the
project survivable by someone else — which is the subject of
[maintenance.md](maintenance.md).
