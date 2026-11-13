#!/bin/bash
# The second boot, on any distribution: did the install actually take?
#
# Run by raba-check.service after multi-user.target, prints TAP on the console,
# and powers the machine off. Everything here is evidence available *after* the
# splash is gone — what was on the screen is the host's business, through QEMU's
# monitor.
#
# Nothing distribution-specific is passed in: the three initramfs generators are
# told apart by which tool is installed, the same way raba-setup does it.

count=0
failed=0

tap() {
    count=$((count + 1))
    if [ "$1" = 0 ]; then
        echo "ok ${count} - $2"
    else
        failed=$((failed + 1))
        echo "not ok ${count} - $2"
        if [ -n "$3" ]; then
            echo "#   $3"
        fi
    fi
}

# Is there a console a person could have looked at? CONFIG_VT gives a kernel 64
# terminals whether or not anything displays them; when no real driver binds,
# `dummycon` takes over, every VT ioctl succeeds and the splash is invisible.
# The host's screenshots mean nothing without this answered.
# `bind` is the one that says whether a driver is actually driving; the names
# alone list every driver the kernel knows about, dummy included.
bound=
for console in /sys/class/vtconsole/*; do
    [ -r "${console}/bind" ] || continue
    if [ "$(cat "${console}/bind")" = 1 ]; then
        bound="${bound}$(cat "${console}/name" 2> /dev/null), "
    fi
done
bound=${bound%, }
echo "# bound console drivers: ${bound:-none}"
if [ -n "$bound" ] && grep -qv dummy <<< "${bound//, /$'\n'}"; then
    tap 0 'a real console driver is bound'
else
    tap 1 'a real console driver is bound' "only ${bound:-nothing} — a VT would accept everything and display nothing"
fi

# Which initramfs, and how to look inside it.
if command -v lsinitcpio > /dev/null; then
    family=mkinitcpio
    image=$(ls -1t /boot/initramfs-*.img 2> /dev/null | grep -v fallback | head -1)
    contents=$(lsinitcpio "$image" 2> /dev/null)
elif command -v lsinitramfs > /dev/null; then
    family=initramfs-tools
    image=/boot/initrd.img-$(uname -r)
    contents=$(lsinitramfs "$image" 2> /dev/null)
elif command -v lsinitrd > /dev/null; then
    family=dracut
    image=/boot/initramfs-$(uname -r).img
    contents=$(lsinitrd "$image" 2> /dev/null)
else
    family=unknown
    image=
    contents=
fi
echo "# ${family}, ${image}"

if [ -n "$contents" ]; then
    tap 0 'the initramfs can be read back'
else
    tap 1 'the initramfs can be read back' "no listing for ${image:-no image found}"
fi

if grep -q 'usr/bin/raba$' <<< "$contents"; then
    tap 0 'the binary is in the real initramfs'
else
    tap 1 'the binary is in the real initramfs' 'no usr/bin/raba'
fi

if grep -q 'etc/raba/raba.conf' <<< "$contents"; then
    tap 0 'so is the frozen configuration'
else
    tap 1 'so is the frozen configuration' 'no etc/raba/raba.conf'
fi

if grep -qE 'raba/themes/.*\.raba' <<< "$contents"; then
    tap 0 'and the theme it resolved to'
else
    tap 1 'and the theme it resolved to' 'no theme in the image'
fi

if raba-setup --check > /tmp/check.out 2>&1; then
    tap 0 'raba-setup says the image is current'
else
    tap 1 'raba-setup says the image is current' "$(tr '\n' ' ' < /tmp/check.out)"
fi

# `not drawing` is what raba prints when it gave up: no theme, no terminal,
# somebody else on the screen. A boot that drew says nothing at all.
journal=$(journalctl -b --no-pager 2>&1)
if grep -q 'raba: not drawing' <<< "$journal"; then
    tap 1 'raba did not refuse to draw' "$(grep -m1 'not drawing' <<< "$journal")"
else
    tap 0 'raba did not refuse to draw'
fi

if systemctl is-failed --quiet 'raba-*.service'; then
    tap 1 'no raba unit failed' "$(systemctl list-units --failed --no-legend 'raba-*' | tr '\n' ' ')"
else
    tap 0 'no raba unit failed'
fi

if [ ! -e /run/raba.sock ]; then
    tap 0 'nothing of raba is left running'
else
    tap 1 'nothing of raba is left running' 'the control socket is still there'
fi

# The splash was holding a terminal. Somebody else has it now, which is the
# whole point of handing it back.
if systemctl is-active --quiet getty@tty1.service || systemctl is-active --quiet display-manager.service; then
    tap 0 'the login prompt took the screen back'
else
    tap 1 'the login prompt took the screen back' "$(systemctl is-active getty@tty1.service)"
fi

# Whatever raba wrote down, which is where a quiet ending leaves its reason.
if [ -s /run/raba.log ]; then
    sed 's/^/#   log: /' /run/raba.log
else
    echo "#   log: /run/raba.log is empty or absent"
fi
echo "# active terminal now: $(fgconsole 2> /dev/null || echo unknown)"

echo "1..${count}"
if [ "$failed" = 0 ]; then
    echo "# in-guest: all passed"
else
    echo "# in-guest: ${failed} failed"
fi
systemctl poweroff
