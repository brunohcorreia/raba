#!/bin/bash
# Tier 3c: what happens to a machine when RABA, or the power, fails.
#
# Tier 3b (scripts/distro-test.sh) asks whether a real distribution installs
# this and draws. This asks the question the project's own notes put first:
#
#     "no pior caso, se quebrar, nao podemos impedir o computador de dar boot"
#
# Nothing here is a unit test of RABA. Every case is a whole machine, booted,
# and the assertion is always the same one: it came up anyway. A splash that
# costs somebody their computer is not a splash, it is a brick.
#
# Two groups:
#
#   power cuts   The virtual machine loses power at a chosen moment, usually
#                inside the splash, and is then booted again from the same
#                disk. RABA writes nothing to persistent storage at boot — only
#                to /run, which is a tmpfs — so the claim under test is that a
#                cut cannot corrupt anything of ours, and the machine boots.
#
#   sabotage     The installed RABA is deliberately broken before the splash
#                starts: binary deleted, art truncated, configuration replaced
#                with nonsense, a terminal that does not exist. The machine has
#                to reach multi-user and show a login prompt all the same, and
#                the reason has to be in the journal once, in words.
#
# Both groups boot an already-installed image rather than installing each time.
# distro-test.sh --keep-installed leaves one behind; this builds it if it is
# missing, which takes about ten minutes, and then each case is about thirty
# seconds.
#
# Usage:
#   scripts/resilience.sh --distro arch [--cuts] [--sabotage] [--case <name>]
#   scripts/resilience.sh --distro arch --cut-at 2.0
#
# SPDX-License-Identifier: GPL-3.0-or-later
set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
distro=arch
run_cuts=0
run_sabotage=0
only_case=
cut_points=(1.0 1.6 2.2 3.0 4.5)
keep=0

die() {
    echo "resilience: $*" >&2
    exit 1
}

while [ $# -gt 0 ]; do
    case "$1" in
        --distro) distro="${2:-}"; shift 2 ;;
        --cuts) run_cuts=1; shift ;;
        --sabotage) run_sabotage=1; shift ;;
        --case) only_case="${2:-}"; run_sabotage=1; shift 2 ;;
        --cut-at) cut_points=("${2:-}"); run_cuts=1; shift 2 ;;
        --keep) keep=1; shift ;;
        -h|--help) sed -n '2,40p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) die "unknown option: $1" ;;
    esac
done
if [ "$run_cuts" = 0 ] && [ "$run_sabotage" = 0 ]; then
    run_cuts=1
    run_sabotage=1
fi

work="${root}/target/distro/${distro}"
installed="${work}/installed.qcow2"
seed="${work}/seed.img"
firmware=/usr/share/edk2/x64/OVMF.4m.fd
[ -r "$firmware" ] || firmware=/usr/share/OVMF/OVMF_CODE.fd
[ -r "$firmware" ] || die "no OVMF firmware found (package: edk2-ovmf)"
command -v qemu-system-x86_64 > /dev/null || die "qemu is not installed"

if [ ! -s "$installed" ]; then
    echo "resilience: no installed image yet; building one with tier 3b"
    "${root}/scripts/distro-test.sh" --distro "$distro" --keep-installed \
        || die "the install run failed; fix tier 3b first"
    [ -s "$installed" ] || die "the install run left no ${installed}"
fi

# --- one boot ----------------------------------------------------------------

# boot <disk> <seconds> [kill-after] [extra-disk]
#
# Runs until the guest powers itself off, until `kill-after` seconds have passed
# — which is the power cut, a SIGKILL to qemu with no shutdown of any kind — or
# until the deadline. Leaves the console in $serial.
boot() {
    local disk="$1" limit="$2" cut="${3:-}" extra="${4:-}"
    local disks=(-drive "file=${disk},if=virtio,format=qcow2")
    # The cloud-init seed goes back in on every boot. Without it the datasource
    # is gone, and cloud-init spends a minute of each case looking for it before
    # giving up — which is a minute per boot of a harness whose whole point is
    # that each boot is cheap.
    [ -s "$seed" ] && disks+=(-drive "file=${seed},if=virtio,format=raw")
    [ -n "$extra" ] && disks+=(-drive "file=${extra},if=virtio,format=raw")
    local accel=(-cpu max)
    [ -w /dev/kvm ] && accel=(-enable-kvm -cpu host)

    : > "$serial"
    qemu-system-x86_64 \
        "${disks[@]}" \
        -bios "$firmware" \
        -m 2048 -smp 2 "${accel[@]}" \
        -display none -vga std \
        -netdev user,id=net0 -device virtio-net-pci,netdev=net0 \
        -serial "file:${serial}" \
        > /dev/null 2>&1 &
    local qemu=$!

    local waited=0
    while kill -0 "$qemu" 2> /dev/null; do
        if [ -n "$cut" ] && awk "BEGIN{exit !($waited >= $cut)}"; then
            # The power cut. No SIGTERM, no ACPI, no flush: the machine stops
            # existing between two instructions, which is what a power cut is.
            kill -9 "$qemu" 2> /dev/null
            wait "$qemu" 2> /dev/null
            return 99
        fi
        if awk "BEGIN{exit !($waited >= $limit)}"; then
            kill -9 "$qemu" 2> /dev/null
            wait "$qemu" 2> /dev/null
            return 98
        fi
        sleep 0.1
        waited=$(awk "BEGIN{print $waited + 0.1}")
    done
    wait "$qemu" 2> /dev/null
    return 0
}

# What the guest said, with the journal's wrapping taken off.
tap_of() {
    sed -E 's/\x1b\[[0-9;?]*[a-zA-Z]//g; s/\r$//; s/^\[[ 0-9.]+\] //; s/^[A-Za-z0-9_.@-]+\[[0-9]+\]: //' \
        "$serial"
}

# Did the machine come up? Two independent signs, because a machine that boots
# and a machine that runs our checks are different claims and the first is the
# one that matters.
came_up() {
    local text
    text="$(tap_of)"
    grep -qa 'login:' <<< "$text" || grep -qa '1\.\.[0-9]' <<< "$text"
}

passes=0
failures=0
ok() { passes=$((passes + 1)); echo "ok - $1"; }
no() { failures=$((failures + 1)); echo "not ok - $1"; [ -n "${2:-}" ] && echo "#   $2"; }

serial="${work}/resilience-serial.log"

# --- power cuts --------------------------------------------------------------

if [ "$run_cuts" = 1 ]; then
    echo "# power cuts"
    for when in "${cut_points[@]}"; do
        cut="${work}/cut.qcow2"
        rm -f "$cut"
        qemu-img create -f qcow2 -F qcow2 -b "$installed" "$cut" > /dev/null \
            || die "could not make an overlay"

        boot "$cut" 90 "$when"
        local_cut_rc=$?
        if [ "$local_cut_rc" != 99 ]; then
            no "power cut at ${when}s" "the machine was already gone before the cut"
            continue
        fi

        # The same disk, with whatever the cut left on it.
        boot "$cut" 180
        if came_up; then
            ok "the machine boots again after losing power at ${when}s"
        else
            no "the machine boots again after losing power at ${when}s" \
               "$(tap_of | tail -3 | tr '\n' ' ')"
        fi
        if grep -qa '^not ok' <<< "$(tap_of)"; then
            no "nothing failed on the boot after the cut at ${when}s" \
               "$(tap_of | grep -a '^not ok' | head -1)"
        else
            ok "nothing failed on the boot after the cut at ${when}s"
        fi
        [ "$keep" = 1 ] || rm -f "$cut"
    done
fi

# --- sabotage ---------------------------------------------------------------

# Each case is a word on a small vfat disk the guest reads before the splash
# starts. The same trick as the cloud-init seed: the host chooses what to break
# without ever editing the guest's filesystem.
case_disk() {
    local name="$1" disk="${work}/case.img"
    rm -f "$disk" "${work}/case"
    echo "$name" > "${work}/case"
    mkfs.vfat -n RABACASE -C "$disk" 64 > /dev/null 2>&1 \
        || die "could not make the case disk"
    mcopy -i "$disk" "${work}/case" :: || die "could not write the case disk"
    echo "$disk"
}

CASES=(none no-binary truncated-art garbage-config impossible-tty socket-in-the-way
       state-in-the-way)

if [ "$run_sabotage" = 1 ]; then
    echo "# sabotage"
    for name in "${CASES[@]}"; do
        [ -n "$only_case" ] && [ "$only_case" != "$name" ] && continue
        disk="$(case_disk "$name")"
        broken="${work}/broken.qcow2"
        rm -f "$broken"
        qemu-img create -f qcow2 -F qcow2 -b "$installed" "$broken" > /dev/null \
            || die "could not make an overlay"

        boot "$broken" 180 "" "$disk"
        text="$(tap_of)"
        if came_up; then
            ok "${name}: the machine still boots"
        else
            no "${name}: the machine still boots" "$(tail -3 <<< "$text" | tr '\n' ' ')"
        fi
        if grep -qa '^not ok' <<< "$text"; then
            no "${name}: every survival check passed" \
               "$(grep -a '^not ok' <<< "$text" | head -1)"
        else
            ok "${name}: every survival check passed"
        fi
        grep -aE '^(ok|not ok|# sabotage|#   )' <<< "$text" | sed 's/^/#   /' | head -14
        [ "$keep" = 1 ] || rm -f "$broken" "$disk"
    done
fi

echo "1..$((passes + failures))"
if [ "$failures" != 0 ]; then
    echo "resilience: ${failures} failed" >&2
    exit 1
fi
echo "resilience: ${distro} survived everything it was asked to"
