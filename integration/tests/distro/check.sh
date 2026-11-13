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
    image=$(find /boot -maxdepth 1 -name 'initramfs-*.img' ! -name '*fallback*' \
        -printf '%T@ %p\n' 2> /dev/null | sort -rn | head -1 | cut -d' ' -f2-)
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
#
# One of those reasons is not a fault: a second instance finding the first and
# declining is the conflict check doing its job, and on an initramfs that does
# not kill its own processes at switch_root it is the *expected* path — the
# instance from the initramfs is still drawing.
journal=$(journalctl -b --no-pager 2>&1)
refusals=$(grep 'raba: not drawing' <<< "$journal" | grep -v 'another raba is already running')
if [ -z "$refusals" ]; then
    tap 0 'raba did not refuse to draw for a bad reason'
else
    tap 1 'raba did not refuse to draw for a bad reason' "$(head -1 <<< "$refusals")"
fi
if grep -q 'another raba is already running' <<< "$journal"; then
    echo "#   a second instance declined, which means the first survived switch_root"
fi

if systemctl is-failed --quiet 'raba-*.service'; then
    tap 1 'no raba unit failed' "$(systemctl list-units --failed --no-legend 'raba-*' | tr '\n' ' ')"
else
    tap 0 'no raba unit failed'
fi

# A process, not a file. The socket file outliving the process is normal and
# documented: systemd SIGKILLs whatever the initramfs left behind at
# switch_root, so Drop never runs, and the next bind() replaces the stale file
# anyway. What would be a bug is a daemon still holding a terminal.
if pgrep -x raba > /dev/null; then
    tap 1 'nothing of raba is left running' "$(pgrep -ax raba | tr '\n' ';')"
elif raba-ctl ping > /dev/null 2>&1; then
    tap 1 'nothing of raba is left running' 'something still answers the control socket'
else
    tap 0 'nothing of raba is left running'
    if [ -e /run/raba.sock ]; then
        echo "#   a stale socket file remains; bind() replaces it"
    fi
fi

# The splash was holding a terminal. Somebody else has it now, which is the
# whole point of handing it back.
if systemctl is-active --quiet getty@tty1.service || systemctl is-active --quiet display-manager.service; then
    tap 0 'the login prompt took the screen back'
else
    tap 1 'the login prompt took the screen back' "$(systemctl is-active getty@tty1.service)"
fi

# Which terminal the screen was actually showing while the splash was meant to
# be on it. Written by the harness's own watcher, not by anything RABA ships.
if [ -s /run/raba-watch.log ]; then
    echo "#   screen was showing: $(awk '{print $2}' /run/raba-watch.log \
        | uniq -c | awk '{printf "%s x%s  ", $2, $1}')"
fi

# Every console the kernel has, and what is on it, at the moment the checks run.
# Reading this costs nothing and answers "was anything ever drawn anywhere" when
# the watcher comes back empty-handed.
for console in /dev/vcs[0-9]*; do
    [ -e "$console" ] || continue
    echo "#   ${console}: $(wc -c < "$console" 2>&1) bytes, $(tr -cd '[:graph:]' < "$console" 2>/dev/null | wc -c) of them ink"
done
if [ -s /run/raba-watch-proc.log ]; then
    echo "#   the splash process, while it lasted:"
    # First and last of each run of identical states, so twelve seconds of
    # sampling reads as a story rather than as a hundred identical lines.
    awk '{key=$3" "$4} key!=last {print "#     "$0; last=key}' /run/raba-watch-proc.log | head -12
    echo "#     last seen: $(tail -1 /run/raba-watch-proc.log)"
fi
if [ -s /run/raba-watch-status.log ]; then
    echo "#   what the splash said about itself:"
    awk '{$1=""; key=$0} key!=last {print "#     "$0; last=key}' /run/raba-watch-status.log | head -8
fi
if [ -s /run/raba-watch.err ]; then
    sed 's/^/#   watcher: /' /run/raba-watch.err
fi

# The console RABA actually drew on, named by RABA, and what was on it.
#
# This is the assertion the screenshots were supposed to make and could not: a
# console with no KMS driver under it is drawn correctly and photographs black,
# because the emulator is still rendering a video mode the guest stopped using.
# The characters are read out of /dev/vcs instead, which is the kernel's own
# copy of the screen.
#
# Named by RABA, because asking "which console has the most on it" — an earlier
# version of this — gets you the boot log on tty1 every time, three thousand
# characters of it, and a check that passes on a machine where the splash never
# drew. Two ways to have been told, and a machine offers one or the other:
#
#   while it was drawing   The watcher asked RABA which terminal it held and
#                          read that console there and then. The only way on a
#                          machine where the splash ends properly, because
#                          ending properly means handing the console back and
#                          clearing it.
#   after the fact         On an initramfs that does not kill its own
#                          processes, the instance is still holding its
#                          terminal when the checks run and the art is still on
#                          it. The state file names the terminal.
splash_tty=$(cat /run/raba-watch-tty 2>/dev/null)
ink=$(cat /run/raba-held-ink 2>/dev/null || echo 0)
art=/run/raba-held.txt
if [ "${ink:-0}" -le 100 ]; then
    splash_tty=$(sed -n 's/.*terminal=\([0-9]*\).*/\1/p' /run/raba.state 2>/dev/null | head -1)
    if [ -n "$splash_tty" ]; then
        art="/dev/vcs${splash_tty}"
        ink=$(tr -cd '[:graph:]' < "$art" 2>/dev/null | wc -c)
    fi
fi
# A hundred characters is a logo. A console nobody drew on reads zero, and a
# cursor or a stray prompt is a handful.
if [ -z "$splash_tty" ]; then
    tap 1 'the art was on the console' 'nothing ever said which terminal it took'
elif [ "${ink:-0}" -gt 100 ]; then
    tap 0 "the art was on tty${splash_tty} (${ink} characters)"
    fold -w 160 "$art" 2>/dev/null | sed 's/^/#   |/' | grep -v '^#   |  *$' | head -22
else
    tap 1 "the art was on tty${splash_tty}" "that console held ${ink} characters"
fi

# What the instance inside the initramfs said, if anything. This is the only
# window onto the half of the boot that happens before the journal exists.
if [ -s /run/raba.early.log ]; then
    sed 's/^/#   early: /' /run/raba.early.log
else
    echo "#   early: /run/raba.early.log is empty or absent"
fi
if [ -e /run/raba.state ]; then
    echo "#   state: $(tr -d '\000' < /run/raba.state | tr '\n' ' ')"
else
    echo "#   state: /run/raba.state is absent"
fi

# Whatever raba wrote down, which is where a quiet ending leaves its reason.
if [ -s /run/raba.log ]; then
    sed 's/^/#   log: /' /run/raba.log
else
    echo "#   log: /run/raba.log is empty or absent"
fi
# Everything the boot said about raba, verbatim and in order, with the monotonic
# clock. A failure here is a timing story — who bound the socket, who declined,
# who was killed and when — and no amount of summarising substitutes for the
# lines themselves.
echo "# --- what the boot said about raba ---"
# --no-hostname, because the hostname *is* "raba-<distro>" and without it
# every kernel line in the boot matches the filter.
journalctl -b -o short-monotonic --no-hostname --no-pager 2>/dev/null \
    | grep -iE 'raba|switch_root|frame buffer device|dummy device' \
    | sed 's/^/#   /' | head -60
echo "# --- units ---"
systemctl list-units --no-legend --no-pager 'raba-*' 2>/dev/null | sed 's/^/#   /'
# /sys, not fgconsole(1): that one is in a package a cloud image may not have.
echo "# active terminal now: $(cat /sys/class/tty/tty0/active 2> /dev/null || echo unknown)"
# `-x`, because without it this matches raba-check — the script printing the
# line — and every healthy run has reported the harness as a surviving splash.
# The `|| echo nothing` that used to be on the end could never fire either:
# `tr` succeeds on empty input, so the fallback was unreachable.
still=$(pgrep -ax raba | tr '\n' ';')
echo "# still running: ${still:-nothing}"
raba-ctl ping > /dev/null 2>&1
echo "# raba-ctl ping says: $?"

echo "1..${count}"
if [ "$failed" = 0 ]; then
    echo "# in-guest: all passed"
else
    echo "# in-guest: ${failed} failed"
fi
systemctl poweroff
