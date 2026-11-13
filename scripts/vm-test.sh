#!/usr/bin/env bash
# Tier 3a: boot a real kernel in QEMU, run RABA's checks against a real console,
# and come back in about a second.
#
# No disk, no distribution, no init system. The kernel is the one already on this
# machine and the initramfs is built here from two static binaries. That is what
# makes a VT test something you run on every commit rather than once a month.
#
# Usage:
#   scripts/vm-test.sh                 run the checks
#   scripts/vm-test.sh --self-test     assert that a failing run comes back failed
#   scripts/vm-test.sh --keep          leave the initramfs behind for inspection
#   scripts/vm-test.sh --console       show the guest's VGA console as well
#   scripts/vm-test.sh --coverage      also measure what the guest ran, line
#                                      by line, and leave the profile where
#                                      scripts/coverage.sh will find it
#
# See docs/development.md on the tiers and crates/raba-vmtest for the guest side.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target="x86_64-unknown-linux-musl"
work="${root}/target/vm"
image="${work}/initramfs.img"
timeout_seconds="${RABA_VM_TIMEOUT:-60}"

self_test=0
keep=0
show_console=0
coverage=0
for arg in "$@"; do
    case "$arg" in
        --self-test) self_test=1 ;;
        --keep) keep=1 ;;
        --console) show_console=1 ;;
        --coverage) coverage=1 ;;
        -h|--help) sed -n '2,18p' "$0" | sed 's/^# \?//'; exit 0 ;;
        *) echo "vm-test: unknown option $arg" >&2; exit 2 ;;
    esac
done

die() { echo "vm-test: $*" >&2; exit 1; }

# --- what we need on the host ------------------------------------------------

command -v qemu-system-x86_64 >/dev/null || die "qemu-system-x86_64 is not installed.
Install it and run this again:
    sudo pacman -S --needed qemu-base          # Arch, CachyOS
    sudo apt install qemu-system-x86           # Debian, Ubuntu
    sudo dnf install qemu-system-x86-core      # Fedora"
command -v cpio >/dev/null || die "cpio is not installed (package: cpio)"

rustup target list --installed 2>/dev/null | grep -qx "$target" \
    || die "the $target toolchain is missing. Add it with:
    rustup target add $target
A static binary is what lets the initramfs be two files and no loader."

# The kernel. /boot is often an ESP that only root can read, so the copy the
# kernel package keeps under /usr/lib/modules is tried first — it is the same
# image and it is world readable.
find_kernel() {
    local candidates=(
        "${RABA_VM_KERNEL:-}"
        "/usr/lib/modules/$(uname -r)/vmlinuz"
        "/boot/vmlinuz-linux"
        "/boot/vmlinuz-$(uname -r)"
        "/boot/vmlinuz"
    )
    for candidate in "${candidates[@]}"; do
        [ -n "$candidate" ] && [ -r "$candidate" ] && { echo "$candidate"; return; }
    done
    # CI images name the kernel after a flavour nothing else predicts
    # (vmlinuz-6.8.0-1021-azure and the like), so fall back to whatever is there.
    local found=
    for found in /boot/vmlinuz-*; do break; done
    [ -e "$found" ] || found=
    [ -n "$found" ] && [ -r "$found" ] && { echo "$found"; return; }
    return 1
}
kernel="$(find_kernel)" || die "no readable kernel image found.
Looked in /usr/lib/modules/$(uname -r)/vmlinuz and /boot. Set RABA_VM_KERNEL to one."

# --- build the guest ---------------------------------------------------------

echo "vm-test: building the guest for $target"
cargo build --release --target "$target" -p raba-vmtest -p raba >/dev/null
cargo build --release -p raba-gen >/dev/null

bin="${root}/target/${target}/release"

# The instrumented binaries, when asked for: `raba`, which the checks start as
# a child process, and the harness itself, which drives the `raba` *library*
# directly for every check in vt_checks.
#
# Both, and not only `raba`, since the harness started running its checks in a
# child of PID 1. PID 1 leaves through isa-debug-exit and runs no `atexit`, so
# it could never write counters; its child exits the ordinary way and does.
# Until that split existed, half of tier 3a ran, passed, and measured as
# untested — crates/raba/src/vt read 77% while the checks exercising it were
# green.
#
# Their own target directory, so an ordinary run that follows a coverage run
# does not boot an instrumented binary and wonder why the image doubled.
cover_out="${root}/target/coverage/vm"
if [ "$coverage" = 1 ]; then
    command -v base64 >/dev/null || die "base64 is not installed (package: coreutils)"
    echo "vm-test: building an instrumented raba and harness for the guest"
    rm -rf "$cover_out"
    mkdir -p "$cover_out"
    CARGO_TARGET_DIR="${root}/target/coverage/guest" \
        RUSTFLAGS="-C instrument-coverage" \
        cargo build --release --target "$target" -p raba -p raba-vmtest >/dev/null
    bin_covered="${root}/target/coverage/guest/${target}/release"
    [ -x "${bin_covered}/raba" ] || die "the instrumented raba did not build"
    [ -x "${bin_covered}/raba-vmtest" ] || die "the instrumented harness did not build"
    # The objects the counters belong to: llvm-cov needs the binary that wrote
    # a profile, and neither of these ever existed on the host.
    {
        echo "${bin_covered}/raba"
        echo "${bin_covered}/raba-vmtest"
    } > "${cover_out}/objects.txt"
fi

[ -x "${bin}/raba-vmtest" ] || die "raba-vmtest did not build"
[ -x "${bin}/raba-ctl" ] || die "raba-ctl did not build"
[ -x "${bin}/raba" ] || die "raba did not build"

# A fixture to play. Generated rather than committed: it has to match whatever
# the format currently is, and a stale one would fail for the wrong reason.
rm -rf "$work"
mkdir -p "${work}/root"/{dev,proc,run,sys,fixtures}
"${root}/target/release/raba-gen" generate matrix \
    --columns 80 --lines 25 --frames 8 --delay 10 \
    -o "${work}/root/fixtures/vmtest.raba" >/dev/null

if [ "$coverage" = 1 ]; then
    install -m 755 "${bin_covered}/raba-vmtest" "${work}/root/init"
    install -m 755 "${bin_covered}/raba" "${work}/root/raba"
else
    install -m 755 "${bin}/raba-vmtest" "${work}/root/init"
    install -m 755 "${bin}/raba" "${work}/root/raba"
fi
# M4: the daemon checks drive the real client against the real daemon.
install -m 755 "${bin}/raba-ctl" "${work}/root/raba-ctl"

( cd "${work}/root" && find . -print0 | cpio --null --create --format=newc --quiet ) \
    | gzip -1 > "$image"
echo "vm-test: initramfs $(du -h "$image" | cut -f1), kernel $(basename "$kernel")"

# --- run ---------------------------------------------------------------------

# Both consoles, in this order, and the order matters twice.
#
# `console=tty0` is what makes the kernel register the VGA text console at all.
# Without it only ttyS0 is a console, the guest has terminals that nothing draws,
# and every VT test from M3 onward passes while proving nothing — which is what
# the "not just a serial line" check in the guest exists to catch.
#
# `console=ttyS0` comes last because the last one named becomes /dev/console, and
# /dev/console is where PID 1's output goes. So results leave by the serial line
# that -serial stdio captures, while tty0 stays free to be drawn on and inspected.
append="console=tty0 console=ttyS0 panic=-1 loglevel=3"

# An instrumented binary is several times the size of the shipped one, and an
# initramfs is unpacked into RAM rather than read from a disk.
memory=256M
[ "$coverage" = 1 ] && memory=1G
[ "$self_test" = 1 ] && append="$append rabatest.selftest=fail"

display=(-display none)
[ "$show_console" = 1 ] && display=()

# The CPU model, which is not a performance question here but a correctness one.
#
# QEMU's default `qemu64` is roughly a 2003 processor. Distributions that build
# for x86-64-v3 or v4 — CachyOS, and increasingly others — produce kernels that
# simply will not run on it, and the failure has no error message at all: the
# kernel faults before it can print, `panic=-1` reboots, `-no-reboot` makes QEMU
# exit 0, and you are left with a silent success. `-cpu max` offers everything
# the emulator knows how to provide.
#
# KVM when the kernel will allow it, because a second a run is the whole premise
# of this tier. `-cpu host` is both faster and strictly more capable.
accel=(-cpu max)
if [ -w /dev/kvm ]; then
    accel=(-enable-kvm -cpu host)
fi

set +e
output="$(timeout "$timeout_seconds" qemu-system-x86_64 \
    -kernel "$kernel" \
    -initrd "$image" \
    -append "$append" \
    -m "$memory" \
    -no-reboot \
    "${accel[@]}" \
    "${display[@]}" \
    -serial stdio \
    -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
    2>&1)"
qemu_status=$?
set -e

[ "$keep" = 1 ] || rm -rf "${work}/root"

# TAP, as the guest printed it. Anything the kernel said is left out unless it
# failed, because a boot log around five result lines hides them.
# The YAML block under a failure carries the reason, and it is indented — which
# is exactly what a naive filter drops, leaving "not ok 14" and no way to know why.
# The profile blocks are cut out first: a megabyte of base64 in a terminal
# helps nobody, and it is not a result.
echo "$output" \
    | sed '/^# raba-profile-begin/,/^# raba-profile-end/d' \
    | grep -E '^(ok|not ok|# |1\.\.|TAP| +(---|\.\.\.|reason:))' || true

if [ "$coverage" = 1 ]; then
    # One block per profile, `# raba-profile-begin <name> <bytes>` to
    # `# raba-profile-end`. See crates/raba-vmtest/src/profile.rs.
    blocks=0
    while read -r name; do
        blocks=$((blocks + 1))
        echo "$output" \
            | sed -n "/^# raba-profile-begin ${name} /,/^# raba-profile-end/p" \
            | sed '1d;$d' \
            | tr -d '\r' \
            | base64 -d > "${cover_out}/${name}" \
            || die "the guest's profile did not decode"
    done < <(echo "$output" | sed -n 's/^# raba-profile-begin \([^ ]*\) .*/\1/p')
    if [ "$blocks" = 0 ]; then
        die "the guest produced no profile. Either every raba it started was
killed, or the binary in the image was not the instrumented one."
    fi
    echo "vm-test: ${blocks} profile(s) in ${cover_out}; scripts/coverage.sh merges them"
fi

# --- verdict -----------------------------------------------------------------

# isa-debug-exit reports (value << 1) | 1, so the guest's 0x10 and 0x11 arrive
# here as 33 and 35. The TAP output is parsed as well rather than instead: when
# the machine dies before it can write to the port, the plan line is what says
# whether anything ran at all.
plan="$(echo "$output" | grep -oE '^1\.\.[0-9]+' | tail -1 || true)"
failures="$(echo "$output" | grep -c '^not ok' || true)"

case "$qemu_status" in
    33) verdict=pass ;;
    35) verdict=fail ;;
    124) echo "vm-test: timed out after ${timeout_seconds}s" >&2; verdict=fail ;;
    *)
        if [ -n "$plan" ] && [ "$failures" = 0 ]; then
            verdict=pass    # powered off by sysrq instead; the plan line decides
        else
            echo "vm-test: qemu exited $qemu_status" >&2
            echo "$output" | tail -30 >&2
            verdict=fail
        fi
        ;;
esac

if [ -z "$plan" ]; then
    echo "vm-test: the guest never printed a plan line — it did not reach the checks" >&2
    # Silence plus a clean exit is the signature of a kernel that faulted before it
    # could say anything. The usual cause is a CPU the kernel was not built for,
    # which the accel block above is meant to prevent — so if this is what you are
    # reading, look there first.
    if [ -z "$(echo "$output" | tr -d '[:space:]')" ]; then
        echo "vm-test: the guest produced no output at all. Most likely the kernel" >&2
        echo "         faulted before printing: try RABA_VM_KERNEL with a generic" >&2
        echo "         kernel, or check that this one runs on \"${accel[*]}\"." >&2
    else
        echo "$output" | tail -20 >&2
    fi
    verdict=fail
fi

if [ "$self_test" = 1 ]; then
    # Inverted on purpose: the run was told to fail, so a pass is the bug.
    if [ "$verdict" = fail ] && [ "$failures" -ge 1 ]; then
        echo "vm-test: self-test passed — a failing check is reported as a failure"
        exit 0
    fi
    echo "vm-test: SELF-TEST FAILED — the harness reported success for a run that failed" >&2
    exit 1
fi

[ "$verdict" = pass ] || { echo "vm-test: FAILED ($failures check(s))" >&2; exit 1; }
echo "vm-test: passed"
