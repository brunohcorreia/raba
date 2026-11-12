# Troubleshooting

## Your machine will not boot

Interrupt the bootloader and add this to the kernel command line:

```
raba.enable=0
```

In GRUB, press `e` at the menu, find the `linux` line, append it, press `Ctrl-X`. In
systemd-boot, press `e`. In Limine, press `e`. The change is not persistent, so a
reboot restores the previous behaviour.

With that set, RABA exits immediately and touches nothing.

Once you are booted, remove RABA from the initramfs:

```sh
sudo sed -i 's/ raba//' /etc/mkinitcpio.conf    # Arch; check the HOOKS line by hand
sudo mkinitcpio -P
```

Then please open an issue. A boot failure is the most serious bug this project can
have, and it is worth your time to report it. Include `/run/raba.log` if you can reach
it, your bootloader, init system, initramfs generator, and whether the disk is
encrypted.

---

## Nothing appears at boot

Work through these in order.

**Is RABA actually in the initramfs?**

```sh
sudo raba-setup --check
```

This is the single most common cause. Installing the package does not add the hook,
and editing the config does not rebuild the image.

**Is the hook in the right place?**

On Arch, `raba` must appear in `HOOKS` in `/etc/mkinitcpio.conf`, and it must come
*before* `encrypt` if you use disk encryption. Rebuild with `mkinitcpio -P`.

**Does the theme exist?**

```sh
raba --check
```

Reports a missing or unreadable theme without drawing anything.

**Is there a framebuffer console at all?**

```sh
ls /dev/fb0
zgrep -E 'CONFIG_(VT|FRAMEBUFFER_CONSOLE)=' /proc/config.gz
```

RABA draws text cells through the kernel console. With `nomodeset`, with `CONFIG_VT=n`,
or on a kernel built without the framebuffer console, there is nothing to draw on and
RABA exits silently by design.

**Is `ShowDelay` longer than your boot?**

If `ShowDelay=5` and the system is ready in three seconds, nothing is ever shown. Set
it to `0`.

**Is Plymouth also installed?**

Two splash programs will fight over the same terminal. Remove one.

---

## Changing the config does nothing

Expected, and the single most common complaint about this class of software.

RABA runs before your root filesystem is mounted, so it cannot read
`/etc/raba/raba.conf` at boot. It reads a copy baked into the initramfs when the image
was generated.

```sh
sudo raba-set-theme <name>     # edits the config and rebuilds
sudo raba-setup --check        # tells you whether the image is stale
```

Or rebuild by hand after editing: `mkinitcpio -P`, `dracut --force`, or
`update-initramfs -u`.

---

## Kernel messages are printed over the animation

RABA lowers the console log level when it starts, so this only affects messages emitted
before it runs. Add to your kernel command line, in this order:

```
quiet loglevel=3
```

The order matters — `quiet` must come first. For systemd's own status output, add
`systemd.show_status=false`.

---

## Characters show as blanks, question marks or wrong symbols

The Linux console can display only 256 or 512 distinct glyphs, a limit inherited from
VGA hardware. Characters outside the loaded console font are replaced, and the kernel
discards the original codepoint.

Safe: ASCII, CP437 block elements (`█ ▀ ▄ ░ ▒ ▓`), box-drawing characters.
Not safe: braille (`⠋⠙⠹`), the quadrant blocks (`▘▝▖▗`), arrows, emoji.

In between: accented letters and CP437's Greek and mathematics. These draw with the
kernel's built-in font and may disappear when `setfont` loads a replacement — which is
why art can look correct for the first second of boot and wrong afterwards. See
[glyphs.md](glyphs.md).

Check a theme:

```sh
raba-gen --check /usr/share/raba/themes/yours.raba
```

If you built the theme with `--allow-unsafe-glyphs`, this is why. Rebuild without it.

---

## The logo is tiny on a 4K display

The console font is typically 8x16 pixels, giving roughly 480x135 cells on a 4K screen.
A 20x20 logo is physically very small, and RABA cannot scale it — in text mode the
cell is the smallest unit.

Options, in increasing order of intrusiveness:

1. Use art drawn at a larger cell size.
2. Load a larger console font in the initramfs (`ter-132b` from terminus, 16x32, via
   the `consolefont` hook on Arch). This also changes your console after boot.

---

## The screen flickers or the art jumps mid-boot

The console resolution changes when the real GPU driver loads and replaces the firmware
framebuffer. RABA re-centres the canvas when it notices, so the art moves.

On UEFI, `simpledrm` on the firmware framebuffer avoids the transition. This is what
Fedora defaults to for Plymouth and the reasoning applies here.

---

## The animation is cut off before it finishes

Working as intended. `Outro.MaxDuration` caps the outro at 1.5 seconds by default so a
long animation cannot become a boot delay. Raise it if you want, understanding that you
are adding that much to your boot:

```ini
[Outro]
MaxDuration=3.0
```

---

## The splash stays on screen and hides the login prompt

`Handoff.Mode=retain` leaves the final frame on screen so a display manager starting
immediately produces no visible gap. If the display manager fails to start, that frame
sits over a login prompt you cannot see.

Switch to `Handoff.Mode=clear`, and fix the display manager — `systemctl status
ly@tty2.service`.

---

## The transition to the display manager is not seamless

Both programs must show the same art at the moment of handoff.

```ini
# /etc/raba/raba.conf
[Outro]
Theme=arch-outro
```

```lua
-- /etc/ly/config.lua
animation = "dur",
dur_file_path = "/usr/share/raba/themes/arch.dur",
```

The final frame of the outro must match the first frame of the file Ly plays.

If Ly rejects the file, check the format version. Ly accepts only `formatVersion` 7,
`colorFormat` of `"16"` or `"256"`, and `encoding` of `"utf-8"`. Version 8 is refused.
Build with `raba-gen --format-version 7`, which is the default.

---

## The disk encryption prompt is missing or garbled

Known limitation. RABA currently steps aside when a passphrase is needed and lets
`cryptsetup` prompt on the console directly, because getting this wrong means an
unbootable machine. A proper integration is planned via the systemd password agent
protocol.

If you see no prompt at all, boot with `raba.enable=0` and report it.

---

## My disk stopped unlocking automatically after installing RABA

Expected, and recoverable. Adding anything to the initramfs changes the TPM measurements
your key was sealed to.

Unlock with your recovery passphrase, then re-enroll:

```sh
sudo systemd-cryptenroll --wipe-slot=tpm2 /dev/sdXY
sudo systemd-cryptenroll --tpm2-device=auto --tpm2-pcrs=7+11 /dev/sdXY
```

A kernel update will do the same thing again. The durable fix is a signed PCR policy
rather than literal PCR values — see [secure-boot.md](secure-boot.md).

---

## Collecting information for a bug report

```sh
raba --check
raba-setup --check
cat /run/raba.log
lsinitcpio /boot/initramfs-linux.img | grep -i raba   # Arch
systemd-analyze blame | head -20
```

Add your distribution, init system, bootloader, initramfs generator, whether the disk
is encrypted, and the output of `cat /proc/cmdline`.
