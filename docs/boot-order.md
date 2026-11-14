# Where RABA goes in the boot sequence

## Kernel command line

Parameter **order does not matter** to the kernel, with one exception that matters to us.

### The exception: `quiet` before `loglevel=`

```
quiet loglevel=3        ← console log level 3
loglevel=3 quiet        ← console log level 4
```

`quiet` sets the console log level to 4. A `loglevel=` that comes **after** it wins; one
that comes before is overwritten. Last value wins, which is the general rule for
duplicate parameters.

This is the only ordering rule in the kernel command line that affects a splash.

### How much of `quiet` a splash needs

printk severity levels, lowest number is most severe:

```
0 EMERG   1 ALERT   2 CRIT   3 ERR   4 WARNING   5 NOTICE   6 INFO   7 DEBUG
```

`loglevel=N` prints messages **more severe than N** — so `loglevel=3` shows emerg, alert
and crit, and hides errors.

`quiet` sets the console log level to **4**, which still shows errors. Since 2018 that
value is configurable at kernel build time via `CONSOLE_LOGLEVEL_QUIET`, so a distribution
may have picked something stricter. Older kernels also let `quiet` overwrite an earlier
`loglevel=`, which is the origin of the ordering rule above.

**What RABA recommends on the command line: just `quiet`.**

Going to `loglevel=3` hides `KERN_ERR`, and errors emitted before the splash starts are
exactly the ones worth seeing — a failing disk, a firmware problem. The window the command
line governs is small, because RABA takes over as soon as it starts. Suppressing real
errors to tidy up a window of a few hundred milliseconds is a bad trade.

`loglevel=3` stays documented for people who want it.

### What RABA does while drawing: redirect, not suppress

Lowering the console log level is a blunt instrument — it is global policy, and anything
below the threshold is simply lost from the console.

The console has a better mechanism. `TIOCLINUX` subcode 11, `TIOCL_SETKMSGREDIRECT`,
**sends kernel messages to a different virtual terminal**. This is what `setlogcons` does.
Passing 0 restores the default, which is "whichever VT is active".

So while RABA draws, kernel messages go to a scratch terminal instead of over the art:

- nothing is suppressed — the messages are still printed, just elsewhere
- `dmesg` and the journal are untouched either way
- the user can switch VT and read them while the splash is still up
- it is a single ioctl to set and a single ioctl to restore
- global log level policy is never modified, so there is nothing to get wrong on exit

The setter returns the previous target, so restoring is exact rather than assumed.

This is strictly better than the `printk` sysctl approach here, and it is
what the implementation should use. Lowering the log level stays as a fallback for a
console where the ioctl is unavailable.

> **Tradeoff:** redirection moves *everything*, including a genuine
> emergency. A kernel panic takes the console regardless, so the true catastrophes still
> surface. For everything between, the escape valve is `Boot.Timeout`, which reveals the
> console rather than exiting — see [coexistence.md](coexistence.md).

### A conventional line

```
root=UUID=… rw cryptdevice=UUID=…:root quiet systemd.show_status=false vt.global_cursor_default=0
```

| parameter | effect |
|---|---|
| `quiet` | console log level 4; errors and worse still shown |
| `loglevel=3` | stricter: hides errors too. Optional |
| `systemd.show_status=false` | suppress systemd's unit status lines |
| `vt.global_cursor_default=0` | hide the blinking cursor |
| `raba.enable=0` | the kill switch |

`raba.enable=0` follows the `plymouth.enable=0` convention. The kernel does not
recognise it, so it is passed through to userspace; RABA reads `/proc/cmdline` directly.

RABA redirects kernel messages away from its terminal at startup and restores on exit, so
`quiet` only matters for messages printed *before* RABA runs. Nothing is lost either way —
`dmesg` and the journal still have everything.

---

## mkinitcpio: the part people get wrong

The `HOOKS` array mixes two different kinds of thing, and conflating them is why forum
advice about ordering contradicts itself.

### Most hooks are build-time only

Of the hooks in a typical array, only a few have any runtime behaviour at all:

| runtime behaviour | build-time only |
|---|---|
| `udev`, `keymap`, `consolefont`, `encrypt`, `resume`, `usr` | `base`, `autodetect`, `microcode`, `modconf`, `kms`, `keyboard`, `block`, `filesystems`, `fsck` |

For the build-time ones, position in the array affects what goes *into* the image, not
when anything runs. `autodetect` is the notable one: it narrows the module set, so
everything after it is filtered.

### Runtime hooks run in phases, not purely in array order

From mkinitcpio's `init`:

```
run_earlyhook   ← $EARLYHOOKS    (only udev uses this)
modprobe $MODULES                ← DRM drivers load here
run_hook        ← $HOOKS         (encrypt, keymap, consolefont, resume)
break=premount
resolve root, fsck, mount /sysroot
run_latehook    ← $LATEHOOKS     (usr)
run_cleanuphook ← $CLEANUPHOOKS  (reverse order)
switch_root
```

**A `run_earlyhook` runs before every `run_hook`, regardless of array position.** Array
order only decides sequence *within* a phase. This is why two forum posts can both be
right about where a hook "has to" go.

`encrypt` defines only `run_hook`.

### Why RABA is a `run_hook`, not a `run_earlyhook`

Tempting, since early is the goal. But `run_earlyhook` fires **before** `modprobe
$MODULES`, which is where the `kms` hook's DRM drivers load. At that point there may be
no framebuffer console at all, or only a low-resolution firmware one that changes
underneath us moments later.

Starting in the `run_hook` phase, as early as possible within it, is the real sweet spot:
drivers are up, the console is at its final resolution, and almost nothing else has run.

### The two supported placements

**Earliest — before `encrypt`:**

```
HOOKS=(base udev autodetect microcode modconf kms keyboard keymap consolefont raba block encrypt filesystems fsck)
```

RABA starts as soon as the console is usable. Requires the hide-while-prompting
behaviour from [luks.md](luks.md), because `cryptsetup` will want the console afterwards.

**Safest — after `encrypt`:**

```
HOOKS=(base udev autodetect microcode modconf kms keyboard keymap consolefont block encrypt raba filesystems fsck)
```

The passphrase prompt happens on a clean console, then RABA starts. No coordination
needed, nothing to get wrong. Costs you the splash during the prompt.

**Without encryption the two are identical**, so most people never have to choose.

### What must come before RABA either way

| hook | why |
|---|---|
| `kms` | the DRM driver, so the console is at final resolution |
| `consolefont` | a larger font must be loaded before the terminal is measured |
| `keyboard`, `keymap` | needed by `encrypt`; harmless but conventional to keep together |

`consolefont` is the one people miss. It is also the fix for the tiny-logo-on-4K problem
— load a 16x32 font there and the art doubles in size.

---

## Not replacing the `encrypt` hook

Plymouth once shipped `plymouth-encrypt` and `sd-plymouth`, replacement hooks that
wrapped encryption. **Both are now deprecated**, with upstream telling users to go back to
the plain `encrypt` and `plymouth` hooks.

That experiment has been run. A forked copy of another project's hook rots, diverges, and
breaks when upstream changes. RABA will not ship one.

The same reasoning rules out the other shortcut available to us. mkinitcpio's `encrypt`
hook probes for a binary literally named `plymouth`:

```sh
if command -v plymouth >/dev/null 2>&1 && plymouth --ping 2>/dev/null; then
```

We could install something called `plymouth` and inherit that integration for free. We
will not. Impersonating another project to capture its integration points is the kind of
thing that gets a package removed from a repository, and deservedly.

The legitimate path is to propose a generic splash interface upstream, so the hook stops
hardcoding one implementation. That helps every future splash. It is also slow, which is
why [luks.md](luks.md) does not depend on it.

---

## Bootloaders

RABA never interacts with a bootloader. By the time it exists, the bootloader's job is
finished. "Supporting" a bootloader means two things only: knowing where its kernel command
line lives, and optionally handing it a first-frame image.

| bootloader | command line | then |
|---|---|---|
| GRUB | `/etc/default/grub`, `GRUB_CMDLINE_LINUX_DEFAULT` | `grub-mkconfig -o /boot/grub/grub.cfg` |
| systemd-boot | `/boot/loader/entries/*.conf`, `options` | — |
| Limine | `/boot/limine.conf`, `cmdline` | — |
| rEFInd | `/boot/refind_linux.conf` | — |
| syslinux | `/boot/syslinux/syslinux.cfg`, `APPEND` | — |
| EFISTUB | `efibootmgr -u` arguments | — |
| UKI | `/etc/kernel/cmdline` | rebuild the image |

### Three things that are easy to get wrong

**Multiple entries.** GRUB and systemd-boot generate several — default, fallback, LTS
kernel, recovery. `raba-setup` edits the **default entry only** and says so.

The fallback entry should deliberately keep its messages. It exists so you can see what is
wrong, and a splash is the opposite of that. For the same reason RABA does not install
itself into a fallback initramfs: a splash-free image you can always boot is a recovery
path that costs nothing to preserve.

**`GRUB_GFXPAYLOAD_LINUX`.** Set to `keep`, the kernel inherits GRUB's graphics mode, so
the console comes up at GRUB's resolution. Unset, the mode changes during handover. This
directly determines what resolution RABA measures and whether the screen flickers
underneath it — a non-obvious interaction worth checking first when art appears at the
wrong size.

**The firmware logo.** UEFI publishes the OEM logo through the BGRT ACPI table, and
`CONFIG_FRAMEBUFFER_CONSOLE_DEFERRED_TAKEOVER` keeps it on screen until something writes
to the console. The first thing RABA draws destroys it. Plymouth has
`UseFirmwareBackground` for people who care; in text mode it cannot be preserved. Documented
so a future "my vendor logo disappears" report is not investigated from scratch.

### Covering the gap before us

A bootloader that can display a background image can display the **first frame** of the
animation, so the screen is already correct from the moment the bootloader draws. That is
genuinely earlier than any userspace program can manage.

`raba-gen` can emit the first frame as an image for this. Entirely optional.

## Other initramfs generators

**dracut** — modules declare dependencies rather than relying on array order, via
`depends()` in `module-setup.sh`. Ours declares a dependency on the kms/drm module and
orders before `crypt`.

Dracut matters more than its position in this list suggests. **Void, Gentoo and Chimera
use dracut and do not use systemd**, so "non-systemd" and "which initramfs generator" are
independent axes. Treating dracut as a later concern because it is associated with Fedora
would leave out most of the non-systemd distributions this project claims to serve.
Dracut belongs in v1 alongside mkinitcpio.

**initramfs-tools** — scripts live in `scripts/init-premount/` or `init-top/`, with
`PREREQ=` declaring ordering. `init-top` is the equivalent of early, `init-premount` of
the main phase.

**booster, mkosi, UKI** — later. Note for UKI that the kernel command line is embedded in
the image, so `raba-setup` has to edit `/etc/kernel/cmdline` and trigger a rebuild rather
than touching a bootloader.
