raba(8)

# NAME

raba - ASCII art boot splash

# SYNOPSIS

*raba* [options]

*raba* --preview _file.raba_

# DESCRIPTION

*raba* draws an ASCII art animation on a virtual terminal while the system boots,
then restores the terminal and exits so a display manager can take over.

It is normally started from the initramfs and is not intended to be run by hand,
with the exception of *--preview*.

*raba* plays *.raba* animations — see *raba-gen*(1) for producing them from other
formats. It renders text cells, not pixels. It writes to a virtual terminal in *KD_TEXT*
mode and relies on the kernel framebuffer console. It does not open a DRM device, does
not need a GPU driver, and does not require DRM modules in the initramfs.

# OPTIONS

*--preview* _file.raba_
	Render _file.raba_ in the current terminal and exit. Does not touch any virtual
	terminal, does not require root, and changes no system state. This is the
	intended way to work on art.

*--config* _path_
	Read configuration from _path_ instead of the compiled-in location.
	See *raba.conf*(5).

*--tty* _n_
	Draw on _/dev/ttyn_. Overrides the configuration file.

*--check*
	Validate configuration and theme, report what would happen, draw nothing.
	Exits non-zero on a problem. Safe to run on a live system.

*--version*, *--help*

# KERNEL COMMAND LINE

Because *raba* runs before the root filesystem is mounted, it cannot read
_/etc/raba/raba.conf_ at boot. It reads a copy baked into the initramfs, plus these
kernel parameters:

*raba.enable=0*
	Do nothing and exit immediately. The kill switch. Add this from your bootloader
	if a splash problem is preventing you from booting.

*raba.debug=1*
	Write a trace to _/run/raba.log_ and draw nothing.

*raba.tty=*_n_
	Draw on _/dev/ttyn_.

*quiet loglevel=3*
	Not read by *raba*, but recommended. Suppresses kernel messages that would
	otherwise be printed over the animation. *raba* lowers the console log level
	itself on startup, so this only affects messages emitted before it runs. The
	parameters must appear in this order.

# EXIT STATUS

*raba* exits *0* in every situation that is not a programming error, including a
missing theme, an unreadable theme, an absent framebuffer console and a disabled
kill switch. This is deliberate: a non-zero exit from a boot-time program can stall a
boot, and no splash is always better than no system.

Problems are reported on the console log and in _/run/raba.log_, not through the exit
status. *raba --check* is the command that reports problems by exit status.

# SIGNALS

*SIGTERM*, *SIGINT*
	Restore the terminal to *KD_TEXT*, switch back to the original virtual terminal,
	and exit 0.

*SIGUSR1*
	Begin the outro animation, as though *raba-ctl quit* had been called.

A terminated instance is resumed rather than restarted: *raba* records its terminal,
frame index and phase in _/run/raba.state_ on every frame, and a later instance that
finds a fresh state file adopts it and continues. Being killed — at *switch_root*, by
the kernel, or by an init system — is therefore recoverable rather than fatal.

A panic handler performs the same terminal restoration before aborting. A virtual
terminal left in *KD_GRAPHICS* mode can cause a later *VT_WAITACTIVE* to hang, which
would stall the boot, so restoration is attempted on every exit path.

# FILES

_/etc/raba/raba.conf_
	Configuration. Only read when the root filesystem is available; at boot a frozen
	copy inside the initramfs is used instead. See *raba.conf*(5).

_/usr/share/raba/raba.defaults_
	Distribution defaults. Do not edit; override in _/etc/raba/raba.conf_.

_/usr/share/raba/themes/_
	Installed themes.

_/run/raba.pid_
	PID of the running instance.

_/run/raba.sock_
	Control socket. See *raba-ctl*(1).

_/run/raba.state_
	Terminal, frame index and phase, so a later instance can resume.

_/run/raba.log_
	Diagnostics from the current boot. Copied to _/var/log/raba/last-boot.log_ after
	boot, because _/run_ does not survive a reboot.

# NOTES

## Console glyph limits

The Linux virtual console can display only 256 or 512 distinct glyphs, inherited from
VGA hardware. Characters outside the loaded console font are shown as a replacement
character and the original codepoint is discarded by the kernel.

ASCII, CP437 block elements and box-drawing characters are safe. Braille patterns,
arrows, mathematical symbols and emoji are not. *raba-gen*(1) checks this when building
a theme.

## High-resolution displays

The console font is typically 8x16 pixels and cannot be scaled by *raba*. On a 4K
display a small logo will be physically tiny. There is no fix in text mode; use art
drawn at a larger cell size.

# SEE ALSO

*raba-ctl*(1), *raba.conf*(5), *raba-gen*(1), *raba-theme*(5), *ioctl_console*(2),
*setfont*(8)
