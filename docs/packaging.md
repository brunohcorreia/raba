# Packaging and community conventions

> What RABA will and will not do as a package, and why. Several of these are practices
> the Linux community reacts badly to, for good reasons.

## What RABA will not do

### Impersonate another project to capture its integration

mkinitcpio's `encrypt` hook probes for a binary named `plymouth`:

```sh
if command -v plymouth >/dev/null 2>&1 && plymouth --ping 2>/dev/null; then
```

Shipping a binary called `plymouth` would capture that integration for free. It would
also break systems where real Plymouth is installed, and it is the kind of thing that
gets a package pulled from a repository. RABA does not do it.

The legitimate route is proposing a generic splash interface upstream.

### Fork another project's hook

Plymouth once shipped `plymouth-encrypt` and `sd-plymouth`, replacements for the standard
hooks. **Both are now deprecated**, with upstream telling users to go back to plain
`encrypt` and `plymouth`.

A forked copy of another project's hook diverges and then breaks on that project's
schedule. The experiment has been run; there is nothing to learn by repeating it.

### Edit configuration owned by other packages

`/etc/default/grub`, `/etc/mkinitcpio.conf`, `/etc/ly/config.lua` — none of these belong
to RABA.
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

## What RABA does do

| | |
|---|---|
| `backup=()` | for `/etc/raba/raba.conf`, so pacman handles it properly |
| defaults split | `/usr/share/raba/raba.defaults` belongs to the package; `/etc` belongs to the user |
| FHS paths | no `/usr/local`, no `/opt` |
| `.install` message | states the required manual steps plainly |
| credit | fastfetch for logo art, Plymouth for the integration model, psplash for VT handling |
| reproducible builds | worth having; cheap in Rust |
| plain README | cosmetic software with a privileged runtime, stated as such |

---

## Expected criticism, and the answer to each

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

The reason for three packages is therefore the trademark one and the tidiness one, not
bytes. Even all 534 fastfetch logos would be about 800 KiB installed. Anybody arguing
the split on size is arguing from a number nobody measured.

The `raba` package carries exactly one theme of its own, `generic.raba`, which is not a
logo. `pacman -S raba` has to produce a splash that draws something, and the fallback
cannot live in a package that a trademark complaint could remove.

---

## Publishing to the AUR

The AUR is a git server. A package is a repository with two files in it — `PKGBUILD`
and `.SRCINFO` — plus whatever the `PKGBUILD` references by name; here that is
`raba.install`. There is no review queue and no upload form: pushing is publishing.

Both files are kept in [`packaging/aur/raba`](../packaging/aur/raba) and the AUR
repository is a copy of that directory, which is why the recipe is version-controlled
here rather than only there.

### Once, to get an account

1. Register at <https://aur.archlinux.org/register> — username, email, nothing else.
2. Make a key for it, kept apart from any other, so that revoking it later costs
   nothing else:

   ```sh
   ssh-keygen -t ed25519 -f ~/.ssh/aur -C "aur"
   cat ~/.ssh/aur.pub
   ```

3. Paste that public key into *My Account → SSH Public Key*.
4. Tell SSH which key belongs to that host, in `~/.ssh/config`:

   ```
   Host aur.archlinux.org
       User aur
       IdentityFile ~/.ssh/aur
       IdentitiesOnly yes
   ```

5. Check it, which also tells you the username the AUR thinks you are:

   ```sh
   ssh aur@aur.archlinux.org help
   ```

### Each release

```sh
cd packaging/aur/raba
updpkgsums                              # the digest of the new tag's tarball
makepkg --printsrcinfo > .SRCINFO       # never written by hand
makepkg -f                              # it has to build from the tarball, not the tree
namcap PKGBUILD *.pkg.tar.zst           # the lints a user would hit
```

Then push. The first time, the repository has to be created by cloning the empty one
the AUR makes on demand:

```sh
git clone ssh://aur@aur.archlinux.org/raba.git aur-raba
cp PKGBUILD .SRCINFO raba.install aur-raba/
cd aur-raba
git add PKGBUILD .SRCINFO raba.install
git commit -m "Initial import: raba 0.1.0"
git push origin master
```

`master`, not `main`. The AUR has not renamed it.

### What the AUR will not forgive

- **`.SRCINFO` out of step with `PKGBUILD`.** The web interface reads only `.SRCINFO`,
  so a stale one shows the wrong version to everybody while `makepkg` builds the right
  one. Regenerate it in the same commit, every time.
- **A `pkgver` that moves without `pkgrel` resetting to 1**, or a `pkgrel` that does not
  increase when the recipe changes under an unchanged `pkgver`. Nobody's upgrade path
  works otherwise.
- **Sources that are not fixed.** A tag can be moved; the digest is what makes the
  tarball the one that was tested. `SKIP` is for VCS packages and nothing else.
- **Dropping a package that other packages depend on.** Not a problem RABA has yet, and worth
  remembering before `raba-themes` is ever renamed.

A boot-critical package has one more obligation, and it is not written in any AUR rule:
whatever is pushed has been booted. The matrix in `scripts/resilience.sh` is what makes
that sentence true rather than hopeful.
