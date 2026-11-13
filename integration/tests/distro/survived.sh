#!/bin/bash
# The machine was sabotaged before the splash. Did it boot anyway?
#
# This is the whole of what a sabotage case asserts. Not that RABA worked — the
# case was chosen to stop it working — but that the computer its owner needs
# came up, reached a login prompt, and said why in a place somebody can find.
#
# Runs instead of check.sh when /run/raba-case-applied exists.
#
# SPDX-License-Identifier: GPL-3.0-or-later

count=0
failed=0
tap() {
    count=$((count + 1))
    if [ "$1" = 0 ]; then
        echo "ok ${count} - $2"
    else
        failed=$((failed + 1))
        echo "not ok ${count} - $2"
        [ -n "${3:-}" ] && echo "#   $3"
    fi
}

case_name=$(sed -n 's/^raba-case: case \([a-z-]*\):.*/\1/p' /run/raba-case.log 2>/dev/null | head -1)
echo "# sabotage: ${case_name:-unknown}"
sed 's/^/#   /' /run/raba-case.log 2>/dev/null

# 1. The machine is up. `is-system-running` says `running` when everything
#    started and `degraded` when something failed and the rest carried on.
#    Either is a booted computer. `maintenance` and `emergency` are not.
state=$(systemctl is-system-running 2>/dev/null)
case "$state" in
    running | degraded | starting)
        tap 0 "the machine booted (${state})" ;;
    *)
        tap 1 "the machine booted" "systemd says ${state}" ;;
esac

# 2. Somebody can log in. The point of the whole project is that this is never
#    taken away.
if systemctl is-active --quiet getty@tty1.service \
    || systemctl is-active --quiet serial-getty@ttyS0.service \
    || systemctl is-active --quiet display-manager.service; then
    tap 0 'a login prompt is up'
else
    tap 1 'a login prompt is up' "$(systemctl is-active getty@tty1.service)"
fi

# 3. The root filesystem is writable, which is how a boot that only *looks*
#    finished gives itself away.
if touch /var/lib/raba-survived 2> /dev/null; then
    tap 0 'the root filesystem is mounted and writable'
    rm -f /var/lib/raba-survived
else
    tap 1 'the root filesystem is mounted and writable'
fi

# 4. Nothing of RABA is still holding a terminal. A broken splash that refuses
#    to let go is worse than one that never drew.
if pgrep -x raba > /dev/null; then
    tap 1 'nothing of raba is left holding the screen' "$(pgrep -ax raba | tr '\n' ';')"
else
    tap 0 'nothing of raba is left holding the screen'
fi

# 5. Whatever went wrong is written down somewhere a person would look. The
#    deleted-binary case is the exception: systemd reports that one itself, as
#    a failed unit, and RABA is not there to say anything.
journal=$(journalctl -b --no-pager --no-hostname 2>/dev/null)
said=$(grep -c 'raba: ' <<< "$journal")
if [ "$case_name" = no-binary ]; then
    if systemctl is-failed --quiet raba-start.service; then
        tap 0 'systemd reports the missing binary'
    else
        tap 1 'systemd reports the missing binary' "$(systemctl is-active raba-start.service)"
    fi
elif [ "$said" -gt 0 ]; then
    tap 0 "the reason is in the log (${said} line(s))"
    grep 'raba: ' <<< "$journal" | head -3 | sed 's/^/#   /'
else
    tap 1 'the reason is in the log' 'nothing in the journal and nothing in dmesg'
fi

echo "1..${count}"
if [ "$failed" = 0 ]; then
    echo "# in-guest: survived"
else
    echo "# in-guest: ${failed} failed"
fi
systemctl poweroff
