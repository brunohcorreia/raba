#!/bin/sh
# Which terminal the screen is showing, and what is on it, while the splash is
# meant to be up.
#
# Harness only: nothing here ships in any package. Started by raba-watch.service
# beside the splash, it samples for the first twelve seconds of the boot and
# leaves its findings in /run for check.sh to read afterwards.
#
# This is the view from inside the machine, and it is the half the screenshots
# cannot give: a splash drawn on a terminal nobody is looking at is invisible
# for a reason no amount of photographing the monitor will explain.
#
# A file rather than a heredoc inside the cloud-config, like its five siblings
# in this directory. Ninety lines of shell inside a YAML block are ninety lines
# nothing can check, in the exact construct that once swallowed a whole script
# and cost a forty-minute run.
#
# SPDX-License-Identifier: GPL-3.0-or-later
end=$(( $(cut -d. -f1 /proc/uptime) + 12 ))
best=0
while [ "$(cut -d. -f1 /proc/uptime)" -lt "$end" ]; do
    now=$(cat /sys/class/tty/tty0/active 2>/dev/null)
    printf '%s %s\n' "$(cut -d' ' -f1 /proc/uptime)" "$now" >> /run/raba-watch.log
    # What is *on* that console, read from the kernel's own copy of the
    # screen rather than from a photograph of the monitor. /dev/vcsN is
    # the text of virtual console N, and it is the only answer that does
    # not depend on whether the emulator and the guest agree about the
    # video mode — which on a machine with no KMS driver in its initramfs
    # they do not, and a correct splash photographs as a black screen.
    case "$now" in
        tty[0-9]*)
            n=${now#tty}
            # Every console, tty1 included. An earlier version skipped
            # tty1 on the assumption that it belongs to the login prompt,
            # and reported that nothing was ever drawn on an Arch machine
            # whose splash the camera had photographed: with a systemd
            # initramfs, RABA adopts the console that is already active,
            # and that is tty1. The fullest snapshot wins, so a login
            # prompt's handful of characters never displaces a screen of
            # art.
            if ! cat "/dev/vcs$n" > /run/raba-watch-screen.new 2>> /run/raba-watch.err
            then
                echo "could not read /dev/vcs$n" >> /run/raba-watch.err
            fi
            ink=$(tr -cd '[:graph:]' < /run/raba-watch-screen.new | wc -c)
            if [ "$ink" -gt "$best" ]; then
                best=$ink
                mv /run/raba-watch-screen.new /run/raba-watch-screen.txt
            fi
            ;;
    esac
    # What the splash process is doing, if it is still there: the
    # state letter from /proc/PID/stat (R running, S sleeping, D in
    # uninterruptible I/O, T stopped) and the kernel function it is
    # waiting in. A splash that has stopped drawing but is still alive
    # is the failure this was written to catch, and "which syscall is
    # it sitting in" is the question that distinguishes a wedged write
    # from a loop that has decided there is nothing to draw.
    for pid in $(pgrep -x raba 2>/dev/null); do
        printf '%s pid=%s state=%s wchan=%s\n' \
            "$(cut -d' ' -f1 /proc/uptime)" "$pid" \
            "$(awk '{print $3}' "/proc/$pid/stat" 2>/dev/null)" \
            "$(cat "/proc/$pid/wchan" 2>/dev/null)" >> /run/raba-watch-proc.log
    done
    # And ask the splash itself what it thinks it is doing. The process
    # answers the socket while wedged — that is how a second instance
    # finds it — so `status` is the one probe that reports the phase and
    # the frame from inside the loop rather than inferring them.
    if [ -S /run/raba.sock ]; then
        said=$(raba-ctl status 2>&1 | tr '\n' ' ')
        printf '%s %s\n' "$(cut -d' ' -f1 /proc/uptime)" "$said" \
            >> /run/raba-watch-status.log
        # The console the splash says it holds, photographed while it
        # still holds it. Reading it after the boot works only where the
        # instance was killed and left the art behind; where the splash
        # ends properly it hands the console back and clears it, and by
        # then there is nothing to find.
        held=$(printf '%s' "$said" | sed -n 's/.*terminal: \([0-9]*\).*/\1/p')
        if [ -n "$held" ] && [ "$held" != 0 ]; then
            echo "$held" > /run/raba-watch-tty
            if cat "/dev/vcs$held" > /run/raba-held.new 2>/dev/null; then
                held_ink=$(tr -cd '[:graph:]' < /run/raba-held.new | wc -c)
                if [ "$held_ink" -gt "$(cat /run/raba-held-ink 2>/dev/null || echo 0)" ]
                then
                    echo "$held_ink" > /run/raba-held-ink
                    mv /run/raba-held.new /run/raba-held.txt
                fi
            fi
        fi
    fi
    # Written every pass, not at the end: the post-boot checks run while
    # this is still sampling, and a verdict that arrives after they have
    # read it is no verdict at all.
    echo "$best" > /run/raba-watch-ink
    sleep 0.1
done
echo "$best" > /run/raba-watch-ink
