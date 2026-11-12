# Disk encryption

> Design note. Not implemented.

**Summary: handling passphrases is not RABA's job, and the design is better when it
doesn't.** This document explains how LUKS unlocking actually works, what integrating
would cost, and why the recommendation is to stay out of it.

---

## How unlocking works at boot

A LUKS header holds keyslots. A passphrase unlocks a keyslot, which releases the master
key, which sets up the dm-crypt mapping. Something in the initramfs has to collect that
passphrase. There are two completely different mechanisms, depending on your initramfs.

### Path A — busybox initramfs (mkinitcpio `encrypt` hook)

No protocol. The hook runs `cryptsetup` and `cryptsetup` reads from the console.

From `hooks/encrypt` in mkinitcpio:

```sh
if command -v plymouth >/dev/null 2>&1 && plymouth --ping 2>/dev/null; then
    plymouth ask-for-password \
        --prompt="A password is required to access the ${cryptname} volume" \
        --command="cryptsetup open --type luks --key-file=- ${resolved} ${cryptname}"
else
    echo "A password is required to access the ${cryptname} volume:"
    while ! eval cryptsetup open --type luks "${resolved}" "${cryptname}"; do
        sleep 2
    done
fi
```

Two things to notice.

**Plymouth is hardcoded by name.** The hook probes for a binary called `plymouth` and
pings it. There is no generic splash interface. RABA cannot obtain this integration
without either patching mkinitcpio upstream or shipping a binary pretending to be
Plymouth.

**`--command=` means Plymouth holds the secret.** Plymouth reads the keystrokes, builds
the passphrase, and pipes it into `cryptsetup` stdin via `--key-file=-`. The splash
program is in the key path.

### Path B — systemd initramfs (`sd-encrypt` hook, `systemd-cryptsetup`)

A real protocol, documented at [systemd.io/PASSWORD_AGENTS](https://systemd.io/PASSWORD_AGENTS/).

1. `systemd-cryptsetup@.service` needs a passphrase.
2. It writes `/run/systemd/ask-password/ask.XXXX`, an INI file with `Message=`, `PID=`,
   `Echo=`, `NotAfter=` and `Socket=`.
3. A **password agent** watches that directory, presents a prompt, and sends the answer
   back on the socket named in `Socket=`.
4. The requester deletes both files.

The default agent is `systemd-ask-password-console.service`, which runs
`systemd-tty-ask-password-agent --watch --console` and prompts on `/dev/console`. It is
enabled out of the box.

Plymouth's integration is `systemd-ask-password-plymouth.service`, which runs
`systemd-tty-ask-password-agent --watch --plymouth` — systemd's own agent, forwarding to
Plymouth.

Several agents may run at once; the first to answer wins.

### Also worth knowing

With `systemd-cryptenroll` and a TPM2, the common boot needs no passphrase at all. A
recovery passphrase still exists, so the prompt path must keep working — it just becomes
rare, which makes it *more* likely to be broken without anyone noticing.

---

## The actual problem RABA has

It is narrower than "RABA must handle passwords".

If RABA is drawing on a virtual terminal and `cryptsetup` prompts on the same terminal,
the two collide: the prompt is drawn over, or the animation is drawn over the prompt, and
the user is staring at ASCII art wondering why the machine will not boot.

Avoiding that collision and collecting the passphrase are **separable problems**. Only
the first one is RABA's.

---

## Three levels of integration

### Level 0 — stay out of the way

Put RABA's hook **after** `encrypt` in `HOOKS`:

```
HOOKS=(base udev autodetect modconf kms keyboard keymap consolefont block encrypt raba filesystems fsck)
```

The passphrase prompt happens first, on a clean console, exactly as it would with no
splash installed. RABA starts afterwards.

- No collision, because RABA is not running yet.
- No secret handling at all.
- No patch to anything.
- Boot sequence becomes: firmware → passphrase prompt → splash → display manager.

The cost is that there is no splash during the prompt. That is arguably correct — a
passphrase prompt is exactly the moment you want the machine to look like it is asking
you something, not playing an animation.

**This is the v1 design.**

> **Correction:** earlier drafts of `integration.md` said `raba` must come *before*
> `encrypt`. That was copied from Plymouth's requirements, and it is wrong for RABA.
> Plymouth goes first because Plymouth *provides* the prompt. RABA does not, so it goes
> after. Fixed.

### Level 1 — hide and resume, without touching the secret (systemd only)

RABA watches `/run/systemd/ask-password/` with inotify. When an `ask.*` file appears it
deactivates: restores the terminal and stops drawing. When the file disappears it resumes.

RABA **never opens the socket and never sees the passphrase.** It uses the protocol
purely as a notification that someone else is about to use the console.
`systemd-ask-password-console.service` does the prompting, as it does when no splash is
installed.

- The splash can start before `encrypt` and cover the whole boot.
- Still zero secret handling, so no new security surface.
- Roughly fifty lines: an inotify watch and the existing deactivate/reactivate paths.

The equivalent for Path A would need `raba-ctl deactivate` called before `cryptsetup`,
which means an upstream change to mkinitcpio's `encrypt` hook.

### Level 2 — RABA collects the passphrase

What Plymouth does. RABA draws a prompt overlay, reads keystrokes, and either replies on
the systemd socket or pipes into `cryptsetup --key-file=-`.

This is the one to be suspicious of.

---

## Why Level 2 is a different kind of program

It moves RABA into the key path for full disk encryption. The requirements change in kind,
not degree:

- **Lock the buffer.** `mlock(2)` so the passphrase cannot reach swap.
- **Zero it after use.** The `zeroize` crate, with a destructor that actually runs.
- **Fixed-capacity buffer from the start.** A Rust `String` that grows reallocates and
  leaves a copy of the old contents in freed memory. This is easy to get wrong and
  invisible when you do.
- **No core dumps.** `prctl(PR_SET_DUMPABLE, 0)`.
- **Never log it**, including at `Level=debug`, including on an error path.
- **Terminal echo must be off** and must be restored even on panic.

And the user-facing failure modes are nastier than a missing splash. Plymouth's are
documented: a wrong passphrase returns to the same screen with no error message, and
`ask-for-password` with redirected output has returned an empty password. The first is
merely bad; the second is the class of bug where a user cannot unlock their disk.

Set against that, the benefit is that the animation keeps playing while you type.

---

## Is it RABA's job?

No. Three reasons, in order of weight.

**1. The requirement does not actually ask for it.** The project's goal is to be
*compatible* with disk encryption. Level 0 satisfies that completely: encryption works,
boots work, and the splash does not interfere. "Compatible with" is not "provides the
prompt for".

**2. It is a second thing.** The stated philosophy is to do one thing well. Playing an
animation and brokering disk encryption secrets are two things, and the second one is
harder than the first.

**3. The security surface is disproportionate.** Every other failure mode in this project
is "no splash" or, at worst, "no boot" — bad, recoverable, visible. Mishandling a
passphrase is a different category, it is silent, and it compromises the thing the
encryption exists to protect. That is a poor trade for an animation that keeps playing
during a prompt.

---

## Plan

| | scope | secret handling | when |
|---|---|---|---|
| **Level 0** | hook after `encrypt` | none | v1 |
| **Level 1** | inotify hide/resume on systemd | none | v2 |
| **Level 2** | collect the passphrase | full | only with a real reason, and a security review |

Level 1 is where this should stop. It gets the splash covering the whole boot, including
the moment of the prompt, without RABA ever touching a key.

If Level 2 is ever revisited, the honest path is upstream: propose a generic splash
interface for mkinitcpio's `encrypt` hook rather than shipping a binary named `plymouth`.
The hook probing for one specific implementation by name is the actual bug, and fixing it
helps every future splash, not just this one.
