# Coexistence

> What happens when RABA is not the only thing that wants the screen.

## Another splash is running

### What actually goes wrong

Two splash programs on one console is not merely untidy. The documented failure is worse:
if one of them leaves a terminal in `KD_GRAPHICS`, a later `VT_ACTIVATE` fails silently
and `VT_WAITACTIVE` **hangs**. That is a stalled boot, caused by cosmetics, which is the
exact outcome this project exists to avoid.

Plymouth has caused this. We can cause it for Plymouth just as easily.

### The generic check: is the terminal already in graphics mode

Before any list of known programs, there is a check that catches everything — including
splashes nobody has written yet.

If the terminal RABA would use is already in `KD_GRAPHICS`, **something owns it.** We do
not need to know what. Yield.

This is the primary check because a blocklist of competitors is always out of date, and
because the specific failure we fear — a hung `VT_WAITACTIVE` against a terminal stuck in
graphics mode — is exactly what this detects.

Named checks (`/run/plymouth.pid`, a live `plymouthd`) exist on top, purely to produce a
better message than "the terminal is busy".

### Two checks, at two different times

**At initramfs build time — fail loudly.** If our hook sees `plymouth` in the same `HOOKS`
array, or a Plymouth dracut module enabled alongside ours, it refuses and explains. A
build-time error the user reads at their keyboard is infinitely better than a boot-time
hang they meet at a black screen.

**At runtime — exit quietly.** Before touching a terminal, check for `/run/plymouth.pid`
and for a live `plymouthd`. If one is there, log the reason and exit 0. No drawing, no
terminal acquisition, no contention.

Runtime detection is the backstop, not the plan. A configuration that reaches runtime
detection is one the build check should have caught.

### Put the check in the binary, not in the service files

Tempting to express this per init system — a systemd `Condition`, an OpenRC `start_pre`,
a guard at the top of a runit `run` script. That is six implementations of one rule, five
of which will drift.

**The check lives in `raba` itself.** It runs before the terminal is touched, on every
start, regardless of who started it. Every init system gets it for free and none of them
need to know it exists.

systemd units additionally carry `ConditionPathExists=!/run/plymouth.pid`, because a
skipped unit shows up usefully in `systemctl status` as a condition failure rather than as
a service that ran and did nothing. That is a diagnostic nicety layered on top of the real
check, not a substitute for it.

### Not `conflicts=('plymouth')`

Covered in [packaging.md](packaging.md), repeated because it is tempting: the packages
share no files and conflict only when both are *enabled*. Forcing removal of a working
splash to install ours is rude and technically wrong. Detect, refuse, explain.

### What if we start first and Plymouth starts after?

Plymouth does not check for us. It will draw over RABA.

Mitigation is cheap: take the terminal with `VT_PROCESS` so we are signalled on a VT
switch away, and treat losing the terminal as a reason to stop drawing rather than to
fight for it. **Whoever is second wins, and RABA yields.** A splash is not worth
contending over.

### Older splashes

`fbsplash` / `splashutils` is effectively dead but still installable on Gentoo. Same
treatment: detect, yield, exit 0.

---

## Enabling RABA while another splash is enabled

What should happen when someone runs `systemctl enable raba.service` on a machine where
Plymouth is set up.

### What we cannot do

There is no hook on `systemctl enable`. It creates a symlink; nothing gets to intervene.
Any design that depends on intercepting it does not work.

### What we will not do

**Disable the other program.** Running `systemctl disable plymouth-start.service` on
someone's behalf is hostile, and it is the kind of thing that earns a package a
reputation. Their splash works today. We are the newcomer.

**Disable ourselves.** A unit that runs `systemctl disable` on itself is surprising
behaviour that leaves the user with a disabled service they explicitly enabled and no
clear reason why. It also fights the user if they genuinely intend to remove Plymouth
next.

### What we do

| moment | behaviour |
|---|---|
| package install | the `.install` message states the conflict if Plymouth is present |
| `raba-setup` | detects it, refuses to proceed, prints what to disable |
| initramfs build | our hook sees `plymouth` in `HOOKS` and fails the hook loudly |
| `systemctl enable` | succeeds — we cannot intercept it |
| boot | the condition is skipped; `raba` itself detects and exits 0 |
| `systemctl status raba` | shows the condition failure, which is the diagnosis |

The user stays in control, nothing is silently changed on their system, and the splash
simply does not appear until they resolve it. `raba-setup --check` and
`systemctl status raba` both say why.

The important property: **enabling RABA on a Plymouth machine is harmless.** It does not
fight, does not break Plymouth, and does not break the boot. It just does nothing, and
says so when asked.

### The message

Worth getting right, since it is the only thing most people will read:

```
RABA will not start: Plymouth is active on this system.

Two splash programs contending for the same terminal can hang a boot, so
RABA stays out of the way.

To use RABA instead:
    sudo systemctl disable plymouth-start.service plymouth-quit.service
    sudo sed -i 's/ plymouth//' /etc/mkinitcpio.conf   # check the HOOKS line
    sudo raba-setup --apply

To keep Plymouth, no action is needed. RABA will remain inactive.
```

Name the commands, say what happens if they do nothing, and do not editorialise about
which is better.

---

## Something else wants our terminal

### getty

If RABA holds tty1 and `getty@tty1` starts, they fight over the same device.

The fix is not to hardcode a terminal. Acquire a free one with `VT_OPENQRY`, the way
psplash does, activate it, and switch back on exit. A terminal nothing else claims cannot
be contended.

### kmscon

kmscon replaces the kernel console with a userspace terminal on a given VT. If it owns the
VT we picked, our escape codes go somewhere unexpected. `VT_OPENQRY` avoids this too, for
the same reason.

### A display manager or X starting on our VT

Same category, same answer: yield. `raba-ctl deactivate` exists precisely so whatever comes
next can say "I want the screen now" before taking it.

### Another RABA

The initramfs starts one; a service starts another. Both draw; the screen is garbage.

A pidfile plus an abstract-namespace socket lock settles it: the second instance sees the
first, and — per [early-boot.md](early-boot.md) — either exits or *resumes* it. Resumption
is the same code path used after a `switch_root` kill, so this costs nothing extra.

---

## When RABA should not draw at all

Drawing is wrong in more situations than "something else is drawing". RABA should check
the kernel command line and stay silent when the user is plainly trying to see what the
machine is doing:

| on the command line | why |
|---|---|
| `raba.enable=0` | the explicit kill switch |
| `rd.break`, `break=` | the user asked for a shell in the initramfs |
| `emergency`, `rescue`, `single`, `S`, `1` | recovery modes |
| `systemd.unit=emergency.target`, `rescue.target` | same |
| `init=/bin/sh`, `init=/bin/bash` | debugging |
| `nomodeset` | likely no usable console |
| `console=ttyS*` with no video console | serial only |

The principle: **a command line that says "I am debugging" means the splash is not
wanted.** Drawing over a rescue shell is how a splash earns a reputation for getting in
the way.

### Serial consoles specifically

Painting a full-canvas animation over a 115200 baud serial line is slow, ugly, and makes
the console unusable for its actual purpose. Detect a serial-only console and do nothing.
On a mixed setup, draw to video and leave the serial line alone.

Headless servers are the same case and the same answer.

---

## Revealing the console instead of exiting

`Boot.Timeout` was originally specified as "give up and exit". That is the wrong behaviour.

If a boot stalls, the user needs to see **why**. Exiting restores the terminal but the
messages that explain the stall were redirected away while RABA was drawing, so the screen
is simply blank and the user still knows nothing.

> **On timeout, RABA deactivates: it restores the kernel message redirection, releases the
> terminal, and stops drawing — without exiting.** The console becomes live again and boot
> messages appear from that point on.

Exit still happens when told to, or when the handoff completes. The timeout is not a
shutdown, it is a surrender.

As implemented in M3, "redirected away" means to **the terminal the user came from** —
not to nowhere. So the messages that explain a stall are not lost while the splash is up;
they are already on the terminal the display returns to. That is deliberate: it is also
what makes the case RABA cannot clean up after — being killed outright — leave the user
somewhere useful rather than somewhere silent. See
[milestones.md](milestones.md#where-the-kernel-prints).

This matters for an environment that is easy to overlook: a root filesystem over NFS or
iSCSI, or a large degraded RAID, can legitimately take minutes. A 30-second timeout that
exits would have killed the splash of a perfectly healthy boot. A timeout that reveals the
console is correct in both cases — the slow-but-fine boot and the genuinely stuck one.

Given that, the default should be generous. Thirty seconds is a guess; the right value
comes from measuring real boots, including slow ones.

---

## Hibernation

The `resume` hook runs in the initramfs, so RABA starts on a resume from hibernation too.

Two things do not carry over:

- **The outro makes no sense.** Resume has no display manager handoff; the restored image
  takes over the machine. The outro should be skipped on a resume path.
- **Resume is fast.** Often faster than `ShowDelay`, so frequently nothing is drawn at
  all, which is fine.

Detect a resume by the presence of `resume=` handling in the initramfs rather than
guessing, and treat it as "loop until replaced, never play the outro".
