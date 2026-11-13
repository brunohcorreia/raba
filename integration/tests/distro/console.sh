#!/bin/bash
# Give the virtual machine a screen worth photographing.
#
# Cloud images boot with `console=ttyS0` and nothing else, so the kernel never
# registers the VGA console: `dummycon` binds, every VT ioctl succeeds, the
# splash draws into a terminal that is not connected to a display, and a
# screenshot of the window shows a blank. That is correct behaviour for a
# serial-only machine and useless for testing a splash, so the machine is made
# to look like the desktop it is standing in for.
#
# `console=tty0` goes *before* the serial one: the last console named becomes
# /dev/console, and the test's own TAP has to keep leaving by the serial line.
#
# This edits configuration that belongs to other packages, which RABA itself
# refuses to do (docs/packaging.md). A test harness arranging a throwaway
# virtual machine is a different thing from a package on somebody's computer.
set -x

add_console() {
    local file="$1"
    [ -f "$file" ] || return 0
    if grep -q 'console=ttyS0' "$file"; then
        sed -i 's/console=ttyS0/console=tty0 console=ttyS0/' "$file"
    else
        sed -i 's/$/ console=tty0 console=ttyS0/' "$file"
    fi
}

# systemd-boot entries, and the UKI command line.
if [ -d /boot/loader/entries ]; then
    for entry in /boot/loader/entries/*.conf; do
        [ -f "$entry" ] || continue
        if grep -q '^options' "$entry"; then
            if grep -q 'console=ttyS0' "$entry"; then
                sed -i 's/console=ttyS0/console=tty0 console=ttyS0/' "$entry"
            else
                sed -i 's/^options .*/& console=tty0 console=ttyS0/' "$entry"
            fi
        fi
    done
fi
add_console /etc/kernel/cmdline

# GRUB.
if [ -f /etc/default/grub ]; then
    if grep -q 'console=ttyS0' /etc/default/grub; then
        sed -i 's/console=ttyS0/console=tty0 console=ttyS0/g' /etc/default/grub
    else
        sed -i 's/^\(GRUB_CMDLINE_LINUX_DEFAULT="\)/\1console=tty0 console=ttyS0 /' /etc/default/grub
    fi
    if command -v grub-mkconfig > /dev/null; then
        grub-mkconfig -o /boot/grub/grub.cfg
    elif command -v grub2-mkconfig > /dev/null; then
        grub2-mkconfig -o /boot/grub2/grub.cfg
    fi
fi

cat /proc/cmdline
cat /boot/loader/entries/*.conf 2> /dev/null
