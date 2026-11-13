raba-setup(8)

# NAME

raba-setup - set up and verify a RABA installation

# SYNOPSIS

*raba-setup* [options]

# DESCRIPTION

*raba-setup* looks at the machine and says what it needs in order to show a boot
splash: which initramfs generator builds the image, whether *raba* is in it,
whether the image still matches the configuration on disk, and whether rebuilding
it will stop an encrypted disk from unlocking itself.

Run with no options it *changes nothing*. That is the default on purpose: the one
thing here that can cost you a boot is one flag away, and everything it would do
is printed first, in the order it would happen.

*raba-setup* does not edit files belonging to other packages — not
_/etc/mkinitcpio.conf_, not _/etc/default/grub_, not a display manager's unit. It
prints the line and the command; you run them. A program that rewrites another
package's configuration bypasses the *.pacnew* handling that exists to stop
exactly that, and the failure mode of getting it wrong is a machine that does not
boot.

# OPTIONS

*--apply*
	Rebuild the initramfs, by running the generator this machine uses. The only
	state-changing thing *raba-setup* does, and it is somebody else's program
	doing it.

	Refuses when there is no theme to put in the image, and when *raba* is not
	in *HOOKS* — in both cases rebuilding would change nothing. Asks first when
	a volume unlocks itself through a TPM; see *DISK ENCRYPTION*.

*--check*
	Report whether each image under _/boot_ still carries the configuration that
	would be generated now, and exit non-zero if any does not.

	The real comparison reads the frozen copy out of the image, which needs
	*lsinitrd*(1) or *bsdtar*(1). With neither, the only evidence is a
	timestamp, and the answer says so rather than pretending.

*--set-theme* _name_
	Put _name_ in the *Boot.Theme* line of *raba.conf*(5), then do what
	*--apply* does.

	One command because doing only the first half is the mistake *raba.conf*(5)
	warns about: *raba*(8) runs before the root filesystem is mounted and reads
	a copy of the configuration baked into the initramfs, so editing
	_/etc/raba/raba.conf_ changes nothing until the image is regenerated. The
	symptom is the old logo after a reboot and the conclusion that the program
	is broken.

	_name_ is a theme in _/usr/share/raba/themes_ without the _.raba_, an
	absolute path to a file, or *auto* for the one matching *os-release*(5). It
	is checked before anything is written; a name that does not resolve would
	otherwise leave a valid-looking file and an image with no splash in it.

	This is the only file *raba-setup* ever writes, and it is RABA's own. Your
	comments and your other settings are left exactly as they were.

*--yes*, *-y*
	Do not ask before rebuilding an image on a machine whose disk unlocks
	itself.

*--freeze*
	Print the configuration an initramfs should carry: the whole effective
	configuration with every path resolved. Used by the initramfs hooks. Exits
	non-zero when no theme resolves, which is how a hook knows to put nothing in
	the image rather than something broken.

*--print-files*
	Print the files that go in beside it, one per line. Also for the hooks.

*--root* _dir_
	Look under _dir_ instead of _/_. For testing; *--apply* refuses it.

*--config* _file_, *--defaults* _file_, *--themes* _dir_, *--cmdline* _file_, *--os-release* _file_
	Override the paths *raba.conf*(5) is assembled from. For testing.

*--version*, *--help*

# DISK ENCRYPTION

If an encrypted volume releases its key through a TPM, *raba-setup* says so
before it offers to touch anything.

Putting RABA inside the initramfs changes the measurement that key is sealed
against — PCR 9, and PCR 11 for a unified kernel image — so the next boot asks
for a recovery passphrase instead of unlocking itself. This is not specific to
RABA. Any change to initramfs contents does it, including the ones your
distribution makes on its own.

*raba-setup* checks, warns, and asks. It does not re-enrol anything: that is
*systemd-cryptenroll*(1)'s job and your decision. See _docs/secure-boot.md_,
which also covers signed PCR policies — the way to stop every kernel update
breaking unlock.

# EXIT STATUS

*0*
	The dry run always. *--check* when every image is current. *--apply* when
	the image was rebuilt.

*1*
	*--check* when an image is out of date or nothing could be compared;
	*--apply* when it declined or the generator failed; any usage error.

# SEE ALSO

*raba*(8), *raba.conf*(5), *raba-ctl*(1), *mkinitcpio*(8), *dracut*(8),
*update-initramfs*(8), *systemd-cryptenroll*(1)
