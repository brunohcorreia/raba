#!/bin/bash
# RABA's dracut module: what goes into a dracut-built initramfs.
# SPDX-License-Identifier: GPL-3.0-or-later
#
# dracut is v1 and not later on purpose. Void, Gentoo and Chimera are
# non-systemd distributions that use dracut, and they are exactly the
# constituency that a systemd-first splash leaves out. See docs/milestones.md.
#
# The same rule as the mkinitcpio hook: this cannot fail an image build.
# dracut's `inst_*` functions call `dfatal` and abort the build when handed
# something that is not there, so nothing is handed to them unchecked, and
# `install` returns 0 on every path. `integration/tests/dracut-build.sh` runs
# this file against a stub of dracut that aborts where dracut aborts.

# shellcheck disable=SC2154
# $initdir, $moddir and $systemdsystemunitdir are dracut's, set before it
# sources this file. A module that assigned them would be a module that broke.

# Is there anything to install? 0 yes, 1 no.
check() {
    # `raba-setup` is as required as `raba`: without it there is no way to
    # resolve a theme on this machine, and an image with a splash binary and no
    # art is worse than an image with neither.
    require_binaries raba raba-setup || return 1
    return 0
}

depends() {
    # Nothing. RABA needs a console, which the kernel provides, and that is the
    # whole point of a text-mode splash: no DRM module, no GPU driver, no
    # plymouth-style renderer plugins. See docs/early-boot.md.
    return 0
}

install() {
    local frozen file unit_dir

    if ! frozen="$(raba-setup --freeze)"; then
        dwarning "raba: no theme resolved; this image gets no splash (try: raba --check)"
        return 0
    fi

    inst_multiple raba
    # `-o`: optional. The splash works without raba-ctl; only the ways of
    # asking it to leave early do not.
    inst_multiple -o raba-ctl

    while IFS= read -r file; do
        if [[ -f "$file" ]]; then
            inst_simple "$file"
        else
            dwarning "raba: $file went away while the image was being built"
        fi
    done < <(raba-setup --print-files)

    # Resolved on this machine, because an initramfs has no os-release(5) to
    # resolve `Theme=auto` against. See raba.conf(5).
    mkdir -p "$initdir/etc/raba"
    printf '%s' "$frozen" > "$initdir/etc/raba/raba.conf"

    if dracut_module_included "systemd"; then
        unit_dir="${systemdsystemunitdir:-/usr/lib/systemd/system}"
        # raba-initrd.service and not raba-start.service: systemd carries
        # unit state across switch_root, so the real root would never start a
        # unit of the same name that had already run in here. See
        # integration/systemd/raba-initrd.service.
        if [[ -f "$unit_dir/raba-initrd.service" ]]; then
            inst_simple "$unit_dir/raba-initrd.service"
            # Wanted from inside the image only, by hand rather than through
            # `systemctl enable`: one symlink, no host state, nothing that needs
            # systemd to be runnable on the machine building the image.
            mkdir -p "$initdir/$unit_dir/sysinit.target.wants"
            ln -sf ../raba-initrd.service \
                "$initdir/$unit_dir/sysinit.target.wants/raba-initrd.service"
        else
            dwarning "raba: raba-initrd.service is missing; this image gets no splash"
        fi
    elif [[ -f "$moddir/raba-start.sh" ]]; then
        # `pre-trigger`: udev is running, so whatever console driver is going to
        # bind is binding, and the root filesystem is still a long way off.
        # Earlier than this and the splash can be drawn onto a console the
        # kernel is about to replace.
        inst_hook pre-trigger 10 "$moddir/raba-start.sh"
    else
        dwarning "raba: the start script is missing; this image gets no splash"
    fi

    return 0
}
