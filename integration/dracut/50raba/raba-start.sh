#!/bin/sh
# Start the splash in a dracut initramfs that is not running systemd.
# SPDX-License-Identifier: GPL-3.0-or-later

# Backgrounded: dracut sources its hooks, so blocking here blocks the boot.
# Being killed later is not an event — /run/raba.state carries the terminal and
# the frame across switch_root. See docs/early-boot.md.
if command -v raba > /dev/null; then
    raba &
fi
:
