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

With a systemd-based initramfs, use `sd-raba` instead of `raba`.

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

Any change to `/etc/raba/raba.conf` requires regenerating the image. `raba-set-theme`
does both.

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

Three units, modelled on Plymouth's:

| unit | role |
|---|---|
| `raba-start.service` | starts RABA, if the initramfs did not |
| `raba-quit.service` | sends `quit` once the system is up |
| `raba-quit-wait.service` | blocks until RABA has exited |

```sh
sudo systemctl enable raba-start.service raba-quit.service raba-quit-wait.service
```

### OpenRC, runit, s6, dinit

Service scripts ship in `/usr/share/raba/init/`. Copy the one for your system and
enable it the usual way. These start RABA outside the initramfs, so the splash appears
slightly later — after the root filesystem is mounted rather than before.

---

## Handing off

RABA knows nothing about display managers. It draws until told to stop, then restores the
terminal and exits. The entire interface is one command:

```sh
raba-ctl quit --wait
```

Three ways to arrange for it to be called, in order of preference.

### 1. Let the packaged unit do it

`raba-quit.service` is ordered before `display-manager.service` and fires on
`multi-user.target`. For most systemd setups this is all you need, and it works the same
whether you run GDM, SDDM, LightDM, greetd, Ly or no display manager at all.

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

`Boot.Timeout` releases the terminal on its own. The splash is up for a fixed time and
then gets out of the way. Less precise, no configuration, never stuck.

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
