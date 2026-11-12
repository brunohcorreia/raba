raba.conf(5)

# NAME

raba.conf - configuration file for raba(8)

# SYNOPSIS

_/etc/raba/raba.conf_

_/usr/share/raba/raba.defaults_

# DESCRIPTION

An INI-style file of *Key=Value* pairs grouped into sections. Keys are
case-sensitive. Lines beginning with *#* are comments.

Two files are read. _/usr/share/raba/raba.defaults_ holds distribution defaults and
belongs to the package; it is replaced on upgrade and must not be edited.
_/etc/raba/raba.conf_ holds your overrides and is never touched by the package
manager. Any key absent from your file falls back to the default.

# CHANGES REQUIRE REBUILDING THE INITRAMFS

*raba*(8) runs before the root filesystem is mounted, so it cannot read this file at
boot. The initramfs hook reads it when the image is generated and bakes a copy inside.

*Editing this file has no effect until the initramfs is regenerated.*

Use *raba-set-theme*(8), which rebuilds the initramfs for you, or regenerate by hand:

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

*Tty=*_n_
	Virtual terminal to draw on. Default: *1*

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
	Default: *true*

*Theme=*_name_|*auto*
	Same resolution rules as *Boot.Theme*. With *auto*, the boot theme's name with
	*-outro* appended is tried first.

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
	Default: _/run/raba.log_

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
