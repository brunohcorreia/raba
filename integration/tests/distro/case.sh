#!/bin/sh
# Break RABA on purpose, before the splash starts, and write down what was done.
#
# Harness only: nothing here ships in any package. The case to apply is read
# from a small vfat disk the host attaches — the same trick as the cloud-init
# seed and the payload disk — so the harness can choose a different sabotage per
# boot without ever having to edit the guest's filesystem from outside.
#
# Every case is something a person could plausibly do to their own machine: an
# upgrade that removed a file, a half-written download, a configuration edited
# in anger at two in the morning. The assertion is never that RABA works
# afterwards. It is that the computer still boots, which is the first promise in
# CONTRIBUTING.md, rule 1, and the one that matters more than any splash.
#
# SPDX-License-Identifier: GPL-3.0-or-later

log() {
    echo "raba-case: $*" >> /run/raba-case.log
    echo "raba-case: $*" > /dev/kmsg 2>/dev/null || true
}

# The marker the post-boot units condition on: with it, the strict checks stand
# down and the only question asked is whether the machine came up.
applied() {
    : > /run/raba-case-applied
    log "$*"
}

disk=$(blkid -L RABACASE 2>/dev/null)
if [ -z "$disk" ]; then
    log "no case disk attached; nothing broken"
    exit 0
fi

mkdir -p /run/raba-case
mount -o ro "$disk" /run/raba-case 2>/dev/null || {
    log "the case disk would not mount; nothing broken"
    exit 0
}
case_name=$(tr -d '\r\n' < /run/raba-case/case 2>/dev/null)
umount /run/raba-case 2>/dev/null

case "$case_name" in
    none | '')
        log "case none: nothing broken, this is the control"
        ;;
    no-binary)
        # An upgrade that went wrong, a file deleted by hand, a /usr that did
        # not finish unpacking. systemd will fail the unit, which is correct and
        # must stay harmless.
        rm -f /usr/bin/raba
        applied "case no-binary: /usr/bin/raba deleted"
        ;;
    truncated-art)
        # A download that stopped halfway, or a filesystem that lost a block.
        # The decoder has a failure table for this; what is under test here is
        # that a boot survives it.
        for theme in /usr/share/raba/themes/*.raba; do
            [ -f "$theme" ] || continue
            dd if=/dev/zero of="$theme" bs=1 count=0 seek=100 conv=notrunc 2>/dev/null
            truncate -s 100 "$theme" 2>/dev/null
        done
        applied "case truncated-art: every theme cut to 100 bytes"
        ;;
    garbage-config)
        printf 'this is not a configuration file\n[Boot\nTheme=\n\xff\xfe\n' \
            > /etc/raba/raba.conf
        applied "case garbage-config: /etc/raba/raba.conf replaced with nonsense"
        ;;
    impossible-tty)
        # A terminal number the kernel does not have. 63 was the first guess and
        # it was wrong: MAX_NR_CONSOLES is 63, devtmpfs makes /dev/tty63, and
        # RABA drew on it perfectly well — no error, nothing in the log, and a
        # case that asserted a complaint nobody had any reason to make. 64 is
        # one past the end: the device node does not exist and the open fails
        # with a sentence naming the path.
        printf '[Boot]\nTty=64\n' > /etc/raba/raba.conf
        applied "case impossible-tty: Tty=64"
        ;;
    socket-in-the-way)
        # Something else is already at the control socket's path, and it is not
        # a socket. bind() has to replace it rather than give up.
        #
        # The `rm` is not tidiness. By the time this runs the splash from the
        # initramfs is usually already listening, so the path is a live socket —
        # and opening a socket inode for writing fails with ENXIO, which on a
        # redirection into the special built-in `:` kills the whole shell. That
        # is how the first run of this case ended: the sabotage never happened,
        # the marker was never written, and the strict checks ran against a
        # machine nobody had broken.
        rm -f /run/raba.sock
        : > /run/raba.sock
        applied "case socket-in-the-way: a plain file at /run/raba.sock"
        ;;
    state-in-the-way)
        # The resume file's path is a directory, so every write to it fails.
        rm -f /run/raba.state
        mkdir -p /run/raba.state
        applied "case state-in-the-way: /run/raba.state is a directory"
        ;;
    *)
        log "case ${case_name}: not a case this script knows; nothing broken"
        ;;
esac
exit 0
