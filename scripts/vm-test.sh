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
for arg in "$@"; do
    case "$arg" in
        --self-test) self_test=1 ;;
        --keep) keep=1 ;;
        --console) show_console=1 ;;
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
    return 1
}
kernel="$(find_kernel)" || die "no readable kernel image found.
Looked in /usr/lib/modules/$(uname -r)/vmlinuz and /boot. Set RABA_VM_KERNEL to one."

# --- build the guest ---------------------------------------------------------

echo "vm-test: building the guest for $target"
cargo build --release --target "$target" -p raba-vmtest -p raba >/dev/null
cargo build --release -p raba-gen >/dev/null

bin="${root}/target/${target}/release"
[ -x "${bin}/raba-vmtest" ] || die "raba-vmtest did not build"
[ -x "${bin}/raba" ] || die "raba did not build"

# A fixture to play. Generated rather than committed: it has to match whatever
# the format currently is, and a stale one would fail for the wrong reason.
rm -rf "$work"
mkdir -p "${work}/root"/{dev,proc,sys,fixtures}
"${root}/target/release/raba-gen" generate matrix \
    --columns 80 --lines 25 --frames 8 --delay 10 \
    -o "${work}/root/fixtures/vmtest.raba" >/dev/null

install -m 755 "${bin}/raba-vmtest" "${work}/root/init"
install -m 755 "${bin}/raba" "${work}/root/raba"

( cd "${work}/root" && find . -print0 | cpio --null --create --format=newc --quiet ) \
    | gzip -1 > "$image"
echo "vm-test: initramfs $(du -h "$image" | cut -f1), kernel $(basename "$kernel")"

# --- run ---------------------------------------------------------------------

# console=ttyS0 puts our output on the serial line that -serial stdio captures,
# while the emulated VGA adapter still gives the guest a real virtual terminal to
# draw on. That split is the whole trick: tty0 is under test, ttyS0 reports on it.
append="console=ttyS0 panic=-1 loglevel=3"
[ "$self_test" = 1 ] && append="$append raba.selftest=fail"

display=(-display none)
[ "$show_console" = 1 ] && display=()

set +e
output="$(timeout "$timeout_seconds" qemu-system-x86_64 \
    -kernel "$kernel" \
    -initrd "$image" \
    -append "$append" \
    -m 256M \
    -no-reboot \
    "${display[@]}" \
    -serial stdio \
    -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
    2>&1)"
qemu_status=$?
set -e

[ "$keep" = 1 ] || rm -rf "${work}/root"

# TAP, as the guest printed it. Anything the kernel said is left out unless it
# failed, because a boot log around five result lines hides them.
echo "$output" | grep -E '^(ok|not ok|# |1\.\.|TAP)' || true

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
