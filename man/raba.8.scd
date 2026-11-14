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

*--theme* _file.raba_
	Loop _file.raba_ on a virtual terminal until something says to stop. This is
	the boot mode.

*--outro* _file.raba_
	Play _file.raba_ once on *quit*, bounded by *--max-duration*. Absent means no
	outro.

*--show-delay* _seconds_, *--timeout* _seconds_, *--max-duration* _seconds_
	*Boot.ShowDelay*, *Boot.Timeout* and *Outro.MaxDuration*. See *raba.conf*(5)
	for what each means; the defaults are the same. A *--timeout* of *0* disables
	it, which is not recommended.

*--tty* _n_|*auto*
	*Boot.Tty*: the virtual terminal to draw on, or *auto* for a free one.

*--config* _path_
	Read _path_ instead of _/etc/raba/raba.conf_. The distribution's
	_raba.defaults_ is still read underneath it. See *raba.conf*(5).

*--defaults* _path_, *--themes* _path_, *--cmdline* _path_, *--os-release* _path_
	Override _/usr/share/raba/raba.defaults_, _/usr/share/raba/themes_,
	_/proc/cmdline_ and _/etc/os-release_. Intended for testing.

*--socket* _path_, *--state* _path_
	Override _/run/raba.sock_ and _/run/raba.state_. Intended for testing.

*--check*
	Validate configuration and theme, report what would happen, draw nothing.
	Exits non-zero on a problem. Safe to run on a live system.

	This is the strict reading of the files the boot path reads leniently: at
	boot an unknown key or an unparseable value is noted and stepped over, which
	is the only behaviour that cannot stop a machine from starting, and the
	price of it is that a typo does nothing visible. *--check* is where it
	becomes visible, while you are at the keyboard.

	It also reports keys that this version parses and does not act on — see
	*NOT YET HONOURED* in *raba.conf*(5) — and decodes the theme, because
	"the file exists" and "the animation will play" are different questions.

*--version*, *--help*

# PRECEDENCE

Later lines win, key by key. A setting absent from every layer keeps the default
documented in *raba.conf*(5).

```
compiled-in defaults
  /usr/share/raba/raba.defaults   the distribution's
    /etc/raba/raba.conf           yours
      /proc/cmdline               raba.enable, raba.debug, raba.tty
        command-line options      the ones above
```

The kernel command line sits above both files deliberately: it is the layer you
can reach from a bootloader on a machine that will not boot, and a kill switch a
file could override would not be one.

# KERNEL COMMAND LINE

Because *raba* runs before the root filesystem is mounted, it cannot read
_/etc/raba/raba.conf_ at boot. It reads a copy baked into the initramfs, plus these
kernel parameters:

*raba.enable=0*
	Do nothing and exit immediately. The kill switch. Add this from your bootloader
	if a splash problem is preventing you from booting. Nothing overrides it:
	no terminal is opened and no socket is bound.

*raba.debug=1*
	Write the report *--check* prints to *Log.File* — _/run/raba.log_ by default
	— and draw nothing. This is what to ask a reporter for when the splash did
	not appear and no command can be run on the machine while it boots.

*raba.tty=*_n_
	Draw on _/dev/ttyn_, overriding *Boot.Tty*.

A *raba.* parameter that is not one of these is reported rather than ignored:
*raba.enabled=0* typed at a bootloader prompt would otherwise do nothing, and
silently.

*quiet loglevel=3*
	Not read by *raba*, but recommended. Suppresses kernel messages that would
	otherwise be printed over the animation. *raba* lowers the console log level
	itself on startup, so this only affects messages emitted before it runs. The
	parameters must appear in this order.

# WHEN THE SCREEN CHANGES HANDS

*raba* watches the virtual terminal it borrowed. It stops drawing and exits when
either of two things happens:

- the display moves to another terminal — a display manager taking one of its own,
  a compositor started from a login, or somebody pressing *Ctrl+Alt+F2*;
- the terminal *raba* is on enters *KD_GRAPHICS* mode, which is what a compositor
  taking that same terminal looks like from here.

It leaves quietly: no switch back to the original terminal, no mode change, nothing
written to the console. Every one of those would take the screen away from whatever
has just taken it. Kernel message redirection is put back, because that is *raba*'s
to undo.

This is the whole handoff on a machine with no display manager. The control socket is
root-only (*raba-ctl*(1)), a compositor in a user session cannot ask *raba* to quit,
and *Boot.Timeout* is a backstop rather than an answer — so the kernel's own answer to
"who has the screen" is used instead. Nothing about it is specific to an init system,
a display server or a compositor.

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

These are *blocked* rather than handled: no signal handler runs, and the frame
loop asks the kernel once a frame whether one is pending. A signal is therefore
acted on at a frame boundary, where the terminal is in a known state.

The block is lifted the moment the first signal arrives, so a *second* *SIGTERM*
is delivered the ordinary way and ends the process at once. A *raba* that was
somehow stuck would otherwise ignore the signal, and nothing in a boot splash is
allowed to need *SIGKILL*.

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
	copy inside the initramfs is used instead. Absent is not an error — every
	setting has a default. See *raba.conf*(5).

_/usr/share/raba/raba.defaults_
	Distribution defaults. Do not edit; override in _/etc/raba/raba.conf_.

_/usr/share/raba/themes/_
	Installed themes.

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
