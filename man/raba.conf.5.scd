raba.conf(5)

# NAME

raba.conf - configuration file for raba(8)

# SYNOPSIS

_/etc/raba/raba.conf_

_/usr/share/raba/raba.defaults_

# DESCRIPTION

An INI-style file of *Key=Value* pairs grouped into sections. Sections and keys
are case-sensitive; values are not, except where they are a path or a theme name.
Whitespace around a key or a value is not part of it.

A *#* starts a comment only at the beginning of a line. One inside a value is
part of the value, because paths contain them. The same key given twice takes
its last value.

# A BROKEN FILE STILL BOOTS

*raba*(8) reads this file as root, before the root filesystem is mounted, and is
not allowed to fail. So an unknown key, an unknown section, a value that does not
parse, a file that is not UTF-8 and a file too large to be a configuration are
all *noted and stepped over*, and the setting keeps the value it had. A section
this version does not know is skipped whole, so a file written for a later
version still boots an earlier one.

The price is that a typo does nothing, silently. *raba --check* is where it stops
being silent: it reads the same files with the same parser, prints every
complaint and exits non-zero. Run it after editing this file — and before
rebuilding the initramfs, which is what makes an edit take effect.

Two files are read. _/usr/share/raba/raba.defaults_ holds distribution defaults and
belongs to the package; it is replaced on upgrade and must not be edited.
_/etc/raba/raba.conf_ holds your overrides and is never touched by the package
manager. Any key absent from your file falls back to the default.

# CHANGES REQUIRE REBUILDING THE INITRAMFS

*raba*(8) runs before the root filesystem is mounted, so it cannot read this file at
boot. The initramfs hook reads it when the image is generated and bakes a copy inside.

*Editing this file has no effect until the initramfs is regenerated.*

Use *raba-setup*(8) *--set-theme* _name_, which rebuilds the initramfs for you, or
regenerate by hand:

```
mkinitcpio -P          # Arch
dracut --force         # Fedora, openSUSE
update-initramfs -u    # Debian, Ubuntu
```

*raba-setup --check* compares the configuration on disk against the copy inside the
initramfs and reports a mismatch.

# [Boot] SECTION

*Theme=*_name_|*auto*
	Theme to play while the system boots. A bare _name_ resolves to
	_/usr/share/raba/themes/name.raba_; a path starting with */* is used as given.

	*auto* resolves the *ID=* field of *os-release*(5) at initramfs build time,
	falling back to *ID_LIKE=* and then to a generic theme. Resolution happens when
	the image is generated, not at boot, so a distribution upgrade that changes *ID=*
	takes effect at the next initramfs rebuild.

	Default: *auto*

*Tty=*_n_|*auto*
	Virtual terminal to draw on. *auto* asks the kernel for a free one, which is
	the only setting that cannot land on top of something already in use.

	A terminal named by number is taken whatever is on it — that is what naming
	one means. Naming the terminal that is *already displayed* has one further
	consequence: kernel messages cannot be moved aside, because aside is where
	the user is, so they print over the animation exactly as they would with no
	splash installed. Use *quiet loglevel=3* if that matters.

	Default: *auto*

*ShowDelay=*_seconds_
	Wait this long before drawing anything. On a machine that boots faster than the
	delay, nothing is ever shown. Useful to avoid a splash that flashes for a
	fraction of a second. Accepts fractions. Default: *0*

*Timeout=*_seconds_
	Stop drawing and reveal the console after this long without a *quit* command.

	*raba* does not exit on timeout — it restores kernel message redirection and
	releases the terminal, so boot messages become visible again. A stalled boot
	should show you why it stalled, and an exit would leave a blank screen instead.

	A root filesystem over NFS or iSCSI, or a large degraded array, can legitimately
	take minutes, so this value should be generous. *0* disables it, which is not
	recommended. Default: *30*

*FrameRateOverride=*_fps_
	Ignore the frame rate stored in the theme. *0* uses the theme's own value.
	Default: *0*

# [Outro] SECTION

The outro plays exactly once, after *raba-ctl quit*, on the way to the display
manager.

*Enable=*_bool_
	A boolean is *true*, *yes*, *on* or *1*, or *false*, *no*, *off* or *0*.

	Default: *true*

*Theme=*_name_|*auto*
	Same resolution rules as *Boot.Theme*. With *auto*, the boot theme's name with
	*-outro* appended is looked for *beside the boot theme* — so a theme given as
	a path has its outro looked for in the same directory rather than in the
	system one. An outro that is not found is not an error; none plays.

	Default: *auto*

*MaxDuration=*_seconds_
	Hard ceiling on the outro. The animation is cut off at this point regardless of
	how many frames remain. Without a ceiling, a long outro becomes the boot delay
	this project exists to avoid. Default: *1.5*

*FinishFrame=*_bool_
	When a *quit* arrives mid-frame, let the current frame finish before starting the
	outro. Prevents a visible jump. Default: *true*

# [Progress] SECTION

Draws a progress indicator over the animation. Off by default.

There is no reliable source of boot progress on Linux — nothing in the boot sequence
knows how far along it is. Read _docs/progress.md_ before enabling this.

*Enable=*_bool_
	Default: *false*

*Style=*_pulse_|_bar_|_percent_
	*pulse* sweeps back and forth and claims no knowledge of progress. *bar* fills
	according to *Source*. *percent* adds a number.

	Default: *pulse*

*Source=*_ctl_|_time_|_none_
	Where the value comes from.

	*ctl* takes it from *raba-ctl progress*. Nothing calls that by default, so the
	bar stays at zero until something is wired up. Honest and init-agnostic.

	*time* interpolates against the duration of the previous boot. This is Plymouth's
	approach, and it inherits Plymouth's failure modes: the cache is unavailable when
	_/var_ is a separate filesystem, a first boot has no data, and a failed boot never
	updates it. Not recommended.

	*none* draws a static empty bar.

	Default: *ctl*

*Position=*_above_|_below_
	Relative to the animation. Default: *below*

*Width=*_cells_
	*0* matches the animation width. Default: *40*

*Color=*_n_, *BackgroundColor=*_n_
	Palette indices. Defaults: *7* and *8*

# [Handoff] SECTION

*Mode=*_retain_|_clear_
	*retain* leaves the last frame on screen when *raba* exits, so whatever draws next
	produces no visible gap. *clear* blanks the terminal first.

	*retain* looks better and is riskier: if nothing takes the screen, the final frame
	stays on over a login prompt you cannot see. Distributions avoid the equivalent
	Plymouth option in generic scripts for this reason. *Boot.Timeout* bounds the
	exposure.

	Default: *retain*

*RestoreTty=*_bool_
	Switch back to the terminal that was active before *raba* started.
	Default: *true*

# DISK ENCRYPTION

RABA does not collect disk encryption passphrases. *cryptsetup* prompts on the console,
as it does when no splash is installed.

Place the *raba* hook *after* the *encrypt* hook so RABA is not yet running when the
prompt appears. This differs from Plymouth, which must come first because it supplies
the prompt.

See _docs/luks.md_ for the reasoning.

# [Log] SECTION

*Level=*_error_|_warn_|_info_|_debug_
	Default: *warn*

*File=*_path_
	Where *raba.debug=1* writes its report. Must be absolute.

	Default: _/run/raba.log_

# NOT YET HONOURED

Every key above is parsed and validated. These are the ones this version then
does nothing with, because the feature behind them does not exist yet:

```
Boot.FrameRateOverride   Outro.FinishFrame
Progress.*               Handoff.Mode   Handoff.RestoreTty   Log.Level
```

They are listed rather than quietly accepted, and *raba --check* names any of
them that your file actually sets. A configuration key that is accepted and
ignored is a key that has stopped meaning anything.

# EXAMPLE

```
# /etc/raba/raba.conf

[Boot]
Theme=cachyos
Tty=1
ShowDelay=0.3
Timeout=20

[Outro]
Enable=true
Theme=cachyos-outro
MaxDuration=1.0

[Handoff]
Mode=retain
```

# SEE ALSO

*raba*(8), *raba-ctl*(1), *raba-theme*(5), *os-release*(5)
