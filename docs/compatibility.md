# Compatibility

An upgrade must not stop a machine from booting, and must not stop a theme
somebody made from playing. This is the list of things that are promises and the
rules for changing each of them.

It exists because RABA has an unusual failure mode: a copy of the binary and a
copy of the configuration live *inside an initramfs*, and that copy is not
upgraded when the package is. Any given boot can therefore run last month's
`raba` against this month's `/etc`, or this month's `raba-ctl` against last
month's daemon.

---

## What is a promise

| | versioned by | if it meets something newer |
|---|---|---|
| the `.raba` format | `RABA` magic + a `u16` version | refused by name, `raba` exits 0, the machine boots |
| the control protocol | `protocol: 1` in `status` | an unknown command is rejected cleanly; `raba-ctl` exits 0 |
| `raba.conf`(5) | nothing, on purpose | unknown keys and sections are stepped over |
| `/run/raba.state` | freshness, not version | an unreadable state file is a fresh start |
| the initramfs hooks | nothing | a missing flag must not fail an image build |
| installed paths | the package | a file another package references is never moved |

---

## The rules

**1. A new build reads every file an old build could.**
`tests/fixtures/valid/v1-generic.raba` is committed as bytes and decoded by
`crates/raba-core/tests/compatibility.rs`. A decoder change that would stop a
user's theme from playing fails there. Fixtures are added, never edited.

**2. A new format version is a last resort, and the generator keeps emitting the
old one.**
The decoder refuses a version it does not know — it does not guess. So a version
bump splits the world, and `raba-gen` must keep writing the *lowest* version that
expresses the art: a theme that uses nothing new stays a v1 file forever. Prefer
an additive change inside the current version to a bump. The palette and movement
ops sketched in [milestones.md](milestones.md) are the cases where a bump will
eventually be justified; neither is justified yet.

**3. An old `raba` meeting a new file must boot the machine.**
This is what makes rule 2 survivable. A version it cannot read is reported and
skipped, the exit status is 0, and the boot continues with no splash — the same
path as a missing theme. There is no "best effort" partial decode, because a
half-read animation is drawn on a console as root.

**4. No `raba.conf` key may change behaviour by being absent.**
Defaults are compiled in, and a file written for a newer RABA has to boot an
older one. That is why unknown keys are recorded and stepped over instead of
rejected, and why `config::NOT_HONOURED` names keys that are accepted and not yet
acted on: the key exists, the file is valid, and the version that implements it
will read the same file.

**5. The protocol adds, it does not rename.**
`status` field names are API — `docs/protocol.md` says so. A client must tolerate
fields it does not know. A new command is a new verb; an old daemon rejecting it
is a clean rejection, and `raba-ctl` still exits 0 where a boot script depends on
that (`man 1 raba-ctl`).

**6. An initramfs hook may not require a flag the installed binary might not
have.**
The hooks call `raba-setup --freeze` and `--print-files`. Adding a required flag
to either breaks an image build on a machine mid-upgrade. New flags are optional,
and the hook checks before using one. The rule the hooks already live by —
*nothing here may fail an image build* — covers this case too.

**7. The binary and the theme inside an image travel together.**
Whatever the image carries is internally consistent, because the same hook run
put both in. The upgrade hazard is the image as a whole going stale, which
`raba-setup --check` and the pacman hook report, not the two halves disagreeing.

---

## What is explicitly not a promise

The art. Logos change because distributions change them, and a theme is rebuilt
from its recipe on every package build. `themes/golden/*.txt` pins what each one
renders to, so a change is visible in a diff and deliberate — but a logo that
upstream redraws will be redrawn here too.

Internal crate APIs. `raba-core`, `raba-term` and `raba`-as-a-library are not
published and have no semver obligation to anybody outside this repository.
