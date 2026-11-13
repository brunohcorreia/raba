# Starting as early as possible

> Implemented, and measured: the splash starts inside the initramfs and the display
> stays on its terminal across `switch_root` — 1.53s to 6.62s on Fedora, 3.98s onward on
> Arch, with no console in between. See [milestones.md](milestones.md), "Row 3,
> reopened", for the journal lines.
>
> Supersedes the "run after `switch_root`" proposal in [risks.md](risks.md) §1, which
> traded too much away.

The goal is for the splash to appear at the earliest moment the hardware can show it,
and to stay up continuously until something replaces it.

## What the floor actually is

Userspace cannot draw before there is a console to draw on. The sequence:

```
firmware logo
  │
kernel entry ─────────────── nothing can draw here
  │
framebuffer available ────── efifb / simpledrm, very early on UEFI
  │
initramfs unpacked
  │
init script runs hooks ───── ◄── the earliest RABA can draw
  │
root mounted, switch_root
  │
display manager
```

So the earliest practical point is "as soon as our hook runs". Getting there means:

- being in the initramfs, as early in the hook order as possible
- a framebuffer console that exists before the real GPU driver loads — on UEFI,
  `simpledrm` on the firmware framebuffer provides this, which is why Fedora defaults
  Plymouth to it
- a statically linked binary with no dynamic loader work to do

Below that floor you would need either a kernel patch — the in-kernel bootsplash series
was proposed and not merged — or the bootloader. Which leads to:

### The bootloader can cover the gap before us

GRUB can display a background image, and other bootloaders have equivalents. Setting that
image to the **first frame** of the animation means the screen shows the right thing from
the moment the bootloader draws, and RABA picks it up from there.

That is a genuinely earlier start than any userspace program can achieve, and it costs a
PNG and a documentation page. `raba-gen` can emit the first frame as an image for exactly
this purpose.

Entirely optional, and it degrades to "splash starts slightly later" if unused.

## What runs before us: PID 1

The question "can we run before PID 1" has a clean answer: **no, and nothing can.** PID 1
is by definition the first userspace process. Everything before it is kernel or firmware.

A normal boot has **two** PID 1s:

```
firmware ──► bootloader ──► kernel
                              │
                              └─► execs /init          ← PID 1, in the initramfs
                                    │
                                    ├─ runs hooks ──► raba          ← a child of PID 1
                                    │
                                    └─ switch_root ──► /sbin/init   ← PID 1, for real
                                                         │
                                                         └─► raba resumes
```

In a mkinitcpio initramfs, `/init` is a shell script with `#!/usr/bin/ash`, run by busybox
as PID 1. Our hook is a function *inside* that script, so RABA is spawned directly by PID
1. In a systemd initramfs, `/init` is systemd itself and RABA is a unit.

So RABA is always a child of PID 1, in both halves of the boot. There is no earlier
userspace position to occupy.

### The inits we have to work with

| PID 1 | used by |
|---|---|
| systemd | Arch, Fedora, Debian, Ubuntu, openSUSE, CachyOS |
| runit | Void, Artix |
| OpenRC (via `openrc-init` or sysvinit) | Gentoo, Alpine, Artix |
| s6 / s6-linux-init | Artix, Obarun |
| dinit | Chimera, Artix |
| SysVinit | Devuan, Slackware |
| busybox init | Alpine, embedded |
| GNU Shepherd | Guix |

What they share is the only thing we need: they start processes. RABA's design
deliberately requires nothing beyond that — no dependency ordering, no socket activation,
no cgroup semantics — which is what makes the list above a documentation problem rather
than an engineering one.

### The only way to draw earlier

Not userspace. Either a kernel patch — the in-kernel bootsplash series was proposed and
not merged — or the bootloader drawing the first frame as a background image, which
[boot-order.md](boot-order.md) covers.

## The `switch_root` problem

Starting early means spanning `switch_root`, where the initramfs hands off to the real
root. systemd is blunt about it:

> The switch-root operation will result in a killing spree of all running processes.

Plymouth survives via `KillMode=none`, `SendSIGKILL=no`, living outside its cgroup, and a
dedicated `plymouth-switch-root.service` that re-roots the daemon onto `/sysroot`. All of
it is systemd-specific, and it has a known upstream bug where `/dev` is not yet bind
mounted under `/sysroot` at that moment.

Copying that gives us a systemd-only solution to an agnostic requirement.

### What a survivor's `/run` actually is

Measured on a Debian 13 guest, 13 Nov 2026, and worth writing down because it looks
like a bug in RABA and is not one.

`switch_root` is `chroot` in **the calling process**. Every other process — including a
splash started from `/scripts/init-premount` — keeps the root directory it was started
with, which is the initramfs rootfs that has just been deleted. And initramfs-tools
hands `/run` to the new root with `mount --move`, so from that moment the survivor's own
`/run` is an empty directory in a filesystem nobody else can see.

Three consequences, all observed in one boot:

- **The control socket keeps working.** The listener holds the inode, and the inode went
  with the tmpfs, so a client in the real root reaches it at `/run/raba.sock`. This is
  why a second instance correctly declines.
- **The state file stops updating.** Every `save` after the move writes somewhere
  invisible. The file a later instance would read is whatever was written *last before*
  the move — in that boot, frame 2 at 1.522s, while `raba-ctl status` showed the splash
  still looping frames at 1.8s and beyond. A successor resuming from it restarts a few
  frames earlier than the truth, which is a blink, and the freshness check in
  `state::load` refuses it outright if the gap grows.
- **It looks exactly like a wedged process.** It is not. The test for "is it still
  drawing" is `raba-ctl status`, whose `frame:` advances, and not the mtime of a file.

Nothing to fix in the program: a process cannot climb out of a root directory it has
been left holding. What this changes is the diagnosis, and `docs/troubleshooting.md`
should not send anybody to the state file to answer "is it alive".

## Instead: be resumable, and stop caring whether we died

The state RABA needs to continue is tiny:

```
vt number, frame index, animation phase, start timestamp, theme path
```

Write it to `/run/raba.state` on every frame — it is a handful of bytes to a tmpfs, and
`/run` is carried across `switch_root` rather than destroyed.

Then **one code path handles both cases**:

```
start
  ├─ state file exists and is fresh ──► resume: adopt the VT, continue from that frame
  └─ no state file ───────────────────► fresh start: acquire a VT, draw frame 0
```

Now survival is an optimisation rather than a requirement:

- **If we survive the switch** — nothing happens, the loop continues. Best case, free.
- **If we are killed** — the next instance resumes on the same VT at the same frame. The
  screen holds the last frame in the meantime, because nothing cleared it. Visually this
  is a brief pause, not a flash of black.

The same mechanism absorbs every other way the process can die: an OOM kill, a crash, a
`SIGKILL` from an init system we did not anticipate. **Being killed stops being an
event.** For a program whose first requirement is never breaking a boot, that is worth
more than the clean survival path.

### What it needs

- a static binary, so an unlinked initramfs file keeps working from its existing mapping
- no open file descriptors into paths that vanish
- the theme copied to `/run` at startup, or re-read from the new root on resume
- state writes that are atomic and never block a frame
- a freshness check, so a stale state file from a previous boot is ignored

### Per init system

| init | survival | resume |
|---|---|---|
| systemd | `KillMode=none`, `SendSIGKILL=no` — try it | unit ordered `DefaultDependencies=no` |
| runit | stage 1 script | stage 2 service |
| OpenRC | `sysinit` runlevel | `boot` runlevel |
| s6 | `rc.init` | early service |
| dinit | `early-prepare` target | normal service |

Every row is "try to survive, resume if not". The resume path is identical everywhere,
which is what keeps this agnostic.

## Disk encryption, revisited

Starting before `encrypt` means RABA is running when `cryptsetup` prompts, so the
collision described in [luks.md](luks.md) is real again and has to be handled.

RABA still does not collect the passphrase. It only needs to **get out of the way**:

- **systemd initramfs** — watch `/run/systemd/ask-password/` with inotify. An `ask.*`
  file appears, RABA deactivates; it disappears, RABA resumes. The socket is never
  opened and the passphrase is never seen.
- **busybox initramfs** — no protocol exists. Detect a running `cryptsetup` by scanning
  `/proc` and deactivate while it lives. Crude, but the failure mode is benign: if the
  detection misses, the prompt is drawn over, which is ugly and still boots.

Both are strictly "hide and resume". Neither puts RABA in the key path.

The honest fix is upstream: mkinitcpio's `encrypt` hook probes for a binary literally
named `plymouth`. A generic splash interface there would help every future splash. Worth
proposing; not worth waiting for.

For anyone who prefers certainty over an early splash, placing the hook after `encrypt`
remains supported and documented.

## Revised staging

- **v1** — initramfs hook, resumable state, fresh start and resume on the same path.
  Hook placed after `encrypt` by default; earlier placement documented and supported.
- **v2** — LUKS hide-and-resume, so the default placement moves earlier.
- **v3** — bootloader first-frame image for a gapless start.

The resumable-state design belongs in v1. It is small, it is the thing that makes early
start safe, and retrofitting it later would touch every part of the main loop.
