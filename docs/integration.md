# Integration

RABA does not modify your bootloader or your initramfs configuration. Those files
belong to other packages, and a bug in a script that edits them produces a machine that
will not boot.

`raba-setup` shows you what to change. With `--apply` it makes the changes it can make
safely. Everything below can also be done by hand.

```sh
raba-setup            # dry run, the default
raba-setup --apply
raba-setup --check    # is the initramfs current?
```

---

## Initramfs

RABA must be inside the initramfs to appear before the root filesystem is mounted. This
is required if you use disk encryption.

### mkinitcpio (Arch, CachyOS, Manjaro, EndeavourOS, Artix)

Add `raba` to `HOOKS` in `/etc/mkinitcpio.conf`:

```
HOOKS=(base udev autodetect modconf kms keyboard keymap consolefont block encrypt raba filesystems fsck)
```

Order matters. If you use disk encryption, `raba` must come **after** `encrypt`.

RABA does not provide the passphrase prompt — `cryptsetup` does, on the console. Placing
RABA after `encrypt` means it is not yet running when the prompt appears, so the two
cannot collide. You get the prompt on a clean console, then the splash.

(Plymouth requires the opposite order because Plymouth *supplies* the prompt. See
[luks.md](luks.md) for why RABA deliberately does not.)

```sh
sudo mkinitcpio -P
```

One hook name for both kinds of initramfs. There is no `sd-raba`: the hook
notices whether it is building a systemd image and adds a unit or a runtime
script accordingly. Plymouth shipped a separate `sd-plymouth` and deprecated it.

The hook puts a copy of your configuration inside the image with the theme path
already resolved, because `Theme=auto` needs `os-release`(5) and an initramfs
has none. `raba-setup --freeze` is what writes it, and `raba --check` tells you
what it will say.

### dracut (Fedora, RHEL, openSUSE)

The package installs a module at `/usr/lib/dracut/modules.d/50raba/`.

```sh
sudo dracut --force
```

### initramfs-tools (Debian, Ubuntu)

```sh
echo 'FRAMEBUFFER=y' | sudo tee /etc/initramfs-tools/conf.d/raba
sudo update-initramfs -u
```

### Remember

Any change to `/etc/raba/raba.conf` requires regenerating the image.
`raba-setup --set-theme <name>` does both.

---

## Kernel command line

Optional but recommended:

```
quiet loglevel=3
```

`quiet` must come first. This suppresses kernel messages that would print over the
animation.

RABA lowers the console log level itself when it starts, so this only affects messages
emitted *before* it runs. Unlike Plymouth, RABA does not need a `splash` parameter to
appear at all.

For systemd's status output, add `systemd.show_status=false`.

### Where to put it

| bootloader | file | then |
|---|---|---|
| GRUB | `/etc/default/grub`, `GRUB_CMDLINE_LINUX_DEFAULT` | `grub-mkconfig -o /boot/grub/grub.cfg` |
| systemd-boot | `/boot/loader/entries/*.conf`, `options` line | — |
| Limine | `/boot/limine.conf`, `cmdline` | — |
| rEFInd | `/boot/refind_linux.conf` | — |
| UKI | `/etc/kernel/cmdline` | rebuild the image |

---

## Init systems

### systemd

Four units, modelled on Plymouth's:

| unit | role |
|---|---|
| `raba-initrd.service` | the splash inside a systemd initramfs; the image hooks install it, you never enable it |
| `raba-start.service` | the splash on the real root, continuing what the initramfs started |
| `raba-quit.service` | sends `quit` once the system is up |
| `raba-quit-wait.service` | blocks until RABA has exited |

```sh
sudo systemctl enable raba-start.service raba-quit.service raba-quit-wait.service
```

The first two are the same program under two unit names, and the second name is not
cosmetic. systemd carries unit state across `switch_root`: a unit that was active in
the initramfs is still active in the real root, its start job is already satisfied, and
nothing ever starts it again. With one name for both, an Arch guest measured a splash
from 1.35s to 3.98s and then six seconds of boot log before the login prompt — the real
root's copy never ran, because as far as systemd was concerned it already had.

What happens instead: `switch_root` sends `SIGTERM`, the initramfs instance writes the
terminal and the frame to `/run/raba.state` and leaves the art on the screen rather than
handing the console back, and `raba-start.service` — a unit this system has not seen
before — starts, adopts that terminal and carries on from that frame. The screen never
changes hands. See `docs/early-boot.md`.

### OpenRC, runit, s6, dinit

Service files ship in `/usr/share/raba/init/`. Copy the one for your system and enable
it the usual way. These start RABA outside the initramfs, so the splash appears slightly
later — after the root filesystem is mounted rather than before. Running both is fine:
the second instance finds the first on the control socket and declines.

| init | install as | the thing to watch |
|---|---|---|
| OpenRC | `/etc/init.d/raba` | `stop_pre` hands off with `raba-ctl quit --wait` |
| runit | `/etc/sv/raba/{run,finish}` | `finish` takes the service down; runsv would otherwise restart a splash that finished |
| s6 | the `run` script of an s6-rc longrun | same problem, s6's spelling: mark it `down-when-finished` |
| dinit | `/etc/dinit.d/raba` | `restart = false`, because exiting is how this succeeds |

The supervision systems all share one trap: a boot splash *exits on purpose*, and a
supervisor's job is to restart things that exit. Left alone, runit and s6 bring the
splash straight back up over whatever took the screen.

---

## Handing off

RABA knows nothing about display managers. It draws until told to stop, then restores the
terminal and exits. The entire interface is one command:

```sh
raba-ctl quit --wait
```

Three ways to arrange for it to be called, in order of preference.

### 1. Let the packaged unit do it

`raba-quit.service` fires on `multi-user.target`, ordered *after*
`display-manager.service`. For most systemd setups this is all you need, and it works the
same whether you run GDM, SDDM, LightDM, greetd, Ly or no display manager at all.

After, rather than before, so that the machine with a display manager gets route 3 by
default: the display manager takes its terminal, RABA sees the display move and exits
without restoring anything, and the screen goes from the animation to the greeter with
no console in between. By the time this unit runs there is usually nothing left to quit
and `raba-ctl` exits 0. On a machine with no display manager the ordering names a unit
that does not exist, systemd ignores it, and this is what ends the splash.

Under all of it is `Boot.Timeout` in `raba.conf` — the console comes back after thirty
seconds whatever else does or does not happen.

### 2. Tie it to a specific display manager

For a tighter transition, deactivate before it takes the screen and quit after:

```sh
sudo systemctl edit display-manager.service
```

```ini
[Service]
ExecStartPre=/usr/bin/raba-ctl deactivate
ExecStartPost=/usr/bin/raba-ctl quit --wait
```

The same applies to a per-tty unit — `ly@tty2.service`, say — or to the start of any
display manager's service script on a non-systemd init.

`raba-ctl` exits 0 when no instance is running, so none of this breaks when RABA is
absent or disabled.

### 3. Do nothing

RABA watches the screen it was given. When the display moves to another terminal, or
when the terminal it is on goes into graphics mode, it stops drawing and exits — without
switching anything back, because switching back would take the screen from whatever just
took it.

That covers the case this section is really about: **no display manager at all.** A
compositor started from an autologin on tty1 takes a virtual terminal, and the splash
sees it happen. Nothing to configure, nothing to call, and nothing specific to systemd,
Wayland or Hyprland in it.

`Boot.Timeout` is the backstop under that, not the mechanism: if nothing ever takes the
screen, the splash gets out of the way anyway. Set `Timeout=0` only if you are sure
something will.

### Booting straight into a compositor

```ini
# /etc/raba/raba.conf
[Boot]
Timeout=60      # generous: the compositor is what really ends this
```

Then nothing else. Do **not** enable `raba-quit.service` on this kind of machine — it
fires on `multi-user.target`, which is reached long before a user session has drawn
anything, and the console would show in between.

If you want the handoff at an exact instant rather than when the terminal changes hands,
the compositor can ask for it — but `raba-ctl` talks to a root-only socket, so it has to
be root that runs it. A `systemd` system unit ordered after your session is the usual
way; a line in `hyprland.conf` is not, because that runs as you.

> Why the socket is not opened up to the logged-in user: `message` draws attacker-chosen
> text on a console as root, and a convincing fake `login:` prompt is a credential
> harvester. See [security.md](security.md). The kernel already says when the screen
> changed hands, so there is nothing to buy with that risk.

### Booting to a text console

No display manager is a supported configuration. `raba-quit.service` fires on
`multi-user.target` either way, and getty takes the terminal back.

### A seamless transition

If whatever comes next can draw the same art RABA finishes on, the handoff is invisible.
This is a property of the art, not of any integration: make the last frame of the outro
match the first thing the next program shows.

Some display managers can display an animation of their own — Ly, for instance, plays
`.dur` files, which `raba-gen` can also emit. That is a nice-to-have, not a requirement,
and nothing about RABA's design depends on it.

---

## Disk encryption

RABA does not collect your passphrase and does not want to. `cryptsetup` prompts, as it
would with no splash installed.

Place `raba` **after** `encrypt` in `HOOKS` and everything works: prompt first on a clean
console, splash afterwards.

A planned improvement will let RABA start earlier and simply hide itself while a prompt is
pending — still without ever touching the passphrase. See [luks.md](luks.md) for how LUKS
unlocking actually works and why this boundary is where it is.

---

## Conflicts

Do not run RABA and Plymouth together. Two programs contending for the same virtual
terminal produce a black screen or a hang. The package declares a conflict.

```sh
sudo pacman -Rns plymouth
```

---

## Verifying

```sh
raba-setup --check
raba --check
lsinitcpio /boot/initramfs-linux.img | grep raba    # Arch
lsinitrd | grep raba                                # dracut
lsinitramfs /boot/initrd.img-$(uname -r) | grep raba
```

Then test in a virtual machine before rebooting the machine you care about. See
[CONTRIBUTING.md](../CONTRIBUTING.md#testing).
