# Packaging and community conventions

> What we will and will not do, and why. Several of these are things the Linux community
> reacts badly to, for good reasons.

## Things we deliberately will not do

### Impersonate another project to capture its integration

mkinitcpio's `encrypt` hook probes for a binary named `plymouth`:

```sh
if command -v plymouth >/dev/null 2>&1 && plymouth --ping 2>/dev/null; then
```

Shipping something called `plymouth` would hand us that integration for free. It would
also break systems where real Plymouth is installed, and it is the kind of thing that gets
a package pulled from a repository. We will not.

The legitimate route is proposing a generic splash interface upstream.

### Fork another project's hook

Plymouth once shipped `plymouth-encrypt` and `sd-plymouth`, replacements for the standard
hooks. **Both are now deprecated**, with upstream telling users to go back to plain
`encrypt` and `plymouth`.

A forked copy of someone else's hook diverges and then breaks on their schedule. The
experiment has been run; we do not need to repeat it.

### Edit configuration owned by other packages

`/etc/default/grub`, `/etc/mkinitcpio.conf`, `/etc/ly/config.lua` — none of these are ours.
A `post_install` that rewrites files belonging to another package bypasses pacman's
`.pacnew` handling, which is exactly the complaint behind the `postfix` bug report
(FS#48324).

`raba-setup` shows what to change and only acts when explicitly asked with `--apply`.

### Enable services on install

Arch convention is that installing a package does not start or enable it. A boot-critical
package that enables itself without being asked is worse than merely impolite.

### Declare `conflicts=('plymouth')`

**Reversing an earlier decision here.** `conflicts` means packages cannot coexist, and
pacman will force a removal. RABA and Plymouth share no files — they conflict only if both
are *enabled*, and a user may reasonably have Plymouth installed but inactive.

Forcing removal of someone's working splash to install ours is rude and technically
wrong.

> Instead: detect a running `plymouthd` at startup, refuse to draw, and log why. The
> `.install` message mentions it. No `conflicts`, no `replaces`.

### Vendor system libraries, or download unverified sources at build time

Standard packaging hygiene. Rust dependencies are fetched with `--locked` in `prepare()`
and built `--offline --frozen`, per the Arch Rust packaging convention.

---

## Things we will do

| | |
|---|---|
| `backup=()` | for `/etc/raba/raba.conf`, so pacman handles it properly |
| defaults split | `/usr/share/raba/raba.defaults` is ours to replace; `/etc` is the user's |
| FHS paths | no `/usr/local`, no `/opt` |
| `.install` message | states the required manual steps plainly |
| credit | fastfetch for logo art, Plymouth for the integration model, psplash for VT handling |
| reproducible builds | worth having; cheap in Rust |
| honest README | cosmetic software with a privileged runtime, stated as such |

---

## The criticism we should expect, and the honest answer

**"Another boot splash?"** — Fair. The README's comparison table says what is actually
different (initramfs size, no GPU driver dependency) and explicitly refuses the easy claim
that Plymouth is slow, which is partly a measurement artifact.

**"Why a new file format?"** — Because the runtime parser for a gzipped JSON format is a
dependency and an unbounded allocation in early boot. [format.md](format.md) gives the
reasoning; `raba-gen` converts from formats that have editors.

**"Cosmetic software that can break my boot."** — The most legitimate criticism there is.
The answers are architectural: exit 0 on every error path, `raba.enable=0`, a timeout, a
hook that cannot fail an image build, resumable state so being killed is a non-event, and
a test suite built around failure cases rather than success cases. If those are not true,
the criticism stands.

**"Rust rewrite of something that works."** — It is not a rewrite; nothing here replaces
Plymouth's feature set. It is a smaller program with a narrower goal.

**"It silences my kernel log."** — RABA lowers the *console* log level and restores it on
exit. `dmesg` and the journal are untouched. Worth stating in the README before someone
assumes otherwise.

**"systemd-first project pretending to be agnostic."** — The real defence is shipping and
testing the runit, OpenRC, s6 and dinit paths rather than stubbing them, and keeping
systemd-specific features optional. [risks.md](risks.md) treats this as a maintenance
obligation, not a checkbox.

---

## Repository layout

```
raba            core, AUR
raba-git        VCS build, AUR convention
raba-themes     generated .raba files
raba-gen        build tool; a makedepends of raba-themes
```

Keeping themes in a separate package means trademark removal requests touch one package,
and users who want only the program are not carrying 534 logos.

**And the split is not about size.** Measured, 13 Nov 2026, with the PKGBUILD in
`packaging/aur/raba`:

| | |
|---|---|
| one theme, on disk | 9.5 KB average; `arch.raba` is 9424 bytes |
| one theme, gzipped into an initramfs | 1.8 KB — and **only one ever goes in**, the frozen one |
| all 50 themes, installed | 464 KiB |
| `raba-themes` package | **74 KiB** |
| `raba` package | 401 KiB |
| `raba-gen` package | 560 KiB |

So the honest reason for three packages is the trademark one and the tidiness one, not
bytes. Even all 534 fastfetch logos would be about 800 KiB installed. Anybody arguing
the split on size is arguing from a number nobody measured.

The `raba` package carries exactly one theme of its own, `generic.raba`, which is not a
logo. `pacman -S raba` has to produce a splash that draws something, and the fallback
cannot live in a package that a trademark complaint could remove.
