#!/usr/bin/env bash
# Build a real initramfs with the real mkinitcpio, boot it, and watch the splash
# come up inside it.
#
# scripts/vm-test.sh boots an image this project assembled itself, which proves
# the program and proves nothing about the hook. This one hands the job to
# mkinitcpio: our install hook runs inside a real build, the image it produces
# is the one a user would get, and the checks run from inside it as an ordinary
# runtime hook — after `raba` in HOOKS, so the splash has already been started
# by the hook under test.
#
# Needs mkinitcpio, so it runs on Arch-shaped machines and skips elsewhere. The
# stubbed harnesses in integration/tests cover what it cannot.
#
# Usage:
#   scripts/image-test.sh            build, boot, report
#   scripts/image-test.sh --keep     leave the image and sandbox behind
#   scripts/image-test.sh --console  show the guest's VGA console as well

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work="${root}/target/image-test"
image="${work}/initramfs.img"
timeout_seconds="${RABA_IMAGE_TIMEOUT:-90}"

keep=0
show_console=0
for arg in "$@"; do
    case "$arg" in
        --keep) keep=1 ;;
        --console) show_console=1 ;;
        -h | --help)
            sed -n '2,20p' "$0" | sed 's/^# \?//'
            exit 0
            ;;
        *)
            echo "image-test: unknown option $arg" >&2
            exit 2
            ;;
    esac
done

die() {
    echo "image-test: $*" >&2
    exit 1
}

skip() {
    echo "# skipped: $*"
    exit 0
}

command -v mkinitcpio > /dev/null || skip "mkinitcpio is not installed"
command -v qemu-system-x86_64 > /dev/null || skip "qemu-system-x86_64 is not installed"

# The kernel the machine is already running, preferring the world-readable copy:
# /boot is root-only on a great many machines.
find_kernel() {
    local candidate
    for candidate in \
        "${RABA_VM_KERNEL:-}" \
        "/usr/lib/modules/$(uname -r)/vmlinuz" \
        "/boot/vmlinuz-linux" \
        "/boot/vmlinuz-$(uname -r)"; do
        [ -n "$candidate" ] && [ -r "$candidate" ] && {
            echo "$candidate"
            return
        }
    done
    return 1
}
kernel="$(find_kernel)" || die "no readable kernel image found; set RABA_VM_KERNEL"

echo "image-test: building"
cargo build --release -p raba -p raba-setup -p raba-gen > /dev/null

bin="${root}/target/release"
for program in raba raba-ctl raba-setup raba-gen; do
    [ -x "${bin}/${program}" ] || die "${program} did not build"
done

# --- a machine for mkinitcpio to look at -------------------------------------

rm -rf "$work"
mkdir -p "${work}"/{themes,etc,hooks/install,hooks/hooks}
mkdir -p "${work}"/overlay/{upper,work}
sandbox_bin="${work}/overlay/upper"

"${bin}/raba-gen" generate matrix \
    --columns 80 --lines 25 --frames 8 --delay 10 \
    -o "${work}/themes/imagetest.raba" > /dev/null

cat > "${work}/etc/raba.conf" << CONF
[Boot]
Theme=imagetest
# No timeout: this test is about the splash being up, and a reveal in the
# middle of it would be testing the clock.
Timeout=0
ShowDelay=0
CONF
echo 'ID=imagetest' > "${work}/etc/os-release"

# The binaries, staged where the overlay below will put them: /usr/bin. Not a
# directory of our own on PATH — `add_binary raba` installs the binary into the
# image *at the path it resolved to*, so a raba found in a sandbox directory
# lands in the image at that sandbox path, and nothing in the image finds it.
install -m 755 "${bin}/raba" "${sandbox_bin}/raba"
install -m 755 "${bin}/raba-ctl" "${sandbox_bin}/raba-ctl"

# raba-setup, pointed at the sandbox rather than at this machine's own
# configuration — the hook calls it with no arguments, by design.
cat > "${sandbox_bin}/raba-setup" << WRAPPER
#!/bin/sh
exec "${bin}/raba-setup" "\$@" \\
    --config "${work}/etc/raba.conf" \\
    --defaults /nonexistent \\
    --themes "${work}/themes" \\
    --cmdline /nonexistent \\
    --os-release "${work}/etc/os-release"
WRAPPER
chmod 755 "${sandbox_bin}/raba-setup"

# The hooks under test, plus the one that checks them.
install -m 644 "${root}/integration/mkinitcpio/install/raba" "${work}/hooks/install/raba"
install -m 755 "${root}/integration/mkinitcpio/hooks/raba" "${work}/hooks/hooks/raba"
install -m 644 "${root}/integration/tests/rabacheck/install/rabacheck" \
    "${work}/hooks/install/rabacheck"
install -m 755 "${root}/integration/tests/rabacheck/hooks/rabacheck" \
    "${work}/hooks/hooks/rabacheck"

cat > "${work}/mkinitcpio.conf" << 'CONF'
MODULES=()
BINARIES=()
FILES=()
# `base` brings busybox and the init script. No `udev`: the console this draws
# on is devtmpfs and the kernel's, which is the entire point of a text splash.
HOOKS=(base raba rabacheck)
COMPRESSION="gzip"
CONF

# --- build the image, with the real thing ------------------------------------

# mkinitcpio is run unmodified, as root, in a throwaway user namespace whose
# /usr/bin is an overlay with our three binaries on top. Both of the things that
# were in the way need exactly that and nothing less:
#
#   - mkinitcpio resets PATH to `/usr/bin:/bin` on startup, so a sandbox
#     directory prepended to PATH is discarded before any hook runs;
#   - `add_binary raba` copies the binary into the image at the path it resolved
#     to, so even a found raba would be installed at /home/.../target/... and
#     the image's own PATH would not find it.
#
# Nothing outside the namespace is touched: the overlay is in target/ and it
# disappears with the process. If the machine forbids either piece, the test
# skips rather than pretending.
#
# fakeroot on top of that, because being root in a namespace is not being root:
# host-owned files appear as 65534 there, `cp --preserve=all` cannot chown to an
# id the namespace has no mapping for, and every add_binary would return
# non-zero — which is exactly the thing this test exists to prove does not
# happen. fakeroot makes ownership look the way it does to a real root build, so
# the image gets the ownership a package build would give it.
#
# /usr/bin and not `command -v`: this machine has a distribution wrapper at
# /usr/local/bin/mkinitcpio that calls the real one and then asks an interactive
# question about the bootloader. A test wants the program the package ships.
mkinitcpio_real="/usr/bin/mkinitcpio"
[ -x "$mkinitcpio_real" ] || mkinitcpio_real="$(command -v mkinitcpio || true)"
[ -n "$mkinitcpio_real" ] && [ -x "$mkinitcpio_real" ] \
    || skip "mkinitcpio is not installed"

unshare --user --map-root-user --mount true 2> /dev/null \
    || skip "unprivileged user namespaces are not available here"
command -v fakeroot > /dev/null \
    || skip "fakeroot is not installed (package: fakeroot)"

cat > "${work}/build-in-ns.sh" << 'NS'
#!/bin/sh
# Inside the namespace: lay the staged binaries over /usr/bin, then become
# whatever was asked for. 97 means the overlay was refused, which is a skip and
# not a failure.
set -eu
mount -t overlay overlay -o "lowerdir=/usr/bin,upperdir=$1,workdir=$2" /usr/bin || exit 97
shift 2
exec "$@"
NS
chmod 755 "${work}/build-in-ns.sh"

echo "image-test: mkinitcpio"
set +e
unshare --user --map-root-user --mount \
    "${work}/build-in-ns.sh" "${sandbox_bin}" "${work}/overlay/work" \
    fakeroot "$mkinitcpio_real" \
    --config "${work}/mkinitcpio.conf" \
    --hookdir "${work}/hooks" \
    --hookdir /usr/lib/initcpio \
    --kernel "$(uname -r)" \
    --generate "$image" > "${work}/build.log" 2>&1
build_status=$?
set -e
case "$build_status" in
    0) ;;
    97) skip "overlayfs is not available inside a user namespace here" ;;
    *)
        sed 's/^/#   /' "${work}/build.log" >&2
        die "mkinitcpio failed; the hook is supposed to make that impossible"
        ;;
esac
grep -iE '^(==> )?(ERROR|WARNING)' "${work}/build.log" | sed 's/^/# /' || true
echo "image-test: image $(du -h "$image" | cut -f1)"

# --- boot it -----------------------------------------------------------------

append="console=tty0 console=ttyS0 panic=-1 loglevel=3"
display=(-display none)
[ "$show_console" = 1 ] && display=()
accel=(-cpu max)
[ -w /dev/kvm ] && accel=(-enable-kvm -cpu host)

set +e
output="$(timeout "$timeout_seconds" qemu-system-x86_64 \
    -kernel "$kernel" \
    -initrd "$image" \
    -append "$append" \
    -m 512M \
    -no-reboot \
    "${accel[@]}" \
    "${display[@]}" \
    -serial stdio \
    2>&1)"
set -e

[ "$keep" = 1 ] || rm -rf "${work}/overlay" "${work}/hooks" "${work}/themes" "${work}/etc" \
    "${work}/build-in-ns.sh"

echo "$output" | grep -E '^(ok|not ok|# |1\.\.)' || true

plan="$(echo "$output" | grep -oE '^1\.\.[0-9]+' | tail -1 || true)"
failures="$(echo "$output" | grep -c '^not ok' || true)"

if [ -z "$plan" ]; then
    echo "image-test: the guest produced no plan line. Last of what it said:" >&2
    echo "$output" | tail -20 | sed 's/^/#   /' >&2
    exit 1
fi
if [ "$failures" != "0" ]; then
    echo "image-test: ${failures} failed" >&2
    exit 1
fi
echo "image-test: passed"
