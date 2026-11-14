# Secure Boot, measured boot and TPM

> The practical consequence first: **installing RABA can break TPM-based automatic disk
> unlocking.** If that applies to you, read §3 before installing.

---

## 1. The Secure Boot chain, and where the initramfs sits

```
UEFI firmware  ── verifies ──►  shim        (signed by Microsoft)
shim           ── verifies ──►  GRUB        (signed by the distribution)
GRUB           ── verifies ──►  vmlinuz     (signed)
GRUB           ── loads    ──►  initramfs   ← NOT verified
```

That last line is a long-standing gap, not an oversight on anyone's part recently.
GRUB checks the kernel's signature and does not check the initrd's. `/boot` is typically
unencrypted, so the initramfs can be unpacked, modified and repacked without breaking any
signature — the attack behind CVE-2016-4484 and its relatives. The signed kernel stays
untouched, so most checks miss it entirely.

### What this means for RABA

**In a classic Secure Boot setup, the RABA binary and its theme live in the unverified
zone.** This is worth being clear about rather than glossing over:

- RABA does not *create* the gap. Everything in your initramfs is already unverified, and
  an attacker who can modify it does not need RABA to be there — they can add their own
  hook.
- RABA does *widen* the surface slightly. It is one more root-privileged binary running
  early, with a parser and a socket. That is the cost, and it is why
  [security.md](security.md) treats the parser and socket as seriously as it does.
- Signing the theme ourselves would be theatre. An attacker who can replace the theme can
  replace the binary that checks it.

### Unified Kernel Images close the gap

A UKI is a single PE binary containing the EFI stub, kernel, initramfs and command line,
signed as one unit. The initramfs is inside the signature, so tampering breaks it.

**With a UKI, RABA is covered by the existing signature chain.** Nothing extra to do, no
scheme to invent, and the configuration to recommend to anyone who cares.

---

## 2. Measured boot: which PCRs RABA disturbs

Separate from verification, the TPM *measures* boot components into Platform
Configuration Registers. Those measurements can seal a LUKS key, which is how automatic
unlocking works.

| PCR | covers | does RABA change it? |
|---|---|---|
| 0–3 | firmware | no |
| 4 | bootloader code | no |
| 7 | Secure Boot state: keys, db, dbx | **no** |
| 8 | kernel command line (GRUB) | **yes, if you add parameters** |
| 9 | initramfs and command line (GRUB) | **yes** |
| 11 | UKI components (measured by systemd-stub) | **yes** |
| 12 | command line, credentials | possibly |
| 14 | shim MOK | no |

RABA disturbs measurements in two independent ways:

1. **Adding the binary and theme to the initramfs** changes PCR 9, and PCR 11 for a UKI.
2. **Adding kernel parameters** — `quiet loglevel=3` and friends — changes PCR 8 and 12,
   and with a UKI the command line is embedded, so rebuilding it changes PCR 11 too.

Either one is enough to break a TPM-sealed key.

---

## 3. The failure this causes

If your disk unlocks automatically via `systemd-cryptenroll --tpm2-device=auto`, and the
enrollment is bound to a PCR that RABA changes, then after installing RABA:

> The machine boots, the TPM refuses to release the key because the measurements no longer
> match, and you are asked for your recovery passphrase instead.

This is not hypothetical and it is not specific to us. The Arch Wiki documents the same
thing happening when **mkinitcpio 42 added a service to the initramfs**: PCR values
shifted and users had to re-enroll. Any change to initramfs contents does this.

### Does it apply to you?

It depends on which PCRs your enrollment is bound to, and sources disagree about the
default — some say PCR 7, others PCR 11.

```sh
systemd-cryptenroll /dev/sdXY            # list enrolled slots
cryptsetup luksDump /dev/sdXY            # look for a tpm2 token
```

- **Bound to PCR 7 only** — Secure Boot state. RABA does not change it. You are fine.
  (Note that PCR 7 alone does not detect kernel or initramfs changes at all, which is why
  stricter guides do not recommend it by itself.)
- **Bound to PCR 9, 11, or a set including them** — you will need to re-enroll.

### What `raba-setup` does about it

It detects a TPM2 token on the LUKS header and **warns before touching the initramfs**,
with the re-enrolment commands. `raba-setup` on its own changes nothing and prints the
warning; `raba-setup --apply` asks before rebuilding, and `--yes` is how you say you read
it. Being surprised by a passphrase prompt on a machine you thought unlocked itself is a
bad first impression, and it is one `lsblk` away from being avoidable.

"Could not check" is never printed as "nothing found": without root the LUKS header is
unreadable, and the warning then says to run it as root rather than giving an all-clear
it has not earned.

### Recovering

Keep a recovery key enrolled **before** changing anything.

```sh
# unlock with the recovery passphrase, then:
sudo systemd-cryptenroll --wipe-slot=tpm2 /dev/sdXY
sudo systemd-cryptenroll --tpm2-device=auto --tpm2-pcrs=7+11 /dev/sdXY
```

### The durable fix: signed PCR policies

Re-enrolling on every change does not scale — a kernel update rebuilds the initramfs and
shifts the measurement again.

systemd v252 and later can bind a volume to a **public key** rather than literal PCR
values. Any kernel carrying PCR information signed by that key satisfies the policy, so
routine updates stop breaking unlock:

```sh
systemd-cryptenroll --tpm2-public-key=… --tpm2-public-key-pcrs=11 /dev/sdXY
```

The UKI must carry a `.pcrsig` section; `ukify inspect` shows whether it does.
`systemd-pcrlock` can also predict PCR values before you reboot into them.

Both are worth documenting and neither is ours to implement. Our job is to warn, and to
point here.

---

## 4. Kernel lockdown

Most distributions enable kernel lockdown in integrity mode automatically when Secure
Boot is on. It blocks `/dev/mem`, `/dev/kmem`, `/dev/port`, `kexec_load`, hibernation,
unsigned module loading, raw PCI access and `ioperm`.

**None of these are things RABA does.** Terminal ioctls, writing to a console, reading
`/proc/cmdline` and lowering the console log level are all unaffected.

It is worth an explicit test in the VM matrix rather than an assumption, since lockdown's
scope has grown over time and a DRM backend would touch more of it than a text one does.

---

## 5. Other integrity mechanisms

**IMA / EVM** — measures or appraises files against a policy. In an appraisal
configuration, executables need a signature in an extended attribute. This is enterprise
territory; if a distribution ships such a policy, the RABA package must follow its
convention the same way any other package does. Nothing RABA-specific.

**dm-verity** — verifies a read-only root filesystem. It does not cover the initramfs, so
it neither helps nor hinders us.

**Remote attestation** — any setup that attests PCR values to a server has the same
problem as §3, for the same reason, with the same fix.

---

## 6. Summary

| setup | RABA verified? | breaks TPM unlock? |
|---|---|---|
| no Secure Boot | n/a | no |
| Secure Boot, shim + GRUB, separate initrd | **no** — the gap predates us | yes, if sealed to PCR 9 |
| Secure Boot + UKI | **yes**, inside the signature | yes, if sealed to PCR 11, unless using a signed policy |
| UKI + signed PCR policy | yes | no |

Two things to carry into the code:

1. Recommend UKI to anyone who cares about Secure Boot. It is the configuration where
   RABA is covered for free and adds no verification gap.
2. `raba-setup` checks for a TPM2 token and warns before touching the initramfs.
