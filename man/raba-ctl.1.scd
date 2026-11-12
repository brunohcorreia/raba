raba-ctl(1)

# NAME

raba-ctl - control a running raba instance

# SYNOPSIS

*raba-ctl* _command_ [options]

# DESCRIPTION

*raba-ctl* sends commands to a running *raba*(8) over the control socket at
_/run/raba.sock_. It is the counterpart to the *plymouth*(1) client and exists for the
same reason: something has to tell the splash when the system is ready.

If no instance is running, *raba-ctl* exits 0 silently. Callers are usually boot-time
units that must not fail when the splash is absent.

# COMMANDS

*quit* [*--wait*]
	Ask *raba* to finish. The outro animation plays once, bounded by *MaxDuration*,
	then the terminal is restored. With *--wait*, block until it has exited.

*deactivate*
	Stop drawing and release the terminal, but keep running. Used before a display
	manager takes the screen, so the two do not fight over the terminal.

*reactivate*
	Resume drawing after *deactivate*.

*message* _text_
	Display _text_ below the animation.

*progress* _fraction_
	Set the progress indicator to _fraction_, between *0.0* and *1.0*. Requires
	*Progress.Source=ctl* in *raba.conf*(5).

	Nothing reports progress by default — see _docs/progress.md_ for why there is no
	reliable source of boot progress and what the alternatives cost.

*progress* *--advance* _fraction_
	Add _fraction_ to the current value.

*ping*
	Exit 0 if an instance is running, non-zero otherwise.

*status*
	Print the current state: theme, terminal, animation phase, uptime.

# EXAMPLES

Hand off to a display manager:

```
raba-ctl deactivate
# display manager starts and takes the terminal
raba-ctl quit --wait
```

In a systemd unit:

```
[Service]
ExecStart=/usr/bin/raba-ctl quit --wait
```

# EXIT STATUS

*0*
	Command delivered, or no instance was running.

*1*
	An instance is running but refused or failed the command.

*2*
	Usage error.

# SEE ALSO

*raba*(8), *raba.conf*(5)
