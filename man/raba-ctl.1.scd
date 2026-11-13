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
	Exit 0 if an instance is running, *1* otherwise. The one command for which
	"nothing is listening" is a failure, because reporting that is what it is for.

*status*
	Print the current state: theme, terminal, animation phase, uptime. The reply is
	*key: value* lines; see _docs/protocol.md_ for the field list and for the rule
	that an unknown key is to be ignored rather than treated as an error.

# OPTIONS

*--socket* _path_
	Use _path_ instead of _/run/raba.sock_. Intended for testing.

*--version*, *--help*

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
	Command delivered, or no instance was running. The second half is the important
	one: the callers are boot-time units, and a display manager that failed to start
	because the splash was absent would be the splash breaking a boot. *ping* is the
	exception.

*1*
	An instance is running but refused or failed the command.

*2*
	Usage error, including a command containing a line break — which would end the
	request early and leave the remainder looking like a second command.

# SEE ALSO

*raba*(8), *raba.conf*(5), _docs/protocol.md_
