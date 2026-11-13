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

# 5. Whatever went wrong is written down somewhere a person would look.
#
#    Three expectations, because the cases are not all the same shape and
#    demanding one sentence from all of them was how this check first failed.
#
#      systemd   The binary is gone, so RABA is not there to say anything and
#                systemd reports the failed unit itself. Nothing else can.
#      silence   RABA was *supposed* to cope. A plain file at the control
#                socket's path is something bind() replaces; coping with it
#                correctly means there is nothing to report, and a complaint
#                would be the defect. The assertion is the absence.
#      reason    The rest: RABA could not do its job and has to say why, once,
#                in words, in the log this machine actually keeps.
case "$case_name" in
    no-binary)         expect=systemd ;;
    socket-in-the-way) expect=silence ;;
    *)                 expect=reason ;;
esac

journal=$(journalctl -b --no-pager --no-hostname 2>/dev/null)
said=$(grep -c 'raba: ' <<< "$journal")
if [ "$expect" = systemd ]; then
    if systemctl is-failed --quiet raba-start.service; then
        tap 0 'systemd reports the missing binary'
    else
        tap 1 'systemd reports the missing binary' "$(systemctl is-active raba-start.service)"
    fi
elif [ "$expect" = silence ]; then
    if [ "$said" = 0 ] && ! systemctl is-failed --quiet raba-start.service; then
        tap 0 'raba took the path back and had nothing to complain about'
    else
        tap 1 'raba took the path back and had nothing to complain about' \
            "$(grep 'raba: ' <<< "$journal" | head -1)"
    fi
elif [ "$said" -gt 0 ]; then
    tap 0 "the reason is in the log (${said} line(s))"
    grep 'raba: ' <<< "$journal" | head -3 | sed 's/^/#   /'
else
    tap 1 'the reason is in the log' 'nothing in the journal and nothing in dmesg'
    # Everything of ours that this boot did say, because the alternative is
    # reinstalling the guest to find out. A failure of this check means the
    # machine survived something and never said what, and the first question is
    # always whether the units ran at all.
    echo '#   what the raba units said:'
    journalctl -b --no-pager --no-hostname -u 'raba*' 2>/dev/null | tail -20 | sed 's/^/#     /'
    echo '#   how they ended:'
    systemctl --no-pager --no-legend list-units 'raba*' --all 2>/dev/null | sed 's/^/#     /'
fi

echo "1..${count}"
if [ "$failed" = 0 ]; then
    echo "# in-guest: survived"
else
    echo "# in-guest: ${failed} failed"
fi
systemctl poweroff
