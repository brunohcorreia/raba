#!/bin/sh
# Start the splash in a dracut initramfs that is not running systemd.
# SPDX-License-Identifier: GPL-3.0-or-later

# Backgrounded: dracut sources its hooks, so blocking here blocks the boot.
# Being killed later is not an event — /run/raba.state carries the terminal and
# the frame across switch_root. See docs/early-boot.md.
# A splash that complains onto the screen it is drawing on has defeated itself,
# so raba's own output goes to a file. /run is carried into the real root by
# every initramfs generator worth the name, so what it said is still readable
# after the boot it happened on. If /run is not mounted yet the output goes
# away, never the splash: a missing log directory must not cost a boot its
# animation.
raba_log=/dev/null
[ -w /run ] && raba_log=/run/raba.early.log
if command -v raba > /dev/null; then
    raba > "$raba_log" 2>&1 &
fi
:
